# forge_mcp

The MCP server an agent drives. Look at the library — lists, contact sheets,
audio plots — run a generator, ship a clip or a sound, ask what this machine
can do. A library crate with one entry point, served over stdio by
`forge mcp`. Not published: it is the toolkit's own agent surface, and the
engine-free crates beneath it are the library surface.

```json
{ "mcpServers": { "forge": { "command": "./target/debug/forge", "args": ["mcp"] } } }
```

## Three rules this server lives by

**stdout is the JSON-RPC frame stream and nothing else.** Every diagnostic
goes to stderr; `serve` installs a panic hook that does the same. A stray
`println!` corrupts the protocol and the failure looks like the server
crashing for no reason.

**Refusals are successful frames.** A clip that does not exist comes back as
an error *result* carrying the list of clips that *do* — so the agent fixes
its own call next turn. An `Err(ErrorData)` would be rendered opaquely by the
client and teach it nothing.

**The server never decides what ships without a human.** There is no review
queue: `promote_clip` and `promote_audio` write the library directly, and so
they refuse a name that is already taken unless told `overwrite`. There is
no promote for a mesh at all — a body or a model goes through the genart
skills, where a human looks at the lift, the rig and the views before
anything is filed.

## The renderer is this binary

Every picture shells out to `std::env::current_exe()` — `forge views`,
`forge sheet`, `forge doctor --json` — never a second binary and never a
name on `PATH`. An MCP client launches the server from wherever it likes
with whatever working directory it has; the one path that is always right
is our own. It also means Bevy is linked once, the server builds in seconds,
and the sheet an agent sees is the sheet a human would get from the same
command.

The project comes from `--project <DIR>`, then `$FORGE_PROJECT`, then the
walk up from the working directory to the first `forge.toml` — the same
walk every other `forge` verb does. The flag and the variable must name the
root itself.

## The tools

| Verb | Tool | Arguments | What it does |
|---|---|---|---|
| looking | `list_models` | `kind?` (model\|body), `filter?` | bodies and models with prompt, tags, provenance, generator, measured size |
| looking | `list_clips` | `filter?`, `tag?` | clips with measured length, record and the recipe's non-identity knobs |
| looking | `list_audio` | `kind?` (sfx\|music\|voice), `filter?` | sounds with cached measurements; `!` defect, `?` worth a look |
| looking | `render_model` | `name_or_path`, `views?`, `head_row?`, `cull_off?`, `return_image?` | `forge views` — a mesh from every angle; a path under `out/` renders culling off |
| looking | `render_clip_strip` | `clip`, `body?`, `frames?`, `views?`, `columns?`, `cell?`, `t0?`, `t1?`, `head_row?`, `return_image?` | `forge sheet` — poses across a clip on a body; exit 1 (no bones driven, frozen) comes back as an error result *with* the picture |
| looking | `inspect_audio` | `name_or_path`, `return_image?` | `forge_audio` in process: numbers, the record, a waveform-over-spectrogram plot |
| making | `generate_clips`, `generate_audio` | see `tools/generate.rs` | `forge gen …`; writes only under `out/` |
| shipping | `promote_clip`, `promote_audio` | see `tools/promote.rs` | the library's own doors; refuse a taken name unless `overwrite` |
| asking | `doctor` | — | `forge doctor --json` relayed, plus the renderer, the stage body and the library as scanned now |

Images are inlined as PNG up to 3.5 MB; over that the frame says where the
file is. Every PNG goes under `<out>/mcp/`, so it is still there when the
human opens it and `out/` being gitignored covers it. Renders have a
three-minute ceiling, generators twenty, doctor five; a timeout is a refusal
that names the ceiling and points at `doctor`.

## Layout

```text
lib.rs       serve(config) / run(config), and the error a caller sees
config.rs    Config::from_args — the project, and the exe-relative renderer rule
server.rs    ForgeServer: the state, the instructions text, the sum of the routers
util.rs      refusals, inline images, supervised subprocesses and their ceilings
tools/       one file per verb; each exposes `router()` and tools/mod.rs sums them
```

Every `rmcp` type stays inside this crate. The workspace pins `rmcp = "=1.8.0"`
because its `#[tool_router]` / `#[tool_handler]` macros churn between minors,
and a bump has to be one crate's problem: the binary sees a `Config`, a
`serve` and a `ServeError`, nothing else.

## Tests

No GPU and no sample library. The tests build a temporary project with the
toolkit's `rigs/humanoid` profile, promote the fixture mannequin as a body
and the pinned `gen_roll.npz` as a clip through `forge_library`'s own doors,
and point the renderer at a path that does not exist — so a test that
reaches it gets an unlaunchable refusal naming it rather than a Bevy
window. `cargo test -p forge_mcp` covers refusal shapes, name resolution
(a typed path wins over a library file name), config precedence, the
instructions text and the pinned tool surface.
