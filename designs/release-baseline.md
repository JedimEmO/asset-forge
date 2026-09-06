# Agent-facing release baseline

Recorded 2026-09-05. This is an implementation checkpoint, not a release approval.
The intended product is one local toolkit serving several independent game projects.

## Contracts and platform

The current toolkit version is 0.1.0. Manifest and asset sidecar writers use schema 2;
generator records write schema 2 and read schemas 1–2. These changes did not alter
those formats. Project compatibility requirements are not yet enforced.

Linux x86_64 is the first distribution target. The prior real production evidence
comes from the prepared NVIDIA RTX 4090 machine with 24 GB VRAM.
A clean GPU-machine installation, lower-memory cards and other operating systems
have not passed this release's acceptance gate.

Backend pins remain in `backends/*/backend.toml`. ARDY, TRELLIS.2, SkinTokens,
ComfyUI and the MOSS wrapper pack have recorded commits there. ACE-Step runs
inside the pinned ComfyUI host; its host commit and workflow identify the executor.
A staged distribution hashes the exact backend definitions and workflows it contains.
Weights, local installation receipts and acceptance records are excluded.

## Implemented foundation

- One canonical toolkit resolver serves initialization and generation.
  `FORGE_HOME` takes precedence over legacy `FORGE_TOOLKIT`, then executable discovery.
  An invalid explicit override does not silently select a different installation.
  Relative and symlinked toolkit paths become canonical before child processes launch.
- CLI and MCP initialization write project-specific agent instructions and MCP configuration.
  Existing instructions and client settings are preserved. `agent-config` adds missing files
  to an existing game. Files are published complete without replacing concurrent writes.
- Missing-toolkit initialization refuses before creating a partial project.
- `release/package.py` stages a runtime layout with the binary, Python launcher,
  rig profile, backend definitions, workflows, installation scripts and license texts.
  It uses an allowlist and refuses existing destinations.
- The installed layout has no sample `forge.toml`, game library or development tree.
  `bin/forge` finds its runtime files without a toolkit environment variable.

## Acceptance evidence

| Evidence | What it demonstrates | What it does not demonstrate |
|---|---|---|
| `crates/forge/tests/shared_install.rs` | Two simultaneous MCP sessions launched from generated configuration select distinct external games despite unrelated cwd and conflicting inherited project settings; custom files survive; aliases and symlinks resolve | Real generation or complete cross-project job isolation under faults |
| `python/tests/test_distribution.py` | A staged toolkit outside both games initializes, diagnoses, generates fake SFX and verifies each game without Forge/Python path overrides; payload hashes and exclusions match | A fresh OS install, release-optimized binary, or real backend installation |
| Existing CLI and MCP integration suites | Existing generation, promotion, checks and both MCP transports still work with the new initialization behavior | Unfamiliar-agent success without scripted hints |
| Scrapyard assets and `designs/decisions.md` | Real production exposed preparation, geometry, attachment, grounding and audio-envelope problems | All supported kinds meet a frozen quality rubric |

The distribution fixture copies the currently built Forge binary. It exercises the
packaged runtime paths, but the source checkout remains on the machine; a later
clean-machine test must prove there are no remaining implicit system dependencies.

## Open release issues

| ID | Owning milestone | Required result |
|---|---|---|
| R01 | M0 | Freeze per-kind briefs, quality rubrics and attempt budgets; record fresh real baselines with all rejected candidates and intervention time |
| R02 | M1 | Enforce toolkit/project version compatibility and test upgrades, rollback and backend-store adoption |
| R03 | M1 | Validate clean-machine installation prerequisites and complete canonical workflow-guide delivery; current generated guide is introductory |
| R04 | M2 — implemented | `verify`, `audit` and `manifest_check` invoke the CLI gates and return structured results; damaged-fixture tests compare verdicts and confirm no repair |
| R05 | M2 — implemented, retrieval limitation remains | Bound-project initialization activates its queue without reconnecting; embedded workflow v1 is discoverable through resources/list and resources/read. Oversized images still require local filesystem access |
| R06 | M3 | Reproduce TRELLIS instability and test bounded recovery, duplicate requests, restarts, cancellation and partial-output handling across games |
| R07 | M4 | Establish per-kind real quality evidence; audio onset, tails and loops need explicit evaluation beyond validity gates |
| R08 | M4 | Maintain attachment, grounding and external image/atlas contracts with an external Bevy consumer fixture |
| R09 | M5 | Three fresh-agent sessions pass the frozen supported brief without maintainer hints or implementation edits |
| R10 | M6 | Freeze a candidate revision, build release binaries, check crate/distribution packaging and finish notices and clean-install verification |

Props, humanoids and clips have prior real production examples. SFX, music and
voice have model workflows, but model output quality is not established by the
procedurally synthesized audio accepted for the game. No asset kind is marked
release-qualified until R01 and its quality/consumer gates pass.

## Next implementation checkpoint

Measure R01's real per-kind baselines and address R06's recovery failures.
The remaining oversized-image retrieval limitation is explicit in the workflow guide.
No reliability or artistic-quality claim is closed by fake workflow tests.

## Verification at this checkpoint

`just ci` completed with exit 0 on 2026-09-05: formatting, strict Clippy and
rustdoc, workspace Rust tests, 250 Python tests, headless smoke, library audit,
body checks, manifest/verification, MCP checks and sessions, and fake production
workflows. The new external-game tests and distribution test ran in that gate.
No real backend was generated or newly qualified during this checkpoint.

A development build is staged at `out/asset-forge-development-20260905`.
It is not a release-optimized or published artifact. Its distribution manifest
records the payload hashes and the explicit development-candidate status.


## MCP workflow implementation

The surface now has 30 tools and one embedded workflow resource at
`forge://guides/v1/workflow`. `verify`, `audit` and `manifest_check` run the same
executable gates as CLI and return structured verdicts with the full report.
Tests damage only temporary fixtures, assert failure parity with CLI, and confirm
checks do not repair them. The real fake-character/prop loop now verifies through MCP.

A session started without a project serializes bootstrap calls. After initialization
it activates exactly one queue using the project's selected tier and host settings.
The no-reconnect workflow test deliberately removes the inherited FORGE_FAKE variable;
its successful placeholder output therefore exercises the selected fake tier.
Creating another game never switches the bound library.


Staged acceptance found and fixed two additional gaps: fake-tier MCP preflight
incorrectly required a real model installation, and the runtime package omitted
`rigs/humanoid/profile.toml`, which character preparation needs. The fixture now
chooses fake tier explicitly, and permanent tests cover both regressions.
The earlier staged runs failed on these conditions; they are not counted as passes.

All four MCP integration sessions subsequently passed against
`out/asset-forge-mcp-20260905-r4/bin/forge` and that distribution's own resources:
stdio, HTTP, initialization without reconnecting, and the complete fake character/prop
loop. The embedded guide was read before initialization and on both transports.
These are scripted acceptance tests on the prepared machine, not blind-agent trials
or real-backend artistic-quality evidence.

Final MCP checkpoint verification: `just ci` exited 0, with 571 Rust tests
passed (2 existing ignored) and 250 Python tests passed. Staged MCP acceptance
passed all four sessions. Logs and a structured result are retained under
`out/mcp-acceptance-20260905/`.


## Cross-project recovery checkpoint

Production GPU ownership now uses one per-user shared location,
`~/.cache/asset-forge/gpu`, across all games. Job records and outputs remain
project-local. Previously each project held a different card.lock, so exclusivity
was only proven within one project. Manual `gpu --free` now acquires the same
lease before unloading models. A cross-project cancellation test verifies that the
waiting game proceeds after release while partial outputs and job IDs stay isolated.

Real-trial briefs and two-attempt budgets are frozen in
`release/baseline-briefs.json`, including reference hashes and explicit visual/audio
review requirements. Fresh trials have not run: a separate ComfyUI host at
`comfyui-minimax-h3` held approximately 10.6 GB when the GPU was inspected, leaving
12,361 MiB free versus the 22,528 MiB mesh budget. Permission to unload that
unrelated host is pending. No backend is newly quality-qualified by this checkpoint.

Verification: `just ci` exited 0: 573 Rust tests passed, 2 existing tests
ignored, and 250 Python tests passed. The full installed-backend `doctor --json`
also exited 0; this checks installation readiness, not generated asset quality.
Logs, doctor output, GPU snapshot, and brief hash are retained in
`out/recovery-acceptance-20260905/`.

## First fresh prop baseline — 2026-09-05

After the GPU recovered, `forge gpu --free` cleared the persisted withheld
state. Two real TRELLIS prop lifts ran in `/tmp/forge-real-baseline-20260905`,
using the frozen magnet reference and seeds 101/102. Seed 101 was selected:
a complete visible shell and a readable top silhouette, with uneven side
surfaces. Seed 102 was rejected before normalization because both pale tips
have large holes, visible with culling disabled. Both candidates are retained.

Seed 101 normalized to 0.500 m tall, lowest Y 0.000 m, and 5,319 triangles.
It was promoted only in the acceptance project; verify, audit and manifest
check each exited 0. Its 1.43 m length exposes an ambiguity in the frozen
brief: a flat game pickup should probably specify longest dimension. This
run preserves the original height criterion rather than changing it afterward.

The trials exposed a CLI help defect: inside a project, generator help
entered the GPU queue. Help now bypasses project discovery and job creation;
a regression test covers it. Full `just ci` exited 0 with 574 Rust tests
passed (2 ignored) and 250 Python tests passed. Another observed CLI
inconsistency remains: views resolves relative file paths from cwd, while
generation resolves them from the selected project. Absolute review paths
worked. Logs, review sheets, decisions, CI results and a copy of the trial
project are retained under `out/real-baseline-20260905/`.

This is the prop baseline, not a release qualification. The other five kinds,
external-consumer checks and blind-agent acceptance remain outstanding.

## Completed bounded real-model trials — 2026-09-05

The frozen briefs have now been attempted across all six kinds. Failure and
missing human judgment are results, not passes. Nothing from this baseline
replaced the game's accepted library.

| Kind | Result | Evidence and remaining gap |
|---|---|---|
| Prop | Candidate selected | Seed 101 selected; 102 rejected for hollow tips. Integrity gates pass. Scale/consumer qualification remains. |
| Character | Failed within budget | Seed 101 has visible neck/shoulder gaps and questionable rear head closure. Seed 102 exited -11 in TRELLIS after 3.2 s. Neither was skinned or promoted. |
| Clips | Partial | Six takes generated. Idle and swipe fail the requested behavior. Run 101 yields a readable 0.60 s cycle on the previously accepted rusher, 27 driven bones, zero orphaned curves. Audit rebuild and posed replay pass. Fresh character generation and full contact quality remain unqualified. |
| SFX | Listening pending | Seed 101 meets numeric limits (0 ms lead, DC +0.0068, no clipped run); semantic match and click-free tail need listening. Seed 102 fails the 50 ms onset budget at 70 ms. Neither promoted. |
| Music | Failed | Both tracks fade to long silence despite the loop brief: end level drops 53.6/55.0 dB. Final OGG 101 also has a clipped run of 3. Both rejected. |
| Voice | Failed | Designed references generated, but both cloned lines return silent 1 s audio at -120 dBFS and exit 5 without a success record. No shim or pin change attempted. |

Two initial music invocations lacked required `--record` and were refused
before generation. They are retained as intervention evidence; the corrected
invocations consumed the two actual model renders. The music success check
runs before OGG encoding (`audio/music.py`), so final-file inspection can
find clipping that generation missed. That gate/record mismatch is a release
defect, independent of the musical-loop failure.

Recovery after the TRELLIS crash passed: the GPU was free and the subsequent
ARDY sweep completed. Final `forge gpu --free` also passed. The acceptance
library's verify, audit and manifest checks exited 0; the one selected run
rebuilds byte-for-byte. These checks do not turn visual or audio failures
into successes. No implementation changed during this continuation, so the
last full CI remains the 574-Rust/250-Python passing help-fix checkpoint.

All logs, invocation/elapsed/exit records, raw and rejected candidates, review
images, hashes and an archived trial project are in
`out/real-baseline-20260905/`. The structured `result.json` distinguishes
failed, partial and listening-pending outcomes.

Next release work follows this evidence: diagnose/isolate the native TRELLIS
crash; check and measure the final encoded music file; make usable loops
through a recorded upstream process; resolve the known MOSS speech runtime
incompatibility or explicitly exclude speech from the release contract; and
repeat the character/idle/swipe briefs after upstream improvements. Consumer
and blind-agent gates remain required before release qualification.

## Final-container validation and crash diagnostics — 2026-09-05

Music now decodes the finished OGG, checks silence/clipping/truncation and
measures that decoded file before writing a success record. Pre-encoding
PCM validation remains. Rejected output remains available for inspection.
Three real-codec regression tests cover Vorbis overshoot on otherwise clean
PCM, clean final-file measurements, and invalid-container refusal. Running
the new gate on the saved baseline rejects arena101.ogg's three-sample
clipped run; arena102 passes this gate but still fails the separate loop
brief. No saved generator record was changed.

A separately labeled seed-102 TRELLIS diagnostic with PYTHONFAULTHANDLER=1
succeeded in 109.5 seconds. This does not erase the earlier crash or establish
a fix. The backend now defaults fault tracing on, so another fatal signal
can leave a traceback in its job log. No core dump was available for the
original crash and its native root cause remains unknown.

The saved speech service traceback establishes the immediate failure:
MossTTSDelayModel._sample calls _get_initial_cache_position, which is absent
in the installed transformers generation implementation. The wrapper
catches AttributeError and returns silence. No model shim, backend pin or
host environment was changed; compatible speech execution remains release
work. Evidence is retained in out/release-fixes-20260905/.

Verification: final `just ci` exited 0, 574 Rust tests passed
(2 ignored) and 253 Python tests passed. The first CI run timed
out when packaged fake generation waited behind the concurrent diagnostic
GPU job; the serial rerun passed. Both logs are retained with the result.

## 2026-09-05 — Isolated speech follow-up

The user accepted both isolated diagnostic clips as intelligible with complete
word endings. `moss_speech` now runs the 1.7B speech checkpoint with Transformers
5.0.0 and torch 2.9.1+cu128; voice design stays on ComfyUI. Real CLI and MCP runs
from the external baseline project passed their audio gates and wrote provenance
records. No diagnostic or new speech file was promoted into the shipped library.
The original failed Comfy baseline records and logs remain unchanged.

The packaged installer also built a new interpreter and passed its probe using
existing weights. Its first offline attempt correctly refused missing cached
PyTorch wheels. TRELLIS startup reliability remains unresolved: one of five
import-only runs crashed, but ten gdb runs passed without a native backtrace.
Evidence for this follow-up is under `out/speech-isolation-20260905/`.

The fresh packaged installation then generated the short line successfully from
the external project in 30.5 seconds. Final `just ci` passed: 578 Rust tests,
2 ignored, and 257 Python tests. `out/speech-isolation-20260905/result.json`
indexes the logs and unmodified output/record copies. The original CLI and the
fresh-install short WAVs can be compared by their recorded hashes.


## 2026-09-05 — TRELLIS startup mitigation qualified on this machine

The default installation now runs checksum-pinned standalone CPython 3.11.16
with the existing conda CUDA/dependency environment. The original conda Python
and SciPy packages remain untouched. Fresh installs add the runtime by default;
adoption can add it with `--with-pinned-runtime`.

The candidate passed 100 full imports on performance core 4; the installed
launcher passed another 100 with normal scheduling. Four real lifts succeeded:
the temporary trial in 106.0 seconds, normal installed seeds 102/103 in
105.8/98.0 seconds, and the final packaged toolkit's seed 104 in 97.1 seconds.
That final command ran from `/tmp` against the external baseline project,
without toolkit/interpreter overrides. All four outputs passed GLB validation
and wrote lift records naming Python 3.11.16. None was promoted or accepted
against the frozen character-quality brief.

The packaged installer created a new runtime using the existing dependencies
and weights. Reuse passed with unreachable HTTP/HTTPS proxies. This exercises
the runtime installation, not a clean rebuild of all CUDA extensions.
The native root cause remains unproven; the chosen build is a tested mitigation.
All failed controls remain in `out/trellis-reliability-20260905/`.

Native process crashes now retain the observed signal in job diagnostics.
A real-child regression verifies that SIGSEGV preserves a partial output,
writes no success record and releases the shared card for another project.
Doctor now tests the actual pipeline import and the native toolchain.
Character geometry, animation behavior, music loops, SFX listening and the
independent-agent acceptance gates still require their separate quality work.

Final `just ci` passed: 579 Rust tests (2 ignored) and 263 Python tests.
`out/trellis-reliability-20260905/result.json` indexes the unmodified trial logs,
validated mesh hashes, runtime receipt and final CI log.


## 2026-09-05 — Character, combat motion and recorded music-loop follow-up

One existing startup-diagnostic mesh, installed-runtime seed 102, passed the
seven-view raw review. Seeds 103 and 104 were rejected for weaker rear head
closure. The selected 1.8 m robot passed prepare, skin, export and explicit
run/melee rig checks. Front, side and rear posed strips show coherent joints
and a closed head and torso. It is promoted only in the external acceptance
project as `release_rusher`, with `release_run` and `release_swipe`.
The original failed two-attempt character baseline remains unchanged.

The user accepted the original seed-101 rifle shot and the isolated short
speech line. Both exact files are now promoted in the acceptance project.
Twenty-four follow-up idle takes remain rejected for nearly static poses.
This still prevents the complete three-clip character brief from passing.

Music has an explicit loop recipe: source offset, period, wrap crossfade and
algorithm version. Original generated source bytes are retained and hashed;
promotion preserves the recipe and source provenance. WAV retains the exact
selected period. OGG is decoded again and refused if encoding changes that
period; historical full-track behavior is unchanged. Fake recipes say that
no loop transform ran.

The first real follow-up loop failed its level seam. The second passed numeric
limits but the user rejected its combat theme. A stronger combat prompt then
failed with a 30 ms crossfade; a separately recorded one-bar crossfade passed
numeric limits. Listening acceptance for that last revision is pending.
Every rejected candidate and source remains intact.

A fresh CLI agent completed two independent placeholder projects from one
packaged toolkit in roughly 6.4 minutes, including documented rejection and
correction, promotion and verification. An additional fixture body/clip bundle
was self-contained. The native bundle command does not export a prop/audio
library together. The CLI documentation gap found by the trial is repaired
with `forge guide`. This is not one of the three complete real M5 sessions.

Evidence is retained under `out/release-quality-20260905/`; the independent
trial is `/tmp/forge-blind-cli-20260905/REPORT.md`. Full integrated verification
is in progress. Consumer/attachment contracts, complete real blind trials,
clean-machine installation and candidate packaging remain release work.


Integrated verification then passed: `just ci` exited 0 with 583 Rust tests
passed, two ignored, and 277 Python tests passed. External verify, audit and
manifest-check also passed. The actual two-clip robot bundle has one skin,
two named animations and no external buffer or image references.

The first CI attempt stopped on a fixed Clippy finding. The next encountered
a native crash in the default Conda Python during a placeholder test.
Ten isolated controls and the full rerun passed; the native cause remains
unresolved. The earlier failures are retained, not overwritten.

The user described the stronger music revision as closer, without accepting
its combat theme. A heavy-industrial-metal audition was retained without review when the user
selected aggressive electronic music with distorted bass and breakbeats.
That direction has its own 160 BPM follow-up and phrase-aligned loop recipe;
the original 120 BPM baseline remains unchanged. Music has not been promoted
on numeric evidence.


The user then accepted the aggressive electronic combat loop, including its
repeated joins. Seed 205 is promoted as `release_combat_loop` in the acceptance
project. Its 30.000-second WAV retains the 60-second generated source hash,
12-second selection offset and 1.5-second wrap crossfade. The requested tempo
is 160 BPM; the original 120 BPM baseline is still preserved separately.
The measured half-second boundary level difference is -0.826 dB, DC +0.0014,
with no clipped samples or silent lead/tail. Final project verify checked
25 items successfully; audit and manifest-check also exited zero.

`out/release-quality-20260905/result.json` indexes the final status and
`archives.json` names hashed snapshots of the acceptance project and blind
CLI trial. Idle generation, consumer/attachment contracts, complete real
blind trials, compatibility, clean installation and final packaging remain.


## 2026-09-06 — Accepted guided idle and three-clip export

The user accepted the weight-shifting guard generated through authored sparse
key constraints. It is promoted as release_idle alongside release_run and
release_swipe. This closes the missing idle in the follow-up character fixture;
it does not change the original failed text-only baseline or its retry budget.
The STATIC advisory remains recorded. Front, side and rear body reviews and
the explicit idle contact gate passed (118 contact frames, worst Y -0.009 m).

The final rusher bundle contains one skin and three named animations with no
external buffer or image references. Project verify checked 31 items; audit
and manifest-check passed. Full just ci passed with 583 Rust tests, two
ignored, and 284 Python tests. Twenty isolated runs of the previously crashing
voice test also passed; the native crash cause remains unresolved.

The diagnostic found and fixed late keyframe-input rejection before model
loading. Authored key generation is documented through the CLI, but remains
unavailable through the current MCP generate_clips tool. Consumer/attachment
contracts, complete real independent agent trials, compatibility enforcement,
clean installation and final packaging remain release work.

Current evidence, export hashes and a complete project archive are indexed in
out/idle-diagnosis-20260906/result.json. Earlier results remain intact.
