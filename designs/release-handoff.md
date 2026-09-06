# Asset Forge release handoff — 2026-09-06

Start here in a fresh session. Implementation checkpoint: `43c998b` on `forge2`.
This document is a continuation plan, not release approval. Read AGENTS.md and
relevant skills before acting. `designs/decisions.md` wins over specifications.
The milestone definitions remain in `designs/agent-ready-release.md`; the dated
entries in `designs/release-baseline.md` preserve the history. Earlier status
paragraphs in those files are historical and can be superseded by later entries.

## Product and boundaries

Deliver one versioned local Asset Forge installation, outside the game repos,
serving several independent projects. First target: Linux x86_64, prepared
NVIDIA RTX 4090 / 24 GB. No remote service, cloud infrastructure or new image
model. References and VFX images are external inputs. A submodule is optional.
Keep asset contracts engine-neutral; Bevy is the first verified consumer.
Do not spend this phase redesigning the playable scrapyard game.

## Completed evidence to preserve

- Shared installation discovery, external project initialization and packaging
  prototype; isolated libraries, generated agent configuration and shared GPU lease.
- 30 MCP tools, including verify, audit and manifest_check; embedded workflow
  guide also available through `forge guide`; fake sessions over both transports.
- Isolated MOSS speech runtime; TRELLIS pinned-runtime mitigation tested on this
  machine; explicit music-loop recipes and final-container audio validation.
- Preparation transform replay, explicit reference-clip contact checks, and
  early validation of authored motion constraints.
- External acceptance project has a reviewed prop, rusher body with idle/run/swipe,
  user-accepted rifle SFX, speech and aggressive electronic combat loop.
- Last full `just ci`: 583 Rust tests passed, two ignored, 284 Python tests passed.
  Final external verify: 31 checked; audit and manifest-check passed.
- One fresh CLI agent completed two fake projects. This is not a full real M5 trial.

The original frozen baseline failed several briefs. Follow-up successes do not
rewrite that result or reset its attempt budgets. The accepted idle uses authored
sparse constraints and ARDY, with its STATIC advisory retained. Do not reroll it
merely to remove that warning. The user accepted its weight-shifting guard.

The default Conda Python crashed once during a placeholder voice test. Twenty
fresh-process reproductions and the later full CI passed; its cause is unresolved.
TRELLIS has a tested interpreter mitigation, not a proven native root-cause fix.
Fresh CUDA-extension installation has not been qualified.

## Work order and completion gates

### 1. Maintain the external consumer workflow — first implementation slice

Inspect `designs/scrapyard/tools/`, `designs/scrapyard/batch-02/README.md`,
manifest/export code and the existing game integration. Identify what can become
supported recipes without promoting pose-specific constants into universal rules.

Build a small Bevy consumer fixture outside the toolkit workspace, using a staged
Forge installation and delivered assets. It must load a normalized prop and a
skinned character, play the three named animations, and exercise animation events,
body motion scale, a weapon socket/attachment and flat-floor grounding. Reuse
accepted outputs where appropriate; generate only if a missing fixture requires it.
The accepted rusher has no established rifle pose: use the existing armed scavenger
fixture for weapon checks rather than claiming the rusher guard supports a rifle.

Specify units, axes, pivot/socket transforms, root-motion ownership, grounding
ownership and notice/provenance delivery. Add an external VFX/atlas contract for
alpha, frame layout, timing and pivots; do not add raster generation. Establish how
prop/audio files accompany a character bundle: current `bundle` exports body and
clips, not an entire game library.

Exit: documented commands reproduce the delivery, a consumer launched from an
unrelated directory loads it, and automated checks plus rendered review exercise
the contracts without bespoke edits to Forge or its generated assets. Keep a small
repeatable fixture rather than relying on the full game's procedural presentation.

### 2. Close remaining agent workflow gaps

Expose the authored motion-constraint route over MCP using the same validation and
queue path as CLI, or explicitly revise the supported tools-only brief before trials.
The current `generate_clips` tool does not offer key constraints. The accepted idle
therefore does not establish a complete tools-only character workflow.

Resolve or explicitly qualify oversized review-image retrieval for tools-only
clients. Unify relative review-path behavior with explicit project selection;
`views` was observed to resolve relative files from cwd while generation used the
project. Keep CLI/MCP guides and configuration wrappers consistent.

Exit: both transports exercise the documented external-project path, including
constraint input, review retrieval and actionable invalid-input refusal.

### 3. Enforce compatibility and finish recovery qualification

Define and enforce a game's required toolkit compatibility before generation;
retain older project/record readers and never silently migrate accepted assets.
Test upgrades, rollback, backend-store adoption and path resolution with spaces,
symlinks and unrelated working directories.

Inventory existing recovery tests first. Fill demonstrated gaps in duplicate
submission, cancellation, reconnect/restart, partial outputs and cross-project
isolation. Diagnostics must retain observed failures without secrets; retries must
be bounded. Investigate the Python crash if it recurs, preserving native evidence;
do not call it fixed based on successful reruns.

Exit: compatibility mismatches refuse early, interrupted work returns actionable
terminal states, and another game proceeds without library contamination.

### 4. Qualify installation and freeze a candidate

Build a release-optimized distribution from its explicit allowlist. Verify disk,
driver, Python, Blender/ffmpeg and backend prerequisites; selected licence acceptance
must occur through the existing doors. Test unpacking elsewhere with the development
checkout inaccessible, two independent games sharing the install, and the optional
submodule path. Distinguish adoption of cached dependencies from a clean installation.

Exit: documented install, upgrade, rollback and uninstall preserve game libraries
and shared model stores. Publishable contents, dependency pins, notices, support
matrix and checksums are reviewable. If clean hardware/OS testing is unavailable,
record that gate as open rather than substituting a temp directory on this machine.

### 5. Run three fresh real agent trials

Freeze the candidate, briefs, rubrics and budgets before running. Retain the original
`release/baseline-briefs.json`; any revised trial brief is versioned separately.
Use fresh agents with only delivered instructions, no conversation or maintainer
hints. Cover two independent games on one installed toolkit, optional submodule,
CLI plus skills and tools-only clients. No simultaneous GPU generations.

Each complete trial includes prop, rigged character with three clips, weapon
attachment, SFX, looping music, speech, rejection/retry, interruption recovery,
verification and consumer load. Log every attempt and intervention. Human aesthetic
review is evidence; it must not be replaced by a numerical pass.

Exit: three complete sessions meet the frozen brief with zero maintainer fixes.
A failure returns to its owning implementation stage, with a new candidate and
clearly identified subsequent trials. Fake runs remain separate fault coverage.

### 6. Final release gate

Run `just ci`, crate/distribution packaging checks and candidate installation gates
on the exact revision to release. Assemble changelog, checksums, evidence and known
limitations. Confirm each claimed kind/platform has passed its required gates.
Prepare a concrete release artifact for review; publishing remains a separate action.

## Local evidence and recovery

The current acceptance project is `/tmp/forge-real-baseline-20260905`.
It is external test data, not part of commit 43c998b. Do not rely on /tmp surviving.
A complete snapshot is retained locally at:

`out/idle-diagnosis-20260906/acceptance-project-final.tar.gz`

SHA256: `ef4dc9c0273d9ee0ef6e70d1762a27cb48de4d1f434936197c3ab69beb4cd3ba`

Verify the hash before restoring to a new scratch directory. Do not overwrite a
live project. The archive includes authored constraints, selected and rejected
outputs, records and the final exported rusher. All out/ evidence is gitignored;
it exists on this machine and is not supplied by cloning the repository.

The final rusher GLB is `out/export/release_rusher_three_clips.glb` inside that
project. SHA256: `eb46f14567670360e72b3790ee9438d9183d5a69480a8376038ab78f19944f83`.
It contains one skin and `release_idle-loop`, `release_run-loop`, `release_swipe`,
with no external image or buffer URIs.

Evidence indexes, relative to the toolkit:

- `out/idle-diagnosis-20260906/result.json`: current acceptance, export/archive hashes, CI.
- `out/release-quality-20260905/result.json` and `archives.json`: character/audio and fake agent trial.
- `out/trellis-reliability-20260905/result.json`: interpreter mitigation and real lifts.
- `out/speech-isolation-20260905/result.json`: isolated speech and installation evidence.
- `out/real-baseline-20260905/`: original frozen baseline, including failures.

## Instructions for the next session

Start with stage 1. Inspect current implementation before adding abstractions.
Complete the external consumer slice and its checks, then update this plan with
what passed and what remains. Do not rerun finished model auditions, rewrite
records, reset baseline scores, regenerate accepted assets or declare the full
release complete. Follow AGENTS.md GPU and provenance rules. Ask only for missing
information that blocks a concrete next action. Do not commit or publish without
an explicit request; the prior commit request covered checkpoint 43c998b.

## Stage 1 continuation result — 2026-09-06

The local external consumer slice passed. This entry supersedes the instruction
above to start stage 1; **start stage 2 next**. Stages 2–6 remain open, and this
is not release approval or a new real-agent trial.

The supported fixture recipe is `release/consumer/stage.py`, included in the
explicit distribution allowlist with its standalone Bevy workspace, pinned lockfile,
grounding measurement, delivery verifier and software-render review wrapper.
`designs/consumer-contract.md` documents the commands, units, axes, pivots,
sockets, motion-scale application, root/controller ownership, grounding,
companion prop/audio delivery, notices and external atlas input contract.

The final local installation and consumer are outside the toolkit workspace:

- `/home/mmy/forge-stage1-20260906/install-final`
- `/home/mmy/forge-stage1-20260906/consumer-final`

The copied consumer executable launched from `/home/mmy` under Xvfb and llvmpipe.
It loaded the normalized magnet, the accepted rusher with idle/run/swipe, and
the scavenger with aim/run/fire and a runtime `hand_l` rifle attachment.
Six named animations changed bone transforms, every authored event fired,
18 companion audio files decoded, and 360 socket-transform checks passed.
Independent Bevy skinning checked 2,118,600 foot-vertex samples; the lowest Y
was −0.000000969 m, inside the 2 mm runtime tolerance. The maximum applied
visual-root correction was 0.062582 m. A separate existing rusher export
verified scale 0.9167000055 on Hips translation exactly once, with rotations
unchanged; both main fixture bodies record identity scale.

Codex reviewed the four final screenshots, including early fire and swipe poses.
An earlier black capture is preserved in `consumer-02`; the final recipe warms
render pipelines before starting all animation clocks. This is consumer rendering
review, not a new human aesthetic acceptance of the assets.

Validation: full `just ci` passed, 583 Rust tests, two ignored, 290 Python tests.
Standalone consumer compilation and Clippy with warnings as errors passed.
The delivery verifier checked 225 files and both complete original manifests.
The exported accepted rusher still matches the hash recorded above, and the
original acceptance archive hash was rechecked unchanged after the work.

Evidence index: `out/consumer-contract-20260906/result.json`.
The same directory retains `ci.log`, runtime evidence, four reviewed images and
`consumer-final.tar.gz`; its SHA-256 is recorded in `result.json`. Verify that
hash before extracting into a new directory. Earlier evidence directories and
the frozen baseline were not changed. The final consumer directory also retains
its original delivery receipt and runtime evidence.

Limits stay explicit: this was a debug development installation with cached
dependencies, not a clean-install qualification. The fixture plays independent
clips; blended contact, terrain IK and support-hand/finger IK remain consumer
work. The external VFX contract is documented and descriptor/hash checked,
without claiming VFX playback. Audio loading does not replace listening review.
No accepted assets were regenerated, no shipped records were edited, and no
commit or publication was made.

## Showcase archive — 2026-09-06

Relay Run is now the sole game showcase. The older Scrapyard tools, production
reports and rendered evidence referenced above are preserved unchanged at tag
`archive/scrapline-20260906` (commit `92f8866bf7e93c62f3c0efefbd5f1100e6b5e61b`).
`designs/scrapyard/README.md` identifies the retained shared consumer inputs.
The consumer staging recipe accepts both the new asset-notice location and the
original layout in existing external acceptance projects. Completed evidence,
accepted assets and the frozen release brief remain unchanged.

Archive follow-up validation: the acceptance snapshot SHA-256 was rechecked,
restored into a new scratch directory and used with the current checkout in
the updated consumer staging recipe. Delivery verification passed for 227 files,
six named animations, grounding and VFX. Both library manifests remained
unchanged, and scale 0.916700005531311 was applied to Hips translation once.
Evidence: `out/archive-consumer-check-20260906/delivery/`. This checks staging
after archival; it does not replace the original rendered acceptance evidence.
