//! Finding animation clips on disk, and the typed record beside each one.
//!
//! No parsing happens here, on purpose: [`forge_library`] is the one reader
//! of the sidecar format, so a schema it does not know is an error there
//! rather than gibberish here, and a nested object is its business. What
//! this module owns is the *walk* — which files exist, and what Bevy's
//! asset server should be asked to load — plus the handful of derived numbers
//! a browser row shows.

use std::path::{Path, PathBuf};

use forge_library::{
    AssetRecord, Catalog, Project, Query, Sidecar,
    schema::{ClipRecipe, DEFAULT_FPS, Kind},
    sidecar,
};

/// One animation clip file found under the clip directory.
#[derive(Debug, Clone)]
pub struct ClipEntry {
    /// Path relative to the asset root, as Bevy's asset server wants it.
    pub asset_path: String,
    /// File stem, for display.
    pub name: String,
    /// The record beside it, when there is one that parses.
    ///
    /// `None` covers both "no sidecar" and "a sidecar nobody can read". The
    /// second is a finding rather than a normal state, so [`discover`] logs it
    /// with the file's name; the panel can only say that nothing is recorded.
    pub sidecar: Option<Sidecar>,
}

impl ClipEntry {
    /// A record with nothing known about it, for a clip made this session.
    #[must_use]
    pub fn bare(asset_path: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            asset_path: asset_path.into(),
            name: name.into(),
            sidecar: None,
        }
    }

    /// The entry for a clip the library catalog found.
    ///
    /// An unreadable sidecar is reported here, not swallowed: a clip that
    /// silently loses its provenance looks exactly like one that never had
    /// any.
    #[must_use]
    pub fn from_record(record: &AssetRecord) -> Self {
        if let Some(err) = &record.sidecar_error {
            bevy::log::warn!("{}: {err}", record.name);
        }
        Self {
            asset_path: record.rel_path.clone(),
            name: record.name.clone(),
            sidecar: record.sidecar.clone(),
        }
    }

    /// The same record with a prompt recorded against it.
    ///
    /// What a freshly generated take knows about itself: ARDY was told a
    /// sentence and produced motion, and nothing else about the clip exists
    /// yet. Recording it as a real [`Sidecar`] rather than as a loose string
    /// means the metadata panel needs no special case for a take.
    #[must_use]
    pub fn with_prompt(mut self, prompt: &str) -> Self {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return self;
        }
        let mut sidecar = Sidecar::new(Kind::Clip, self.name.clone());
        sidecar.prompt = Some(prompt.to_owned());
        self.sidecar = Some(sidecar);
        self
    }

    /// The prompt that produced the clip, when one was recorded.
    #[must_use]
    pub fn prompt(&self) -> Option<&str> {
        self.sidecar.as_ref()?.prompt.as_deref()
    }

    /// The edit recipe the sidecar records, when it has one.
    #[must_use]
    pub fn recipe(&self) -> Option<&ClipRecipe> {
        self.sidecar.as_ref()?.recipe.as_ref()
    }

    /// Frames per second the recipe's trims are measured against.
    ///
    /// The recipe stores trims in seconds and [`forge_motion::Edit`] wants
    /// frames, so a record that lost its `fps` still needs an answer; ARDY only
    /// ever emits 20.
    #[must_use]
    pub fn fps(&self) -> f32 {
        self.sidecar
            .as_ref()
            .and_then(|s| s.measured.as_ref())
            .and_then(|m| m.fps)
            .filter(|fps| *fps > 0.0)
            .unwrap_or(DEFAULT_FPS)
    }

    /// Seconds of built clip, measured or derived from the frame count.
    ///
    /// Early records recorded `frames` and `fps` but never a duration, so
    /// deriving it is what let those clips show a length in the browser at
    /// all.
    #[must_use]
    pub fn duration_s(&self) -> Option<f32> {
        let measured = self.sidecar.as_ref()?.measured.as_ref()?;
        measured.duration_s.or_else(|| {
            let frames = measured.frames? as f32;
            let fps = measured.fps.filter(|f| *f > 0.0)?;
            Some(frames / fps)
        })
    }

    /// Mean root speed over the clip, metres per second.
    #[must_use]
    pub fn avg_speed_mps(&self) -> Option<f32> {
        self.sidecar.as_ref()?.measured.as_ref()?.avg_speed_mps
    }
}

/// Every clip the project's library holds, in catalog order.
#[must_use]
pub fn discover(project: &Project) -> Vec<ClipEntry> {
    Catalog::scan(project)
        .find(&Query::of_kind(Kind::Clip))
        .into_iter()
        .map(ClipEntry::from_record)
        .collect()
}

/// Every clip under `asset_root/clip_dir`, sorted by name — for a directory
/// that is not a library, such as a sweep's output under `out/`.
#[must_use]
pub fn discover_dir(asset_root: &Path, clip_dir: &str) -> Vec<ClipEntry> {
    let root = asset_root.join(clip_dir);
    let mut found: Vec<PathBuf> = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "glb" || e == "gltf") {
                found.push(path);
            }
        }
    }
    found.sort();

    found
        .into_iter()
        .filter_map(|path| {
            let asset_path = path
                .strip_prefix(asset_root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            let name = path.file_stem()?.to_string_lossy().into_owned();
            // An unreadable sidecar is reported by the panel, not swallowed
            // here: a clip that silently loses its provenance looks exactly
            // like one that never had any.
            let sidecar = sidecar::load_beside(&path).unwrap_or_else(|err| {
                bevy::log::warn!("{name}: {err}");
                None
            });
            Some(ClipEntry {
                asset_path,
                name,
                sidecar,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use forge_library::schema::Measured;

    use super::*;

    fn entry_with(measured: Measured) -> ClipEntry {
        let mut sidecar = Sidecar::new(Kind::Clip, "walk");
        sidecar.measured = Some(measured);
        ClipEntry {
            asset_path: String::from("clips/walk.glb"),
            name: String::from("walk"),
            sidecar: Some(sidecar),
        }
    }

    #[test]
    fn a_duration_is_derived_when_only_frames_and_fps_were_recorded() {
        let entry = entry_with(Measured {
            frames: Some(30),
            fps: Some(20.0),
            ..Measured::default()
        });
        assert!((entry.duration_s().expect("derived") - 1.5).abs() < 1e-6);
        assert!((entry.fps() - 20.0).abs() < f32::EPSILON);
        assert!(entry.avg_speed_mps().is_none());
    }

    #[test]
    fn a_missing_or_zero_fps_falls_back_to_the_generator_rate() {
        let bare = ClipEntry::bare("clips/x.glb", "x");
        assert!((bare.fps() - DEFAULT_FPS).abs() < f32::EPSILON);
        assert!(bare.duration_s().is_none());
        let zero = entry_with(Measured {
            frames: Some(10),
            fps: Some(0.0),
            ..Measured::default()
        });
        assert!((zero.fps() - DEFAULT_FPS).abs() < f32::EPSILON);
        assert!(zero.duration_s().is_none());
    }

    #[test]
    fn a_prompt_makes_a_real_record_and_blank_makes_none() {
        let entry = ClipEntry::bare("out/sweeps/a.glb", "a").with_prompt("  a person waves  ");
        assert_eq!(entry.prompt(), Some("a person waves"));
        assert!(entry.recipe().is_none());
        let blank = ClipEntry::bare("out/sweeps/a.glb", "a").with_prompt("   ");
        assert!(blank.sidecar.is_none());
    }

    #[test]
    fn a_directory_walk_finds_clips_and_reads_what_is_beside_them() {
        let dir = tempfile::tempdir().expect("tempdir");
        let clips = dir.path().join("clips/deep");
        std::fs::create_dir_all(&clips).expect("mkdir");
        std::fs::write(clips.join("b.glb"), b"glb").expect("write");
        std::fs::write(clips.join("a.glb"), b"glb").expect("write");
        let mut sidecar = Sidecar::new(Kind::Clip, "a");
        sidecar.prompt = Some(String::from("a person rolls"));
        sidecar::save(&clips.join("a.json"), &sidecar).expect("sidecar");
        std::fs::write(clips.join("c.txt"), b"not a clip").expect("write");

        let entries = discover_dir(dir.path(), "clips");
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(entries[0].asset_path, "clips/deep/a.glb");
        assert_eq!(entries[0].prompt(), Some("a person rolls"));
        assert!(entries[1].sidecar.is_none());
    }
}
