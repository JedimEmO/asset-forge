# forge

The one command line over the library crates: every verb here is wiring and
printing. The project is found, the flags become a request, a library crate
does the work, the result is printed and becomes an exit code. `forge mcp`
is the same binary serving the same library to an agent, and the studio is
the same binary opening a window — which is what keeps all three from
disagreeing about what a promote does or what `verify` checks.

Not meant for crates.io; it is built by any `just` recipe in the
[repository root](../../README.md), or by `cargo build -p forge`.

## The verbs

```text
forge init [--name]                       make a project here
forge catalog [--kind] [--filter] [--tag] what the library holds
forge manifest [--check]                  project the library into assets/library.json
forge verify | audit | rebake | migrate   the engine-free checks and repairs
forge promote clip|body|model|audio ...   the four doors into the library
forge audio inspect|list                  measure a sound, or every sound
forge rig export-contract|fixture|check   the profile's contract and mannequin; one mesh held to it
forge gen <cmd> [args…]                   a generator, through python/forge_gen
forge doctor [--json]                     what this machine can do, every backend probed
forge gpu [--json]                        who holds the card, and whether the largest backend fits
forge sheet <clip> [--body]               a clip on a body as a contact sheet
forge views <name|path.glb>               one mesh from every angle, culling off for a lift
forge turntable <body>                    every view of a body, posed on the reference clip
forge bones <clip> [--body]               which bones a clip drives, no GPU
forge studio [--model] [--audio] …        the viewer window
forge mcp                                 serve the MCP tools over stdio, for an agent
```

`forge <verb> --help` carries the full flag table for each.

## Exit codes

- `0` — it worked, or a check passed.
- `1` — a gate did not hold: `verify`, `audit`, `manifest --check`, an audio
  file that is silent or clipped, a bake that failed, a doctor with a
  backend that is not ok.
- `2` — the call could not be honoured as written: a flag that does not
  parse, a file that is not there, a name already in use, no project above
  the working directory. A refusal says what does exist.
- `3`–`6` — `forge gen` relaying the Python layer's own table: missing
  backend, input rejected, backend failed, missing tool.

## Finding the project

Every command that touches a library finds it the same way: `--project <dir>`
names the root, otherwise the walk up from the working directory stops at
the first `forge.toml`. `forge init` is the one command that expects not to
find one. Outside the toolkit checkout, `forge init` also needs
`FORGE_TOOLKIT` (or `FORGE_HOME`) pointing at the checkout so it can install
the rig profile — it exits 2 and says so when it cannot.
