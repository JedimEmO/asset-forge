# Working in asset-forge

A local toolkit for making, judging and shipping game assets — meshes, rigged
characters, animation clips, audio — with every generator on the user's GPU.
The generating half is the easy half; the rules below are about the judging
half, and about not breaking what a record claims. Lessons live in
`designs/decisions.md` (it wins over any spec); install traps in
`designs/hosting.md`; the step-by-step for each asset kind is a skill under
`.claude/skills/`.

## The one-way rule

Derived artefacts are never repaired by hand. A `.blend`, a `.glb`, a baked
clip, a sidecar — each is a function of a record and a source, and the fix
for a defect is upstream of it. A hole in the back of a skull is a seed to
sweep or a reference PNG to revise, never a patch in Blender: a hand-repaired
file is the one thing in the chain nobody can re-derive. Hand-editing a
record to make a gate pass is the same defect in a smaller file.

## Look before you spend

- `just views` on the raw lift *before* the rig. One front view
  underdetermines a shape; culling off shows what is actually there.
- Read the reference PNG (the Read tool shows images) before lifting it, and
  again when a fit gate refuses the mesh — the gate's message names the
  image as the fix because it usually is.
- A clip is judged on the strip rendered on the real body, not on the
  review table. Numbers say it is wired; the picture says what it is.
- The user's eye in the studio outranks every sheet you render. "Still
  hollow" means still hollow; go back to the seed or the reference.

## The GPU is shared

One 24 GB card, and nothing on it co-resides. Peaks measured 2026-08-30
(`designs/hosting.md` § GPU co-residency and § the first real run of the
audio path): ARDY's sweep is the expensive one at
**15.4 GB**, then ACE-Step at **13.1 GB**, MOSS-SoundEffect at
**10.0 GB**, MOSS-TTS at **7.1 GB** over the voice designer's **5.3 GB**
(the pack unloads neither, so they add up); TRELLIS.2 at 1024³ is
**4.7 GB** and SkinTokens **3.3–4.4 GB**. Each `backend.toml`'s `vram_gb`
is a **budget** that sits above its measured peak — never quote one as a
measurement, and never leave one under one.

The audio models run inside the ComfyUI host now, so there is no resident
ACE-Step server of its own and no `--stop-server`. **`forge gpu --free` is
the door for the card; for the MOSS pack only `systemctl --user restart
forge-comfy` returns it (measured, 4.4 s).** `POST /free` unloads native
models — ACE-Step's card comes back with no intervention at all — and does
nothing for what TTS-Audio-Suite loaded: there is no unload node in the
pack at this pin, and 9.1 GB stayed on the card after an effect. What
remains otherwise is the unit's ~0.4 GB of CUDA context. `just gpu` before
any generate; never run two generates at once, and never start one while a
studio window with a model loaded is still up on the real adapter — or
while the ComfyUI unit holds a model. Doctor says who is holding the card;
believe it.

## Records

- `null` means unknown. A default is never written as a measurement.
- Provenance is `recorded | reconstructed | unknown` and only ever moves
  down. A migration nulls what it cannot know; it never launders a guess up.
- A clip recipe states every knob. Nothing is inherited from the sweep or
  from the clip it is about to replace.
- A reference PNG claims integrity (sha256) and a row in
  `assets-src/SOURCES.md`, never regeneration. A PNG without a row fails
  `forge verify`. **The reference is brought, not made here** — no image
  model ships in this toolkit; it comes through `import_reference` /
  `forge ref import` (Phase 3), and the sample library's are drawn in Grok
  and said to be, in `SOURCES.md`.
- Bodies, models and audio claim integrity; clips claim reproduction
  (`forge audit`, ≤ 1 mm). Do not promise the wider claim for the narrower
  kind.
- A lift record names its texture baker (nvdiffrast, non-commercial). Keep
  it there; it is a licence fact, not a detail.

## The manifest is the only thing a consumer sees

Sidecars are the truth and `assets/library.json` is a projection of them,
committed and checked. After any hand add, remove or rename under `assets/`,
run `just manifest` and then `just manifest-check`. A stale manifest does not
fail here — it fails in the consumer, with a message that names nothing.

## What the project chose

`forge.toml`'s `[make]` says which of the six kinds this project makes and
`[hardware]` says which register it runs in. Everything downstream reads
them: `forge setup` installs only what a chosen kind needs, and doctor
prints **`off`** for a kind that was not chosen — never probed, never a
reason to exit 1. Doctor's words are `ok | partial | missing | broken |
off`, and **exit 1 only while a *chosen* backend is not ok**. Tier `fake` is
a first-class answer that chooses nothing, so a machine with no card reads
all-off and exits 0. A `forge.toml` written before those tables reads as
every kind chosen with the tier detected, so nothing goes quiet.

A licence is accepted at a door, never in a file: `forge setup --yes <id>`,
or the MCP `setup`'s `accept` after `licences` showed the text, recorded in
`$FORGE_BACKENDS_HOME/licences.json` beside the installs — the install is
what is licensed. A bare `--yes` is refused. Nothing in `forge.toml` can
accept anything: it is hand-edited, and that would let an acceptance be
typed rather than given.

## Verification without a display

- `just ci` is the gate: fmt, clippy + rustdoc (warnings as errors), Rust
  tests, pytest, headless smoke, audit, check-bodies, manifest-check,
  verify, mcp-check, mcp-session, ci-fake — the same list GitHub Actions
  runs. It
  excludes eye-renders (not byte-stable across GPUs), every real generator,
  every Blender step and anything that rewrites `assets/`; ci-fake runs the
  generate pipelines on placeholders in a throwaway project, and
  mcp-session runs the agent's whole path over both transports against a
  tempdir project at tier `fake`.
- No GPU, or the card is busy? `FORGE_FAKE=1` makes every `forge gen`
  write branded placeholders through the same doors and validators (a fake
  refuses to overwrite a real file); `just ci-fake` is that, end to end.
- Headless sheets and views need a wgpu adapter — llvmpipe is enough — and
  no window. `env -u DISPLAY -u WAYLAND_DISPLAY` is how CI runs them.
- To drive the studio window for real: Xvfb plus python-xlib XTest. Call
  `set_input_focus` on the window first or every key is dropped; leave ~1 s
  between keys under llvmpipe (one frame per second: press and release in
  one frame is one keystroke, sometimes zero); screenshot to confirm where
  the selection landed rather than counting presses.
- `pkill -f <pattern>` matches the shell running the pkill when the pattern
  appears in the same command line, and kills it. Record the PID at launch
  and kill by PID.

## Commits

- Only when the user asks. Narrative messages: what changed and why, in one
  breath.
- Never commit `*.blend1`, `out/`, `backends/*/.env`, `backends/*/.checkout`,
  or a reference PNG without its `SOURCES.md` row.
- A new install trap goes to `designs/hosting.md` under its backend, dated.
  A new lesson goes to `designs/decisions.md` with the date it was learned.
  Neither goes only in a commit message.

## Working from another project

- From a project made by `forge init`, the recipes run as
  `just --justfile <toolkit>/justfile --working-directory . <recipe>` with
  `FORGE_HOME=<toolkit>` exported; `forge` walks up from the working
  directory to the project's `forge.toml`. The dev recipes (`fmt`, `check`,
  `test`, `pytest`, `ci`, …) are the exception the other way: they always
  act on the toolkit checkout — `just ci` is the toolkit's own gate, and a
  project verifies its library with `just verify`, `just audit`,
  `just manifest-check`.
- Every recipe builds and runs `./target/debug/forge` itself, and
  `.mcp.json` launches that same binary — on a fresh clone run any recipe
  once (`just doctor`) before the MCP server can start; `just install`
  puts a global `forge` on PATH for shells outside the checkout.
- A shipped record or sidecar is never edited by hand — not to fix a typo,
  not to make a gate pass. The door is `forge promote <kind> … --overwrite`
  with the corrected flags, then `just manifest`.
