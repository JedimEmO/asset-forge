//! Standing up a daemon: one lock, one listener, one endpoint file.
//!
//! `daemon.lock` is held with `flock` for as long as the process lives, so a
//! second `forge serve` in the same project exits naming the first pid
//! rather than racing it for the port. The endpoint file is written after
//! the listener is bound — the kernel picks the port and the file records
//! what it picked — and removed on the way out.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use fs4::fs_std::FileExt as _;

use crate::ServeError;
use crate::http::ServeState;
use crate::store::{Daemon, JobStore, own_start_ticks};

/// A held `daemon.lock`. Dropping it (or dying) releases the daemon's claim
/// on the project.
#[derive(Debug)]
pub struct DaemonLock {
    file: std::fs::File,
    store: JobStore,
}

impl DaemonLock {
    /// Take the lock, or refuse naming whoever has it.
    ///
    /// # Errors
    ///
    /// [`ServeError::Refused`] when another daemon holds it;
    /// [`ServeError::Io`] when the file will not open.
    pub fn take(store: &JobStore) -> Result<Self, ServeError> {
        let path = store.daemon_lock_path();
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| ServeError::io(&path, &e))?;
        if !file.try_lock_exclusive().unwrap_or(false) {
            let who = store.daemon().map_or_else(
                || String::from("another forge serve"),
                |daemon| format!("pid {} on {}", daemon.pid, daemon.url),
            );
            return Err(ServeError::refused(format!(
                "a daemon is already serving this project: {who}. `forge stop` ends it, \
                 `forge serve --status` says what it is doing."
            )));
        }
        Ok(Self {
            file,
            store: store.clone(),
        })
    }
}

impl Drop for DaemonLock {
    fn drop(&mut self) {
        self.store.remove_daemon();
        let _ = fs4::fs_std::FileExt::unlock(&self.file);
    }
}

/// A token nobody can guess: 32 hex characters out of the kernel's own
/// randomness, with the clock as the fallback a test can still run on.
#[must_use]
pub fn token() -> String {
    // Sixteen bytes, read exactly: `/dev/urandom` never ends, so a
    // read-the-whole-file call on it is an out-of-memory kill with a
    // confusing name on it.
    urandom(16).map_or_else(
        || {
            format!(
                "{}{}",
                forge_library::clock::monotonic_token(),
                forge_library::clock::monotonic_token()
            )
        },
        |bytes| {
            use std::fmt::Write as _;
            bytes.iter().fold(String::new(), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            })
        },
    )
}

/// Exactly `count` bytes of the kernel's randomness.
fn urandom(count: usize) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let mut file = std::fs::File::open("/dev/urandom").ok()?;
    let mut bytes = vec![0_u8; count];
    file.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

/// Write the endpoint file for a bound listener.
///
/// # Errors
///
/// [`ServeError::Io`] when it cannot be written.
pub fn announce(
    store: &JobStore,
    project_root: &Path,
    address: SocketAddr,
    token: &str,
    version: &str,
) -> Result<Daemon, ServeError> {
    let daemon = Daemon {
        forge_serve: 1,
        pid: std::process::id(),
        start_ticks: own_start_ticks(),
        port: address.port(),
        token: token.to_owned(),
        url: format!("http://127.0.0.1:{}", address.port()),
        version: version.to_owned(),
        project: project_root.display().to_string(),
        started: forge_library::clock::now_iso(),
    };
    store.write_daemon(&daemon)?;
    Ok(daemon)
}

/// The state every handler shares, for a bound daemon.
#[must_use]
pub fn state(queue: Arc<crate::LocalQueue>, daemon: &Daemon) -> Arc<ServeState> {
    Arc::new(ServeState {
        queue,
        token: daemon.token.clone(),
        started: daemon.started.clone(),
        version: daemon.version.clone(),
    })
}

/// The `/dev/urandom` read is one line; the tests here are about the lock.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_daemon_lock_is_refused_and_names_the_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = JobStore::open(dir.path()).expect("store");
        let daemon = Daemon {
            forge_serve: 1,
            pid: std::process::id(),
            start_ticks: own_start_ticks(),
            port: 41773,
            token: token(),
            url: String::from("http://127.0.0.1:41773"),
            version: String::from("0.1.0"),
            project: dir.path().display().to_string(),
            started: forge_library::clock::now_iso(),
        };
        store.write_daemon(&daemon).expect("write");
        let first = DaemonLock::take(&store).expect("first");
        let refusal = DaemonLock::take(&store).expect_err("a second is refused");
        let text = refusal.to_string();
        assert!(
            text.contains(&format!("pid {}", std::process::id())),
            "{text}"
        );
        assert!(text.contains("forge stop"), "{text}");
        drop(first);
        assert!(
            store.daemon().is_none(),
            "the endpoint file goes with the lock"
        );
        DaemonLock::take(&store).expect("the lock is free again");
    }

    #[test]
    fn a_token_is_long_and_never_the_same_twice() {
        let a = token();
        let b = token();
        assert_eq!(a.len(), 32, "{a}");
        assert_ne!(a, b);
    }
}
