# forge_gen

The generator launcher. `python3 python/forge_gen <cmd> [--json] [--fake]`
— or `forge gen <cmd>` from the Rust binary, which is the same thing with
`--json` appended. `python3 python/forge_gen --help` lists the tree;
`backends/README.md` says what a backend directory is and how the launcher
finds an interpreter. Tests: `cd python && python3 -m pytest -q`.


The system launcher stays stdlib-only and dispatches to isolated backend
interpreters or ComfyUI workflows. Speech uses `moss_speech`; voice design
uses `moss_tts` on ComfyUI. Mesh preparation, skinning and generation use the
current command tree, not the preserved `spike_*.py` experiments.

For asset workflows and review gates, read the canonical procedures under
[`.agents/skills/`](../.agents/skills/). For installation and the current backend
map, read [`backends/README.md`](../backends/README.md).
