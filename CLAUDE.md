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

One 24 GB card. TRELLIS.2 at 1024³ (~22 GB), ARDY (~16 GB), MOSS (~6–12 GB)
and the ACE-Step server (resident ~8 GB until `--stop-server`) do not
co-reside. `just gpu` before any generate; never run two generates at once,
and never start one while a studio window with a model loaded is still up
on the real adapter. Doctor says who is holding the card; believe it.

## Records

- `null` means unknown. A default is never written as a measurement.
- Provenance is `recorded | reconstructed | unknown` and only ever moves
  down. A migration nulls what it cannot know; it never launders a guess up.
- A clip recipe states every knob. Nothing is inherited from the sweep or
  from the clip it is about to replace.
- A reference PNG claims integrity (sha256) and a row in
  `assets-src/SOURCES.md`, never regeneration. A PNG without a row fails
  `forge verify`.
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

## Verification without a display

- `just ci` is the gate: fmt, clippy, tests, headless smoke, audit,
  check-bodies, manifest-check, verify. It excludes eye-renders (not
  byte-stable across GPUs), every generator, every Blender step and anything
  that rewrites `assets/`.
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

## Multi-agent work

- Phases are workflows with disjoint file ownership. An agent owns the paths
  its prompt lists and nothing else.
- Never `SendMessage` a workflow-internal agent by id. It is not addressable
  from the parent; the fallback resumes it from its transcript and forks a
  duplicate that edits the same files as the real one.
- An agent's question is answered by folding the answer into the next
  phase's prompts, or by the parent making the edit itself.
- Every prompt gives its agents a stated default for every decision they
  might otherwise ask about. An agent that has to ask has a prompt bug.
