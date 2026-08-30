//! The card: one exclusive `flock(2)`, a projection of who holds it, and
//! the ladder that gets it back from the `ComfyUI` host.
//!
//! # Why a lock and not a pidfile
//!
//! `out/serve/card.lock` is held with `flock(LOCK_EX)` for exactly as long
//! as the job runs. **The kernel releases it when the holder dies** — SIGKILL,
//! the OOM killer, a laptop lid — so a crashed generate cannot leave the
//! card claimed by a process that is not there. A pidfile can only ever be
//! *checked*, and every check is a race plus a story about a stale file;
//! `music.py`'s resident ACE-Step server shipped exactly that bug, where a
//! stale pid eventually named a stranger.
//!
//! **The lock is the truth and `card.json` is a projection** — the same
//! relation sidecars and the manifest already have. The sidecar is written
//! after the lock is taken and overwritten by the next acquirer, so a stale
//! one is harmless.
//!
//! The lease is taken by the daemon's worker **and** by
//! `commands/generate.rs` when no daemon is up. That is the whole answer to
//! "two doors race for the card": the door that is up takes the one lock. A
//! worker being singular is not the card lock — it holds nothing against a
//! second terminal.
//!
//! # What Rust knows about `ComfyUI`
//!
//! Exactly two endpoints, `GET /system_stats` and `POST /free`, because the
//! card must answer with no Python alive — after a crash, before the first
//! job, and inside `forge gpu`. The graph is Python's: nothing here posts a
//! prompt, reads a history or fetches a view.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use fs4::fs_std::FileExt as _;
use serde::{Deserialize, Serialize};

use crate::ServeError;
use crate::store::write_atomic;

/// How long `forge gpu --json` is believed before it is asked again.
const CARD_CACHE: Duration = Duration::from_secs(2);

/// How close to the **floor** counts as "the card came back". The resident
/// floor creeps — 1.09 to 1.53 GB over seven model swaps, measured — and
/// that must not trip the ladder.
const BACK_WITHIN_GB: f64 = 0.5;

/// What the card holds when the host has nothing loaded, in GB.
///
/// The number this whole ladder is judged against, so it is the measured
/// one with headroom: the `ComfyUI` unit idle holds ~0.4 GB of CUDA context,
/// creeping to ~0.7 GB after several model swaps, and the resident floor
/// with a desktop up measured 1.09 → 1.53 GB over seven swaps
/// (`designs/hosting.md`, `ComfyUI`, 2026-08-30). Two gigabytes is above the
/// largest of those and far below the smallest model the ladder exists for
/// — MOSS-VoiceGenerator at 5.3 GB.
///
/// **Why a floor and not the job's own `before`.** `before` is read seconds
/// before the job, so a model an *earlier* job left resident is inside it
/// and can never be seen: an MCP speech job went 16.44 → 16.38 GB free and
/// was released "clean" with 7.3 GB of MOSS still on the card, and
/// `forge gpu --free` printed "14.7 GB free before, 14.7 GB after … the
/// card is back" one line above "holding pid 693788 8.1 GB" (2026-08-30).
/// A test that compares a number against itself passes for the wrong
/// reason. `before` is still recorded on the row, because what the job
/// started with is worth knowing; it is just not the question.
const IDLE_FLOOR_GB: f64 = 2.0;

/// How long the ladder polls `/system_stats` before it restarts the unit.
pub const FREE_POLL_S: u64 = 15;

/// How often it polls.
const FREE_POLL_EVERY: Duration = Duration::from_millis(500);

/// `out/serve/card.json`, the projection.
#[must_use]
pub fn card_json_path(state_dir: &Path) -> PathBuf {
    state_dir.join("card.json")
}

/// `out/serve/card.lock`, the truth.
#[must_use]
fn card_lock_path(state_dir: &Path) -> PathBuf {
    state_dir.join("card.lock")
}

/// Who holds the card, for humans and for `forge gpu`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardState {
    /// A job id, `cli:<pid>`, or `foreign` when the host would not give the
    /// card back and the lease is being withheld.
    pub holder: String,
    /// The process holding it, when there is one.
    pub pid: Option<u32>,
    /// When it was taken.
    pub since: String,
    /// The backend's `vram_gb` **budget** — never a measurement.
    pub need_gb: Option<f64>,
    /// What is running: `moss_sfx sfx`.
    pub what: Option<String>,
    /// Why the card is withheld, when it is.
    pub note: Option<String>,
}

impl CardState {
    /// Read the projection, when there is one that parses.
    #[must_use]
    pub fn read(state_dir: &Path) -> Option<Self> {
        let bytes = std::fs::read(card_json_path(state_dir)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Whether the card is being withheld from card jobs, and why.
    ///
    /// The withholding lives in its own file rather than in `card.json`,
    /// because `card.json` is overwritten by whoever holds the lease next
    /// and a withholding must outlive an ordinary job that ran in between.
    /// `card.json` still *says* `foreign` whenever nothing else holds it —
    /// that is the projection doing its job.
    #[must_use]
    pub fn withheld(state_dir: &Path) -> Option<String> {
        std::fs::read_to_string(withheld_path(state_dir))
            .ok()
            .map(|note| note.trim().to_owned())
            .filter(|note| !note.is_empty())
    }

    /// Write the projection.
    fn write(&self, state_dir: &Path) -> Result<(), ServeError> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| ServeError::Io(format!("card.json: {e}")))?;
        write_atomic(&card_json_path(state_dir), text.as_bytes())
    }
}

/// Withhold the card: no card job runs until something proves it is free.
///
/// This is step 5 of the ladder, and it is deliberate. A daemon that hands
/// out a card it cannot prove is free produces an OOM three jobs later with
/// nothing naming the cause.
///
/// # Errors
///
/// [`ServeError::Io`] when the projection cannot be written.
pub fn withhold(state_dir: &Path, note: &str) -> Result<(), ServeError> {
    write_atomic(&withheld_path(state_dir), note.as_bytes())?;
    foreign_state(note).write(state_dir)
}

/// Clear a withholding — what `forge gpu --free` does once the card is back.
pub fn release_withhold(state_dir: &Path) {
    let _ = std::fs::remove_file(withheld_path(state_dir));
    if CardState::read(state_dir).is_some_and(|state| state.holder == "foreign") {
        let _ = std::fs::remove_file(card_json_path(state_dir));
    }
}

/// `out/serve/card.withheld` — the note that says no card job may start.
fn withheld_path(state_dir: &Path) -> PathBuf {
    state_dir.join("card.withheld")
}

/// The projection of a withheld card.
fn foreign_state(note: &str) -> CardState {
    CardState {
        holder: String::from("foreign"),
        pid: None,
        since: forge_library::clock::now_iso(),
        need_gb: None,
        what: None,
        note: Some(note.to_owned()),
    }
}

/// An exclusive hold on the card, released by dropping it — or by dying.
#[derive(Debug)]
pub struct CardLease {
    file: std::fs::File,
    state_dir: PathBuf,
    taken: Instant,
}

impl CardLease {
    /// Take the card if it is free right now, else `None`.
    ///
    /// # Errors
    ///
    /// [`ServeError::Io`] when the lock file cannot be opened.
    pub fn try_acquire(
        state_dir: &Path,
        holder: &str,
        need_gb: Option<f64>,
        what: Option<&str>,
    ) -> Result<Option<Self>, ServeError> {
        let path = card_lock_path(state_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ServeError::io(parent, &e))?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| ServeError::io(&path, &e))?;
        if !file.try_lock_exclusive().unwrap_or(false) {
            return Ok(None);
        }
        let lease = Self {
            file,
            state_dir: state_dir.to_path_buf(),
            taken: Instant::now(),
        };
        // After the lock, never before: the projection describes a hold that
        // already exists.
        CardState {
            holder: holder.to_owned(),
            pid: Some(std::process::id()),
            since: forge_library::clock::now_iso(),
            need_gb,
            what: what.map(str::to_owned),
            note: None,
        }
        .write(state_dir)?;
        Ok(Some(lease))
    }

    /// Take the card, waiting up to `max` for whoever has it.
    ///
    /// # Errors
    ///
    /// As [`Self::try_acquire`]; `Ok(None)` when `max` ran out.
    pub fn acquire(
        state_dir: &Path,
        holder: &str,
        need_gb: Option<f64>,
        what: Option<&str>,
        max: Duration,
    ) -> Result<Option<Self>, ServeError> {
        let deadline = Instant::now() + max;
        loop {
            if let Some(lease) = Self::try_acquire(state_dir, holder, need_gb, what)? {
                return Ok(Some(lease));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Seconds the lease has been held.
    #[must_use]
    pub fn held_s(&self) -> f64 {
        self.taken.elapsed().as_secs_f64()
    }
}

impl Drop for CardLease {
    fn drop(&mut self) {
        // The projection goes with the hold — unless something withheld the
        // card on purpose, in which case that note is what it says next.
        match CardState::withheld(&self.state_dir) {
            Some(note) => {
                let _ = foreign_state(&note).write(&self.state_dir);
            }
            None => {
                let _ = std::fs::remove_file(card_json_path(&self.state_dir));
            }
        }
        let _ = fs4::fs_std::FileExt::unlock(&self.file);
    }
}

/// One process on the card, as `forge gpu --json` labels it.
#[derive(Debug, Clone, PartialEq)]
pub struct CardApp {
    /// Its pid.
    pub pid: u32,
    /// The command `nvidia-smi` reports.
    pub name: String,
    /// What it holds, in GB.
    pub gb: f64,
    /// The backend whose env it ran from, when that can be told.
    pub backend: Option<String>,
}

/// The card as the existing reader sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct CardView {
    /// The card's name.
    pub name: String,
    /// Its total memory in GB.
    pub total_gb: f64,
    /// What is free in GB.
    pub free_gb: f64,
    /// Who is holding it.
    pub apps: Vec<CardApp>,
    /// The whole object, for `/v1/status`.
    pub raw: serde_json::Value,
}

impl CardView {
    /// The foreign process holding the most memory — who a blocked job
    /// waits on. `Some((pid, name, gb))`.
    #[must_use]
    pub fn largest_foreign(&self) -> Option<(u32, String, f64)> {
        self.apps
            .iter()
            .max_by(|a, b| a.gb.partial_cmp(&b.gb).unwrap_or(std::cmp::Ordering::Equal))
            .map(|app| {
                (
                    app.pid,
                    app.backend.clone().unwrap_or_else(|| app.name.clone()),
                    app.gb,
                )
            })
    }
}

/// `forge gpu --json`, cached for two seconds.
///
/// **There is no second `nvidia-smi` reader here.** `forge gpu` already
/// labels each holding process with the backend whose env it ran from, and
/// a second reader of that fact would be a second story about it.
#[derive(Debug)]
pub struct CardReader {
    forge: PathBuf,
    project: PathBuf,
    cached: Mutex<Option<(Instant, Option<CardView>)>>,
}

impl CardReader {
    /// A reader that re-invokes this binary.
    #[must_use]
    pub fn new(forge: PathBuf, project: PathBuf) -> Self {
        Self {
            forge,
            project,
            cached: Mutex::new(None),
        }
    }

    /// The card, or `None` when there is no `nvidia-smi` to ask — which is
    /// a runner, and a runner blocks nothing.
    #[must_use]
    pub fn read(&self) -> Option<CardView> {
        if let Ok(guard) = self.cached.lock()
            && let Some((at, view)) = guard.as_ref()
            && at.elapsed() < CARD_CACHE
        {
            return view.clone();
        }
        let view = self.read_uncached();
        if let Ok(mut guard) = self.cached.lock() {
            *guard = Some((Instant::now(), view.clone()));
        }
        view
    }

    /// Ask the binary, without the cache.
    fn read_uncached(&self) -> Option<CardView> {
        let output = std::process::Command::new(&self.forge)
            .arg("--project")
            .arg(&self.project)
            .arg("gpu")
            .arg("--json")
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let line = text
            .lines()
            .rev()
            .find(|line| line.trim().starts_with('{'))?;
        let raw: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
        let mb = |key: &str| raw.get(key).and_then(serde_json::Value::as_f64);
        let apps = raw
            .get("apps")
            .and_then(serde_json::Value::as_array)
            .map(|apps| {
                apps.iter()
                    .filter_map(|app| {
                        Some(CardApp {
                            pid: u32::try_from(app.get("pid")?.as_u64()?).ok()?,
                            name: app.get("name")?.as_str()?.to_owned(),
                            gb: app.get("used_mb")?.as_f64()? / 1024.0,
                            backend: app
                                .get("backend")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_owned),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(CardView {
            name: raw
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            total_gb: mb("total_mb").unwrap_or(0.0) / 1024.0,
            free_gb: mb("free_mb").unwrap_or(0.0) / 1024.0,
            apps,
            raw,
        })
    }
}

/// What the release ladder observed.
#[derive(Debug, Clone, PartialEq)]
pub struct CardRelease {
    /// Free VRAM after the ladder, when it could be read.
    pub after_gb: Option<f64>,
    /// The floor it was judged against: the card with nothing but the
    /// host's idle context on it. `None` when the host would not say how
    /// big the card is.
    pub floor_gb: Option<f64>,
    /// Whether the unit was restarted. Loud on purpose.
    pub restarted: bool,
    /// Whether free VRAM came back to within half a gigabyte of
    /// [`Self::floor_gb`] — **not** to what this job happened to start
    /// with, which is a number an earlier job's leftovers are already
    /// inside of.
    pub returned: bool,
    /// What to say in the log and, when it did not come back, in
    /// `card.json`.
    pub note: Option<String>,
}

/// Free VRAM in GB as `ComfyUI` reports it.
///
/// Believe `/system_stats` for *free* and `nvidia-smi` for *peak*: the host
/// reports what torch has allocated now, which is nowhere near the peak of
/// a run.
#[must_use]
pub fn comfy_free_gb(base_url: &str) -> Option<f64> {
    comfy_vram_gb(base_url).map(|(free, _)| free)
}

/// Free and total VRAM in GB, from one `GET /system_stats`.
///
/// The total is what makes a *floor* possible, and a floor is what "the
/// card is back" has to be a claim about: the card's size is the only
/// fixed thing in the measurement.
#[must_use]
pub fn comfy_vram_gb(base_url: &str) -> Option<(f64, Option<f64>)> {
    let response = crate::wire::get(
        &format!("{}/system_stats", base_url.trim_end_matches('/')),
        None,
        Duration::from_secs(5),
    )
    .ok()?;
    let value = response.json()?;
    let device = value.get("devices")?.as_array()?.first()?;
    let free = device.get("vram_free")?.as_f64()?;
    let total = device.get("vram_total").and_then(serde_json::Value::as_f64);
    Some((
        free / 1_073_741_824.0,
        total.map(|bytes| bytes / 1_073_741_824.0),
    ))
}

/// The free VRAM an idle host shows, from the card's own size.
///
/// `None` when `/system_stats` did not say how big the card is: a floor
/// nobody can compute is unknown, never a guess.
#[must_use]
pub fn idle_floor_gb(total_gb: Option<f64>) -> Option<f64> {
    total_gb
        .filter(|total| *total > IDLE_FLOOR_GB)
        .map(|total| total - IDLE_FLOOR_GB)
}

/// The ladder of `designs/serve.md` §5, run after a comfy child exits.
///
/// `POST /free` → poll → `systemctl --user restart <unit>` → poll →
/// **withhold**. Steps 4 and 5 are a safety net: `hosting.md` records
/// `/free` giving the card back on both spike runs, so a restart here is a
/// line worth reading and never routine.
pub fn release_comfy(
    base_url: &str,
    unit: Option<&str>,
    before_gb: Option<f64>,
    say: impl FnMut(&str),
) -> CardRelease {
    let unit = unit.unwrap_or(DEFAULT_UNIT).to_owned();
    let name = unit.clone();
    let mut restart = move || {
        std::process::Command::new("systemctl")
            .arg("--user")
            .arg("restart")
            .arg(&unit)
            .status()
            .is_ok()
    };
    release_comfy_with(base_url, &name, &mut restart, before_gb, FREE_POLL_S, say)
}

/// The unit the ladder restarts when nothing names another.
const DEFAULT_UNIT: &str = "forge-comfy.service";

/// The ladder itself, with the restart and the patience handed in.
///
/// Production passes `systemctl --user restart <unit>` and
/// [`FREE_POLL_S`] seconds of patience; the crate's own test passes a closure that counts and
/// one second, because what has to be proved is "once, and then the lease
/// is withheld" — not that `systemctl` can be shadowed on `PATH`, and not
/// that a test runner can wait three quarters of a minute for it.
pub fn release_comfy_with(
    base_url: &str,
    unit: &str,
    restart: &mut dyn FnMut() -> bool,
    before_gb: Option<f64>,
    poll_s: u64,
    mut say: impl FnMut(&str),
) -> CardRelease {
    let base = base_url.trim_end_matches('/');
    // The floor is read once, from the same endpoint the free number comes
    // from, and it is a property of the card rather than of this job.
    let floor = idle_floor_gb(comfy_vram_gb(base).and_then(|(_, total)| total));
    let back = move |free: Option<f64>| match (free, floor) {
        (Some(now), Some(floor)) => now + BACK_WITHIN_GB >= floor,
        // A floor nobody could compute is not evidence either way, so the
        // ladder falls back to the weaker question it can answer — did this
        // job give back what it took — and every message says which of the
        // two numbers it used.
        (Some(now), None) => before_gb.is_none_or(|before| now + BACK_WITHIN_GB >= before),
        (None, _) => false,
    };
    let against = floor.map_or_else(
        || {
            format!(
                "{} GB before this job (no floor: the host did not say how big the card is)",
                gb(before_gb)
            )
        },
        |floor| format!("a {floor:.1} GB idle floor on this card"),
    );
    let _ = crate::wire::post(
        &format!("{base}/free"),
        None,
        "{\"unload_models\": true, \"free_memory\": true}",
        Duration::from_secs(10),
    );
    let mut free = poll_free(base, &back, poll_s);
    if back(free) {
        return CardRelease {
            after_gb: free,
            floor_gb: floor,
            restarted: false,
            returned: true,
            note: None,
        };
    }

    say(&format!(
        "the card did not come back after /free ({} GB free against {against}) — restarting \
         {unit} once",
        gb(free),
    ));
    let restarted = restart();
    free = poll_free(base, &back, poll_s * 2);
    if back(free) {
        say(&format!(
            "the card came back after the restart ({} GB free against {against})",
            gb(free)
        ));
        return CardRelease {
            after_gb: free,
            floor_gb: floor,
            restarted,
            returned: true,
            note: None,
        };
    }
    let note = format!(
        "the ComfyUI host still holds the card after /free and a restart of {unit} ({} GB free \
         against {against}; this job started with {} GB free). no card job will start until \
         something proves it is free: `forge gpu --free`, or \
         `systemctl --user status {unit}`.",
        gb(free),
        gb(before_gb),
    );
    say(&note);
    CardRelease {
        after_gb: free,
        floor_gb: floor,
        restarted,
        returned: false,
        note: Some(note),
    }
}

/// One GB figure for a message, or the word for not having one.
fn gb(value: Option<f64>) -> String {
    value.map_or_else(|| String::from("unknown"), |gb| format!("{gb:.1}"))
}

/// Poll `/system_stats` until the card is back or the seconds run out.
fn poll_free(base: &str, back: &dyn Fn(Option<f64>) -> bool, seconds: u64) -> Option<f64> {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut last = comfy_free_gb(base);
    loop {
        if back(last) {
            return last;
        }
        if Instant::now() >= deadline {
            return last;
        }
        std::thread::sleep(FREE_POLL_EVERY);
        last = comfy_free_gb(base);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lease_is_exclusive_in_one_process_and_the_projection_follows_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = dir.path();
        let first = CardLease::try_acquire(state, "j-one", Some(8.0), Some("moss_sfx sfx"))
            .expect("acquire")
            .expect("free");
        let projection = CardState::read(state).expect("card.json");
        assert_eq!(projection.holder, "j-one");
        assert_eq!(projection.need_gb, Some(8.0));
        drop(first);
        assert!(
            CardState::read(state).is_none(),
            "the projection goes with the hold"
        );
        let second = CardLease::try_acquire(state, "j-two", None, None)
            .expect("acquire")
            .expect("free again");
        drop(second);
    }

    #[test]
    fn a_withheld_card_stays_withheld_until_it_is_cleared() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = dir.path();
        withhold(state, "the host did not give it back").expect("withhold");
        assert!(CardState::withheld(state).is_some());
        // A lease taken and dropped does not quietly clear a withholding.
        let lease = CardLease::try_acquire(state, "j-three", None, None)
            .expect("acquire")
            .expect("free");
        drop(lease);
        assert!(CardState::withheld(state).is_some());
        release_withhold(state);
        assert!(CardState::withheld(state).is_none());
    }
}
