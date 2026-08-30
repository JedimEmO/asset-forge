# Phase 1 — `forge serve`: one queue, one card, one door

Written 2026-08-30 against `designs/forge2.md` Phase 1, after Phase 0's
three yeses, and synthesised from three drafts and two reviews.
`decisions.md` wins over this file; every rule in `CLAUDE.md` applies to
every line of it.

The daemon exists because two doors now share one 24 GB card and one
library, and because a generate that takes four minutes cannot be a
blocking tool call. Two sentences hold the whole shape together and are
worth pasting into the crate's module doc:

> **Nothing in Rust learns what a graph is, and nothing in Python learns
> what the queue is.**

Nothing under `assets/` moves. Not one clip is rebaked. The library
sidecar stays `schema: 1`. `run_fake` is not touched, so `ci-fake` proves
the same thing on the day this lands as the day before.

## 0. The failures this phase is designed around

Each one names the handling and the test that proves it, because a
handling with no test is a paragraph.

| failure | where it bites | the answer | the test |
|---|---|---|---|
| two doors race for the card | a terminal `just sfx` while an agent's `generate_audio` runs | one exclusive `flock(2)` on `out/serve/card.lock`, taken by the daemon's worker **and** by `commands/generate.rs` when no daemon is up | `card_lease_is_exclusive_across_processes` |
| the holder dies without releasing | SIGKILL, the OOM killer, a laptop lid | `flock` is released by the kernel on death; no pidfile is ever the authority | `a_killed_holder_releases_the_card` |
| a foreign holder of the card | a studio window, the ComfyUI unit mid-swap, a spike script | the job sits in `blocked` with `blocked_by` naming the pid and its GB — a wait with a name, never an OOM | `a_foreign_holder_blocks_rather_than_ooms` |
| two jobs for one output path | an agent and a human both asking for `out/audio/sfx/door.wav` | the out-path lease at admission: the second submit is refused naming the first job's id | `two_jobs_for_one_output_path_are_refused` |
| VRAM does not come back from comfy | a pack that loads outside ComfyUI's memory manager | `/free` → poll → `systemctl --user restart <unit>` → poll → **withhold the lease** and say who holds it | `vram_that_does_not_return_restarts_the_unit_then_withholds` |
| a restart mid-job | a daemon crash, `just ci` on a dev box | a `running` row becomes `interrupted` with `exit: null`. Never resumed, never completed by inference | `an_interrupted_row_is_never_completed_by_inference` |
| a record that lies | ComfyUI serving a "re-roll" from its node cache; the daemon inventing what it did not observe | the daemon **never writes a generator record**; the generator does, and it echoes back what it patched | `records_are_written_only_by_the_generator`, `a_cached_result_says_so` |
| a generate blocks an MCP call for minutes | every agent session | jobs; `wait` has a ceiling; `mcp-session` in CI over both transports | `mcp_session_stdio`, `mcp_session_http` |

---

## 1. The `forge_serve` crate

New crate `crates/forge_serve/`, `publish = false` — it is not a library a
game depends on. It depends on `forge_library`, `tokio`, `axum`, `tower`,
`serde`, `serde_json`, `fs4` (flock), `hyper`/`hyper-util` (three loopback
endpoints, no TLS), `tempfile`. **It does not depend on `rmcp`.**
`forge_mcp` depends on *it*, and the `forge` binary composes the two; that
is the only direction with no cycle, it keeps rmcp's `=1.8.0` pin out of
the daemon's build, and it keeps `forge_serve`'s tests seconds long.

```
crates/forge_serve/src/
  lib.rs        Queue trait, Job/JobSpec/JobState, ServeError, state_dir()
  job.rs        the JSON shape of a row — serde only
  store.rs      the state directory: atomic row writes, restart reconciliation, pruning
  queue.rs      LocalQueue: the FIFO, the single worker, admission, cancellation
  card.rs       CardLease (flock), the free-VRAM read, the comfy free/restart ladder
  logs.rs       per-job log sink, byte-offset tailing, the size cap
  client.rs     RemoteQueue: the HTTP client of a running daemon
  discovery.rs  find(project) -> Option<RemoteQueue>
  executor/mod.rs   trait Executor, ExecutorKind, Refusal, Outcome
  executor/env.rs   spawns python/forge_gen exactly as commands/generate.rs does
  executor/comfy.rs the same spawn, wrapped in the card ladder
  http.rs       the axum router (jobs, status, runs, health); /mcp is nested by the caller
  runs.rs       the out/ walk: list_runs, reading the record beside each output
```

### 1.1 The job table

One row per job, one log file, one record path. On disk as
`out/serve/jobs/<id>.json`, written atomically (temp sibling + `rename`,
exactly `records.py`'s discipline) on every transition:

```json
{
  "forge_job": 1,
  "id": "j-20260830-141207-3f9a",
  "kind": "generate_audio.sfx",
  "executor": "comfy",
  "backend": "moss_sfx",
  "argv": ["sfx", "--prompt", "a heavy iron door", "--seconds", "3",
           "--out", "out/audio/sfx/door.wav", "--record", "out/audio/sfx/door.json",
           "--created-by", "agent:claude", "--json"],
  "outputs_claimed": ["out/audio/sfx/door.wav"],
  "state": "done",
  "blocked_by": null,
  "submitted": "2026-08-30T14:12:07Z",
  "started":   "2026-08-30T14:12:09Z",
  "finished":  "2026-08-30T14:12:41Z",
  "exit": 0,
  "record": "out/audio/sfx/door.json",
  "outputs": ["out/audio/sfx/door.wav"],
  "log": "out/serve/logs/j-20260830-141207-3f9a.log",
  "created_by": "agent:claude",
  "pid": 481233,
  "cached": false,
  "same_as": null,
  "message": null,
  "hint": null,
  "card": {"held_s": 32.1, "vram_before_gb": 22.4, "vram_after_gb": 22.4, "restarted": false},
  "comfy": {
    "template": "backends/moss_sfx/workflows/sfx.api.json",
    "template_sha256": "sha256:9f1c…",
    "inputs": {"prompt": "a heavy iron door", "seconds": 3.0, "seed": 815273},
    "prompt_id": "b1f0…"
  }
}
```

`kind` is `<tool>.<verb>` so `list_runs`, `status` and the Phase 4 TUI can
group without parsing argv. `comfy` is `null` for an `env` job, and — this
is the mechanism, not a detail — **every field in it is echoed back by the
child in its JSON last line, never composed by the daemon.** The one
process that patched the graph is the one that writes the record; the
daemon copies what it was told and observes nothing it did not run.

`state` is one of:

| state | means | `exit` |
|---|---|---|
| `queued` | admitted, waiting for the card | `null` |
| `blocked` | at the head of the queue, a foreign process holds the card; `blocked_by` names it | `null` |
| `running` | the child is alive | `null` |
| `done` | exit 0 | `0` |
| `refused` | exit 2, 3, 4 or 6 — the call was wrong, the backend is absent, the input was rejected. The fix is the caller's next turn and `hint` names it | `2\|3\|4\|6` |
| `failed` | exit 5 — the backend ran and broke. The fix is a log | `5` |
| `cancelled` | SIGTERM to the process group, SIGKILL after 10 s | `null` |
| `interrupted` | the daemon restarted and its child was gone. Nobody knows what happened | `null` |

`refused` and `failed` are separate because the exit-code table already
carries the distinction and collapsing them costs an agent a wasted retry
on every missing backend. `exit` is **never 0 by default** — that is the
`null`-means-unknown rule applied to a process. `message` and `hint` carry
the Python refusal's own words, relayed unchanged; a hint is the next
command to type, and nothing here composes one.

### 1.2 The queue and admission

One FIFO, one worker, `concurrency = 1` and not configurable: a second
worker would only queue behind the lease, at the cost of a second place
for state to disagree. Admission does two things before a row reaches
`queued`, both of them cheap and both of them before the card:

1. **Backend resolution** from `forge_library::backends::Backends`. A
   missing backend is `refused` with exit 3 in under a millisecond, so the
   queue never holds a job that cannot run. **Every generate names its
   backend**, from the one map `forge_library::project::backend_for_verb`
   — `spec_for` derives it from `argv[0]` for the terminal door and the
   MCP tools read the same function — and a `generate_*` spec that names
   none is refused before a row exists. A **fake** job is the one
   exception to the installed check: `run_fake` never imports the backend,
   never reads a template and never resolves a URL, so tier `fake` works
   on a machine that has nothing installed, which is the machine it is
   for.
2. **The out-path lease.** Every job declares `outputs_claimed`. A submit
   whose path is already claimed by a `queued`, `blocked` or `running` row
   is refused naming that row's id. Two doors asking for
   `out/audio/sfx/door.wav` a second apart is the ordinary case here, not
   the exotic one.

**A fake job is an ordinary job.** `FORGE_FAKE=1` and tier `fake` change
nothing about admission, the FIFO or the lease — a placeholder costs
nothing to serialise, and letting fakes skip the queue would quietly lose
`ci-fake` the serialisation it has today.

There is no idempotency key and no append-only transition log in this
phase. Neither retires a named risk, both add state that can disagree with
the rows, and the reader that would want a transition history is `forge
top` in Phase 4.

### 1.3 The card lock

`CardLease` is an exclusive `flock(2)` on `out/serve/card.lock`, plus a
JSON sidecar `out/serve/card.json` written *after* the lock is taken:

```json
{"holder": "j-20260830-141207-3f9a", "pid": 481233, "since": "2026-08-30T14:12:09Z",
 "need_gb": 8.0, "what": "moss_sfx sfx", "note": null}
```

**The lock is the truth and `card.json` is a projection** — the same
relation sidecars and the manifest already have. That is what makes a
killed holder harmless: the kernel drops the flock, and the next acquirer
overwrites the stale sidecar.

The lease is taken by the daemon's worker *and* by `commands/generate.rs`
when no daemon is up. That is the whole answer to "two doors race for the
card": the door that is up takes the one lock. An in-process worker being
singular is not the card lock — it holds nothing against a second terminal.

Before the lease is handed out the daemon reads the card through the
existing reader — `forge gpu --json`, which already labels each holding
process with the backend whose env it ran from — cached for 2 s. If free
VRAM is under the backend's `vram_gb` **budget** and a foreign pid holds
the difference, the row goes `blocked` with
`blocked_by: "pid 4411 forge studio, 8.1 GB"` rather than starting and
OOM-ing. `vram_gb` is a budget and is never quoted back as a measurement;
the blocked message says which it is.

### 1.4 Persistence and restart

State dir: **`<project>/out/serve/`**, not XDG. The queue is per project,
its logs name paths under `out/`, and `rm -rf out/` should lose a job log
and the artefact it describes *together* rather than leave one orphaned
story about the other.

```
out/serve/
  daemon.json        the endpoint file, below
  daemon.lock        exclusive flock while a daemon is up (a second forge serve exits 1 naming the first)
  card.lock          the card lease
  card.json          who holds it, for humans and for forge gpu
  jobs/<id>.json     the row
  logs/<id>.log      the child's interleaved stdout+stderr, capped at 8 MB (head kept, middle elided once)
```

```json
{"forge_serve": 1, "pid": 40988, "start_ticks": 918273, "port": 38411,
 "token": "7c3e…", "url": "http://127.0.0.1:38411", "version": "0.1.0",
 "project": "/home/mmy/games/mygame", "started": "2026-08-30T14:20:02Z"}
```

`daemon.json` is mode 0600 and carries the pid **and** the kernel start
time (`/proc/<pid>/stat` field 22). `music.py` already learned that a pid
file without one eventually signals a stranger, and that lesson comes
across intact.

On start the daemon reconciles every row:

- `queued` and `blocked` → re-queued in `submitted` order. **They never
  ran and derived nothing**; the one-way rule is about a file that exists,
  and there is no half-written `.glb` here to repair.
- `running` → `interrupted`, `exit: null`, with a message naming the
  restart. Not for an `env` child and not for a `comfy` one. A restarted
  daemon cannot `waitpid` on a process it did not fork and cannot read a
  pipe that died with its parent, so it can observe neither the exit code
  nor the last JSON line; and finishing a comfy row out of
  `GET /history/{prompt_id}` would drag `/view` fetching, `measure_wav`
  and a record write into Rust — the second record writer this whole
  design refuses. Whatever record the generator managed to write is found
  by `list_runs`, which is the correct authority, and the fix is a re-run
  from the spec.
- terminal rows → left alone.

`interrupted` never becomes `done`. Rows and their logs are pruned
together at start once they are older than 14 days (`log_keep_days`), so
`out/serve/jobs/` is not a directory that only grows.

### 1.5 The executor trait and its two impls

```rust
pub(crate) trait Executor: Send + Sync {
    fn kind(&self) -> ExecutorKind;                                     // Env | Comfy
    fn preflight(&self, spec: &JobSpec, backends: &Backends) -> Result<Plan, Refusal>;
    fn card_need_gb(&self, plan: &Plan) -> Option<f64>;                 // None = no card
    fn run(&self, plan: &Plan, log: &mut LogSink, cancel: &CancellationToken) -> Outcome;
    fn release_card(&self, plan: &Plan, before_gb: f64) -> CardRelease;  // free + verify + restart
}
pub struct Outcome { pub exit: i32, pub payload: Option<Value>, pub pid: Option<u32> }
```

Blocking, run on `spawn_blocking` from the worker: both are
subprocess-shaped and async buys nothing.

**`EnvExecutor`** is today's launcher call, unmoved:
`python3 <toolkit>/python/forge_gen <argv> --project <root> --json`, with
`FORGE_RIG_PROFILE` set from the project unless the caller set it, stdout
streamed line by line into the log with the last line held back and parsed
as the payload, stderr into the same log — the exact rules
`crates/forge/src/commands/generate.rs::spawn_with` implements today,
lifted into `forge_serve::executor::env` so the daemon and the CLI call
one function. It spawns in its own process group (`process_group(0)`) so
cancel kills the tree, and it kills by **recorded pid**, never
`pkill -f` — that trap is already in `CLAUDE.md`.

**`ComfyExecutor`** is the same spawn wrapped in the card ladder. One job
shape, one log path, one cancel path, one refusal shape.

Every child gets `FORGE_NO_DAEMON=1` in its environment, so a re-entered
`forge gen` never rediscovers the daemon and recurses.

### 1.6 Where the ComfyUI client lives

The graph client is **Python**, a new stdlib-only module
`python/forge_gen/comfy.py`, cut from `spike_reference.py`'s already-working
half (`_get`, `_get_bytes`, `_post_json`, `upload_image`, `patch`,
`run_one`, `fetch_images`, `load_template`), which is then deleted. Four
reasons, in the order they matter:

1. **The record writer must not fork.** `records.py` is the one place that
   knows key order, sorted free-form maps, the atomic write and the
   `None`-means-unknown rule, and `crates/forge_library/tests/python_records.rs`
   pins the two writers to the same bytes. A Rust graph client would make
   a *third* writer of one schema, and this ledger already records what
   three readers of one record format did to each other.
2. **One executor mechanism.** With the graph in Python both executors are
   "spawn `forge gen <verb>`, stream the log, hold the last JSON line,
   read the exit code", and `just sfx` with no daemon up runs the exact
   same code. The daemon is a scheduler, not a second implementation.
3. **`FORGE_FAKE=1` stays untouched.** `run_fake` never imports `comfy`,
   never reads a template and never resolves a URL, so `ci-fake` is a
   control for this whole phase.
4. **The spike code already ran on the real host** on 2026-08-30.

The **card** half is Rust, in `forge_serve::card`, because the card lock
must work with no Python process alive — after a crash, before the first
job, and inside `forge gpu`.

| endpoint | who calls it | why |
|---|---|---|
| `POST /upload/image`, `POST /prompt`, `GET /history/{id}`, `GET /view` | Python, `forge_gen/comfy.py` | these are the graph and the result, and the result becomes a record |
| `GET /object_info` | Python, `forge_gen/doctor.py` through `comfy.py` | backend probing lives in the Python doctor today and stays there; fetched **once per doctor run** and shared across every comfy backend, because on a cold host a per-backend fetch would be six |
| `GET /system_stats`, `POST /free` | Rust, `forge_serve::card` | the card's business, and it must answer with no Python alive |

Exactly one caller per endpoint. Python does not read `/system_stats` and
does not call `/free`: the lease holder does both around the job, whether
that is the daemon or `commands/generate.rs`.

The comfy job flow, per job:

1. Rust: take the lease, read `vram_free` as `vram_before_gb`.
2. Python: load the template, hash the **tracked file**, patch inputs,
   `POST /prompt` with a per-job `client_id` and a `filename_prefix`
   carrying the job id, poll `/history/{id}`, `GET /view` the outputs,
   write them, measure, write the record, print the JSON last line with
   its `comfy` block.
3. Rust: the ladder of §5, then release the lease.

### 1.7 `forge_record: 2`

`SCHEMA = 2` in `python/forge_gen/records.py`; `RECORD_SCHEMA = 2` and
`RECORD_SCHEMA_MIN = 1` in
`crates/forge_library/src/generator_record.rs`. **Both readers accept 1
and 2, and only 2 is ever written.** `records.py:246` and `:320` raise on
any schema but their own today, so without this the writer bump breaks
every read of a shipped v1 record; `generator_record.rs`'s
`a_newer_or_absent_schema_is_refused` test asserts today that
`forge_record: 2` is refused and must be rewritten in the same commit.

Nothing under `assets/` or `assets-src/` is rewritten — not by a
migration, not by hand. Adding four nulls to a shipped sidecar is churn
with no new fact in it. **Only v2 fixtures are held to byte equality** in
`python_records.rs`; a v1 fixture is a read-only case, because `Option`
serialises as `null` and a v1 record round-tripped through the Rust writer
would grow four keys.

The `backend` block gains four keys, appended after the existing six so
key order stays a prefix of what it was:

```json
"backend": {
  "name": "moss_sfx",
  "commit": "58b20a0d…",
  "python": "3.12.7",
  "torch": "2.13.0+cu130",
  "model": "MOSS-SoundEffect-v2.0",
  "model_revision": null,
  "executor": "comfy",
  "comfyui_commit": "169fcf35a2fc163fec31338b816503ddac0d3fcf",
  "workflow_sha256": "sha256:9f1c…",
  "packs": {"https://github.com/diodiogod/TTS-Audio-Suite": "b7e41a2c…"}
}
```

`BACKEND_KEYS` becomes, in this order:

```
("name", "commit", "python", "torch", "model", "model_revision",
 "executor", "comfyui_commit", "workflow_sha256", "packs")
```

- `executor` is `"env" | "comfy"` and is written for **every** record,
  env ones included: a record that says nothing about its executor is one
  nobody can group later.
- For an `env` run the other three are `null`. A default is never written
  as a measurement, and an env run genuinely has no workflow.
- `commit` stays the *generator's* upstream commit where one exists, and
  is `null` for a comfy backend with no checkout of its own — writing the
  ComfyUI commit twice under two names would be the four-places-or-nowhere
  trap in miniature.
- `workflow_sha256` hashes the **tracked template file**, not the patched
  graph. A reader can go and find a tracked file; nobody can check a hash
  of bytes that were never written down. The patch is knobs, and knobs
  live in `params`: `params` gains `workflow: "sfx.api.json"` and every
  patched input beside what it already carries.
- `packs` is `{repo: commit}`, an empty object for native nodes.

---

## 2. The daemon's HTTP API

`axum` on `127.0.0.1`, an ephemeral port by default (the kernel picks; the
real port is written to `daemon.json`). One listener, not two: a unix
socket beside it would mean two auth stories and two client paths for one
loopback queue, and the MCP-over-HTTP client needs the port anyway. Every
route requires `Authorization: Bearer <token>` from `daemon.json` and a
`Host` of `127.0.0.1`/`localhost` — loopback alone is not access control
on a shared box, and the thing behind this port drives a GPU.

| method | route | body / result |
|---|---|---|
| `GET` | `/v1/health` | `{"ok":true,"forge_serve":1,"version":"0.1.0","project":"…","pid":…,"started":"…","uptime_s":…}` |
| `GET` | `/v1/status` | the object below; the `status` tool renders it |
| `POST` | `/v1/jobs` | `{kind, backend?, args{}, created_by}` → `201` with the job row |
| `GET` | `/v1/jobs?state=&kind=&limit=` | rows, newest first |
| `GET` | `/v1/jobs/{id}` | the row; 404 is a refusal naming the ids that exist |
| `GET` | `/v1/jobs/{id}/log?from=<byte>` | `text/plain`, header `X-Forge-Log-Next: <byte>`; SSE with `?follow=1` |
| `POST` | `/v1/jobs/{id}/cancel` | `{"cancelled":true,"was":"running","note":"…"}` |
| `GET` | `/v1/runs?kind=&since=&limit=` | the `out/` walk of §7 |
| `GET` | `/v1/backends` | doctor's per-backend rows, the cheap half |
| `ANY` | `/mcp` | MCP over streamable HTTP |

```json
{"project": "/home/mmy/games/mygame", "tier": "full",
 "comfy_url": "http://127.0.0.1:8188",
 "card": {"name": "NVIDIA GeForce RTX 4090", "total_gb": 23.5, "free_gb": 21.9,
          "holder": null,
          "apps": [{"pid": 4123, "name": "forge studio", "gb": 0.9, "backend": null}]},
 "queue": {"queued": 1, "blocked": 0, "running": 1},
 "jobs": [{"id": "j-…", "kind": "generate_audio.music", "state": "running",
           "elapsed_s": 91.2, "position": 0, "log_tail": ["[music] task 3 submitted"]}],
 "recent": [{"id": "j-…", "kind": "generate_audio.sfx", "state": "done", "finished": "…"}],
 "daemon": {"up": true, "port": 41773, "started": "2026-08-30T13:02:11Z", "version": "0.1.0"}}
```

The `card` block is `forge gpu --json` re-invoked and cached for 2 s — the
existing `nvidia-smi` reader, which already labels a holding process with
the backend whose env it ran from. A second reader of that fact is a
second story about it.

**MCP over streamable HTTP** is
`rmcp::transport::streamable_http_server::StreamableHttpService` (feature
`transport-streamable-http-server`, present in the pinned rmcp 1.8.0)
wrapping the *same* handler `forge mcp` serves over stdio, nested as a
tower service by the binary:

```rust
let mcp = StreamableHttpService::new(
    move || Ok(forge_mcp::handler(mcp_config.clone())),
    LocalSessionManager::default().into(),
    StreamableHttpServerConfig::default().with_stateful_mode(true),
);
let app = forge_serve::http::router(state).nest_service("/mcp", mcp);
```

**The router does not move and is not duplicated.** `forge_mcp` keeps
`tools/`, `ForgeServer` gains one field `queue: Arc<dyn Queue>`, and
`tools::router()` is unchanged as a sum. `forge mcp` stays a thin adapter:
find the project, resolve a queue, serve the identical router over stdio.
Same tools, same instructions text, same frames, whichever door the client
came through.

### The `Queue` trait

```rust
pub trait Queue: Send + Sync {
    fn submit(&self, spec: JobSpec) -> Result<Job, ServeError>;
    fn get(&self, id: &JobId) -> Result<Option<Job>, ServeError>;
    fn list(&self, filter: &JobFilter) -> Result<Vec<Job>, ServeError>;
    fn cancel(&self, id: &JobId) -> Result<Job, ServeError>;
    fn log(&self, id: &JobId, from: u64) -> Result<LogChunk, ServeError>;
    fn status(&self) -> Result<Status, ServeError>;
    fn runs(&self, filter: &RunFilter) -> Result<Vec<Run>, ServeError>;
    /// Block until terminal or `max` elapses; the ceiling is the caller's.
    fn wait(&self, id: &JobId, max: Duration) -> Result<Job, ServeError>;
}
```

Two impls: `LocalQueue` (owns the worker and the state directory) and
`RemoteQueue` (the HTTP client). The CLI and `forge_mcp` hold an
`Arc<dyn Queue>` and **neither knows whether a daemon exists**. That is
the unification that matters, and it buys `mcp-session` for free: the gate
runs on a `LocalQueue` inside the MCP process, which is exactly the
no-daemon path a stranger's first session takes.

---

## 3. The CLI as a client

`forge serve [--foreground] [--port N] [--idle-exit S] [--no-mcp]
[--stop] [--status]`. **`--foreground` is the only in-shell mode**: it logs
to stderr and stays. Without it `forge serve` re-execs itself with
`--foreground` in a process group of its own, stdin closed and both streams
appended to `out/serve/daemon.log`, waits for the child to write
`daemon.json`, prints the port and exits 0 — so `just serve` gives the
terminal back. The child holds `daemon.lock`, and a second `forge serve`
exits naming the first pid. `--idle-exit S` exists because a daemon a
stranger starts by accident should not outlive the session. (The first form
of this section said `--foreground` was "the default when stdout is a TTY,
otherwise it daemonises"; nothing detached at all — the flag chose one log
line — and the real run had to launch the daemon under `setsid nohup`,
2026-08-30.)

**Stopping is a cancel, not a drain.** `forge stop`, `^C` and SIGTERM all
cancel what is running — SIGTERM to the recorded pid's process group,
SIGKILL after the grace — and wait for the worker to write the terminal row
before the process exits. A daemon that exits with its generator alive
drops `card.lock` while the card is still held, and the next door takes the
lease against a running generate; the row is then stamped `interrupted` by
a later start although nothing interrupted it. For the same reason
`reconcile` leaves a `running` row alone while its pid is alive, and
`card_is_held` blocks the next card job behind that pid by name.

`forge_serve::discovery::find(project)`:

1. `$FORGE_SERVE_URL` — a daemon elsewhere, or `off` to force in-process.
2. `<project>/out/serve/daemon.json` → the pid is alive **and** its
   `/proc` start time equals the recorded one **and** `GET /v1/health`
   answers within 300 ms with a matching `project` and `version`.
   → `RemoteQueue`.
3. Otherwise the file is removed when its pid is dead, and the queue is a
   `LocalQueue` in this process — a queue of one, taking the same
   `card.lock`.

`--no-daemon` and `FORGE_NO_DAEMON=1` force (3), and the executors set the
latter in every child. `FORGE_FAKE=1` does not change discovery.

| | daemon up | daemon down |
|---|---|---|
| `forge gen sfx …` | submits, follows the log to stdout with the same line-holding rule, exits the job's exit code, prints the same summary; a `queued behind j-… (position 2)` line when it waits | in-process `LocalQueue`, takes `card.lock`, runs, **writes a job row anyway** so `status` and `list_runs` see it later |
| `^C` on a followed job | **cancels it**, as it does today | as today |
| `forge gen --help` | never submits — argparse's answer needs no queue | same |
| `forge doctor`, `verify`, `promote …` | unchanged; no GPU, no job | unchanged |
| `just sfx door "…"` | unchanged text; queues behind whatever the agent started thirty seconds ago | unchanged |
| MCP `generate_audio` | the daemon's queue | the MCP process's own `LocalQueue` |

`^C` cancels rather than detaches: a human who interrupts a 111-second
image expects the card back, today's `forge gen` kills the child, and
leaving the card held by a job the user believes they stopped is a silent
divergence. `forge job log <id>` is there for someone who wanted to watch
again.

**No `just` recipe is rewritten. That is the acceptance test for this
section.**

New verbs: `forge jobs`, `forge job show|log|cancel <id>` (`--follow` on
`log`), `forge stop`. **The read verbs open no worker**: `forge jobs`,
`forge job show|log` and `forge serve --status` reconcile nothing and
rewrite no row, because a listing that re-queues another session's
`blocked` row hands the next `forge gen` somebody else's forgotten job to
run on the card. `forge gpu` learns `--free`, which releases the host's
models — replacing `forge gen music --stop-server`, which is deleted with
the resident server — and says "the card is back" only when free VRAM
meets the card's idle floor, never when it merely equals what this call
started with.

---

## 4. `backend.toml`, second form

`executor = "env" | "comfy" | "tool"` at the top level. The parse rule, in
both `python/forge_gen/backends.py` and
`crates/forge_library/src/backends.rs`:

- `executor` present → it wins; `env_kind` is required only when
  `executor = "env"`.
- `executor` absent → derived: `env_kind = "none"` → `tool`, anything else
  → `env`. **Every existing `backend.toml` keeps working unedited.**
- Both present and disagreeing → `BackendConfigError`, the class
  `backends.py` already raises for a name/directory mismatch.
- `executor = "comfy"` with an `[env]` table or a `python` key →
  `BackendConfigError`: a comfy backend has no interpreter, and saying it
  has one is a half-truth doctor should refuse at parse time. **`entry`
  is not part of this rule** — it is a module path relative to `forge_gen`
  (`audio.music`, `audio.sfx`, `mesh`, `motion`) and stays one.

```toml
name = "moss_sfx"
role = "sfx"
executor = "comfy"
host = "comfy"                        # which backends/<name> is the service
upstream = "https://github.com/diodiogod/TTS-Audio-Suite"
commit = "b7e41a2c…"
license = "Apache-2.0 (MOSS-SoundEffect-v2); TTS-Audio-Suite: MIT"
vram_gb = 8                           # a budget, unmeasured — never quoted as a peak
entry = "audio.sfx"

[comfy]
workflows = ["sfx.api.json"]          # under backends/moss_sfx/workflows/
nodes = ["MOSSSoundEffectNode", "TTSAudioSuiteUnload"]   # captured from GET /object_info, not from memory
unload_node = "TTSAudioSuiteUnload"   # null for native nodes that honour /free
[[comfy.packs]]
repo = "https://github.com/diodiogod/TTS-Audio-Suite"
commit = "b7e41a2c…"
dir = "TTS-Audio-Suite"
license = "MIT"
pips = ["…"]
nodes = ["MOSSTTSNode", "MOSSSoundEffectNode", "TTSAudioSuiteUnload"]

[[models]]
id = "OpenMOSS-Team/MOSS-SoundEffect-v2.0"
store = "comfy:models/tts"
license = "Apache-2.0"
```

`STORES` gains one prefix form: `store = "comfy:models/<dir>"`, validated
as `comfy:` plus a relative path with no `..` and no leading `/`, resolved
against the `host` backend's `$PREFIX/<base_directory>/models/<dir>/` —
which is what `extra_model_paths.yaml` already points at. That retires the
`[[comfy.models]]` workaround in `backends/comfy/backend.toml`, which
exists in so many words *because "`store` has no word for a comfy folder
yet"*: those eight rows fold into ordinary `[[models]]` and **doctor has
one model list again** instead of two that can disagree.

The node class names in every `[comfy] nodes` list are **captured from the
running host's `/object_info`, never written from memory**; `probe.py`
checks them and doctor turns a missing class into `partial` naming the
pack, so a template that cannot run is a doctor line and not a
`POST /prompt` failure in front of a stranger.

### Doctor's five words

| word | `env` | `comfy` |
|---|---|---|
| `ok` | unchanged: the probe inside the env exits 0, imports, torch, CUDA and weights present | the service answers at `comfy_url`, its commit matches `commit`, every class in `[comfy] nodes` is in `/object_info`, every pack clone is at its pinned commit, every `[[models]]` file is on disk |
| `partial` | the env runs, a weight is not cached | the service answers, packs are right, a node class or a model file is absent — the row names it and its GB |
| `missing` | no `.env`, no override | the service does not answer |
| `broken` | present, unusable; the failing check named | it answers at another commit than pinned, or a pack is off its pin, or a tracked workflow names a class that does not exist. The hint is the `systemctl --user status forge-comfy` line |
| `off` | `[make]` chose no kind that needs it | same |

`off` is not a probe result — it is `[make]` in `forge.toml` not having
chosen the kind, rendered dimmed with a one-line reason
(`off — [make] music = false`), never probed (which is what makes doctor
fast on a props-only project) and never a reason to exit 1.

**Exit 1 only while a *chosen* backend is not `ok`.** The chosen set is
passed down: `forge gen doctor --json --chosen trellis2,ardy,comfy`. Each
row gains `"executor"` and `"chosen"`. A project that makes props and
clips is green with `acestep`, `moss_sfx` and `moss_tts` all `off`, which
is the honest reading and the one that lets a stranger's first
`just doctor` pass. `--make none` and tier `fake` choose nothing: every
row reads `off`, doctor exits 0, and that is how the new gate runs green
on a GitHub runner with no card.

---

## 5. `forge.toml`, `init`, `setup` — and the VRAM ladder

```toml
[make]
props = true          # trellis2 + qwen_image
characters = true     # trellis2 + skintokens + qwen_image
clips = true          # ardy
sfx = false           # moss_sfx (comfy)
music = false         # acestep (comfy)
voice = false         # moss_tts (comfy)

[hardware]
tier = "full"                          # full | lean | fake — detected, overridable
comfy_url = "http://127.0.0.1:8188"
```

Both tables are optional in `crates/forge_library/src/project.rs`
(`Project::make: MakeKinds`, `Project::hardware: Hardware`) and a
`forge.toml` written before this phase reads as **every kind chosen, tier
detected** — so no existing project's doctor silently goes quiet. The
struct uses `deny_unknown_fields`, so the tables land in the same commit
that documents them. The kind → backend map is one fact in one place:
`props → trellis2, qwen_image`; `characters → + skintokens`;
`clips → ardy`; `sfx → moss_sfx`; `music → acestep`; `voice → moss_tts`;
anything comfy → `+ comfy`.

Tier changes registers and variants, never features. `lean` selects the
Q4_K_M GGUF reference template and MOSS-TTS 1.7B; `fake` sets
`FORGE_FAKE=1` for every job the project runs, as a first-class answer and
not an environment trick. **The doors build their queue options from the
project** — `LocalQueueOptions::for_project` — because when each of them
took `..default()` instead, `tier` was always `"full"` and a `--tier fake`
project with `FORGE_FAKE` unset ran the real generator; `ci-fake` and
`mcp-session` could not see it, both exporting `FORGE_FAKE=1`, so
`cli.rs` has a leg that removes it from the environment on purpose. **Both tiers lift at 1024³** — measured at
4.7 GB, and 512³ costs the face.

**`forge init` asks three questions once, on a TTY**, phrased as what you
make and what card you have, never as model names: *what will you make
here* (the six kinds, defaulting to props+characters+clips); *what card is
this* (detected from `nvidia-smi --query-gpu=memory.total`: ≥ 22 GB →
`full`, ≥ 14 → `lean`, none → `fake`; offered, not assumed); *where is
ComfyUI* (default `http://127.0.0.1:8188`, asked only when a chosen kind
uses a comfy backend). Flags, which are also what MCP `init_project`
passes: `--make props,characters,clips` / `--make all` / `--make none`,
`--tier full|lean|fake`, `--comfy-url URL`, `--yes`. With no TTY and no
flags it takes the defaults and prints one line naming each assumption —
it never hangs on a prompt, which is the trap `hf auth login` taught this
repo.

**`forge setup [kind…] [--yes <licence>…] [--dry-run]` prints one screen
before a byte downloads** — and the screen is the bill its own installers
then spend: every weights figure is the sum of the `gb` in that backend's
`backend.toml` (held to it by a test, because the table has to work before
a backend directory exists), and the comfy host is told which model group
to fetch (`--models qwen_image`, `--models none`) rather than pulling all
73.67 GB of image weights behind a 9.5 GB screen. **`--yes` is never
blanket, at either end**: an installer is handed one only when every
licence its own `confirm_license` asks about is on this machine's receipt,
and `--no-flux-controlnet` is always passed because that licence has no id
in the table and a `--yes` about nvdiffrast must not be able to accept a
non-commercial one nobody was shown: per chosen kind the backends, the disk cost
(weights + env + clone), the total, and every licence fact those carry —
nvdiffrast's NVIDIA Source Code License (non-commercial) in full, the
DINOv3 gated login, Llama 3's attribution requirement, the SkinTokens
encoder question, ComfyUI's GPL-3.0 — then asks once. `--yes nvdiffrast
--yes llama3` accepts by name and is repeatable; **a bare `--yes` is
refused**, because a blanket yes to a list nobody read is exactly what the
gate exists to prevent. Resumable: each installer is idempotent and writes
`installed.json`, and a backend whose receipt matches its pin is skipped
with one line. The DINOv3 token is a thing only a human holds; setup says
so and stops with the two commands to run.

Acceptances are appended to `$FORGE_BACKENDS_HOME/licences.json` — beside
the installs, because the install is what is licensed, and not in
`forge.toml`, which is hand-edited and would let an acceptance be typed
rather than given:

```json
{"forge_licences": 1, "accepted": [
  {"id": "nvdiffrast",
   "name": "NVIDIA Source Code License (1-Way Commercial)",
   "backend": "trellis2", "by": "human", "at": "2026-08-30T14:02:11Z", "via": "cli"}]}
```

The licence ids are `nvdiffrast`, `dinov3`, `llama3`,
`skintokens_encoder`, `comfyui_gpl`.

### The comfy VRAM ladder (`forge_serve::card::release_comfy`)

`forge2.md` names "wrapper packs bypass ComfyUI's memory manager" as a
risk, and the first real audio run settled it: **`POST /free` does not give
the card back after TTS-Audio-Suite has loaded a model** (22.85 → 15.07 GB
free across an sfx job; `systemctl --user restart forge-comfy` returns it
in 4.4 s), while native ACE-Step needs nothing at all. So this ladder is
load-bearing for the two MOSS backends and a safety net for the rest —
loud when it fires, and never silent about what it could not prove:

1. Read `vram_free` and `vram_total` before the job → `before`, and the
   **floor**: `total − 2.0 GB`, what this card shows with nothing but the
   host's idle CUDA context on it (measured 0.4–1.53 GB,
   `designs/hosting.md`).
2. After the child exits, `POST /free {unload_models: true, free_memory:
   true}`, then poll `/system_stats` every 500 ms for 15 s. (There is no
   `unload_node` to end the template with: the pack registers no unload
   class at this pin, and `/free` does not unload its models — the unit
   restart of step 4 is the only lever the MOSS pack has, measured
   2026-08-30.)
3. Free VRAM within 0.5 GB of **the floor** → release the lease, record
   `vram_after_gb`. **Not within 0.5 GB of `before`**: `before` is read
   seconds before the job, so a model an earlier job left resident is
   inside it and can never be seen — a speech job went 16.44 → 16.38 GB
   and was released "clean" with 7.3 GB of MOSS on the card, and
   `forge gpu --free` printed "the card is back" one line above "holding
   pid 693788 8.1 GB" (2026-08-30). `before` stays on the row, because
   what a job started with is worth knowing; it is not the question.
4. Not back → `systemctl --user restart <[server] unit>`, wait for
   `/system_stats` up to `ready_timeout_s`, re-read. Log a warning quoting
   both numbers; the row records `card.restarted: true`.
5. Still not back → **the lease is not released to another card job.**
   `card.json` gets `{"holder": "foreign", "note": "…"}`, `status` reports
   it, the next card job sits `blocked` with `blocked_by: "comfy"`, and
   doctor's comfy row goes `broken`. A daemon that hands out a card it
   cannot prove is free produces an OOM three jobs later with nothing
   naming the cause.

---

## 6. Audio through ComfyUI

Three backends move from a venv each to the one host. ACE-Step 1.5 is
native to the pinned host (v0.34.2); the three MOSS models come through
**TTS-Audio-Suite** (MIT), which becomes the second `[[comfy.packs]]`
entry.

| backend | executor | route | `unload_node` |
|---|---|---|---|
| `acestep` | `comfy` | ACE-Step 1.5 native nodes | `null` — native models honour `/free`, measured |
| `moss_sfx` | `comfy` | TTS-Audio-Suite | set: a wrapper pack loads outside ComfyUI's memory manager |
| `moss_tts` | `comfy` | TTS-Audio-Suite | set |

Models move to `comfy:models/…` stores. Templates, API format, tracked:

```
backends/acestep/workflows/music.api.json
backends/moss_sfx/workflows/sfx.api.json
backends/moss_tts/workflows/speech.api.json
backends/moss_tts/workflows/voice.api.json
```

**The pack rule is not optional.** `hosting.md`, 2026-08-30: a pack lives
in four places or nowhere — `[[comfy.packs]]`, `install.sh` (cloned at its
pin *before* the unit starts, because packs are scanned once at startup),
`snapshot.json` (**re-fetched from `GET /v2/snapshot/get_current`, never
hand-edited** — a snapshot typed by hand is the same defect as a
hand-repaired `.blend`) and `hosting.md`'s pins row. Adding
TTS-Audio-Suite means all four in one commit.

### Patch points live inside the template

A node whose `_meta.title` is `PATCH:<key>` has its single primary input
patched with the job's value for `<key>`.
`comfy.patch_points(graph) -> {key: (node_id, field)}` builds the map, and
a template missing a key the verb requires is a **refusal before the
GPU**, naming the key and the file. There is no sidecar manifest of node
ids: that would be one fact written in two files that nothing holds
together, which is `hosting.md`'s four-places-or-nowhere entry in
miniature. A marker travels inside the graph, cannot drift from what it
annotates, and survives a re-export from the ComfyUI UI. This is the one
thing the Phase 0 spike did not do — it hard-coded node ids in
`spike_reference.py` — and it is the difference between a tracked template
being data and being a secret handshake.

### `python/forge_gen/comfy.py`

Stdlib only, so `test_cli.py`'s contract that `--help` never imports torch
still holds:

```python
def base_url(backend, project) -> str          # $FORGE_COMFY_URL -> [hardware] comfy_url -> [server]
def load_template(backend, name) -> tuple[dict, str]      # graph, sha256 of the tracked file
def patch_points(graph) -> dict[str, tuple[str, str]]     # from _meta.title PATCH:<key>
def patch(graph, inputs: dict) -> dict         # refuses an unknown key and a missing required one
def upload_image(base, path, name) -> str
def submit(base, graph, client_id) -> str      # POST /prompt -> prompt_id
def wait_for(base, prompt_id, *, timeout, poll=1.0, on_progress=None) -> dict
def fetch(base, entry, dest) -> list[Path]     # GET /view
def object_info(base, node=None) -> dict       # doctor only
def packs_block(backend) -> dict[str, str]     # {repo: commit} for the record
def was_cached(entry, node_id) -> bool         # "execution_cached" in the /history status messages
```

`cached` is **observed, never inferred**. The unit runs with
`--cache-none`, so an identical graph genuinely re-runs; a
graph-hash-to-job index would claim a cache hit that never happened, which
is "a record that lies". Two observations may set it: `was_cached` on the
`/history` entry for the save node, and the daemon finding an earlier
finished job whose record's output sha256 equals this one's — which is
what fills `same_as`.

### The surgery, module by module

Each of `music.py`, `sfx.py`, `speech.py`, `voice.py` keeps `add_parser`,
`check_inputs`, `build_record`, `measure_wav`, `finish`, `run_fake` and
every exit code. What is replaced is the block between "inputs are
checked" and "the WAV is on disk".

`music.py` **loses** `state_dir`, `log_path`, `pid_path`, `write_pid`,
`read_pid`, `_pid_alive`, `_proc_start`, `_pid_is_ours`, `server_settings`,
`base_url`, `api`, `server_up`, `start_server`, `ensure_server`,
`stop_server`, `submit`, `wait_for`, `download`, `READY_TIMEOUT_S`,
`READY_POLL_S`, `STOP_TIMEOUT_S` and the `--stop-server` flag — with them
the soundfile patch, `backends/acestep/patches/`, `ACESTEP_CHECKPOINTS_DIR`
and one class of bug (a stale pidfile killing a stranger). It **keeps**
`resolve_format`, `read_lyrics`, `request_payload` (renamed
`template_inputs`), `ffmpeg_bin`, `transcode_ogg`, `backend_facts`,
`build_record`, `measure_wav`, `finish`, `run_fake`, untouched.
`--stop-server` is removed rather than kept as a no-op; argparse's
refusal (already a JSON last line) names the new door:
`the ACE-Step server is gone; the card is released by forge serve, or by
systemctl --user stop forge-comfy`.

**Records come out identical in shape** — same `kind`, same `tool`
(`ace_step`, `moss_sound_effect`, `moss_tts`, `moss_voice_generator`),
same `params` keys with the same meanings, same `measured` from the same
`measure_wav`. What is new is the four additive `backend` keys and
`params.workflow`. `assets/audio/**` needs no migration; `just audit`,
`just verify` and `just manifest-check` are untouched.

**One open question, timeboxed before a venv is deleted.** `decisions.md`,
2026-08-23: the cloner cannot open a reference by path in this env
(torchaudio → torchcodec), so speech's inner half reads the clip with
soundfile and hands over codes. Under TTS-Audio-Suite that inner half is
gone and the pack does its own audio loading inside ComfyUI's venv. "The
reference travels as an uploaded file" is a hypothesis, not a measurement:
run one real `speech` through the host before `moss_tts`'s venv is
deleted, and record the outcome under ComfyUI in `hosting.md`.

**Nothing is deleted until a real sound has come out of the new path on
the card**, with its dated `hosting.md` entry. Each deletion is then its
own commit with its reason in `decisions.md`, per the retire-in-phases
rule; the working path is the only fallback while the new one is unproven.

---

## 7. The MCP tools for this phase

Eleven today plus seven: **eighteen**. `mcp-check`'s pinned list becomes,
sorted:

```
cancel doctor generate_audio generate_clips init_project inspect_audio
licences list_audio list_clips list_models list_runs promote_audio
promote_clip render_clip_strip render_model setup status wait
```

`doctor` and `generate_audio` change in behaviour only; nothing is
removed. Every frame below is shaped for a client with no shell and no
memory of the last turn.

**`generate_audio`** keeps every argument, gains `wait_s: number|null`,
and stops blocking:

```json
{"job": "j-20260830-141207-3f9a", "state": "queued", "position": 1, "eta_s": 45,
 "kind": "generate_audio.sfx", "backend": "moss_sfx", "executor": "comfy",
 "out": "out/audio/sfx/door.wav", "record": "out/audio/sfx/door.json",
 "log": "out/serve/logs/j-20260830-141207-3f9a.log",
 "next": "wait {\"job\":\"j-20260830-141207-3f9a\",\"max_s\":120}"}
```

`next` is not decoration: it is the literal call to make, and it is the
difference between an agent that polls correctly and one that invents a
tool. With `wait_s` set the tool waits that long inline and returns the
finished shape if it lands, so a short fake-tier job is still one turn.

**`wait {job, max_s}`** — default 120, ceiling 600.

- Finished → the finished job **plus**, for a `done` audio job, the same
  measurement and plot block `inspect_audio` returns, so the common path
  is two calls and not three.
- Still running → `{"job":…,"state":"running","position":0,
  "elapsed_s":74,"eta_s":30,"log_tail":[…8 lines…],"still_waiting":true}`
  — a **successful** frame. An agent that gets this calls `wait` again and
  has lost nothing.
- `cached` → `"cached": true, "same_as": "j-…-3c11"` and one sentence:
  *ComfyUI served this from its node cache — it is the same bytes as
  `<path>`. If you meant a re-roll, change the seed.*
- Unknown id → a refusal listing the ten most recent job ids.

**`cancel {job}`** → `{"job":…,"cancelled":true,"was":"running",
"note":"SIGTERM to pid 481233, group killed after 10 s; partial outputs
under out/… were left, nothing under assets/ was touched"}`. Cancelling a
terminal job is a refusal naming its state, not an error.

**`status {}`** — no arguments, the tool an agent calls after a context
reset. The `/v1/status` object of §2 rendered as a table: free VRAM and
who holds it, the queue, the running job with a log tail, the last
terminal jobs, and the daemon's own facts. No GPU work, tens of
milliseconds.

**`list_runs {kind?, since?, limit?}`** walks `out/` (`out/audio/*/`,
`out/sweeps/*/`, `out/lifts/`) and `assets-src/voices/` reading the
generator record beside each output:
`{path, kind, tool, prompt, seed, created, created_by, fake, promoted,
record}`. `promoted` is decided by **hashing** — a library sidecar whose
generator output hash equals this one's — not by remembering. No GPU, no
daemon needed; `forge_serve::runs` is the implementation and
`GET /v1/runs` is the same function.

**`init_project {path, name?, make{}, tier?, comfy_url?, adopt?}`** →
the `forge.toml` written and the next step; refuses a directory that
already holds one unless `adopt: true`, and then updates only `[make]` and
`[hardware]`.

**`licences {kinds?}`** → per component, the **full text**, its id, its
backend and `needs_accept: bool`. The text, not a summary: an agent cannot
accept what it was not shown.

**`setup {kinds?, backend?, accept[], no_models?, dry_run?}`** → a job.
Without every required id in `accept` it returns a refusal listing exactly
the ids missing and the sentence *call `licences` first and pass each id
in `accept`*. That refusal is the gate, and it is a tool that does not
exist rather than a prompt asking an agent to behave.

**`doctor {quick?}`** — unchanged in shape, gaining `executor`, `chosen`
and the `off` word per row, with the exit reading of §4.

**Refusals stay successful frames** everywhere — `CallToolResult::error`
with text, never `Err(ErrorData)` — carrying the Python layer's own
`message` and `hint` verbatim plus the last twelve log lines, the shape
`gen_refusal` already produces, now reached through the queue.

---

## 8. The `mcp-session` gate

A **Rust integration test**, `crates/forge/tests/mcp_session.rs`, driving
the server with rmcp's own client. Chosen over a Python client with the
`mcp` package because rmcp is already pinned in the workspace (adding
`client`, `transport-child-process` and
`transport-streamable-http-client` to that pin costs one line and no new
toolchain), because a second protocol implementation in CI is a second
thing to keep current, and because `env!("CARGO_BIN_EXE_forge")` — the
mechanism `crates/forge/tests/cli.rs` already relies on — guarantees the
binary under test is *this* build with no prior `just` step.

One `async fn session(client)` holds the script; **two tests call it,
because "one tool surface, two transports, one queue" is this phase's
central claim and a transport nothing exercises ships ungated**:

- `mcp_session_stdio` — `TokioChildProcess` over `forge mcp --project <tmp>`.
- `mcp_session_http` — spawn `forge serve --project <tmp> --port 0
  --foreground`, read the port and token out of `out/serve/daemon.json`,
  connect `StreamableHttpClientTransport` to
  `http://127.0.0.1:<port>/mcp`, run the same script, `forge stop`.

The project is a `tempfile::tempdir()` with `FORGE_FAKE=1`,
`[hardware] tier = "fake"`, `FORGE_HOME` at the checkout. Every assertion
is on the frame text an agent would read, not on internals:

```
initialize -> tools/list                      the eighteen names, so mcp-check cannot drift from the session
init_project(make: ["sfx"], tier: "fake")     forge.toml has [make] and [hardware]
licences(kinds: ["sfx"])                      at least one entry, non-empty text, needs_accept reported
setup(kinds: ["sfx"], accept: [])             succeeds (sfx is ungated); a gated kind with accept: [] refuses naming the id
doctor()                                      moss_sfx off or missing, every other row off, exit reading 0
generate_audio(kind: "sfx", prompt: "a heavy iron door", name: "door")
                                              a job id comes back AND the frame is not the file
wait(job, max_s: 120)                         done, fake: true, a plot, out/audio/sfx/door.wav and its record
inspect_audio(path)                           measurements, no clipping flag
promote_audio(kind: "sfx", name: "door", …)   written under assets/audio/sfx/, manifest rewritten
verify()                                      green
status()                                      queue empty, card free
```

Two negative legs in the same test, because they are the ones that rot
silently: **`wait` on an unknown job id** must be a successful frame with
`is_error: true` listing the ids that do exist; **a second
`promote_audio` on a taken name** must refuse and echo the record it would
have replaced.

`just mcp-session` = `cargo test -p forge --test mcp_session -- --nocapture`.
`just ci` becomes `fmt-check check test pytest smoke audit check-bodies
manifest-check verify mcp-check mcp-session ci-fake`, and GitHub's `test`
job gains one `- run: just mcp-session` after `just mcp-check`. No GPU, no
display, no backend, no secret, no network.

---

## 9. Tests

**Must keep passing, unchanged.** Every file under `python/tests/` —
`test_records.py`, `test_backends.py` and `test_doctor.py` *grow* cases
rather than changing existing ones; `test_cli.py`'s contract that `--help`
never imports torch is exactly what keeps `comfy.py` stdlib-only;
`test_spike.py` imports `spike_pose`/`spike_skin` only, so deleting
`spike_reference.py` is free. `crates/forge_motion/tests/bake_matches_shipped.rs`,
`crates/forge_rig/tests/*`, `crates/forge_studio/tests/*`,
`crates/forge/tests/cli.rs`. `just ci-fake` end to end, byte-for-byte in
behaviour. `just smoke`, `audit`, `check-bodies`, `manifest-check`,
`verify`. `just mcp-check` with its new list.

**Updated in lockstep, by one owner each.**
`crates/forge_library/tests/python_records.rs` and its fixtures (v2 held
to byte equality per executor, v1 read-only) and
`generator_record.rs::a_newer_or_absent_schema_is_refused` (which asserts
today that `forge_record: 2` is refused) — both **B**, in one commit with
`records.py`.

**New, by the risk each retires.**

| test | file | what it proves |
|---|---|---|
| `card_lease_is_exclusive_across_processes` | `forge_serve/tests/card.rs` | two processes, one lease, the second blocks |
| `a_killed_holder_releases_the_card` | `forge_serve/tests/card.rs` | SIGKILL the holder → the next acquirer gets it; the stale `card.json` is overwritten |
| `a_foreign_holder_blocks_rather_than_ooms` | `forge_serve/tests/card.rs` | a stub card reader short of the budget → `blocked`, `blocked_by` names the pid |
| `vram_that_does_not_return_restarts_the_unit_then_withholds` | `forge_serve/tests/comfy_card.rs` | a stub `/system_stats` that never recovers → a fake `systemctl` on PATH called once, then the lease withheld |
| `an_interrupted_row_is_never_completed_by_inference` | `forge_serve/tests/restart.rs` | a `running` row with a dead pid → `interrupted`, `exit: null` — env and comfy alike |
| `queued_rows_are_requeued_in_submitted_order` | `forge_serve/tests/restart.rs` | a queued row derived nothing and is not interrupted |
| `two_jobs_for_one_output_path_are_refused` | `forge_serve/tests/queue.rs` | the out-path lease, naming the running job |
| `a_fake_job_takes_the_queue_like_any_other` | `forge_serve/tests/queue.rs` | fakes are not a second path |
| `cancel_kills_the_process_group_and_frees_the_card` | `forge_serve/tests/cancel.rs` | a sleeping child tree dies; the lease is released; `cancelled`, `exit: null` |
| `records_are_written_only_by_the_generator` | `forge_serve/tests/no_record_writer.rs` | the crate's own source contains no `forge_record` literal and no record-write path |
| `a_cached_result_says_so` | `forge_serve/tests/comfy_cache.rs` | a stub `/history` reporting `execution_cached` → `cached: true`, `same_as` naming the earlier job |
| `a_second_daemon_refuses_and_names_the_first` | `forge_serve/tests/daemon.rs` | `daemon.lock` |
| `a_stale_daemon_json_falls_back_in_process` | `crates/forge/tests/cli.rs` | dead pid, or a live pid with another start time → the file is removed, the run proceeds |
| routes via `tower::ServiceExt::oneshot` | `forge_serve/tests/http.rs` | submit → poll → log → cancel against a stub executor writing a canned record |
| `record_v2_reads_v1_and_writes_v2` | `forge_library` unit + `python/tests/test_records.py` | both readers accept 1 and 2; the two writers agree byte for byte on v2 |
| `a_comfy_backend_with_an_interpreter_is_refused` | `python/tests/test_backends.py` | the §4 parse rule, and that `entry` is not part of it |
| `comfy_store_resolves_under_the_host_prefix` | `python/tests/test_backends.py` | `comfy:models/<dir>` |
| template patching | `python/tests/test_comfy.py` | `PATCH:` discovery on the tracked templates; an unknown input key refused; a template missing a required key refused before the GPU; `packs_block` shape; `was_cached` on a captured `/history` entry |
| `doctor_says_off_for_an_unchosen_kind_and_exits_zero` | `python/tests/test_doctor.py` | §4's exit rule |
| `setup_refuses_a_gated_kind_without_accept` | `forge_mcp` unit | the licence gate |
| the eighteen names, `wait` on an unknown id | `forge_mcp` unit | the surface and the refusal shape |
| `mcp_session_stdio`, `mcp_session_http` | `crates/forge/tests/mcp_session.rs` | §8 |

Every comfy stub is an HTTP server inside the test. No ComfyUI in CI,
ever.

---

## 10. Three implementers, disjoint files

Every file below is owned by exactly one implementer. Where a file must
change for two, the owner is named and the other hands over a **patch
note** in its return: the exact lines to add, with enough context to apply
blind.

The allocation rule that decides the awkward cases: **one contract, one
owner, in both languages.** `records.py` and `generator_record.rs` are one
contract; `backends.py` and `backends.rs` are one contract; splitting
either across two implementers reintroduces as an org chart precisely the
two-dialects-of-one-schema drift that `python_records.rs` exists to catch.

| path | owner |
|---|---|
| `crates/forge_serve/**` (new crate, all of it, including its tests) | **A** |
| `crates/forge_mcp/src/{lib,config,server,util}.rs` | **A** |
| `crates/forge_mcp/src/tools/mod.rs` | **A** |
| `crates/forge_mcp/src/tools/jobs.rs` (new: `wait`, `cancel`, `status`, `list_runs`) | **A** |
| `crates/forge_mcp/src/tools/{generate,audio,list,promote,render}.rs` | **A** |
| `crates/forge/src/{cli,main}.rs` | **A** |
| `crates/forge/src/commands/{mod,serve,jobs,generate,mcp,gpu}.rs` | **A** |
| `crates/forge/tests/cli.rs` | **A** |
| root `Cargo.toml`, `crates/forge/Cargo.toml`, `crates/forge_mcp/Cargo.toml` | **A** |
| `python/forge_gen/comfy.py` (new) | **B** |
| `python/forge_gen/{records,backends,launcher,placeholders}.py` | **B** |
| `python/forge_gen/cli.py` (command registration only) | **B** |
| `python/forge_gen/audio/{music,sfx,speech,voice}.py` | **B** |
| `python/forge_gen/spike_reference.py` (deleted) | **B** |
| `python/tests/{test_comfy,test_records,test_backends,test_launcher,test_fake_smoke,test_voice}.py` | **B** |
| `crates/forge_library/src/{generator_record,backends}.rs` | **B** |
| `crates/forge_library/tests/python_records.rs` and its fixtures | **B** |
| `crates/forge_library/Cargo.toml` | **B** |
| `backends/{acestep,moss_sfx,moss_tts,comfy}/**` | **B** |
| `crates/forge_library/src/project.rs` (`[make]`, `[hardware]`) | **C** |
| `crates/forge/src/commands/{init,doctor,setup}.rs` | **C** |
| `crates/forge_mcp/src/tools/setup.rs` (new: `init_project`, `licences`, `setup`) | **C** |
| `crates/forge_mcp/src/tools/doctor.rs` | **C** |
| `python/forge_gen/doctor.py` | **C** |
| `python/tests/{test_doctor,test_cli}.py` | **C** |
| `crates/forge/tests/mcp_session.rs` (new) | **C** |
| `justfile`, `.github/workflows/ci.yml` | **C** |
| `README.md`, `backends/README.md`, `CLAUDE.md`, `designs/decisions.md`, `designs/hosting.md`, `designs/forge2.md` | **C** |
| `.claude/skills/forge-setup/SKILL.md`, `.claude/skills/forge-audio/SKILL.md` | **C** |
| the toolkit's own `forge.toml` | **C** |

### The handovers, named

- **C → A**, into `crates/forge/src/cli.rs`: the `Init` flag set
  (`--make`, `--tier`, `--comfy-url`, `--yes`), a `Setup(SetupArgs)`
  variant (positional kinds, `--yes <licence>` repeatable, `--dry-run`),
  and `Doctor`'s `--quick`. Exact struct fields with doc comments.
- **C → A**, into `crates/forge_mcp/src/tools/mod.rs`: `mod setup;` and
  `+ setup::router()` in the sum; `pub(crate) fn router() ->
  ToolRouter<ForgeServer>` is the shape A can rely on in `setup.rs`.
- **C → A**, into `crates/forge/src/commands/mod.rs`:
  `pub(crate) mod setup;` and the `Command::Setup` match arm.
- **C → A**, into the root `Cargo.toml`: the rmcp feature additions its
  gate needs (`client`, `transport-child-process`,
  `transport-streamable-http-client` as dev-dependencies of `forge`).
- **A → C**, into `justfile` and `.github/workflows/ci.yml`: the `serve`,
  `jobs`, `job-log` recipes and the `mcp-session` invocation line.
- **A → B**: the environment contract a spawned generator sees.
- **B → C**, into `backends/README.md` and `designs/hosting.md`: the three
  moved backends' table rows, the TTS-Audio-Suite pack's four-places
  entry, and each licence id with its disk cost for the `setup` screen.
- **B → C**, into `justfile`: the removal of the `stop-music` recipe.
- **A and B → C**, as text: each dated `hosting.md` trap and each
  `decisions.md` lesson they earned. Neither goes only in a commit
  message. Phase 1 is expected to close with at least a `hosting.md` entry
  under ComfyUI for the TTS-Audio-Suite pack in all four places and for
  whatever the speech reference turns out to need, and `decisions.md`
  entries for why the graph client is Python while the card protocol is
  Rust, why the card lock is a `flock` and not a pidfile, and why an
  interrupted job is never completed by inference.

### The interfaces all three honour, blind

1. **`forge_record: 2`** — the ten `backend` keys in the order of §1.7,
   `null` where unknown, both readers accepting 1 and 2, only 2 written.
   B owns both halves and the referee test.
2. **`backend.toml` v2** — `executor`, `host`, `[comfy] {workflows, nodes,
   unload_node}`, `[[comfy.packs]] {repo, commit, dir, license, pips,
   nodes}`, `store = "comfy:models/<dir>"`, and the derive rule for a file
   with no `executor`. B owns the parsers; C's doctor renders from them;
   A's `ComfyExecutor` reads only `executor`, `host` and `vram_gb`.
3. **`forge.toml` `[make]` / `[hardware]`** and the kind → backend map. C
   owns; A reads `tier` to decide `FORGE_FAKE`; B reads `comfy_url`
   through `comfy.base_url`.
4. **The job JSON** of §1.1 (`forge_job: 1`), `daemon.json`, `card.json`,
   and the `Queue` trait's eight methods. A owns; C's gate asserts against
   them; B never sees them.
5. **The HTTP routes** of §2, bearer token, loopback `Host` check.
6. **The `forge gen` contract, unchanged** — the last stdout line is one
   JSON object; exit codes 0/2/3/4/5/6 mean exactly what
   `exit_codes.py` says and the daemon relays them without translation;
   refusals carry `message`|`reason` and `hint`. B keeps writing it, A
   keeps parsing it. **B adds exactly one key for A**: a `comfy` block on
   a comfy run —
   `{"template", "template_sha256", "inputs", "comfyui_commit", "packs",
   "prompt_id", "cached"}` — which A copies into the row verbatim.
7. **Doctor JSON** — `backends.<name>.status ∈ {ok, partial, missing,
   broken, off}`, `.executor`, `.chosen`; `exit_code` 1 only when a
   `chosen` backend is not `ok`. C implements both halves.
8. **The environment contract** — `FORGE_NO_DAEMON=1` and
   `FORGE_SERVE_URL` (A sets and reads), `FORGE_COMFY_URL` (B honours
   first), `FORGE_JOB_ID` (A sets, B may use for `filename_prefix`),
   `FORGE_FAKE`, `FORGE_HOME`, `FORGE_PROJECT`, `FORGE_RIG_PROFILE`,
   `FORGE_BACKENDS`, `FORGE_BACKENDS_HOME` — all three leave the rest
   alone.
9. **The eighteen tool names**, pinned in `mcp-check` (C), produced by
   `tools/mod.rs` (A) plus `tools/setup.rs` (C), asserted in
   `mcp_session.rs` (C).

### Order of landing

**Day one, in parallel, one commit each, tree green:** the three contract
stubs and nothing else. A publishes the `Queue` trait, `Job`, `JobSpec`,
`Status`, `Run` and the job JSON of §1.1 as a doc comment with a worked
example. B publishes `Executor`/`ComfySpec` and `Backend.executor` in both
languages as compiling stubs, and `BACKEND_KEYS` in its new order. C
publishes the `[make]`/`[hardware]` TOML shape, the kind → backend map and
the licence-id table.

Then A and B run independently to the point where `forge serve` runs an
`env` job end to end (A) and `forge gen sfx` runs through ComfyUI with no
daemon up (B). C's gate is written against the contracts and stays red
until both land — which is the point: it is the first thing that proves
the two halves met.

Phase 1 ends green on `just ci` with `mcp-session` in it.
