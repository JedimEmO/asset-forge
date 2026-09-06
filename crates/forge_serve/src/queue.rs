//! `LocalQueue`: one FIFO, one worker, concurrency 1, not configurable.
//!
//! A second worker would only queue behind the card lease, at the cost of a
//! second place for state to disagree. What the single worker is *not* is
//! the card lock: being singular holds nothing against a second terminal,
//! which is why every job — the fake ones included — takes
//! [`CardLease`](crate::CardLease).
//!
//! # Admission
//!
//! Two things happen before a row reaches `queued`, both cheap and both
//! before the card:
//!
//! 1. **Backend resolution**, when the caller named one. A backend that is
//!    not installed is a `refused` row with `exit: 3` written in under a
//!    millisecond, so the queue never holds a job that cannot run.
//! 2. **The out-path lease.** A submit whose `outputs_claimed` path is
//!    already claimed by a `queued`, `blocked` or `running` row is refused
//!    naming that row's id. Two doors asking for `out/audio/sfx/door.wav` a
//!    second apart is the ordinary case here, not the exotic one.
//!
//! **A fake job is an ordinary job.** `FORGE_FAKE=1` and tier `fake` change
//! nothing about admission, the FIFO or the lease — letting fakes skip the
//! queue would quietly lose `ci-fake` the serialisation it has today.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use forge_library::Project;
use forge_library::backends::Backends;
use serde_json::Value;

use crate::backend_facts::BackendFacts;
use crate::card::{CardLease, CardReader, CardState};
use crate::executor::{CancelToken, ExecutorSet, Launch, Plan, terminate_group};
use crate::job::{
    CardFacts, ExecutorKind, Job, JobFilter, JobId, JobSpec, JobState, LogChunk, QueueCounts, Run,
    RunFilter, Status, StatusJob,
};
use crate::logs::{self, LogSink};
use crate::store::JobStore;
use crate::{Queue, ServeError};

/// How long a blocked job waits before it looks at the card again.
const BLOCKED_RECHECK: Duration = Duration::from_secs(2);

/// How often [`Queue::wait`] looks at the row.
const WAIT_POLL: Duration = Duration::from_millis(200);

/// How long a cancel waits for the worker to write the terminal row.
const CANCEL_GRACE: Duration = Duration::from_secs(12);

/// How long a stop waits for the child it cancelled to be gone.
///
/// Two seconds longer than the SIGTERM-to-SIGKILL grace, so the ordinary
/// path — the generator takes the signal and the worker writes the row —
/// always finishes inside it.
const STOP_GRACE: Duration = Duration::from_secs(12);

/// How many log lines a status frame carries for a running job.
const STATUS_TAIL: usize = 8;

/// What a queue needs that the project does not say.
#[derive(Debug, Clone)]
pub struct LocalQueueOptions {
    /// Shared GPU lock and recovery state. None isolates low-level test queues.
    pub card_state_dir: Option<PathBuf>,
    /// This binary, re-invoked as `forge gpu --json` to read the card.
    /// Never a second `nvidia-smi` reader.
    pub forge: PathBuf,
    /// `[hardware] tier`: `full`, `lean` or `fake`. Tier `fake` sets
    /// `FORGE_FAKE=1` for every job the project runs — a first-class
    /// answer, not an environment trick.
    pub tier: String,
    /// Where the `ComfyUI` host is, when the project or the environment
    /// says. `None` falls back to the host backend's own `[server]` block.
    pub comfy_url: Option<String>,
    /// Whether to run the worker at all. A test that only exercises
    /// admission sets this to false, and so does every read verb.
    pub run_worker: bool,
    /// Whether to take rows a **previous** process left `queued` or
    /// `blocked` onto this queue's FIFO.
    ///
    /// True for the daemon, which owns the state directory and will still
    /// be there when they finish. False for an in-process queue, which
    /// exists to run the one job its door was asked for: a `forge gen sfx`
    /// that adopted another session's forgotten row ran a stranger's job on
    /// the card first, unannounced, and then ran its own (2026-08-30). The
    /// rows are left exactly as they are — `forge jobs` shows them, and the
    /// next daemon picks them up in submitted order.
    pub adopt: bool,
    /// What to spawn instead of the toolkit's `python/forge_gen`.
    ///
    /// The seam this crate's own tests put a stub generator through — one
    /// that prints a canned last line and exits with a code — so a queue
    /// test needs neither a backend nor a GPU. `None`, which is what every
    /// door passes, is `forge_library::backends::Backends::python_launcher`
    /// and nothing else.
    pub launcher: Option<Vec<String>>,
}

impl Default for LocalQueueOptions {
    fn default() -> Self {
        Self {
            card_state_dir: None,
            forge: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("forge")),
            tier: String::from("full"),
            comfy_url: None,
            run_worker: true,
            adopt: true,
            launcher: None,
        }
    }
}

impl LocalQueueOptions {
    /// The options a **door** opens a queue with: the project's own
    /// `[hardware]`, and this binary as the card reader.
    ///
    /// Every door builds its options here and not by hand. While each one
    /// filled in `forge` and took `..default()` for the rest, `tier` was
    /// always `"full"` and `comfy_url` always `None`, so a project whose
    /// `forge.toml` said `tier = "fake"` ran the *real* generator with
    /// `FORGE_FAKE` unset — the 2026-08-30 lesson "a gate that can reach a
    /// real generator is not a gate", reopened by a default (`serve.md` §5:
    /// fake is a first-class answer, not an environment trick).
    #[must_use]
    pub fn for_project(project: &Project, forge: PathBuf) -> Self {
        Self {
            card_state_dir: Some(crate::shared_card_dir()),
            forge,
            tier: project.tier().as_str().to_owned(),
            comfy_url: Some(project.hardware.comfy_url.clone()),
            // An in-process queue runs the job its door was asked for and
            // nothing else. See [`Self::adopt`].
            adopt: false,
            ..Self::default()
        }
    }

    /// The options **the daemon** opens its queue with: it owns the state
    /// directory, so it takes what a previous run left.
    #[must_use]
    pub fn for_daemon(project: &Project, forge: PathBuf) -> Self {
        Self {
            adopt: true,
            ..Self::for_project(project, forge)
        }
    }

    /// As [`Self::for_project`], for a door that only reads: no worker, so
    /// nothing is reconciled, re-queued or launched by a listing.
    #[must_use]
    pub fn reader_for_project(project: &Project, forge: PathBuf) -> Self {
        Self {
            run_worker: false,
            ..Self::for_project(project, forge)
        }
    }
}

/// The job currently on the card.
#[derive(Debug)]
struct RunningJob {
    id: JobId,
    pid: Option<u32>,
    cancel: CancelToken,
}

/// The FIFO and the running job, behind one lock.
#[derive(Debug, Default)]
struct Inner {
    fifo: VecDeque<JobId>,
    running: Option<RunningJob>,
}

/// A queue that owns its worker and its state directory.
pub struct LocalQueue {
    project: Project,
    store: JobStore,
    card: CardReader,
    options: LocalQueueOptions,
    inner: Mutex<Inner>,
    wake: Condvar,
    stopping: AtomicBool,
}

impl std::fmt::Debug for LocalQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalQueue")
            .field("project", &self.project.root)
            .finish_non_exhaustive()
    }
}

impl LocalQueue {
    fn card_state_dir(&self) -> &Path {
        self.options
            .card_state_dir
            .as_deref()
            .unwrap_or_else(|| self.store.dir())
    }

    /// Open the state directory, reconcile what a previous run left, prune
    /// what is older than a fortnight, and start the worker.
    ///
    /// **A queue with no worker writes nothing on the way in.** `forge
    /// jobs`, `forge job show|log` and `forge serve --status` are readers,
    /// and a listing that reconciles is a listing that rewrites rows: a
    /// `blocked` row an ended MCP session left behind was turned back into
    /// `queued` by a plain `forge jobs`, and the next `forge gen` in that
    /// project then ran a stranger's forgotten job on the card first
    /// (2026-08-30). Reconciliation belongs to the process that is about to
    /// run the queue, because only it can finish what it re-queues.
    ///
    /// # Errors
    ///
    /// [`ServeError::Io`] when the state directory will not open.
    pub fn open(project: &Project, options: LocalQueueOptions) -> Result<Arc<Self>, ServeError> {
        let store = JobStore::open(&project.root)?;
        let requeue = if options.run_worker {
            let _ = store.prune();
            let left = store.reconcile()?;
            // Reconciling is honest bookkeeping — a row whose process is
            // gone says so — but *running* what a previous process queued
            // is a decision only the daemon may take.
            if options.adopt { left } else { Vec::new() }
        } else {
            Vec::new()
        };
        let queue = Arc::new(Self {
            card: CardReader::new(options.forge.clone(), project.root.clone()),
            project: project.clone(),
            store,
            options,
            inner: Mutex::new(Inner::default()),
            wake: Condvar::new(),
            stopping: AtomicBool::new(false),
        });
        if let Ok(mut inner) = queue.inner.lock() {
            for job in requeue {
                inner.fifo.push_back(job.id);
            }
        }
        if queue.options.run_worker {
            let worker = Arc::clone(&queue);
            std::thread::Builder::new()
                .name(String::from("forge-serve-worker"))
                .spawn(move || worker.work())
                .map_err(|e| ServeError::Io(format!("the worker thread would not start: {e}")))?;
        }
        Ok(queue)
    }

    /// The state directory this queue writes.
    #[must_use]
    pub fn store(&self) -> &JobStore {
        &self.store
    }

    /// The project it serves.
    #[must_use]
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// The card reader, shared with `/v1/status`.
    #[must_use]
    pub fn card(&self) -> &CardReader {
        &self.card
    }

    /// Stop the worker, and **never leave a child alive behind it**.
    ///
    /// A daemon that exited with a generator still running dropped
    /// `card.lock` while that generator held the card, so the next door
    /// took the lease against a live generate and the row was later stamped
    /// `interrupted` although its child was never interrupted (2026-08-30).
    /// So a stop cancels what is running, by recorded pid, exactly as
    /// [`Queue::cancel`] does — `^C` on a followed job has always meant
    /// "give the card back" — and waits for the worker to write the
    /// terminal row and drop the lease before it returns. What it will not
    /// do is drain: a `forge stop` that blocks for a four-minute render is
    /// a stop nobody believes, and the row says `cancelled` with the note
    /// rather than a state nobody can act on.
    pub fn stop(&self) {
        self.stop_within(STOP_GRACE);
    }

    /// [`Self::stop`] with the patience stated, for the test that proves it.
    pub fn stop_within(&self, grace: Duration) {
        self.stopping.store(true, Ordering::SeqCst);
        self.wake.notify_all();
        let running = {
            let Ok(inner) = self.inner.lock() else { return };
            inner
                .running
                .as_ref()
                .map(|current| (current.id.clone(), current.pid, current.cancel.clone()))
        };
        let Some((id, pid, cancel)) = running else {
            return;
        };
        cancel.cancel();
        if let Some(pid) = pid {
            terminate_group(pid);
        }
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if self
                .store
                .read(&id)
                .ok()
                .flatten()
                .is_some_and(|row| row.state.is_terminal())
            {
                return;
            }
            std::thread::sleep(WAIT_POLL);
        }
        // Out of patience with a child still there: SIGKILL its group here
        // rather than in the thread `terminate_group` spawned, because this
        // process is about to exit and that thread would go with it.
        if let Some(pid) = pid
            && crate::store::pid_alive(pid)
        {
            crate::executor::kill_group(pid);
        }
    }

    /// The worker: one job at a time, forever.
    fn work(self: Arc<Self>) {
        loop {
            let next = {
                let Ok(mut inner) = self.inner.lock() else {
                    return;
                };
                loop {
                    if self.stopping.load(Ordering::SeqCst) {
                        return;
                    }
                    if let Some(id) = inner.fifo.pop_front() {
                        break Some(id);
                    }
                    let Ok((guard, _)) = self.wake.wait_timeout(inner, Duration::from_millis(500))
                    else {
                        return;
                    };
                    inner = guard;
                }
            };
            if let Some(id) = next {
                self.run_one(&id);
            }
        }
    }

    /// Take one job from the FIFO through the card to a terminal row.
    fn run_one(&self, id: &JobId) {
        let Ok(Some(mut job)) = self.store.read(id) else {
            return;
        };
        if job.state.is_terminal() {
            return;
        }
        let plan = self.plan_for(&job);
        let Some(lease) = self.take_card(&mut job, &plan) else {
            return;
        };

        let cancel = CancelToken::new();
        {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            inner.running = Some(RunningJob {
                id: id.clone(),
                pid: None,
                cancel: cancel.clone(),
            });
        }
        job.state = JobState::Running;
        job.started = Some(forge_library::clock::now_iso());
        job.blocked_by = None;
        // What the row says about `fake` is what the job *ran* as, not what
        // the submitter happened to know: a tier-`fake` project answers
        // this, and a row that left it null while the child wrote a
        // placeholder would be a row nobody could group by.
        job.fake = Some(plan.fake);
        let _ = self.store.write(&job);

        let log_path = self.project.root.join(&job.log);
        let Ok(sink) = LogSink::create(&log_path) else {
            job.finish(JobState::Failed, Some(5));
            job.message = Some(format!("the job log {} could not be opened", job.log));
            let _ = self.store.write(&job);
            return;
        };
        let sink = Arc::new(Mutex::new(sink));
        let before = self.card.read().map(|view| view.free_gb);
        let started = Instant::now();

        let outcome = if let Some(launch) = self.launch(&job) {
            let executor = ExecutorSet::for_plan(&plan);
            let mut on_pid = |pid: u32| {
                if let Ok(mut inner) = self.inner.lock()
                    && let Some(running) = inner.running.as_mut()
                    && running.id == *id
                {
                    running.pid = Some(pid);
                }
                // On the row as well as in memory: a human reading `forge
                // jobs` while it runs wants the pid `forge gpu` is about to
                // name, and a cancel from another process has only the row.
                if let Ok(Some(mut current)) = self.store.read(id) {
                    current.pid = Some(pid);
                    let _ = self.store.write(&current);
                }
            };
            executor.run(&plan, launch, &sink, &cancel, &mut on_pid)
        } else {
            {
                crate::executor::env::say(
                    &sink,
                    "the toolkit's python/forge_gen was not found from this executable — set \
                     FORGE_HOME to the asset-forge checkout",
                );
                crate::executor::GenOutcome {
                    exit: Some(6),
                    ..crate::executor::GenOutcome::default()
                }
            }
        };

        // The card ladder may have decided nobody gets the card until a
        // human looks. That outlives this job and this lease.
        if let Some(note) = &outcome.note {
            let _ = crate::card::withhold(self.card_state_dir(), note);
        }
        let after = self.card.read().map(|view| view.free_gb);
        let held_s = started.elapsed().as_secs_f64();
        drop(lease);

        job.pid = outcome.pid;
        job.card = Some(outcome.card.clone().unwrap_or(CardFacts {
            held_s,
            vram_before_gb: before,
            vram_after_gb: after,
            restarted: false,
        }));
        self.record_outcome(&mut job, &outcome, cancel.cancelled());
        let _ = self.store.write(&job);
        if let Ok(mut inner) = self.inner.lock()
            && inner.running.as_ref().is_some_and(|r| r.id == *id)
        {
            inner.running = None;
        }
    }

    /// Wait for the card, blocking the row rather than OOM-ing it.
    ///
    /// Free VRAM under the backend's `vram_gb` **budget** with a foreign pid
    /// holding the difference is a wait with a name — `blocked_by: "pid 4411
    /// forge studio, 8.1 GB"` — never an out-of-memory two minutes later.
    fn take_card(&self, job: &mut Job, plan: &Plan) -> Option<CardLease> {
        loop {
            if self.stopping.load(Ordering::SeqCst) {
                return None;
            }
            if let Ok(Some(current)) = self.store.read(&job.id)
                && current.state.is_terminal()
            {
                // Cancelled while it waited.
                return None;
            }
            if let Some(reason) = self.card_is_held(plan) {
                if job.state != JobState::Blocked || job.blocked_by.as_deref() != Some(&reason) {
                    job.state = JobState::Blocked;
                    job.blocked_by = Some(reason);
                    let _ = self.store.write(job);
                }
                std::thread::sleep(BLOCKED_RECHECK);
                continue;
            }
            match CardLease::try_acquire(
                self.card_state_dir(),
                job.id.as_str(),
                plan.need_gb,
                Some(&plan.what),
            ) {
                Ok(Some(lease)) => return Some(lease),
                Ok(None) => {
                    // Another door holds the one lock. That is the design
                    // working, not an error.
                    if job.state != JobState::Blocked {
                        let holder = CardState::read(self.card_state_dir()).map_or_else(
                            || String::from("another forge process"),
                            |state| format!("{} (pid {})", state.holder, state.pid.unwrap_or(0)),
                        );
                        job.state = JobState::Blocked;
                        job.blocked_by = Some(format!("the card lease is held by {holder}"));
                        let _ = self.store.write(job);
                    }
                    std::thread::sleep(Duration::from_millis(300));
                }
                Err(err) => {
                    job.finish(JobState::Failed, Some(5));
                    job.message = Some(format!("the card lock would not open: {err}"));
                    let _ = self.store.write(job);
                    return None;
                }
            }
        }
    }

    /// Who is holding the card against this job, when anyone is.
    fn card_is_held(&self, plan: &Plan) -> Option<String> {
        let need = plan.need_gb?;
        if let Some(note) = CardState::withheld(self.card_state_dir()) {
            return Some(format!("comfy — {note}"));
        }
        // A `running` row whose pid is still alive is another door's
        // generator, left by a daemon or an MCP session that ended without
        // waiting for it. It is not in this queue's FIFO and its lease died
        // with its parent, so nothing else here would see it.
        if let Ok(rows) = self.store.all()
            && let Some(other) = rows.iter().find(|row| {
                row.state == JobState::Running && row.pid.is_some_and(crate::store::pid_alive)
            })
        {
            return Some(format!(
                "{} (pid {}) is still running, left by {} — cancel it or wait for it",
                other.id,
                other.pid.unwrap_or(0),
                other.created_by
            ));
        }
        let view = self.card.read()?;
        if view.free_gb >= need {
            return None;
        }
        let (pid, name, gb) = view.largest_foreign()?;
        Some(format!(
            "pid {pid} {name}, {gb:.1} GB — {:.1} GB free and this job's budget is {need:.1} GB",
            view.free_gb
        ))
    }

    /// Turn the child's verdict into the row's, translating nothing.
    fn record_outcome(
        &self,
        job: &mut Job,
        outcome: &crate::executor::GenOutcome,
        cancelled: bool,
    ) {
        if let Some(payload) = &outcome.payload {
            Self::read_payload(job, payload);
        }
        if cancelled {
            // A cancelled job has no exit code, whatever the shell reports
            // for a signalled child.
            job.finish(JobState::Cancelled, None);
            job.message = Some(format!(
                "SIGTERM to pid {}, group killed after 10 s; partial outputs under out/ were \
                 left, nothing under assets/ was touched",
                job.pid.unwrap_or(0)
            ));
            return;
        }
        if let Some(code) = outcome.exit {
            let state = JobState::of_exit(code);
            job.finish(state, Some(code));
            if job.message.is_none() && state != JobState::Done {
                job.message = Some(format!("forge gen exited {code}; the log is {}", job.log));
            }
        } else {
            job.finish(JobState::Failed, None);
            job.message = Some(outcome.signal.map_or_else(
                || {
                    format!(
                        "the generator ended without an exit status; see {}",
                        job.log
                    )
                },
                |signal| {
                    format!(
                        "the generator was terminated by signal {signal}; see {}",
                        job.log
                    )
                },
            ));
        }
        if let Some(same) = self.same_as(job) {
            job.same_as = Some(same);
        }
    }

    /// Copy the child's own words into the row — and only its own words.
    fn read_payload(job: &mut Job, payload: &Value) {
        let text = |key: &str| payload.get(key).and_then(Value::as_str).map(str::to_owned);
        if let Some(record) = text("record") {
            job.record = Some(record);
        }
        if let Some(outputs) = payload.get("outputs").and_then(Value::as_array) {
            job.outputs = outputs
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
        }
        job.message = text("reason")
            .or_else(|| text("message"))
            .or(job.message.take());
        job.hint = text("hint");
        // Verbatim, never composed: the one process that patched the graph
        // is the one that wrote this.
        job.comfy = payload.get("comfy").cloned();
        // Observed, never inferred: a graph-hash index would claim a cache
        // hit that never happened, which is a record that lies.
        job.cached = payload
            .get("comfy")
            .and_then(|comfy| comfy.get("cached"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        job.payload = Some(payload.clone());
    }

    /// An earlier finished job whose record claims the same output bytes.
    ///
    /// This is the second of the two observations that may set `cached`,
    /// and the one that fills `same_as`. It compares hashes the generator
    /// wrote; nothing here hashes a file to make a claim about a run.
    ///
    /// An earlier job whose record sits at the **same path** as this one's
    /// is never a match: that file on disk is now this job's record, so
    /// reading it back would make the earlier run "claim" bytes it never
    /// wrote. Measured 2026-09-04 — a character re-lifted at another
    /// register over the same name came back `same_as` the prop-register
    /// lift it had just overwritten.
    fn same_as(&self, job: &Job) -> Option<JobId> {
        let mine = self.record_hashes(job)?;
        if mine.is_empty() {
            return None;
        }
        let earlier = self.store.all().ok()?;
        earlier
            .into_iter()
            .filter(|other| {
                other.id != job.id
                    && other.state == JobState::Done
                    && other.record.is_some()
                    && other.record != job.record
            })
            .find(|other| {
                self.record_hashes(other)
                    .is_some_and(|theirs| !theirs.is_empty() && theirs == mine)
            })
            .map(|other| other.id)
    }

    /// The `sha256:` values a job's record claims for its outputs.
    fn record_hashes(&self, job: &Job) -> Option<Vec<String>> {
        let record = job.record.as_ref()?;
        let path = self.project.root.join(record);
        let record = forge_library::GeneratorRecord::load(&path).ok()?;
        Some(
            record
                .outputs
                .iter()
                .filter_map(|output| output.sha256.clone())
                .collect(),
        )
    }

    /// What a job needs before it can be given the card.
    fn plan_for(&self, job: &Job) -> Plan {
        let backends = Backends::discover(&self.project);
        let backend = job.backend.clone();
        let found = backend.as_deref().and_then(|name| backends.get(name));
        let facts = found.map_or_else(BackendFacts::default, |b| BackendFacts::read(&b.dir));
        let host_facts = facts.host.as_deref().and_then(|host| {
            backends
                .get(host)
                .map(|backend| BackendFacts::read(&backend.dir))
        });
        let comfy_url = std::env::var("FORGE_COMFY_URL")
            .ok()
            .filter(|url| !url.is_empty())
            .or_else(|| self.options.comfy_url.clone())
            .or_else(|| host_facts.as_ref().and_then(|host| host.url.clone()));
        let fake = job.fake.unwrap_or_else(|| self.is_fake());
        Plan {
            argv: job.argv.clone(),
            executor: facts.executor,
            // A budget, never a measurement — and `None` when the backend
            // does not declare one, because a made-up number would block a
            // job for a reason nobody wrote down. A fake job's budget is
            // `None` too: a placeholder is written by the stdlib and holds
            // no VRAM, so blocking one behind a real generator's budget
            // would be a wait for memory it will never ask for. It still
            // takes the lease and the queue like any other job.
            need_gb: if fake {
                None
            } else {
                found.and_then(|b| b.vram_gb)
            },
            comfy_url,
            comfy_unit: host_facts.and_then(|host| host.unit),
            what: format!(
                "{} {}",
                backend.as_deref().unwrap_or("forge gen"),
                job.argv.first().map_or("", String::as_str)
            )
            .trim()
            .to_owned(),
            fake: job.fake.unwrap_or_else(|| self.is_fake()),
        }
    }

    /// Whether every job this project runs is a placeholder run.
    fn is_fake(&self) -> bool {
        self.options.tier == "fake"
            || std::env::var("FORGE_FAKE").is_ok_and(|value| value == "1" || value == "true")
    }

    /// The launcher for a job's child, when the toolkit can be found.
    fn launch(&self, job: &Job) -> Option<Launch> {
        let launcher = match self.options.launcher.as_deref() {
            Some([program, args @ ..]) => {
                let mut command = std::process::Command::new(program);
                command.args(args);
                command.current_dir(&self.project.root);
                command
            }
            _ => Backends::python_launcher(&self.project)?,
        };
        Some(Launch {
            project_root: self.project.root.clone(),
            rig_profile: self.project.rig_dir(),
            job_id: job.id.to_string(),
            launcher,
        })
    }

    /// How many rows are ahead of this one in the FIFO.
    fn position(&self, id: &JobId) -> Option<usize> {
        let inner = self.inner.lock().ok()?;
        inner.fifo.iter().position(|queued| queued == id)
    }

    /// One row as a status line.
    fn status_job(&self, job: &Job) -> StatusJob {
        StatusJob {
            id: job.id.clone(),
            kind: job.kind.clone(),
            state: job.state,
            elapsed_s: job.elapsed_s(),
            position: self.position(&job.id),
            finished: job.finished.clone(),
            log_tail: if job.state == JobState::Running {
                logs::tail(&self.project.root.join(&job.log), STATUS_TAIL)
            } else {
                Vec::new()
            },
        }
    }
}

impl Queue for LocalQueue {
    fn submit(&self, spec: JobSpec) -> Result<Job, ServeError> {
        // A generate with no backend is a job with no budget, no admission
        // refusal and no card ladder — the shape the terminal door shipped
        // for a fortnight. It is a caller's mistake, not a machine's, so it
        // is refused before a row exists rather than run as an `env` job
        // whose record will say `comfy`.
        if spec.kind.starts_with("generate_") && spec.backend.is_none() {
            return Err(ServeError::refused(format!(
                "{} was submitted with no backend — every generate names the backend it runs \
                 on, from forge_library::project::backend_for_verb. nothing was queued.",
                spec.kind
            )));
        }
        let backends = Backends::discover(&self.project);
        let id = JobId::fresh();
        let log = format!("out/serve/logs/{id}.log");
        let executor = spec
            .backend
            .as_deref()
            .and_then(|name| backends.get(name))
            .map_or(ExecutorKind::Env, |backend| {
                BackendFacts::read(&backend.dir).executor
            });

        // The out-path lease, before anything is written: a claimed path is
        // refused naming the row that holds it, and nothing is left behind.
        let live: Vec<Job> = self
            .store
            .all()?
            .into_iter()
            .filter(|job| job.state.holds_outputs())
            .collect();
        for path in &spec.outputs_claimed {
            if let Some(holder) = live
                .iter()
                .find(|job| job.outputs_claimed.iter().any(|claimed| claimed == path))
            {
                return Err(ServeError::refused(format!(
                    "{path} is already claimed by {} ({}) — wait for it, cancel it, or write to \
                     another name",
                    holder.id, holder.state
                )));
            }
        }

        // A backend nobody installed cannot run, and the queue does not
        // hold a job that cannot run: exit 3, in under a millisecond, in
        // the backend table's own words. A **fake** job is the exception
        // and always was: `run_fake` never imports the backend, never reads
        // a template and never resolves a URL, so refusing one for a
        // backend it will not touch would take tier `fake` — the answer for
        // a machine with no card — away from the machines it exists for.
        //
        // A stub **launcher** is the other exception, and only this crate's
        // own tests pass one: when the caller has replaced the generator,
        // the backend table is not what runs, and holding a stub to it
        // would make the tests depend on what happens to be installed —
        // which is the trap `decisions.md` records for 2026-08-30.
        let fake = spec.fake.unwrap_or_else(|| self.is_fake());
        if let Some(name) = spec.backend.as_deref()
            && !fake
            && self.options.launcher.is_none()
            && !backends.is_found(name)
        {
            let mut job = Job::admitted(id, &spec, executor, log, JobState::Refused);
            job.finish(JobState::Refused, Some(3));
            job.message = Some(backends.refusal(name));
            job.hint = Some(String::from(
                "`just doctor` has the full table of what this machine can run",
            ));
            self.store.write(&job)?;
            return Ok(job);
        }

        let job = Job::admitted(id.clone(), &spec, executor, log, JobState::Queued);
        self.store.write(&job)?;
        if let Ok(mut inner) = self.inner.lock() {
            inner.fifo.push_back(id);
        }
        self.wake.notify_all();
        Ok(job)
    }

    fn get(&self, id: &JobId) -> Result<Option<Job>, ServeError> {
        self.store.read(id)
    }

    fn list(&self, filter: &JobFilter) -> Result<Vec<Job>, ServeError> {
        self.store.list(filter)
    }

    fn cancel(&self, id: &JobId) -> Result<Job, ServeError> {
        let Some(mut job) = self.store.read(id)? else {
            return Err(self.store.no_such_job(id));
        };
        if job.state.is_terminal() {
            return Err(ServeError::refused(format!(
                "{id} is already {} — there is nothing to cancel",
                job.state
            )));
        }
        let running = {
            let Ok(mut inner) = self.inner.lock() else {
                return Err(ServeError::Io(String::from("the queue lock is poisoned")));
            };
            inner.fifo.retain(|queued| queued != id);
            match inner.running.as_ref() {
                Some(current) if current.id == *id => {
                    current.cancel.cancel();
                    current.pid
                }
                _ => None,
            }
        };
        if let Some(pid) = running {
            terminate_group(pid);
            // The worker writes the terminal row, because it is the one
            // holding the child and the lease.
            let deadline = Instant::now() + CANCEL_GRACE;
            while Instant::now() < deadline {
                if let Some(current) = self.store.read(id)?
                    && current.state.is_terminal()
                {
                    return Ok(current);
                }
                std::thread::sleep(WAIT_POLL);
            }
            return self
                .store
                .read(id)?
                .ok_or_else(|| self.store.no_such_job(id));
        }
        job.finish(JobState::Cancelled, None);
        job.message = Some(String::from(
            "cancelled before it started; nothing ran and nothing was written",
        ));
        self.store.write(&job)?;
        Ok(job)
    }

    fn log(&self, id: &JobId, from: u64) -> Result<LogChunk, ServeError> {
        let Some(job) = self.store.read(id)? else {
            return Err(self.store.no_such_job(id));
        };
        let mut chunk = logs::read_from(&self.project.root.join(&job.log), from)?;
        chunk.done = job.state.is_terminal();
        Ok(chunk)
    }

    fn status(&self) -> Result<Status, ServeError> {
        let all = self.store.all()?;
        let mut counts = QueueCounts::default();
        let mut live = Vec::new();
        let mut recent = Vec::new();
        for job in &all {
            match job.state {
                JobState::Queued => counts.queued += 1,
                JobState::Blocked => counts.blocked += 1,
                JobState::Running => counts.running += 1,
                _ => {}
            }
            if job.state.is_terminal() {
                if recent.len() < 5 {
                    recent.push(self.status_job(job));
                }
            } else {
                live.push(self.status_job(job));
            }
        }
        live.reverse();
        let daemon = self.store.daemon();
        Ok(Status {
            project: self.project.root.display().to_string(),
            tier: self.options.tier.clone(),
            comfy_url: self.options.comfy_url.clone(),
            card: self.card.read().map_or(Value::Null, |view| view.raw),
            queue: counts,
            jobs: live,
            recent,
            daemon: daemon.map_or_else(
                || serde_json::json!({"up": false}),
                |daemon| {
                    serde_json::json!({
                        "up": true,
                        "port": daemon.port,
                        "started": daemon.started,
                        "version": daemon.version,
                    })
                },
            ),
        })
    }

    fn runs(&self, filter: &RunFilter) -> Result<Vec<Run>, ServeError> {
        Ok(crate::runs::walk(&self.project, filter))
    }

    fn wait(&self, id: &JobId, max: Duration) -> Result<Job, ServeError> {
        let deadline = Instant::now() + max;
        loop {
            let Some(job) = self.store.read(id)? else {
                return Err(self.store.no_such_job(id));
            };
            if job.state.is_terminal() || Instant::now() >= deadline {
                return Ok(job);
            }
            std::thread::sleep(WAIT_POLL);
        }
    }
}
