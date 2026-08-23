//! The `forge audio` subcommand as library functions.
//!
//! ```sh
//! forge audio assets/audio/sfx/pistol_shot.wav --out out/shot.png
//! forge audio --list assets/audio
//! ```
//!
//! The binary owns the flags and the exit code; this module owns everything
//! between: decode, measure, draw, and say what was found in words a human
//! reads and an agent greps. A [`Report`] answers [`Report::is_defective`] so
//! the caller can exit non-zero when a file is silent, clipped, or will not
//! decode — the command gates in CI rather than only informing.

use std::path::{Path, PathBuf};

use forge_raster::SaveError;

use crate::decode::{DecodeError, decode};
use crate::metrics::{Metrics, measure};
use crate::plot::{self, PlotLayout};

/// Consecutive full-scale samples before a file is defective rather than
/// merely normalised. Mirrors the threshold the warning uses: a run of three
/// is flat-topping, one or two is what peak normalisation looks like.
const DEFECT_CLIP_RUN: usize = 3;

/// Why an inspection could not be completed.
#[derive(Debug)]
pub enum AudioError {
    /// The file would not decode.
    Decode {
        /// The file that failed.
        path: PathBuf,
        /// Why.
        source: DecodeError,
    },
    /// The plot could not be written.
    Plot {
        /// The PNG that was being written.
        path: PathBuf,
        /// Why.
        source: SaveError,
    },
    /// A directory for the plot could not be created.
    Io {
        /// The path that was being created.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// `list` found nothing to measure.
    NoAudio(PathBuf),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Plot { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::NoAudio(dir) => write!(f, "no audio found under {}", dir.display()),
        }
    }
}

impl std::error::Error for AudioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Decode { source, .. } => Some(source),
            Self::Io { source, .. } => Some(source),
            Self::Plot { .. } | Self::NoAudio(_) => None,
        }
    }
}

/// What measuring one file produced.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// The file decoded and was measured.
    Measured {
        /// The numbers.
        metrics: Metrics,
        /// [`Metrics::warnings`], computed once so the summary and the caller
        /// agree.
        warnings: Vec<String>,
    },
    /// The file would not decode. Only [`list`] produces this — one bad file
    /// in a library should not hide the rest of the table — whereas
    /// [`inspect`] of a single file returns [`AudioError::Decode`] instead.
    Undecodable(DecodeError),
}

/// Everything the command has to say about one file.
#[derive(Debug, Clone)]
pub struct Report {
    /// The file that was inspected.
    pub path: PathBuf,
    /// Measurements, or the reason there are none.
    pub outcome: Outcome,
    /// The plot that was written, if one was asked for.
    pub plot: Option<PathBuf>,
    /// Human-readable text: the multi-line block for [`inspect`], the one-line
    /// row for [`list`]. Printed verbatim by the binary.
    pub summary: String,
}

impl Report {
    /// The measurements, when the file decoded.
    #[must_use]
    pub fn metrics(&self) -> Option<&Metrics> {
        match &self.outcome {
            Outcome::Measured { metrics, .. } => Some(metrics),
            Outcome::Undecodable(_) => None,
        }
    }

    /// Problems worth a human's attention, most serious first. Empty when the
    /// file did not decode; [`Self::is_defective`] carries that case.
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        match &self.outcome {
            Outcome::Measured { warnings, .. } => warnings,
            Outcome::Undecodable(_) => &[],
        }
    }

    /// Silent, clipped, or undecodable.
    ///
    /// Silence and clipping are defects, not observations: a build that ships
    /// either has a real problem, so the gate has to fail. Everything else
    /// [`Metrics::warnings`] reports is worth a look, not a red build.
    #[must_use]
    pub fn is_defective(&self) -> bool {
        match &self.outcome {
            Outcome::Measured { metrics, .. } => {
                metrics.silent || metrics.longest_clip_run >= DEFECT_CLIP_RUN
            }
            Outcome::Undecodable(_) => true,
        }
    }

    /// One character for a table column: `!` defect, `?` worth a look, space
    /// for clean.
    #[must_use]
    pub fn flag(&self) -> char {
        if self.is_defective() {
            '!'
        } else if self.warnings().is_empty() {
            ' '
        } else {
            '?'
        }
    }

    /// The one-line table row, labelled with `label` (the path relative to
    /// the directory that was listed).
    #[must_use]
    pub fn row(&self, label: &str) -> String {
        match &self.outcome {
            Outcome::Measured { metrics: m, .. } => format!(
                "{} {label:34} {:>7.2}s {:>6} Hz {}ch  peak {:>6.1}  lufs {:>6.1}",
                self.flag(),
                m.duration,
                m.sample_rate,
                m.channels,
                m.peak_db,
                m.loudness_lufs
            ),
            Outcome::Undecodable(err) => format!("! {label:34} {err}"),
        }
    }
}

/// Measure one file, optionally draw it, and say what was found.
///
/// `out` is where the plot PNG goes; its parent directory is created. The
/// returned [`Report::summary`] is the block the binary prints, and
/// [`Report::is_defective`] is its exit code.
///
/// # Errors
///
/// [`AudioError::Decode`] when the file will not decode, [`AudioError::Io`]
/// or [`AudioError::Plot`] when the plot cannot be written.
pub fn inspect(file: &Path, out: Option<&Path>) -> Result<Report, AudioError> {
    inspect_with(file, out, &PlotLayout::default())
}

/// [`inspect`] with an explicit plot layout (the binary's `--width`).
///
/// # Errors
///
/// As [`inspect`].
pub fn inspect_with(
    file: &Path,
    out: Option<&Path>,
    layout: &PlotLayout,
) -> Result<Report, AudioError> {
    let audio = decode(file).map_err(|source| AudioError::Decode {
        path: file.to_path_buf(),
        source,
    })?;
    let metrics = measure(&audio);
    let warnings = metrics.warnings();

    let mut lines = vec![
        format!("file:     {}", file.display()),
        format!(
            "format:   {:.3}s  {} Hz  {} ch",
            metrics.duration, metrics.sample_rate, metrics.channels
        ),
        format!(
            "level:    peak {:.1} dBFS   rms {:.1} dBFS   lufs {:.1}   crest {:.1} dB",
            metrics.peak_db, metrics.rms_db, metrics.loudness_lufs, metrics.crest_db
        ),
        format!(
            "shape:    lead silence {:.0} ms   tail {:.0} ms   dc {:+.4}   full-scale {} (run {})",
            metrics.lead_silence * 1000.0,
            metrics.tail_silence * 1000.0,
            metrics.dc_offset,
            metrics.full_scale_samples,
            metrics.longest_clip_run
        ),
    ];
    if warnings.is_empty() {
        lines.push("verdict:  clean".to_owned());
    } else {
        for warning in &warnings {
            lines.push(format!("warning:  {warning}"));
        }
    }

    let mut plot = None;
    if let Some(out) = out {
        if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|source| AudioError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let title = file
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let canvas = plot::render(&audio, &metrics, &title, layout);
        canvas.save_png(out).map_err(|source| AudioError::Plot {
            path: out.to_path_buf(),
            source,
        })?;
        lines.push(format!(
            "plot:     {} ({}x{})",
            out.display(),
            canvas.width(),
            canvas.height()
        ));
        plot = Some(out.to_path_buf());
    }

    Ok(Report {
        path: file.to_path_buf(),
        outcome: Outcome::Measured { metrics, warnings },
        plot,
        summary: lines.join("\n"),
    })
}

/// Measure every audio file under `dir` (recursively: wav, ogg/oga, mp3,
/// flac), sorted by path.
///
/// Each entry is the path relative to `dir` and its report; a file that will
/// not decode is an [`Outcome::Undecodable`] row rather than an error, so one
/// bad file does not hide the table. [`list_table`] renders the result.
///
/// # Errors
///
/// [`AudioError::NoAudio`] when nothing under `dir` has an audio extension.
pub fn list(dir: &Path) -> Result<Vec<(PathBuf, Report)>, AudioError> {
    let entries = crate::discover(dir);
    if entries.is_empty() {
        return Err(AudioError::NoAudio(dir.to_path_buf()));
    }
    Ok(entries
        .into_iter()
        .map(|entry| {
            let outcome = match decode(&entry.path) {
                Ok(audio) => {
                    let metrics = measure(&audio);
                    let warnings = metrics.warnings();
                    Outcome::Measured { metrics, warnings }
                }
                Err(err) => Outcome::Undecodable(err),
            };
            let mut report = Report {
                path: entry.path,
                outcome,
                plot: None,
                summary: String::new(),
            };
            report.summary = report.row(&entry.rel_path);
            (PathBuf::from(entry.rel_path), report)
        })
        .collect())
}

/// The rows of a [`list`] plus the legend line, ready to print.
#[must_use]
pub fn list_table(reports: &[(PathBuf, Report)]) -> String {
    let rows: Vec<&str> = reports.iter().map(|(_, r)| r.summary.as_str()).collect();
    format!(
        "{}\n\n{} file(s)   ! = defect, ? = worth a look",
        rows.join("\n"),
        reports.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16-bit mono PCM WAV, written by hand so the tests depend on nothing
    /// but the decoder under test.
    fn write_wav(path: &Path, samples: &[f32], rate: u32) {
        let data: Vec<u8> = samples
            .iter()
            .flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
            .collect();
        let mut bytes = Vec::with_capacity(44 + data.len());
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&data);
        std::fs::write(path, bytes).expect("write wav");
    }

    /// A decaying tone: loud enough, dynamic enough and prompt enough that
    /// nothing in `Metrics::warnings` fires. A plain sine would warn about its
    /// 3 dB crest factor.
    fn pluck(rate: u32) -> Vec<f32> {
        let n = rate as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                0.8 * (-4.0 * t).exp() * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
            })
            .collect()
    }

    #[test]
    fn a_clean_file_is_not_defective_and_its_plot_has_the_layout_size() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wav = dir.path().join("pluck.wav");
        write_wav(&wav, &pluck(48_000), 48_000);
        // The plot goes under a directory that does not exist yet.
        let png = dir.path().join("plots").join("pluck.png");
        let layout = PlotLayout {
            width: 320,
            ..PlotLayout::default()
        };

        let report = inspect_with(&wav, Some(&png), &layout).expect("inspect");
        assert!(!report.is_defective(), "{}", report.summary);
        assert!(
            report.warnings().is_empty(),
            "warnings: {:?}",
            report.warnings()
        );
        assert_eq!(report.flag(), ' ');
        assert!(
            report.summary.contains("verdict:  clean"),
            "{}",
            report.summary
        );
        assert!(report.summary.contains("plot:     "), "{}", report.summary);
        assert_eq!(report.plot.as_deref(), Some(png.as_path()));

        let image = image::open(&png).expect("read plot back");
        assert_eq!(image.width(), 320);
        assert_eq!(image.height(), layout.total_height());
    }

    #[test]
    fn a_silent_file_is_defective() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wav = dir.path().join("nothing.wav");
        write_wav(&wav, &vec![0.0; 48_000], 48_000);

        let report = inspect(&wav, None).expect("inspect");
        assert!(report.is_defective());
        assert_eq!(report.flag(), '!');
        assert_eq!(report.warnings(), ["file is silent".to_owned()]);
        assert!(report.summary.contains("warning:  file is silent"));
        assert!(report.plot.is_none());
    }

    #[test]
    fn a_clipped_file_is_defective_but_a_quiet_one_only_warns() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rate = 48_000;
        let clipped: Vec<f32> = pluck(rate).iter().map(|s| s * 4.0).collect();
        let clipped_wav = dir.path().join("clipped.wav");
        write_wav(&clipped_wav, &clipped, rate);
        let quiet: Vec<f32> = pluck(rate).iter().map(|s| s * 0.05).collect();
        let quiet_wav = dir.path().join("quiet.wav");
        write_wav(&quiet_wav, &quiet, rate);

        let clipped = inspect(&clipped_wav, None).expect("inspect");
        assert!(clipped.is_defective());
        assert!(clipped.warnings().iter().any(|w| w.contains("clipped")));

        let quiet = inspect(&quiet_wav, None).expect("inspect");
        assert!(!quiet.is_defective(), "{}", quiet.summary);
        assert_eq!(quiet.flag(), '?');
        assert!(quiet.warnings().iter().any(|w| w.contains("very quiet")));
    }

    #[test]
    fn a_file_that_will_not_decode_is_an_error_from_inspect() {
        let dir = tempfile::tempdir().expect("tempdir");
        let not_audio = dir.path().join("not_audio.wav");
        std::fs::write(&not_audio, b"this is not a wav file at all").expect("write");

        let err = match inspect(&not_audio, None) {
            Ok(report) => panic!("decoded garbage: {}", report.summary),
            Err(err) => err,
        };
        assert!(
            matches!(&err, AudioError::Decode { path, .. } if path == &not_audio),
            "{err}"
        );
        assert!(err.to_string().contains("not_audio.wav"), "{err}");
    }

    #[test]
    fn list_walks_recursively_and_keeps_a_bad_file_as_a_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sfx = dir.path().join("sfx");
        std::fs::create_dir_all(&sfx).expect("mkdir");
        write_wav(&sfx.join("pluck.wav"), &pluck(48_000), 48_000);
        write_wav(&dir.path().join("nothing.wav"), &vec![0.0; 4800], 48_000);
        std::fs::write(sfx.join("broken.wav"), b"nope").expect("write");
        // Not audio by extension: never listed.
        std::fs::write(dir.path().join("notes.txt"), b"ignored").expect("write");

        let reports = list(dir.path()).expect("list");
        let labels: Vec<&str> = reports
            .iter()
            .map(|(p, _)| p.to_str().unwrap_or(""))
            .collect();
        assert_eq!(labels, ["nothing.wav", "sfx/broken.wav", "sfx/pluck.wav"]);

        let flags: Vec<char> = reports.iter().map(|(_, r)| r.flag()).collect();
        assert_eq!(flags, ['!', '!', ' ']);
        assert!(matches!(reports[1].1.outcome, Outcome::Undecodable(_)));
        assert!(reports[1].1.is_defective());
        assert!(reports[1].1.summary.starts_with("! sfx/broken.wav"));
        assert!(reports[2].1.summary.starts_with("  sfx/pluck.wav"));

        let table = list_table(&reports);
        assert!(table.ends_with("3 file(s)   ! = defect, ? = worth a look"));
        assert_eq!(table.lines().count(), 5, "{table}");
    }

    #[test]
    fn list_of_nothing_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("readme.md"), b"no audio here").expect("write");
        let err = match list(dir.path()) {
            Ok(reports) => panic!("listed {} file(s) from nothing", reports.len()),
            Err(err) => err,
        };
        assert!(matches!(err, AudioError::NoAudio(_)), "{err}");
    }
}
