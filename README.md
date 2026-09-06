# Asset Forge

Make, review and deliver game assets on your own GPU. Forge provides a CLI,
MCP tools and agent skills for static models, rigged characters, animation,
sound effects, music and speech. Games receive glTF and audio files with
records and a checked `assets/library.json` manifest.

Bring reference PNGs from your preferred image tool. Forge imports their
original bytes and provenance; it does not include an image generator.
Generated assets are reviewed through mesh views, animation strips and audio
plots, with human visual and listening review where available. Passing a
technical check does not establish artistic quality.

**[Relay Run](demos/relay-runner/README.md)** is the current showcase: a
third-person combat runner using generated characters, animation, audio,
enemies, scenery and passing ships. Its native game and WebGPU browser build
use the same game code. [Build the browser version](demos/relay-runner/web/README.md)
from the committed assets. Public hosting is pending; there is no live play
link yet.

## Install from source

The current development target is Linux with an NVIDIA GPU for real generation.
The full register targets a 24 GB card; the lean register targets 16 GB.
Use the fake tier for workflow tests without loading models. Headless visual
review still needs a graphics adapter; software Vulkan can serve that role.
See [backend setup](backends/README.md) for measured memory use, selected
models and installation requirements.

You need Rust via [rustup](https://rustup.rs), `just`, and Python 3.11 or newer.
The checked-in `rust-toolchain.toml` selects the Rust version. On Debian/Ubuntu,
install the Bevy build headers first:

```sh
sudo apt install libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev pkg-config
cargo install just

git clone https://github.com/JedimEmO/asset-forge
cd asset-forge
just install
export FORGE_HOME="$PWD"
forge --version
```

`just install` builds Forge and installs the executable in `~/.cargo/bin`.
Keep `FORGE_HOME` pointing to this checkout when using that executable.
The Python launcher has no runtime package dependencies; backend installers
manage their own environments. Blender and other generation prerequisites
are checked by doctor for the chosen workflow.

A [staged development installation](release/README.md) instead carries
`bin/forge` and its runtime resources together. That executable can locate
its resources without a checkout or `FORGE_HOME`. Clean-install qualification
and fresh agent release trials remain open in the
[release handoff](designs/release-handoff.md).

## Start a game project

Keep each game's sources, accepted assets and records in its own directory.
One toolkit installation and backend store can serve several games.

```sh
forge --project ~/my-game init --name my-game
forge --project ~/my-game setup --dry-run
forge --project ~/my-game doctor --quick
```

`init` selects what the project makes, its GPU tier and its ComfyUI endpoint
when needed. For an unattended workflow test, use
`--tier fake --make none --yes`. Fake outputs are marked placeholders.

Read the licence notices printed by setup before installing selected backends.
Then run `forge --project ~/my-game setup`; unattended acceptance names each
required licence explicitly with `--yes <id>`. A bare setup `--yes` is refused.
Accepted IDs belong to the shared installation, not to hand-edited project
configuration. Backend downloads default to `~/.cache/asset-forge/backends`,
overridable with `FORGE_BACKENDS_HOME`.

Initialization writes `.forge/AGENT.md` and `.forge/mcp.json`, plus root
`AGENTS.md` and `.mcp.json` when absent. Existing instructions and configuration
are preserved. The generated server entry uses an absolute executable and
explicit project path; merge it into your client's configuration as needed.
`forge --project ~/my-game agent-config` adds missing configuration later.

Without `--project`, Forge searches upward from the working directory for
`forge.toml`. With it, an agent can work from an unrelated directory without
selecting another game's library. `FORGE_TOOLKIT` is a legacy alias for
`FORGE_HOME`; an invalid explicit toolkit path fails rather than falling back.

## Make an asset

The [workflow skills](.agents/skills/) contain the complete recipes and review
steps. `forge guide` prints the installed workflow guide; `forge --help` and
subcommand help describe the current CLI.

| Asset | Production path | Review before delivery |
| --- | --- | --- |
| Static model | Import reference → lift → normalize → promote model | Raw mesh from multiple angles, then normalized model |
| Rigged character | Import reference → lift → prepare → skin/fit → export → promote body | Raw mesh, rig checks and animation on the real body |
| Animation | Generate takes → select and bake with an explicit recipe → promote clip | Sweep results and a rendered strip on the target body |
| SFX, music, speech | Generate or author → inspect → promote audio | Measurements, plots and listening |
| Voice identity | Design an audition → review → use its recorded reference for speech | Audition before producing lines |

Import a reference through the recorded door rather than copying it into the
source tree:

```sh
forge --project ~/my-game ref import ~/crate.png \
  --name crate --kind prop --source "Original drawing by me"
```

Use the [prop skill](.agents/skills/forge-prop/SKILL.md) for the remaining
steps. The standard backend paths use TRELLIS.2, SkinTokens, ARDY and the
ComfyUI audio integrations. Pixal3D assets in Relay Run are a documented
[experimental evaluation](designs/pixal3d-evaluation.md), not a registered
replacement for Forge's mesh backend.

The GPU is shared. Check `forge --project ~/my-game gpu` before generation;
do not run competing model loads. Backend budgets and observed peaks are
distinct. The [hosting notes](designs/hosting.md) record unloading behavior
and installation traps.

## Review, records and delivery

Fix defects at their source or through the production commands. Do not repair
a generated mesh, baked clip or shipped sidecar by hand. Keep rejected trials
and review evidence separate from accepted library entries.

- Bodies, models and audio claim file integrity. Clips additionally support
  reproduction checks with `forge audit`.
- Unknown measurements remain `null`. Provenance is `recorded`,
  `reconstructed` or `unknown`; a guess never becomes a measurement.
- After adding or removing library entries through the appropriate doors,
  regenerate the manifest and check it. The manifest is what a consumer reads.

```sh
forge --project ~/my-game verify
forge --project ~/my-game audit
forge --project ~/my-game manifest --check
```

The [consumer contract](designs/consumer-contract.md) defines units, axes,
sockets, root motion, grounding and accompanying notices. The
[external Bevy fixture](release/consumer/README.md) exercises those contracts
without changing accepted assets. `forge bundle` exports a body and named
clips into a self-contained GLB with a bundle record; companion models and
audio need their own delivery, as the fixture demonstrates.

## Agents and documentation

Use the generated game configuration for asset production. The repository's
`.mcp.json` and `.codex/config.toml` instead target this development checkout;
build `target/debug/forge` with `cargo build -p forge` before connecting.
MCP is available over stdio and through `forge serve` at `/mcp`.
Its embedded guide is available at `forge://guides/v1/workflow`; `tools/list`
provides this build's exact tool schemas. Oversized review images currently
need a client that can read the returned local path.

| Document | Purpose |
| --- | --- |
| [AGENTS.md](AGENTS.md) | Repository editing, provenance, GPU and verification rules |
| [CLAUDE.md](CLAUDE.md) | Claude entry point to the repository instructions |
| [.agents/skills](.agents/skills/) | Asset workflow recipes; Claude compatibility lives under `.claude/skills/` |
| [Backend guide](backends/README.md) | Installation, adoption, diagnostics and model environments |
| [Record contract](designs/records.md) | Record schemas and what verification establishes |
| [Decisions](designs/decisions.md) | Dated implementation lessons; wins over older specifications |
| [Release handoff](designs/release-handoff.md) | Completed evidence and remaining qualification gates |
| [Changelog](CHANGELOG.md) | Historical changes, not current setup instructions |

## Develop and verify

```sh
python3 -m venv python/.venv
python/.venv/bin/pip install './python[dev]'
PATH="$PWD/python/.venv/bin:$PATH" just ci
just publish-check
```

`just ci` checks formatting, Clippy, rustdoc, Rust and Python tests, headless
smoke, asset integrity, clip reproduction, manifests, MCP sessions and fake
production pipelines. Compilation and execution have separate CI steps.
Full library contact sheets run with `just sheets`, or the CI workflow's manual
`full_review` option; ordinary PRs keep the rendering smoke, renderer tests and
byte/pose audit. Real generation, Blender authoring and human review are separate checks. `just publish-check` packages seven library crates and
builds them in isolation; it does not publish them. Crates are currently used
through Git or path dependencies.

Checkout recipes build `target/debug/forge`. To run a production recipe from
a game, use `just --justfile "$FORGE_HOME/justfile" --working-directory . <recipe>`.
Development recipes always check the toolkit; CLI validation with an explicit
`--project` checks the game.

The tree separates toolkit code (`crates/`, `python/`, `backends/`, `rigs/`),
the sample library and its inputs (`assets/`, `assets-src/`), and the Relay Run
showcase (`demos/relay-runner/`). `release/` holds distribution tooling and
consumer checks. Build output, model installations and local review runs are
excluded from Git. The older Scrapline demo and its review media are preserved
in [the archive](designs/scrapyard/README.md); shared asset inputs remain active.

## Licences

The code is MIT OR Apache-2.0. Asset terms are separate: consult
[asset notices](assets/LICENSE.md), the [source ledger](assets-src/SOURCES.md)
and each asset's records. The current texture-baking path retains a
non-commercial nvdiffrast notice. Relay Run carries its own
[delivery notices](demos/relay-runner/web/NOTICES.md), including experimental
asset provenance. Setup prints the selected backend notices before acceptance.
