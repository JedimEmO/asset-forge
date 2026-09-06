//! `forge gen <cmd> [args…]`: the Python layer, run whole — now through the
//! queue.
//!
//! The binary knows nothing about a generator's flags and wants to know
//! nothing: the command line after `gen` is handed to `python3
//! <toolkit>/python/forge_gen` verbatim, with `--project <root>` appended so
//! the record it writes carries paths relative to this project, and
//! `--json` so its last stdout line is one object this side can read. On
//! success the object is summarised (record, outputs) unless `--json` was
//! among the arguments, in which case the object itself is the last stdout
//! line here too. On a refusal the exit code is relayed unchanged: 3
//! install something, 4 fix the input, 5 read the log, 6 put a tool on
//! PATH.
//!
//! # What changed when the queue arrived, and what did not
//!
//! What did not: the words, the exit codes, the last-line rule, and every
//! `just` recipe. **No recipe is rewritten — that is the acceptance test.**
//!
//! What did: the child is spawned by a *queue* rather than by this
//! function. With a daemon up, this submits and follows the job's log; with
//! none, a `LocalQueue` in this process runs it, takes the same
//! `card.lock`, and **writes a job row anyway** so `status` and `list_runs`
//! see it later. Either way the card is held by one lock, so a `just sfx`
//! in a second terminal waits instead of racing.
//!
//! `^C` **cancels**, as it always did. A human who interrupts a
//! 111-second image expects the card back, and leaving it held by a job the
//! user believes they stopped is a silent divergence. `forge job log <id>`
//! is there for someone who wanted to keep watching.
//!
//! Two command lines never become jobs: `--help`, which argparse answers
//! without a library, and `doctor`, which describes the machine rather than
//! generating anything and has no output to lease.

use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use forge_library::Project;
use forge_library::backends::{Backends, GenExit, HOME_ENV};
use forge_serve::{Job, JobId, JobState, LocalQueueOptions, Queue, discovery, spec};
use serde_json::Value;

use crate::cli::GenArgs;
use crate::outcome::{Failure, Outcome};

/// What a run of the Python layer left behind.
pub(crate) struct GenResult {
    /// The exit code, as the table reads it; `None` when it was none of them.
    pub(crate) exit: Option<GenExit>,
    /// The raw code, for the message when it is not in the table.
    pub(crate) raw_code: Option<i32>,
    /// The object on the last stdout line, when there was one.
    pub(crate) payload: Option<Value>,
}

/// Whether the command line is only asking for help text.
pub(crate) fn wants_help(args: &GenArgs) -> bool {
    args.rest.iter().any(|a| a == "--help" || a == "-h")
}

/// Print a generator's help without a project.
///
/// `forge gen sfx --help` is an agent reading a flag table before choosing
/// a directory; argparse answers it without ever touching a library, so the
/// project discovery the real call needs is not a reason to refuse. The
/// toolkit still has to be findable — the help lives in the Python layer.
pub(crate) fn help(args: &GenArgs) -> Outcome {
    let Some(toolkit) = crate::toolkit::gen_dir() else {
        return Err(Failure::from_gen(
            GenExit::MissingTool,
            format!(
                "the toolkit's python/forge_gen was not found from this executable — set \
                 {HOME_ENV} to the asset-forge checkout"
            ),
        ));
    };
    let mut command = Command::new("python3");
    command.arg(toolkit.join("python").join("forge_gen"));
    command.args(&args.rest);
    let status = command.status().map_err(|e| {
        Failure::from_gen(
            GenExit::MissingTool,
            format!(
                "python3 could not be started: {e} — the Python layer needs python3 >= 3.11 on PATH"
            ),
        )
    })?;
    match status.code() {
        Some(0) | None => Ok(()),
        Some(code) => Err(Failure::from_gen(
            GenExit::from_code(code).unwrap_or(GenExit::Usage),
            format!("forge-gen exited {code}"),
        )),
    }
}

/// The queue for this project: a daemon's if one answers, else one of our
/// own — and neither this function nor anything above it knows which.
pub(crate) fn queue_for(project: &Project) -> Result<Arc<dyn Queue>, Failure> {
    discovery::queue_with(project, options(project))
        .map_err(|e| Failure::failed(format!("the queue would not open: {e}")))
}

/// The queue for a door that only **reads** it: `forge jobs`, `forge job
/// show|log`, `forge serve --status`.
///
/// The difference is one flag and it is the whole point: a reader opens no
/// worker, so a listing reconciles nothing, re-queues nothing and launches
/// nothing. `forge jobs` used to rewrite a `blocked` row into `queued` and
/// leave it at the head of the FIFO for the next `forge gen` to run
/// (2026-08-30).
pub(crate) fn read_queue_for(project: &Project) -> Result<Arc<dyn Queue>, Failure> {
    discovery::queue_with(project, reader_options(project))
        .map_err(|e| Failure::failed(format!("the queue would not open: {e}")))
}

/// This executable, which the card reader re-invokes as `forge gpu --json`.
fn this_binary() -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("forge"))
}

/// What a queue in this process needs to know — the project's own
/// `[hardware]`, so tier `fake` is the answer it says it is.
fn options(project: &Project) -> LocalQueueOptions {
    LocalQueueOptions::for_project(project, this_binary())
}

/// The same, for a door that only reads the table.
fn reader_options(project: &Project) -> LocalQueueOptions {
    LocalQueueOptions::reader_for_project(project, this_binary())
}

/// Run one generator command and relay its verdict.
pub(crate) fn run(project: &Project, args: &GenArgs) -> Outcome {
    let wants_json = args.rest.iter().any(|a| a == "--json");
    let command_line: Vec<String> = args
        .rest
        .iter()
        .filter(|a| *a != "--json")
        .cloned()
        .collect();
    // `forge gen doctor` with no --json is a person asking: the Python
    // table is the answer, there is no object to summarise, and there is
    // nothing to queue — it describes the machine rather than using it.
    if command_line.first().map(String::as_str) == Some("doctor") {
        let borrowed: Vec<&str> = command_line.iter().map(String::as_str).collect();
        return relay(
            spawn_with(project, &borrowed, true, wants_json)?,
            wants_json,
        );
    }

    let queue = queue_for(project)?;
    let spec = spec::spec_for(&command_line, &project.root, "cli");
    let job = queue.submit(spec).map_err(crate::commands::jobs::failure)?;
    if !job.state.is_terminal() {
        // ^C means "give the card back", not "detach": the child is in its
        // own process group, so the terminal's signal reaches this process
        // and this process cancels the job by its recorded pid.
        cancel_on_interrupt(Arc::clone(&queue), job.id.clone());
        if job.state == JobState::Queued
            && let Ok(status) = queue.status()
            && let Some(position) = status
                .jobs
                .iter()
                .find(|row| row.id == job.id)
                .and_then(|row| row.position)
            && position > 0
        {
            println!("queued behind {position} job(s) — forge jobs says what is ahead");
        }
    }
    let job = follow(queue.as_ref(), &job)?;
    verdict(&job, wants_json)
}

/// Print the job's log as it arrives and hand back the finished row.
fn follow(queue: &dyn Queue, job: &Job) -> Result<Job, Failure> {
    let mut at = 0;
    loop {
        at = crate::commands::jobs::follow(queue, &job.id, at, false)?;
        let current = queue
            .get(&job.id)
            .map_err(crate::commands::jobs::failure)?
            .ok_or_else(|| Failure::failed(format!("the row for {} went missing", job.id)))?;
        if current.state.is_terminal() {
            // One last read, for whatever landed between the two calls.
            let _ = crate::commands::jobs::follow(queue, &job.id, at, false)?;
            return Ok(current);
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// Cancel the job when the terminal interrupts this process.
fn cancel_on_interrupt(queue: Arc<dyn Queue>, id: JobId) {
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async {
            if tokio::signal::ctrl_c().await.is_ok() {
                eprintln!("\nforge: cancelling {id} — the card comes back with it");
                let _ = queue.cancel(&id);
            }
        });
    });
}

/// Turn a finished row into this process's own exit code and last line.
fn verdict(job: &Job, wants_json: bool) -> Outcome {
    let payload = job.payload.clone();
    match job.state {
        JobState::Done => {
            if let Some(payload) = &payload {
                if wants_json {
                    println!("{payload}");
                } else {
                    print!("{}", summary(payload));
                }
            }
            Ok(())
        }
        JobState::Refused | JobState::Failed => {
            if wants_json && let Some(payload) = &payload {
                println!("{payload}");
            }
            let exit = job
                .exit
                .and_then(GenExit::from_code)
                .unwrap_or(GenExit::BackendFailed);
            Err(Failure::from_gen(exit, refusal(exit, payload.as_ref())))
        }
        JobState::Cancelled => Err(Failure::refused(format!(
            "{} was cancelled{}",
            job.id,
            job.message
                .as_deref()
                .map_or_else(String::new, |note| format!(" — {note}"))
        ))),
        JobState::Interrupted => Err(Failure::failed(format!(
            "{} was interrupted: {}",
            job.id,
            job.message.as_deref().unwrap_or("the daemon restarted")
        ))),
        // follow() only returns terminal rows.
        JobState::Queued | JobState::Blocked | JobState::Running => Err(Failure::failed(format!(
            "{} is still {}",
            job.id, job.state
        ))),
    }
}

/// Relay a directly-spawned run (today's path, kept for `doctor`).
fn relay(result: GenResult, wants_json: bool) -> Outcome {
    match result.exit {
        Some(GenExit::Ok) => {
            if let Some(payload) = &result.payload {
                if wants_json {
                    println!("{payload}");
                } else {
                    print!("{}", summary(payload));
                }
            }
            Ok(())
        }
        Some(exit) => {
            if wants_json && let Some(payload) = &result.payload {
                println!("{payload}");
            }
            Err(Failure::from_gen(
                exit,
                refusal(exit, result.payload.as_ref()),
            ))
        }
        None => {
            if wants_json && let Some(payload) = &result.payload {
                println!("{payload}");
            }
            match result.raw_code {
                // Exit 1 is not in the generator table: it is `forge-gen
                // doctor` saying a check did not hold, and it stays a 1 here
                // — the same code every other check in this binary uses.
                Some(1) => Err(Failure::failed(
                    result
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("message").or_else(|| p.get("error")))
                        .and_then(Value::as_str)
                        .map_or_else(
                            || String::from("forge-gen: a check did not hold (exit 1)"),
                            str::to_owned,
                        ),
                )),
                Some(code) => Err(Failure::from_gen(
                    GenExit::BackendFailed,
                    format!("forge-gen exited {code}, which is not a code it speaks"),
                )),
                None => Err(Failure::from_gen(
                    GenExit::BackendFailed,
                    "forge-gen was killed by a signal",
                )),
            }
        }
    }
}

/// Spawn `python3 <toolkit>/python/forge_gen <argv> --project <root> --json`,
/// stream its stdout through (holding the last line back), and return what
/// it exited with and the object on that last line.
///
/// The one caller left is `doctor`, which is not a job: it takes no card,
/// writes no output and is asked for as often as a person is curious.
/// Every generate goes through the queue, whose executor is the same spawn
/// in `forge_serve::executor::env`.
pub(crate) fn spawn(project: &Project, argv: &[&str], relay: bool) -> Result<GenResult, Failure> {
    spawn_with(project, argv, relay, true)
}

/// As [`spawn`], with `--json` optional: without it nothing is held back and
/// there is no payload, only the exit code.
fn spawn_with(
    project: &Project,
    argv: &[&str],
    relay: bool,
    json: bool,
) -> Result<GenResult, Failure> {
    let mut command = launcher(project)?;
    command.args(argv);
    command.arg("--project").arg(&project.root);
    // The Python side defaults its rig profile to the toolkit's own
    // rigs/humanoid; the project's forge.toml may name a different one (or an
    // edited copy under assets-src/rigs). Rigging against one contract and
    // checking against another is the kind of disagreement nobody notices
    // until a body fails rig check, so the project's profile is handed over
    // explicitly. A user's own FORGE_RIG_PROFILE still wins.
    if std::env::var_os("FORGE_RIG_PROFILE").is_none() {
        command.env("FORGE_RIG_PROFILE", project.rig_dir());
    }
    // Every child of this binary, job or not: a re-entered `forge gen`
    // must never rediscover the daemon and recurse.
    command.env(forge_serve::NO_DAEMON_ENV, "1");
    if json {
        command.arg("--json");
    }
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::inherit());
    let mut child = command.spawn().map_err(|e| {
        Failure::from_gen(
            GenExit::MissingTool,
            format!(
                "python3 could not be started: {e} — the Python layer needs python3 >= 3.11 on PATH"
            ),
        )
    })?;
    let stdout = child.stdout.take();
    let mut held: Option<String> = None;
    if let Some(stdout) = stdout {
        let reader = BufReader::new(stdout);
        let mut out = std::io::stdout().lock();
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if let Some(previous) = held.replace(line)
                && relay
            {
                let _ = writeln!(out, "{previous}");
                let _ = out.flush();
            }
        }
    }
    let status = child
        .wait()
        .map_err(|e| Failure::from_gen(GenExit::BackendFailed, format!("forge-gen: {e}")))?;
    let payload = held.as_deref().and_then(parse_object);
    if payload.is_none()
        && let Some(last) = &held
        && relay
    {
        println!("{last}");
    }
    let raw_code = status.code();
    Ok(GenResult {
        exit: raw_code.and_then(GenExit::from_code),
        raw_code,
        payload,
    })
}

/// The launcher command, or the missing-tool refusal naming `FORGE_HOME`.
fn launcher(project: &Project) -> Result<Command, Failure> {
    Backends::python_launcher(project).ok_or_else(|| {
        Failure::from_gen(
            GenExit::MissingTool,
            format!(
                "the toolkit's python/forge_gen was not found from this executable or from \
                 {} — set {HOME_ENV} to the asset-forge checkout",
                project.root.display()
            ),
        )
    })
}

/// A line that is one JSON object, or nothing.
fn parse_object(line: &str) -> Option<Value> {
    let trimmed = line.trim();
    if !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
        return None;
    }
    serde_json::from_str::<Value>(trimmed)
        .ok()
        .filter(Value::is_object)
}

/// The human summary of a success object: the record, the outputs, the
/// time, and any scalar the command added (a seed, a model).
fn summary(payload: &Value) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if let Some(record) = payload.get("record").and_then(Value::as_str) {
        let _ = writeln!(out, "record   {record}");
    }
    if let Some(outputs) = payload.get("outputs").and_then(Value::as_array) {
        for output in outputs.iter().filter_map(Value::as_str) {
            let _ = writeln!(out, "output   {output}");
        }
    }
    if let Some(object) = payload.as_object() {
        for (key, value) in object {
            // `_`-prefixed keys are the Python layer's private channel to
            // its own printer and never reach this line; skipped so they
            // cannot start.
            if key.starts_with('_')
                || matches!(
                    key.as_str(),
                    "ok" | "record" | "outputs" | "elapsed_s" | "fake" | "records"
                )
            {
                continue;
            }
            if let Value::Array(items) = value
                && items.iter().all(Value::is_string)
            {
                for item in items.iter().filter_map(Value::as_str) {
                    let _ = writeln!(out, "{key:<8} {item}");
                }
                continue;
            }
            match value {
                // A table of aligned keys is for scalars. A door that
                // renders a table of its own — a fit's runs — gets its own
                // block, indented under its name, rather than a first line
                // in the column and the rest against the margin.
                Value::String(text) if text.contains('\n') => {
                    let _ = writeln!(out, "{key}:");
                    for line in text.lines() {
                        let _ = writeln!(out, "  {line}");
                    }
                }
                Value::String(text) => {
                    let _ = writeln!(out, "{key:<8} {text}");
                }
                Value::Number(number) => {
                    let _ = writeln!(out, "{key:<8} {number}");
                }
                Value::Bool(flag) => {
                    let _ = writeln!(out, "{key:<8} {flag}");
                }
                _ => {}
            }
        }
    }
    // Whether the run was a --fake one is the record's word, not the
    // summary object's: not every command repeats it there, and the record
    // is what the promote will read.
    let fake = payload.get("fake").and_then(Value::as_bool) == Some(true)
        || payload
            .get("record")
            .and_then(Value::as_str)
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|record| record.get("fake").and_then(Value::as_bool))
            == Some(true);
    if fake {
        out.push_str(
            "fake     true — placeholder outputs that pass the validators and nothing else\n",
        );
    }
    if let Some(elapsed) = payload.get("elapsed_s").and_then(Value::as_f64) {
        let _ = writeln!(out, "elapsed  {elapsed:.1} s");
    }
    out
}

/// The refusal's text from its object: the message (or reason), the hint,
/// and the tail of the log when the backend failed.
fn refusal(exit: GenExit, payload: Option<&Value>) -> String {
    use std::fmt::Write as _;
    let Some(payload) = payload else {
        return format!("{exit} (forge-gen printed no JSON line)");
    };
    let text = payload
        .get("reason")
        .or_else(|| payload.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("(no message)");
    let mut out = format!("{exit}: {text}");
    if let Some(backend) = payload.get("backend").and_then(Value::as_str) {
        let _ = write!(out, " [backend {backend}]");
    }
    if let Some(tool) = payload.get("tool").and_then(Value::as_str) {
        let _ = write!(out, " [tool {tool}]");
    }
    if let Some(hint) = payload.get("hint").and_then(Value::as_str) {
        let _ = write!(out, "\n  hint: {hint}");
    }
    if let Some(tail) = payload.get("log_tail").and_then(Value::as_array) {
        let lines: Vec<&str> = tail.iter().filter_map(Value::as_str).collect();
        if !lines.is_empty() {
            let keep = lines.len().saturating_sub(12);
            out.push_str("\n  log tail:");
            for line in &lines[keep..] {
                out.push_str("\n    ");
                out.push_str(line);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_json_object_on_the_last_line_is_the_payload() {
        assert!(parse_object("{\"ok\": true}").is_some());
        assert!(parse_object("  {\"ok\": true}  ").is_some());
        assert!(parse_object("[1, 2]").is_none());
        assert!(parse_object("fit: 0.83 of the frame").is_none());
        assert!(parse_object("{not json}").is_none());
    }

    #[test]
    fn the_summary_names_record_outputs_and_scalars() {
        let payload: Value = serde_json::from_str(
            "{\"ok\":true,\"record\":\"/r.json\",\"outputs\":[\"/a.glb\"],\"seed\":7,\
             \"model\":\"m\",\"metrics\":{\"x\":1},\"fake\":true,\"elapsed_s\":1.25}",
        )
        .expect("json");
        let text = summary(&payload);
        assert!(text.contains("record   /r.json"));
        assert!(text.contains("output   /a.glb"));
        assert!(text.contains("seed     7"));
        assert!(text.contains("model    m"));
        assert!(!text.contains("metrics"), "objects are not summarised");
        assert!(text.contains("fake     true"));
        assert!(text.contains("elapsed  1.2 s") || text.contains("elapsed  1.3 s"));
    }

    #[test]
    fn a_refusal_carries_reason_hint_and_the_log_tail() {
        let payload: Value = serde_json::from_str(
            "{\"ok\":false,\"error\":\"input_rejected\",\"reason\":\"no flat border\",\
             \"hint\":\"fix the PNG\",\"log_tail\":[\"a\",\"b\"]}",
        )
        .expect("json");
        let text = refusal(GenExit::InputRejected, Some(&payload));
        assert!(text.starts_with("input_rejected: no flat border"));
        assert!(text.contains("hint: fix the PNG"));
        assert!(text.contains("log tail:"));
        assert!(text.ends_with("    b"));
        assert!(refusal(GenExit::BackendFailed, None).contains("no JSON line"));
    }

    #[test]
    fn a_cancelled_job_is_a_refusal_and_not_an_exit_code_from_nowhere() {
        let mut job = Job::admitted(
            JobId::from("j-20260830-141207-3f9a"),
            &forge_serve::JobSpec::new("generate_audio.sfx", vec![String::from("sfx")], "cli"),
            forge_serve::ExecutorKind::Env,
            String::from("out/serve/logs/j.log"),
            JobState::Queued,
        );
        job.finish(JobState::Cancelled, None);
        job.message = Some(String::from("SIGTERM to pid 1"));
        let failure = verdict(&job, false).expect_err("a cancel is not a success");
        assert!(
            failure.message().contains("cancelled"),
            "{}",
            failure.message()
        );
    }
}
