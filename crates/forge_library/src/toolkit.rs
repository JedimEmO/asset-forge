//! Find the shared local toolkit independently of the game library.

use std::path::{Path, PathBuf};

use crate::backends::{HOME_ENV, TOOLKIT_ENV};

/// Locate the toolkit's runtime resources.
///
/// An explicit `FORGE_HOME` (or legacy `FORGE_TOOLKIT`) takes precedence.
/// An invalid explicit location returns `None`, never another installation.
/// Otherwise consider the supplied project, then executable ancestors.
/// Returned paths are canonical so child processes can change directories.
#[must_use]
pub fn root(project: Option<&Path>) -> Option<PathBuf> {
    for key in [HOME_ENV, TOOLKIT_ENV] {
        if let Some(value) = std::env::var_os(key) {
            return candidate(Path::new(&value));
        }
    }
    if let Some(root) = project.and_then(candidate) {
        return Some(root);
    }
    std::env::current_exe()
        .ok()?
        .ancestors()
        .find_map(candidate)
}

fn candidate(path: &Path) -> Option<PathBuf> {
    path.join("python/forge_gen")
        .is_dir()
        .then(|| path.canonicalize().ok())
        .flatten()
}
