# Shared local installation

This is a development distribution layout. It has not passed the real-backend
quality gates or blind-agent release trials.

Keep the toolkit directory outside your game repositories, in a user-writable
location. Its `bin/forge` locates the Python launcher, rigs and backend definitions
from its own executable location. No toolkit `forge.toml` or source checkout is required.
Use Python 3.11 or newer. Real generation additionally needs the selected backends;
rendering needs a working graphics driver. The first target is Linux x86_64.

Initialize each game with an explicit path:

```sh
/path/to/asset-forge/bin/forge --project "/path/to/game one" init --name game_one --tier fake --make none --yes
/path/to/asset-forge/bin/forge --project "/path/to/game two" init --name game_two --tier fake --make none --yes
```

These commands select an empty workflow-test configuration. For production,
choose the required kinds and GPU tier through `init`, then inspect
`setup --dry-run` and `doctor --quick` before installing backends.

Initialization writes `.forge/AGENT.md` and `.forge/mcp.json` inside each game.
New games also receive `AGENTS.md` and `.mcp.json`; existing files are preserved.
Merge the generated MCP server entry into your client's configuration as needed.
The absolute command and explicit project argument work from any working directory.
Run `forge --project /path/to/game agent-config` to add missing configuration later.

`FORGE_HOME` overrides toolkit discovery; `FORGE_TOOLKIT` is its legacy alias.
The first set variable wins. A missing explicit toolkit is an error, not permission
to use a different install. Relative paths are resolved from the invoking directory.
Unsetting both variables lets the binary find its accompanying resources.

Models and environments use the shared machine store selected by
`FORGE_BACKENDS_HOME` (default `~/.cache/asset-forge/backends`). Setup leaves local
links and receipts under this toolkit's `backends/`, so the toolkit must be writable.
A new distribution does not contain those installation receipts or acceptance records.
Use setup's documented adoption paths when attaching existing environments.

Keep each game's `forge.toml`, source records and accepted assets in that game.
The toolkit installation can be removed without deleting game libraries or shared
model stores. Stop its jobs first. To try an upgrade, unpack beside the previous
installation and validate it before changing the game's MCP configuration.
Automatic compatibility enforcement and configuration upgrades remain release work.

To stage this layout from a checkout, build Forge and run:

```sh
cargo build --release -p forge
python3 release/package.py --output out/asset-forge-candidate
```

The packager refuses an existing output directory and selects runtime files from
an explicit allowlist. `distribution.json` records each payload's SHA-256 and the
binary's reported version. Its candidate status is not a release approval.


Production queues across games share `~/.cache/asset-forge/gpu/card.lock` and its
recovery state. Job rows remain in each game's `out/serve`. `forge gpu --free`
refuses to unload models while another Forge job holds the shared lease.
Stop older Forge daemons before adopting this build: older versions used separate
per-project GPU locks and do not participate in the new shared lock.
The current supported scope is one GPU and one local user account.
