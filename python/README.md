# forge_gen

The generator launcher. `python3 python/forge_gen <cmd> [--json] [--fake]`
— or `forge gen <cmd>` from the Rust binary, which is the same thing with
`--json` appended. `python3 python/forge_gen --help` lists the tree;
`backends/README.md` says what a backend directory is and how the launcher
finds an interpreter. Tests: `cd python && python3 -m pytest -q`.
