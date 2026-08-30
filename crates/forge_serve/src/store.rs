//! The state directory: rows written atomically, read back, reconciled
//! after a restart, and pruned with their logs.
//!
//! Every transition rewrites the whole row through a temp sibling and a
//! `rename`, which is `records.py`'s discipline and for the same reason: a
//! reader that catches a half-written row has no way to tell it from a row
//! that says something new.
//!
//! # What a restart may and may not conclude
//!
//! A `queued` or `blocked` row **ran nothing and derived nothing**, so it is
//! re-queued in `submitted` order; the one-way rule is about a file that
//! exists, and there is no half-written `.glb` here to repair. A `running`
//! row becomes `interrupted` with `exit: null` — env and comfy alike — and
//! **never becomes `done`**. A restarted daemon cannot `waitpid` on a
//! process it did not fork and cannot read a pipe that died with its parent,
//! so it can observe neither the exit code nor the last JSON line; and
//! finishing a comfy row out of `GET /history/{prompt_id}` would drag
//! `/view` fetching, measurement and a record write into Rust, which is the
//! second record writer this design refuses.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::job::{Job, JobFilter, JobId, JobState};
use crate::{LOG_KEEP_DAYS, ServeError};

/// The endpoint file a client discovers a daemon by.
///
/// It carries the pid **and** the kernel start time (`/proc/<pid>/stat`
/// field 22), because a pid file without one eventually signals a stranger
/// — a lesson `music.py`'s resident server already paid for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Daemon {
    /// Schema, always 1.
    pub forge_serve: u32,
    /// The daemon's process id.
    pub pid: u32,
    /// `/proc/<pid>/stat` field 22, so a recycled pid is not mistaken for
    /// this one.
    pub start_ticks: u64,
    /// The loopback port the kernel picked.
    pub port: u16,
    /// The bearer token every request must carry.
    pub token: String,
    /// `http://127.0.0.1:<port>`.
    pub url: String,
    /// The daemon's build version.
    pub version: String,
    /// The project it serves, absolute.
    pub project: String,
    /// When it started.
    pub started: String,
}

/// The job table on disk.
#[derive(Debug, Clone)]
pub struct JobStore {
    dir: PathBuf,
}

impl JobStore {
    /// Open (and create) the state directory under a project root.
    ///
    /// # Errors
    ///
    /// [`ServeError::Io`] when the directories cannot be made.
    pub fn open(project_root: &Path) -> Result<Self, ServeError> {
        let dir = crate::state_dir(project_root);
        for sub in [dir.join("jobs"), dir.join("logs")] {
            std::fs::create_dir_all(&sub).map_err(|e| ServeError::io(&sub, &e))?;
        }
        Ok(Self { dir })
    }

    /// `out/serve` itself.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The row's file.
    #[must_use]
    pub fn job_path(&self, id: &JobId) -> PathBuf {
        self.dir.join("jobs").join(format!("{id}.json"))
    }

    /// The log's file.
    #[must_use]
    pub fn log_path(&self, id: &JobId) -> PathBuf {
        self.dir.join("logs").join(format!("{id}.log"))
    }

    /// The endpoint file.
    #[must_use]
    pub fn daemon_path(&self) -> PathBuf {
        self.dir.join("daemon.json")
    }

    /// The daemon's exclusive lock file.
    #[must_use]
    pub fn daemon_lock_path(&self) -> PathBuf {
        self.dir.join("daemon.lock")
    }

    /// Write a row, atomically.
    ///
    /// # Errors
    ///
    /// [`ServeError::Io`] when the temp write or the rename fails.
    pub fn write(&self, job: &Job) -> Result<(), ServeError> {
        let path = self.job_path(&job.id);
        let text = serde_json::to_string_pretty(job)
            .map_err(|e| ServeError::Io(format!("{}: {e}", path.display())))?;
        write_atomic(&path, text.as_bytes())
    }

    /// Read one row.
    ///
    /// A row that will not parse is `None` rather than an error: a file from
    /// a build that said more is not a reason to refuse the whole listing.
    ///
    /// # Errors
    ///
    /// [`ServeError::Io`] when the directory cannot be read at all.
    pub fn read(&self, id: &JobId) -> Result<Option<Job>, ServeError> {
        let path = self.job_path(id);
        match std::fs::read(&path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(ServeError::io(&path, &err)),
        }
    }

    /// Every row, newest first.
    ///
    /// # Errors
    ///
    /// [`ServeError::Io`] when `jobs/` cannot be listed.
    pub fn all(&self) -> Result<Vec<Job>, ServeError> {
        let dir = self.dir.join("jobs");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(ServeError::io(&dir, &err)),
        };
        let mut jobs: Vec<Job> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "json"))
            .filter_map(|path| std::fs::read(path).ok())
            .filter_map(|bytes| serde_json::from_slice::<Job>(&bytes).ok())
            .collect();
        // The id sorts by submission time, so this is chronological without
        // reading a clock.
        jobs.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(jobs)
    }

    /// The rows a filter passes, newest first.
    ///
    /// # Errors
    ///
    /// As [`Self::all`].
    pub fn list(&self, filter: &JobFilter) -> Result<Vec<Job>, ServeError> {
        let mut jobs: Vec<Job> = self
            .all()?
            .into_iter()
            .filter(|job| filter.matches(job))
            .collect();
        if let Some(limit) = filter.limit {
            jobs.truncate(limit);
        }
        Ok(jobs)
    }

    /// The ten most recent ids, for a refusal that names what does exist.
    ///
    /// # Errors
    ///
    /// As [`Self::all`].
    pub fn recent_ids(&self) -> Result<Vec<JobId>, ServeError> {
        Ok(self.all()?.into_iter().take(10).map(|job| job.id).collect())
    }

    /// The refusal for an id nobody has: the ids that do exist.
    #[must_use]
    pub fn no_such_job(&self, id: &JobId) -> ServeError {
        ServeError::NoSuchJob {
            id: id.clone(),
            existing: self.recent_ids().unwrap_or_default(),
        }
    }

    /// Bring the table into agreement with reality after a restart, and
    /// return the rows to re-queue, in `submitted` order.
    ///
    /// # Errors
    ///
    /// [`ServeError::Io`] when a row cannot be rewritten.
    pub fn reconcile(&self) -> Result<Vec<Job>, ServeError> {
        let mut requeue = Vec::new();
        for mut job in self.all()? {
            match job.state {
                JobState::Queued | JobState::Blocked => {
                    // Nothing ran, so nothing is derived and nothing is
                    // repaired: the row goes back in the line it was in.
                    job.state = JobState::Queued;
                    job.blocked_by = None;
                    self.write(&job)?;
                    requeue.push(job);
                }
                JobState::Running => {
                    job.finish(JobState::Interrupted, None);
                    job.message = Some(String::from(
                        "the daemon restarted while this job was running; its child was not \
                         ours to wait on, so neither its exit code nor its last line was \
                         observed. whatever it wrote is found by list_runs; re-run from the \
                         spec to finish the work.",
                    ));
                    self.write(&job)?;
                }
                _ => {}
            }
        }
        requeue.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(requeue)
    }

    /// Drop rows and their logs together once they are older than
    /// [`LOG_KEEP_DAYS`], so `jobs/` is not a directory that only grows.
    ///
    /// Returns how many rows went.
    ///
    /// # Errors
    ///
    /// [`ServeError::Io`] when `jobs/` cannot be listed.
    pub fn prune(&self) -> Result<usize, ServeError> {
        let cutoff = forge_library::clock::unix_seconds().saturating_sub(LOG_KEEP_DAYS * 86_400);
        let mut gone = 0;
        for job in self.all()? {
            if !job.state.is_terminal() {
                continue;
            }
            let stamp = job.finished.as_deref().unwrap_or(&job.submitted);
            let Some(when) = forge_library::clock::parse_stamp(stamp) else {
                continue;
            };
            if when >= cutoff {
                continue;
            }
            // Together, on purpose: a log whose row is gone is an orphaned
            // story about a job nobody can name.
            let _ = std::fs::remove_file(self.job_path(&job.id));
            let _ = std::fs::remove_file(self.log_path(&job.id));
            gone += 1;
        }
        Ok(gone)
    }

    /// Read the endpoint file, when there is one that parses.
    #[must_use]
    pub fn daemon(&self) -> Option<Daemon> {
        let bytes = std::fs::read(self.daemon_path()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Write the endpoint file, mode 0600 — it carries a bearer token that
    /// drives a GPU.
    ///
    /// # Errors
    ///
    /// [`ServeError::Io`] when the write fails.
    pub fn write_daemon(&self, daemon: &Daemon) -> Result<(), ServeError> {
        use std::os::unix::fs::PermissionsExt as _;

        let path = self.daemon_path();
        let text = serde_json::to_string_pretty(daemon)
            .map_err(|e| ServeError::Io(format!("{}: {e}", path.display())))?;
        write_atomic(&path, text.as_bytes())?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| ServeError::io(&path, &e))
    }

    /// Remove the endpoint file — on a clean stop, or when discovery finds
    /// it names a process that is not there.
    pub fn remove_daemon(&self) {
        let _ = std::fs::remove_file(self.daemon_path());
    }
}

/// Write bytes through a temp sibling and a rename.
///
/// # Errors
///
/// [`ServeError::Io`] naming the path that would not take it.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ServeError> {
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| ServeError::io(parent, &e))?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        forge_library::clock::monotonic_token()
    ));
    std::fs::write(&temp, bytes).map_err(|e| ServeError::io(&temp, &e))?;
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        ServeError::io(path, &e)
    })
}

/// Whether a pid is alive, and its kernel start time — the pair that tells
/// this process from a stranger who inherited its number.
#[must_use]
pub(crate) fn proc_start_ticks(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Field 2 is the command, in parentheses, and may itself contain
    // spaces and parentheses: everything after the LAST ')' is fields 3
    // onwards, and field 22 is the start time.
    let tail = stat.rsplit_once(')')?.1;
    tail.split_whitespace().nth(19)?.parse().ok()
}

/// This process's own start time, for the endpoint file.
#[must_use]
pub(crate) fn own_start_ticks() -> u64 {
    proc_start_ticks(std::process::id()).unwrap_or(0)
}

/// Whether a pid names a live process.
#[must_use]
pub(crate) fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{ExecutorKind, JobSpec};

    fn store() -> (tempfile::TempDir, JobStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = JobStore::open(dir.path()).expect("open");
        (dir, store)
    }

    fn row(id: &str, state: JobState) -> Job {
        let mut job = Job::admitted(
            JobId::from(id),
            &JobSpec::new("generate_audio.sfx", vec![String::from("sfx")], "cli"),
            ExecutorKind::Env,
            format!("out/serve/logs/{id}.log"),
            state,
        );
        job.state = state;
        job
    }

    #[test]
    fn a_row_survives_a_write_and_a_read() {
        let (_dir, store) = store();
        let job = row("j-20260830-141207-0001", JobState::Queued);
        store.write(&job).expect("write");
        let back = store.read(&job.id).expect("read").expect("some");
        assert_eq!(back, job);
        assert!(store.read(&JobId::from("j-nope")).expect("read").is_none());
    }

    #[test]
    fn rows_list_newest_first_and_a_filter_narrows_them() {
        let (_dir, store) = store();
        for (id, state) in [
            ("j-20260830-141207-0001", JobState::Done),
            ("j-20260830-141208-0002", JobState::Queued),
            ("j-20260830-141209-0003", JobState::Running),
        ] {
            store.write(&row(id, state)).expect("write");
        }
        let all = store.all().expect("all");
        assert_eq!(all[0].id.as_str(), "j-20260830-141209-0003");
        let queued = store
            .list(&JobFilter {
                state: Some(JobState::Queued),
                ..JobFilter::default()
            })
            .expect("list");
        assert_eq!(queued.len(), 1);
        let one = store
            .list(&JobFilter {
                limit: Some(1),
                ..JobFilter::default()
            })
            .expect("list");
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn own_start_ticks_is_readable_and_a_dead_pid_has_none() {
        assert!(own_start_ticks() > 0, "/proc must answer for this process");
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(0x00ff_ffff));
    }
}
