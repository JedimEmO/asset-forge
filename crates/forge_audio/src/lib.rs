//! Decode game audio, measure it, and render it as something you can look at.
//!
//! The animation half of this toolkit turns motion into contact sheets because
//! an agent cannot watch an animation. This is the same idea for sound: an
//! agent cannot listen, so audio becomes a waveform, a spectrogram and a set of
//! numbers that fail loudly.
//!
//! ```no_run
//! let audio = forge_audio::decode("assets/audio/sfx/pistol_shot.wav")?;
//! let metrics = forge_audio::measure(&audio);
//! for warning in metrics.warnings() {
//!     eprintln!("{warning}");
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Deliberately no audio *output* backend: analysis has to work in CI and on
//! machines with no sound card. Hearing a clip is the studio's job.
//!
//! # What the numbers mean
//!
//! [`cli::list`] measures a library — where a bark 20 LU below its neighbours
//! becomes obvious in a column and is invisible per-file — and [`cli::inspect`]
//! draws one file, because clipping reads as flat-topping, dead air as a gap,
//! and a truncated tail as a cliff.
//!
//! Two measurement notes worth knowing, both learned the hard way. **Clipping
//! is a run, not a count**: peak-normalising to 0 dBFS puts a sample at the
//! rail by construction, so counting full-scale samples flagged five of eight
//! shipped SFX as broken. Only sustained flat-topping warns. And **loudness is
//! approximate and says so** — K-weighting is a high-pass stand-in, which is
//! enough to compare assets in one library and not enough to certify a master.

pub mod cli;
pub mod decode;
pub mod metrics;
pub mod plot;

pub use cli::{AudioError, Report, inspect, list};
pub use decode::{Audio, DecodeError, decode};
pub use metrics::{Metrics, measure, to_db};
pub use plot::{PlotLayout, render};

use std::path::{Path, PathBuf};

/// One audio file found under an asset directory.
///
/// Deliberately only where the file is. This used to carry the contents of its
/// `<stem>.json` sidecar as flattened key/value pairs, read by a line parser
/// that split on the first colon and could not see inside a nested object — so
/// the `generator` and `recipe` blocks arrived as the literal strings `{`
/// and were dropped. Sidecars are the library's job, and a crate that decodes
/// and measures audio has no business having an opinion about them.
#[derive(Debug, Clone)]
pub struct AudioEntry {
    /// Path relative to the search root.
    pub rel_path: String,
    /// File stem, for display.
    pub name: String,
    /// Absolute path on disk.
    pub path: PathBuf,
}

/// Extensions treated as audio.
const AUDIO_EXTENSIONS: [&str; 5] = ["wav", "ogg", "mp3", "flac", "oga"];

/// Every audio file under `root`, sorted by path.
#[must_use]
pub fn discover(root: &Path) -> Vec<AudioEntry> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
        .into_iter()
        .filter_map(|path| {
            let rel = path
                .strip_prefix(root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            let name = path.file_stem()?.to_string_lossy().into_owned();
            Some(AudioEntry {
                rel_path: rel,
                name,
                path,
            })
        })
        .collect()
}
