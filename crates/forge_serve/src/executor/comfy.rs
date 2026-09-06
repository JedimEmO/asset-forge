//! `ComfyExecutor`: the same spawn, wrapped in the card ladder.
//!
//! Before: read `vram_free` from `GET /system_stats` — the number the row
//! records as `vram_before_gb`. After: `POST /free`, poll, restart the unit
//! once if the card has not come back, poll again, and **withhold the
//! lease** if it still has not.
//!
//! What this file does *not* do is as important as what it does. It never
//! posts a prompt, reads a history or fetches a view: the graph client is
//! Python (`python/forge_gen/comfy.py`), because the one process that
//! patched the graph is the one that writes the record, and a second record
//! writer is the thing this whole design refuses.

use std::sync::{Arc, Mutex};

use crate::job::CardFacts;
use crate::logs::LogSink;

use super::{CancelToken, Executor, GenOutcome, Launch, Plan, env};

/// The generator, run against the `ComfyUI` host, with the card ladder
/// around it.
pub(crate) struct ComfyExecutor;

impl Executor for ComfyExecutor {
    fn run(
        &self,
        plan: &Plan,
        launch: Launch,
        log: &Arc<Mutex<LogSink>>,
        cancel: &CancelToken,
        on_pid: &mut dyn FnMut(u32),
    ) -> GenOutcome {
        // A fake job writes a placeholder with the stdlib: it posts no
        // graph, loads no model and holds no VRAM, so reading the host's
        // `/system_stats` and calling `POST /free` around it would have
        // `ci-fake` and `mcp-session` reach into a developer's live host to
        // unload a model no job of theirs put there.
        if plan.fake {
            return env::spawn(plan, launch, log, cancel, on_pid);
        }
        let url = plan.comfy_url.clone().unwrap_or_default();
        let before = if url.is_empty() {
            None
        } else {
            crate::card::comfy_free_gb(&url)
        };
        if let Some(before) = before {
            env::say(log, &format!("card: {before:.1} GB free before the job"));
        }
        let started = std::time::Instant::now();
        let mut outcome = env::spawn(plan, launch, log, cancel, on_pid);
        if url.is_empty() {
            return outcome;
        }
        let release =
            crate::card::release_comfy(&url, plan.comfy_unit.as_deref(), before, |line| {
                env::say(log, line);
            });
        if let Some(after) = release.after_gb {
            env::say(log, &format!("card: {after:.1} GB free after the job"));
        }
        outcome.card = Some(CardFacts {
            held_s: started.elapsed().as_secs_f64(),
            vram_before_gb: before,
            vram_after_gb: release.after_gb,
            restarted: release.restarted,
        });
        outcome.note = release.note;
        outcome
    }
}
