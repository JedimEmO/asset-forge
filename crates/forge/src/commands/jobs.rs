//! `forge jobs`, `forge job show|log|cancel`: reading the queue from a
//! terminal.
//!
//! Every one of these goes through the same `Arc<dyn Queue>` the MCP tools
//! hold, so a human at a terminal and an agent in a session are looking at
//! one table — and a machine with no daemon answers them out of
//! `out/serve/` without anything being up.

use std::io::Write as _;
use std::time::Duration;

use forge_library::Project;
use forge_serve::{Job, JobFilter, JobId, JobState, Queue};

use crate::cli::{JobIdArgs, JobLogArgs, JobsArgs};
use crate::outcome::{Failure, Outcome};

/// How often a `--follow` asks for more of the log.
const FOLLOW_POLL: Duration = Duration::from_millis(200);

/// The queue for this project: the daemon's if one answers, else ours.
pub(crate) fn queue(project: &Project) -> Result<std::sync::Arc<dyn Queue>, Failure> {
    crate::commands::generate::queue_for(project)
}

/// `forge jobs`.
pub(crate) fn list(project: &Project, args: &JobsArgs) -> Outcome {
    let queue = queue(project)?;
    let filter = JobFilter {
        state: args.state.as_deref().and_then(JobState::parse),
        kind: args.kind.clone(),
        limit: Some(args.limit),
    };
    if let Some(state) = &args.state
        && filter.state.is_none()
    {
        return Err(Failure::refused(format!(
            "{state:?} is not a job state — queued, blocked, running, done, refused, failed, \
             cancelled, interrupted"
        )));
    }
    let jobs = queue.list(&filter).map_err(failure)?;
    if args.json {
        let text = serde_json::to_string(&jobs)
            .map_err(|e| Failure::failed(format!("the rows would not serialise: {e}")))?;
        println!("{text}");
        return Ok(());
    }
    if jobs.is_empty() {
        println!("no jobs yet in {}", project.root.display());
        return Ok(());
    }
    for job in &jobs {
        println!(
            "{:<26} {:<11} {:<22} {}",
            job.id,
            job.state.as_str(),
            job.kind,
            one_line(job)
        );
    }
    Ok(())
}

/// `forge job show <id>`.
pub(crate) fn show(project: &Project, args: &JobIdArgs) -> Outcome {
    let queue = queue(project)?;
    let id = JobId::from(args.id.clone());
    let job = queue
        .get(&id)
        .map_err(failure)?
        .ok_or_else(|| unknown(queue.as_ref(), &id))?;
    if args.json {
        let text = serde_json::to_string(&job)
            .map_err(|e| Failure::failed(format!("the row would not serialise: {e}")))?;
        println!("{text}");
        return Ok(());
    }
    println!("job       {}", job.id);
    println!("kind      {}", job.kind);
    println!("state     {}", job.state);
    if let Some(blocked) = &job.blocked_by {
        println!("blocked   {blocked}");
    }
    println!("executor  {}", job.executor);
    if let Some(backend) = &job.backend {
        println!("backend   {backend}");
    }
    println!("submitted {}", job.submitted);
    if let Some(started) = &job.started {
        println!("started   {started}");
    }
    if let Some(finished) = &job.finished {
        println!("finished  {finished}");
    }
    match job.exit {
        Some(code) => println!("exit      {code}"),
        // Never 0 by default: null means nobody observed one.
        None => println!("exit      null"),
    }
    if let Some(record) = &job.record {
        println!("record    {record}");
    }
    for output in &job.outputs {
        println!("output    {output}");
    }
    if job.cached {
        println!(
            "cached    true{}",
            job.same_as
                .as_ref()
                .map_or_else(String::new, |same| format!(" — same bytes as {same}"))
        );
    }
    if let Some(card) = &job.card {
        println!(
            "card      held {:.1} s{}{}",
            card.held_s,
            card.vram_before_gb
                .zip(card.vram_after_gb)
                .map_or_else(String::new, |(before, after)| format!(
                    ", {before:.1} → {after:.1} GB free"
                )),
            if card.restarted {
                ", the ComfyUI unit was restarted"
            } else {
                ""
            }
        );
    }
    if let Some(message) = &job.message {
        println!("message   {message}");
    }
    if let Some(hint) = &job.hint {
        println!("hint      {hint}");
    }
    println!("log       {}", job.log);
    Ok(())
}

/// `forge job log <id> [--follow]`.
pub(crate) fn log(project: &Project, args: &JobLogArgs) -> Outcome {
    let queue = queue(project)?;
    let id = JobId::from(args.id.clone());
    if queue.get(&id).map_err(failure)?.is_none() {
        return Err(unknown(queue.as_ref(), &id));
    }
    follow(queue.as_ref(), &id, args.from, args.follow)?;
    Ok(())
}

/// `forge job cancel <id>`.
pub(crate) fn cancel(project: &Project, args: &JobIdArgs) -> Outcome {
    let queue = queue(project)?;
    let id = JobId::from(args.id.clone());
    let job = queue.cancel(&id).map_err(failure)?;
    if args.json {
        let text = serde_json::to_string(&job)
            .map_err(|e| Failure::failed(format!("the row would not serialise: {e}")))?;
        println!("{text}");
        return Ok(());
    }
    println!("cancelled {}", job.id);
    if let Some(message) = &job.message {
        println!("note      {message}");
    }
    Ok(())
}

/// Print a job's log from `from`, optionally until the job is over.
///
/// Returns the offset it stopped at, so `forge gen` can say where to pick
/// the story up again.
pub(crate) fn follow(
    queue: &dyn Queue,
    id: &JobId,
    from: u64,
    until_done: bool,
) -> Result<u64, Failure> {
    let mut at = from;
    let mut out = std::io::stdout().lock();
    loop {
        let chunk = queue.log(id, at).map_err(failure)?;
        if !chunk.text.is_empty() {
            let _ = out.write_all(chunk.text.as_bytes());
            let _ = out.flush();
        }
        at = chunk.next;
        if !until_done || chunk.done {
            return Ok(at);
        }
        std::thread::sleep(FOLLOW_POLL);
    }
}

/// The one-line summary a listing shows.
fn one_line(job: &Job) -> String {
    if let Some(blocked) = &job.blocked_by {
        return format!("blocked by {blocked}");
    }
    if let Some(message) = &job.message {
        return crate::commands::first_line(message, 60);
    }
    if let Some(output) = job.outputs.first() {
        return output.clone();
    }
    job.argv.join(" ")
}

/// The queue counts, for `forge serve --status`.
pub(crate) fn print_queue(project: &Project) -> Outcome {
    let queue = queue(project)?;
    let status = queue.status().map_err(failure)?;
    println!(
        "queue     {} queued, {} blocked, {} running",
        status.queue.queued, status.queue.blocked, status.queue.running
    );
    for job in &status.jobs {
        println!(
            "          {} {} {}{}",
            job.id,
            job.state,
            job.kind,
            job.elapsed_s
                .map_or_else(String::new, |seconds| format!(" ({seconds:.0} s)"))
        );
    }
    Ok(())
}

/// A queue error as a failure: a refusal is the caller's to fix, everything
/// else is this side's.
pub(crate) fn failure(error: forge_serve::ServeError) -> Failure {
    match error {
        forge_serve::ServeError::Refused(_) | forge_serve::ServeError::NoSuchJob { .. } => {
            Failure::refused(error.to_string())
        }
        other => Failure::failed(other.to_string()),
    }
}

/// The refusal for an id nobody has, carrying the ids that do exist.
fn unknown(queue: &dyn Queue, id: &JobId) -> Failure {
    let existing: Vec<String> = queue
        .list(&JobFilter {
            limit: Some(10),
            ..JobFilter::default()
        })
        .unwrap_or_default()
        .into_iter()
        .map(|job| job.id.to_string())
        .collect();
    if existing.is_empty() {
        Failure::refused(format!("no job {id} — this project has no jobs yet"))
    } else {
        Failure::refused(format!(
            "no job {id} — the jobs that exist: {}",
            existing.join(", ")
        ))
    }
}
