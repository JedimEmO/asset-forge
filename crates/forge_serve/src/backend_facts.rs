//! The three things the scheduler reads out of a `backend.toml`, and
//! nothing else.
//!
//! `executor` (`env` | `comfy` | `tool`), `host` (which `backends/<name>` is
//! the service) and, for a host, the `[server]` block that says where it
//! listens and which systemd unit restarts it. `vram_gb` comes from
//! `forge_library::backends::Backend`, which already reads it.
//!
//! **This is a stopgap with a date on it.** `backend.toml`'s parser is
//! `forge_library::backends` (and `python/forge_gen/backends.py`), one
//! contract with one owner; the moment `Backend` carries `executor` and
//! `host`, this module's first two reads delete themselves and the queue
//! asks the table. Until then it reads three scalars and derives nothing
//! else — no models, no packs, no env, no licence — so there is no second
//! opinion about a backend for anything but where a job runs.

use std::path::Path;

use serde::Deserialize;

use crate::job::ExecutorKind;

/// The `[server]` block of a host backend.
#[derive(Debug, Clone, Default, Deserialize)]
struct ServerToml {
    host: Option<String>,
    port: Option<u16>,
    unit: Option<String>,
}

/// The keys this module reads.
#[derive(Debug, Clone, Default, Deserialize)]
struct BackendToml {
    executor: Option<String>,
    host: Option<String>,
    #[serde(default)]
    server: Option<ServerToml>,
}

/// What a backend says about where its jobs run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackendFacts {
    /// `env` or `comfy`; a `tool` never reaches the queue and reads as
    /// `env` here, because nothing schedules Blender.
    pub(crate) executor: ExecutorKind,
    /// The `backends/<name>` that is the service, for a comfy backend.
    pub(crate) host: Option<String>,
    /// `http://127.0.0.1:8188`, when this backend is the host.
    pub(crate) url: Option<String>,
    /// The systemd unit the card ladder may restart.
    pub(crate) unit: Option<String>,
}

impl Default for BackendFacts {
    fn default() -> Self {
        Self {
            executor: ExecutorKind::Env,
            host: None,
            url: None,
            unit: None,
        }
    }
}

impl BackendFacts {
    /// Read `<dir>/backend.toml`.
    ///
    /// A file that is not there or does not parse reads as `env`: every
    /// `backend.toml` written before this phase named no executor and ran
    /// through the launcher, and that is what the derive rule says.
    #[must_use]
    pub(crate) fn read(dir: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(dir.join("backend.toml")) else {
            return Self::default();
        };
        let Ok(parsed) = toml::from_str::<BackendToml>(&text) else {
            return Self::default();
        };
        let executor = match parsed.executor.as_deref() {
            Some("comfy") => ExecutorKind::Comfy,
            // `tool` is a host program a module execs; it is not a job.
            _ => ExecutorKind::Env,
        };
        let server = parsed.server.unwrap_or_default();
        let url = match (server.host.as_deref(), server.port) {
            (Some(host), Some(port)) => Some(format!("http://{host}:{port}")),
            _ => None,
        };
        Self {
            executor,
            host: parsed.host,
            url,
            unit: server.unit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, text: &str) {
        std::fs::create_dir_all(dir).expect("mkdir");
        std::fs::write(dir.join("backend.toml"), text).expect("write");
    }

    #[test]
    fn a_file_with_no_executor_reads_as_env() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), "name = \"ardy\"\nentry = \"motion\"\n");
        let facts = BackendFacts::read(dir.path());
        assert_eq!(facts.executor, ExecutorKind::Env);
        assert_eq!(facts.host, None);
        assert_eq!(
            BackendFacts::read(Path::new("/nope")).executor,
            ExecutorKind::Env
        );
    }

    #[test]
    fn a_comfy_backend_names_its_host_and_a_host_names_its_unit() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(
            dir.path(),
            "name = \"moss_sfx\"\nexecutor = \"comfy\"\nhost = \"comfy\"\nvram_gb = 8\n",
        );
        let facts = BackendFacts::read(dir.path());
        assert_eq!(facts.executor, ExecutorKind::Comfy);
        assert_eq!(facts.host.as_deref(), Some("comfy"));
        let host = tempfile::tempdir().expect("tempdir");
        write(
            host.path(),
            "name = \"comfy\"\nexecutor = \"tool\"\n\n[server]\nhost = \"127.0.0.1\"\nport = 8188\n\
             unit = \"forge-comfy.service\"\n",
        );
        let facts = BackendFacts::read(host.path());
        assert_eq!(facts.url.as_deref(), Some("http://127.0.0.1:8188"));
        assert_eq!(facts.unit.as_deref(), Some("forge-comfy.service"));
    }
}
