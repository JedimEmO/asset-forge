//! One module per verb. Each takes the parsed arguments and a project, calls
//! the library crate that owns the work, prints, and returns an [`Outcome`].
//!
//! [`Outcome`]: crate::outcome::Outcome

pub(crate) mod audio;
pub(crate) mod catalog;
pub(crate) mod checks;
pub(crate) mod doctor;
pub(crate) mod generate;
pub(crate) mod gpu;
pub(crate) mod init;
pub(crate) mod manifest;
pub(crate) mod promote;
pub(crate) mod rig;

/// One line of at most `width` characters, for a table cell.
pub(crate) fn first_line(text: &str, width: usize) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    if line.chars().count() <= width {
        return line.to_owned();
    }
    let kept: String = line.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}
