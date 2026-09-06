//! Shared toolkit discovery for initialization and generator help.

use std::path::PathBuf;

pub(crate) fn root() -> Option<PathBuf> {
    forge_library::toolkit::root(None)
}

pub(crate) fn gen_dir() -> Option<PathBuf> {
    root()
}

pub(crate) fn profile_dir(rig: &str) -> Option<PathBuf> {
    let dir = root()?.join("rigs").join(rig);
    dir.join(forge_rig::CONTRACT_FILE).is_file().then_some(dir)
}
