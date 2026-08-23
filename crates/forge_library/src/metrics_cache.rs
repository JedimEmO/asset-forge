//! Remembering what audio measured, so nothing decodes twice.
//!
//! Measuring a file means decoding all of it. `list_audio` measures the whole
//! library on every call, and the studio's browser wants the same numbers on
//! every row — which on a library of a hundred sounds is seconds of work to
//! answer a question whose answer has not changed since the last time.
//!
//! So this is the one measured exception to "sidecars are the source of truth
//! and everything else is derived by scanning". It lives in gitignored `out/`,
//! it is keyed on `(path, size, mtime)` so a file that changed is measured
//! again, and losing it costs only time. Nothing reads a value out of here
//! that it could not have computed.
//!
//! # Two processes, last writer wins
//!
//! [`MetricsCache::load`] reads the whole file, [`MetricsCache::save`] writes
//! the whole file, and there is no lock between them. The studio and the MCP
//! server run at the same time and both measure audio, so the ordinary case is
//! two of these open at once — and whichever saves last replaces the other's
//! entries wholesale.
//!
//! That is fine, and it is fine for one reason: **nothing is only in here.**
//! Every entry is a measurement of a file that is still on disk, so a lost
//! entry costs one re-decode and nothing else. The alternative — a lock file,
//! or a merge on save — would be real complexity bought to protect data that
//! is by definition reproducible. The write itself is atomic, so the failure
//! being accepted here is a lost entry, never a corrupt file.
//!
//! # Why the numbers are re-declared here
//!
//! The record is plain `serde` types with the same fields as
//! `forge_audio::Metrics` rather than that type itself, so the on-disk shape
//! is this module's to version: a field the metrics gain tomorrow must not
//! silently invalidate every cache on every machine. [`From`] does the copy.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::{LibraryError, Project, Result, read_to_string, write_atomic};

/// Bumped when the shape of a record changes. An older cache is discarded
/// rather than migrated: it is a cache.
const VERSION: u32 = 1;

/// What one audio file measures.
///
/// Field-for-field what `forge_audio::Metrics` reports, because these are
/// the numbers the library table shows and the ones an agent reads. Every one
/// of them is a fact about the decoded samples; nothing here is asked of a
/// generator, which is why it can be recomputed and therefore cached.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AudioMeasurement {
    /// Length in seconds.
    pub duration: f32,
    /// Samples per second.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: usize,
    /// Loudest absolute sample, linear.
    pub peak: f32,
    /// Loudest absolute sample, dBFS.
    pub peak_db: f32,
    /// Whole-file RMS, dBFS.
    pub rms_db: f32,
    /// Rough integrated loudness, LUFS. Indicative — the K-weighting is
    /// approximated by a high-pass — so it compares assets within one library
    /// and does not certify a master.
    pub loudness_lufs: f32,
    /// Peak-to-RMS ratio, dB. Small means heavily compressed.
    pub crest_db: f32,
    /// Samples sitting at full scale. On its own this is normalisation, not a
    /// fault.
    pub full_scale_samples: usize,
    /// Longest run of consecutive full-scale samples in one channel. *This* is
    /// the one that means audible distortion.
    pub longest_clip_run: usize,
    /// Silence before the first audible sample, seconds.
    pub lead_silence: f32,
    /// Silence after the last audible sample, seconds.
    pub tail_silence: f32,
    /// Mean sample value. A non-zero offset wastes headroom and can thump.
    pub dc_offset: f32,
    /// True when nothing exceeds the silence floor.
    pub silent: bool,
    /// Level difference between the first and last half-second, dB. The audio
    /// analogue of an animation's loop seam.
    pub loop_seam_db: f32,
}

impl From<&forge_audio::Metrics> for AudioMeasurement {
    fn from(m: &forge_audio::Metrics) -> Self {
        Self {
            duration: m.duration,
            sample_rate: m.sample_rate,
            channels: m.channels,
            peak: m.peak,
            peak_db: m.peak_db,
            rms_db: m.rms_db,
            loudness_lufs: m.loudness_lufs,
            crest_db: m.crest_db,
            full_scale_samples: m.full_scale_samples,
            longest_clip_run: m.longest_clip_run,
            lead_silence: m.lead_silence,
            tail_silence: m.tail_silence,
            dc_offset: m.dc_offset,
            silent: m.silent,
            loop_seam_db: m.loop_seam_db,
        }
    }
}

/// One cached measurement, with the stat that says whether it is still valid.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Entry {
    /// File size when it was measured.
    bytes: u64,
    /// Modification time when it was measured, seconds since the epoch.
    mtime_s: u64,
    /// …and the sub-second part, because a script that rewrites a file twice
    /// in a second is exactly what a generation loop does.
    mtime_ns: u32,
    /// The numbers.
    measurement: AudioMeasurement,
}

/// The on-disk shape, versioned so an older cache is dropped rather than
/// misread.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Document {
    version: u32,
    entries: BTreeMap<String, Entry>,
}

/// The audio metrics cache, `out/audio-metrics.json`.
#[derive(Debug, Clone)]
pub struct MetricsCache {
    path: PathBuf,
    entries: BTreeMap<String, Entry>,
    dirty: bool,
}

impl MetricsCache {
    /// Where the cache lives for a project.
    #[must_use]
    pub fn path_for(project: &Project) -> PathBuf {
        project.out.join("audio-metrics.json")
    }

    /// Load the cache, or start an empty one.
    ///
    /// Never fails. A missing, truncated, corrupt or older-version cache is
    /// simply an empty one — recomputing is always available, and a tool that
    /// refuses to run because its cache is bad would be worse than no cache.
    #[must_use]
    pub fn load(project: &Project) -> Self {
        let path = Self::path_for(project);
        let entries = read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Document>(&text).ok())
            .filter(|document| document.version == VERSION)
            .map(|document| document.entries)
            .unwrap_or_default();
        Self {
            path,
            entries,
            dirty: false,
        }
    }

    /// The cached measurement for a file, if it is still the same file.
    ///
    /// Keyed on `(rel_path, size, mtime)`: content hashing would be exact but
    /// means reading every byte, which is most of the cost the cache exists to
    /// avoid. Size-and-mtime is what every build system uses for the same
    /// reason, and the failure mode — a file rewritten to the same size within
    /// the same nanosecond — is not one a generation pipeline produces.
    #[must_use]
    pub fn get(&self, rel_path: &str, file: &Path) -> Option<&AudioMeasurement> {
        let entry = self.entries.get(rel_path)?;
        let (bytes, seconds, nanos) = stat(file)?;
        (entry.bytes == bytes && entry.mtime_s == seconds && entry.mtime_ns == nanos)
            .then_some(&entry.measurement)
    }

    /// Record a measurement. Silently does nothing if the file cannot be
    /// stat'd, since an entry that can never be validated would only grow the
    /// file.
    pub fn insert(&mut self, rel_path: &str, file: &Path, measurement: AudioMeasurement) {
        let Some((bytes, seconds, nanos)) = stat(file) else {
            return;
        };
        self.entries.insert(
            rel_path.to_owned(),
            Entry {
                bytes,
                mtime_s: seconds,
                mtime_ns: nanos,
                measurement,
            },
        );
        self.dirty = true;
    }

    /// Drop entries for assets that are no longer in the library, so a cache
    /// that has seen a year of sweeps does not carry a year of ghosts.
    pub fn retain(&mut self, keep: &[String]) {
        let before = self.entries.len();
        self.entries.retain(|key, _| keep.iter().any(|k| k == key));
        self.dirty |= self.entries.len() != before;
    }

    /// How many measurements are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether anything is cached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Write the cache back, if anything changed.
    ///
    /// # Errors
    ///
    /// Fails when `out/` cannot be created or written. Callers are free to
    /// ignore that: the next run just measures again.
    pub fn save(&self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let document = Document {
            version: VERSION,
            entries: self.entries.clone(),
        };
        let json =
            serde_json::to_vec_pretty(&document).map_err(|e| LibraryError::json(&self.path, e))?;
        write_atomic(&self.path, &json)
    }
}

/// Size and modification time, or `None` when the file is not there.
fn stat(file: &Path) -> Option<(u64, u64, u32)> {
    let metadata = std::fs::metadata(file).ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((metadata.len(), modified.as_secs(), modified.subsec_nanos()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(dir: &Path) -> Project {
        Project::init(dir, "cache_test").expect("init")
    }

    fn measurement() -> AudioMeasurement {
        AudioMeasurement {
            duration: 1.5,
            sample_rate: 48_000,
            channels: 2,
            peak: 0.9,
            peak_db: -0.9,
            rms_db: -18.0,
            loudness_lufs: -20.0,
            crest_db: 17.1,
            full_scale_samples: 0,
            longest_clip_run: 0,
            lead_silence: 0.0,
            tail_silence: 0.05,
            dc_offset: 0.0,
            silent: false,
            loop_seam_db: 1.2,
        }
    }

    #[test]
    fn a_rewritten_file_is_not_served_from_the_cache() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = project(dir.path());
        let file = dir.path().join("bark.wav");
        std::fs::write(&file, b"one").expect("write");

        let mut cache = MetricsCache::load(&project);
        cache.insert("audio/sfx/bark.wav", &file, measurement());
        assert!(cache.get("audio/sfx/bark.wav", &file).is_some());

        std::fs::write(&file, b"a different length entirely").expect("rewrite");
        assert!(
            cache.get("audio/sfx/bark.wav", &file).is_none(),
            "a changed file must be measured again"
        );
    }

    #[test]
    fn it_survives_a_round_trip_through_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = project(dir.path());
        let file = dir.path().join("bark.wav");
        std::fs::write(&file, b"one").expect("write");

        let mut cache = MetricsCache::load(&project);
        cache.insert("audio/sfx/bark.wav", &file, measurement());
        cache.save().expect("save");

        let reloaded = MetricsCache::load(&project);
        assert_eq!(
            reloaded.get("audio/sfx/bark.wav", &file),
            Some(&measurement())
        );
    }

    #[test]
    fn a_corrupt_cache_is_an_empty_cache() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = project(dir.path());
        let path = MetricsCache::path_for(&project);
        std::fs::create_dir_all(path.parent().expect("out")).expect("mkdir");
        std::fs::write(&path, b"{ this is not json").expect("write");
        assert!(MetricsCache::load(&project).is_empty());
    }
}
