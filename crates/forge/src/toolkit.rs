//! Where the toolkit checkout is, for the one thing the binary needs from it
//! that a user project does not carry: the rig profiles it ships.
//!
//! `forge init` copies `rigs/<profile>` into the new project, so it has to
//! find the checkout the binary came from. No absolute path lives in source:
//! the lookup is `$FORGE_TOOLKIT`, then the ancestors of the running
//! executable — a checkout's `target/debug/forge` is two levels under its
//! root, a test binary three — stopping at the first directory that holds
//! both `forge.toml` and `rigs/`.

use std::path::{Path, PathBuf};

use forge_library::backends::TOOLKIT_ENV;
use forge_library::project::PROJECT_FILE;

/// The toolkit root, when one can be found.
pub(crate) fn root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(TOOLKIT_ENV) {
        return Some(PathBuf::from(dir));
    }
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .find(|dir| is_toolkit(dir))
        .map(Path::to_path_buf)
}

/// A directory is the toolkit when it is a forge project that ships rig
/// profiles.
fn is_toolkit(dir: &Path) -> bool {
    dir.join(PROJECT_FILE).is_file() && dir.join("rigs").is_dir()
}

/// The directory holding `python/forge_gen`, found without a project: the
/// same `$FORGE_HOME`/`$FORGE_TOOLKIT`-then-executable-ancestors walk the
/// launcher does, minus the project root it does not have.
///
/// Exists for exactly one caller: `forge gen <cmd> --help` outside a
/// project, where refusing to print help because there is no forge.toml
/// would cost an agent a directory change just to read a flag table.
pub(crate) fn gen_dir() -> Option<PathBuf> {
    let holds_gen = |dir: &Path| dir.join("python").join("forge_gen").is_dir();
    for key in [
        forge_library::backends::HOME_ENV,
        forge_library::backends::TOOLKIT_ENV,
    ] {
        if let Some(dir) = std::env::var_os(key) {
            let dir = PathBuf::from(dir);
            if holds_gen(&dir) {
                return Some(dir);
            }
        }
    }
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .find(|dir| holds_gen(dir))
        .map(Path::to_path_buf)
}

/// The shipped profile directory for `rig`, when the toolkit can be found
/// and ships one by that name.
pub(crate) fn profile_dir(rig: &str) -> Option<PathBuf> {
    let dir = root()?.join("rigs").join(rig);
    dir.join(forge_rig::CONTRACT_FILE).is_file().then_some(dir)
}
