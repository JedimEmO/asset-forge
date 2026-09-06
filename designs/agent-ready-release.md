# Asset Forge: first supported agent-facing release

Status: implementation started, 2026-09-05.
The shared-installation foundation and core MCP workflow are implemented;
real-backend, recovery and release acceptance gates remain open.
Current evidence and blockers are tracked in [the release baseline](release-baseline.md).

## The release promise

An unfamiliar agent can use Forge from a separate game project,
make and inspect supported game assets, recover from documented failures,
and deliver verified files without editing Forge's implementation or receiving maintainer hints.

The primary setup is one versioned local Asset Forge installation serving several independent
game projects. It lives outside those projects and needs no development source checkout.
A pinned submodule is an optional development setup, not a requirement for using Forge.
Each game owns its assets, references and configuration.
Model installations and GPU services remain machine resources shared across projects.
Each game records its required Forge compatibility range; commands report version mismatches
before starting work, so updating the shared installation cannot silently break another game.

The proposed first platform is Linux x86_64 with a verified NVIDIA GPU configuration.
The current 24 GB machine is the reference target, not proof of support for every card.
Lower-memory tiers must earn a tested support entry before we advertise them.
Fake mode remains available for workflow testing without a GPU.

Local stdio and loopback HTTP remain supported transports.
Remote service support is outside the product scope for this release.
The user confirmed that Forge is a local tool and should support one installation
used by several games without living inside their project folders.

## What we already have

Forge has 27 MCP tools, a project format, queued generation, inspection tools,
promotion gates, provenance records and engine-neutral exports.
Existing CI covers Rust/Python tests, headless rendering, fake workflows,
MCP sessions over both transports and packaging of the publishable library crates.
We extend those checks rather than replace the working pipeline.

The external-project MCP tests passed during the assessment.
They use fake generators and therefore do not establish real model quality or clean-machine setup.
The scrapyard production run supplies real evidence, but it also required source fixes,
backend diagnosis and project-specific attachment and grounding code.
Those interventions identify work to retire before release.

## Scope and compatibility

Reference images stay external inputs. We provide their requirements, import validation
and examples through the agent interface; we do not add an image model to this release.

Props, humanoid characters, clips, SFX, music and voice are evaluated individually.
A backend cannot be advertised as supported merely because it produces a valid file.
If a kind misses its quality gate, we repair it before claiming support,
or explicitly mark it experimental and revise the release promise before shipping.
Procedural substitutions cannot count as proof that a model workflow passed.

The existing policy permits opt-in restricted backends with recorded terms.
A release need not replace those backends just to ship the toolkit.
It must accurately distinguish code licensing, backend acceptance and asset notices;
open-source distribution must not be presented as blanket commercial clearance.

Preserve existing project and record readers, the one-way derivation rule,
unknown provenance values, direct promotion semantics and caller authorization.
Any format change needs a version and a compatibility test.
No silent library migration, automatic promotion, or hidden change to accepted assets.

## Milestones

### M0. Establish the release baseline

Reconcile this plan with `decisions.md`, `forge2.md` and the implemented tool surface.
Inventory the remaining gaps before adding new abstractions.

- Record toolkit version, project/manifest/record schemas, backend pins and the tested machine matrix.
- Capture one real baseline for each claimed asset kind, including failed candidates and intervention time.
- Make a concrete issue list from the scrapyard findings: preparation replay, backend crashes,
  pickup geometry, audio envelopes, attachment alignment and foot contact.
- Classify each issue as a release blocker, a documented limitation or later work.

**Exit:** a checked-in support matrix and reproducible baseline distinguish fake evidence,
real generation, visual/audio judgment and engine integration.
We know which kinds the release intends to support and what each must demonstrate.

### M1. Make the installation and project boundary explicit

Package the executable with the Python launcher, rig profiles, backend definitions,
workflow templates and agent documentation it requires.
Backend weights and environments are installed or adopted separately.

- Make an installed toolkit outside all game projects the primary bootstrap path.
- Retain a pinned submodule as an optional development path using the same root-resolution contract.
- Let several game projects share one installation without sharing libraries or project settings.
- Record and check each game's toolkit compatibility requirements before generation.
- Generate explicit project-root and toolkit-root configuration for MCP and CLI use.
- Resolve absolute, relative and symlinked paths consistently, including paths with spaces.
- Remove dependence on the caller's directory, a developer's `target/debug`, or an undeclared checkout.
- Make initialization safely rerunnable and failures clear about any partial state.
- Generate a game-level agent guide and launch configuration without overwriting existing instructions.
- Report disk, driver, environment and acceptance prerequisites before installing models.
- Produce release archives from an explicit allowlist; omit sample games, caches,
  credentials, rejected drafts and unrelated source assets.

**Exit:** install and initialize from an unrelated directory on a clean environment.
One installed toolkit works in two independent game projects, including concurrent requests.
The optional submodule setup passes the same project-isolation checks.
Operations cannot accidentally target the toolkit's sample library.
The unpacked release continues working when the development checkout is unavailable.

Main areas: `toolkit.rs`, project/config resolution, setup, release packaging and templates.

### M2. Complete the agent workflow

Expose the remaining operations an agent needs to finish a job without a shell escape.
Keep CLI and MCP behavior backed by the same implementations.

- Add MCP access to library verification, clip reproduction audit and manifest checks,
  using focused tools or one clearly scoped validation operation after reviewing overlap.
- Make project creation usable without a manual reconnect.
  Bind a server to one explicit project; do not introduce ambiguous global project switching.
- Serve versioned production guides, reference requirements, examples and recovery instructions
  through discoverable MCP resources or an equally accessible documented mechanism.
- Generate client-specific skill/configuration wrappers from one canonical guide source.
- Return stable identifiers, structured results, artifact locations and concise next actions.
- Keep inspection images available to clients; oversized results must have a documented retrieval path.
- Ensure initialization instructions and tool descriptions reflect the actual supported surface.

**Exit:** a tools-only scripted session can initialize, import, generate, wait, inspect,
promote, verify and export in an external project over each transport.
Documentation examples are exercised, so they cannot silently drift from the API.

Main areas: `forge_mcp`, shared validation APIs, canonical guides and MCP session tests.

### M3. Make failures recoverable and repeated calls safe

Preserve the existing queue and card lease. Improve the contract around failures
instead of asking the caller to debug arbitrary backend internals.

- Classify invalid input, unsuitable references, quality rejection, unavailable capacity,
  broken environments, missing acceptance and interrupted jobs separately.
- Attach a stable error code, useful explanation, retry guidance and diagnostic artifact.
- Bound retry policies and report their budget. Never loop indefinitely on a bad reference.
- Verify duplicate submission behavior, cancellation, reconnects, daemon restarts,
  partial outputs and simultaneous requests from different game projects.
- Reproduce the intermittent TRELLIS failures and establish a tested environment fix
  or an honest unsupported configuration. Allocator debugging alone is not a resolution.
- Test GPU reclamation and interference from external viewers or model hosts.
- Keep partial results out of accepted assets; preserve records and explicit overwrite behavior.
- Provide a diagnostic bundle that excludes secrets and records relevant versions and job history.

**Exit:** fault-injection tests cover the job lifecycle, and real backend soak runs
finish or return actionable terminal failures without maintainer intervention.
Project writes and exports have tested path boundaries; a failed job cannot corrupt another project.

Main areas: launcher/backend probes, `forge_serve`, job/error contracts and recovery tests.

### M4. Define what a usable asset delivery contains

Structural validity, review evidence and runtime suitability must remain distinguishable.
Promotion should never imply that an unmeasured quality property passed.

- Publish review criteria for geometry, rigging, animation, SFX, music and voice.
- Capture candidate history and measured rejection reasons alongside human/agent judgments.
- Add audio checks for onset, tails, DC offset and requested loop behavior where measurable.
  A clean waveform alone cannot establish that a sound matches its brief.
- Provide maintained attachment and grounding recipes derived from the game integration work.
  Validate socket transforms, units, facing, foot contact and animation events on actual bodies.
- Document engine export requirements and produce self-contained bundles with provenance and notices.
- Add a supported image/atlas import-export contract for externally created VFX used in examples,
  including alpha convention, frame layout, timing and pivots.
  Raster generation remains outside the release.
- Add a small external consumer fixture that loads exports and exercises their metadata.
  Keep the supported asset contract engine-neutral; Bevy is the first verified example.

**Exit:** every claimed asset kind has real reviewed fixtures and a repeatable consumer check.
An external game uses the delivered files and documented recipes without bespoke source surgery.
Engine-specific presentation choices remain the game's responsibility.

Main areas: manifests/exports, inspection reports, audio checks, recipes and consumer fixtures.

### M5. Run blind external-agent trials

Use agents with fresh context and only the delivered instructions.
The trial cannot rely on our conversation, implementation inspection or maintainer hints.

Run two unrelated game projects against one installed toolkit outside both repositories.
Also exercise the optional submodule setup.
Exercise a tools-only client and a CLI-plus-skills client.
Use real backends for production evidence and fake mode separately for fault coverage.

The shared brief requires a normalized prop, a rigged character with three clips,
a weapon attachment, a short SFX, a looping music track and a voice line.
It also requires one rejected candidate, a documented retry, interruption recovery,
final library verification and a consumer scene loading the exported results.

Before evaluating, freeze the briefs, acceptance rubrics, attempt budgets and test versions.
Record every attempt, GPU time, tool calls, interventions and rejected result.
Do not reset the score by hiding a failed trial or switching to procedural assets.

**Exit:** three fresh end-to-end sessions finish the frozen supported brief,
including both installation modes, with zero maintainer intervention and no source edits.
All delivered files pass structural and integrity checks and the agreed quality rubric.
Any unsupported kind or blocking failure returns to its owning milestone.

### M6. Produce and verify the release candidate

Freeze the supported contracts and package the exact candidate tested in M5.
Publishing is a separate final action after a reviewable candidate exists.

- Run `just ci` and the existing crate packaging checks on the candidate revision.
- Add distribution installation tests and the real-backend evidence report to the release gate.
- Publish checksums, dependency/backend versions, notices, supported platforms and known limitations.
- Document upgrade, rollback and uninstall behavior, preserving game libraries and shared model stores.
- Test the archive after unpacking into a new location, with the source checkout inaccessible.
- Review release contents and examples for secrets, undeclared dependencies and false quality claims.
- Choose the version and changelog from the actual compatibility changes.

**Exit:** the release candidate reproduces the tested setup and workflows from its own contents.
Every remaining limitation is explicit, and every supported promise has linked evidence.

## Order and checkpoints

Start with M0, then M1 and M2. Freeze the shared contracts before M3 and M4 converge.
M5 validates their combined result; M6 packages the candidate that passed.
Packaging details should be exercised early, not first discovered during M6.

The first useful checkpoint is two external games using one Forge installation,
with agent guides, explicit roots and the complete fake MCP workflow.
The next checkpoint is the same experience from a release archive and real backends,
plus the optional submodule compatibility check.
The final checkpoint is the blind trial and verified release candidate.

We estimate implementation effort after M0 measures the backend failures.
Model quality and clean-machine installation are the largest uncertainties;
a fixed calendar promise before those measurements would hide the release risk.

## Outside this release

Remote hosting, multi-tenant cloud scheduling, cloud accounts and billing are outside
the intended local product.
Windows/macOS support, additional skeleton families, new generation models,
and a general-purpose game runtime SDK are deferred.
They do not replace the documented release gates above.

## Starting implementation

The first implementation slice is M0 plus the smallest M1 acceptance fixture:
two throwaway games outside this checkout that initialize through one explicit toolkit path,
receive usable agent instructions, and cannot write to each other's libraries or the toolkit's sample assets.
Keep the prototype game and already accepted asset records intact while building that fixture.
