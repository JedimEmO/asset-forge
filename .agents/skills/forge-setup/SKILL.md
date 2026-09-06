---
name: forge-setup
description: Get a machine ready to make things — answer the three questions `forge init` asks, read the licence screen `forge setup` prints before a byte downloads, accept by name, log in for the gated weights, install or adopt each backend, and read the doctor table down to its fifth word. Use when a command exits 3 saying a backend is missing, when `just doctor` is red, or on a fresh machine.
---

# Setup: `forge init` → `forge setup` → `just doctor`

**Speech update, 2026-09-05:** choosing voice now installs both the ComfyUI
voice designer (`moss_tts`) and the isolated speaker (`moss_speech`). The speaker
pins Transformers 5.0.0 and torch 2.9.1+cu128; its installer can adopt existing
weights with `--adopt-checkpoints`. Historical host speech measurements below
do not describe the new speaker. Do not repair speech by changing ComfyUI.

Three doors, in that order, and each one is where its step actually
happens:

| Step | Door | What it decides |
|---|---|---|
| what you make, what card, where ComfyUI | `forge init` (MCP: `init_project`) | `[make]` and `[hardware]` in `forge.toml` — everything after reads them |
| what that costs and what it asks of you | `forge setup` (MCP: `licences`, then `setup`) | the one screen, the acceptance, the installers |
| what this machine can actually do | `just doctor` (MCP: `doctor`) | five words per backend, and the exit code |

Every generator lives in its own environment under a `$PREFIX` outside the
tree (`$FORGE_BACKENDS_HOME`, default `~/.cache/asset-forge/backends/<name>`)
and `backends/<name>/` holds only gitignored links to it — except the ones
the ComfyUI host runs, which have no environment of their own at all.
`just doctor` is where every diagnosis starts and ends; nothing in this
skill guesses at what the table can say.

## Prerequisites (check, don't assume)

- Linux. An NVIDIA card **only if you want the real generators**: the tier
  question below has `fake` as a first-class answer, and everything except
  a real generate works without one.
- Rust via rustup (`rust-toolchain.toml` pins 1.96.1 and rustup fetches
  it), `just`, and Bevy's headers — Debian/Ubuntu:
  `sudo apt install libasound2-dev libudev-dev pkg-config`. Every recipe
  builds `./target/debug/forge` first (minutes the first time — it links
  Bevy), and `.mcp.json` launches that binary: on a fresh clone, run any
  recipe once before the MCP server can start.
- A system `python3` ≥ 3.11 (the launcher is stdlib-only and never imports
  torch; an older interpreter is refused with exit 6 naming the version
  and the path — put a newer python3 first on PATH); `conda` for
  `trellis2` only.
- Blender ≥ 4.2 on PATH or `$BLENDER_BIN`, and `ffmpeg` — host tools, not
  backends; doctor lists them and nothing installs them. Blender is only
  needed if you chose `props` or `characters`.
- Disk: **do not quote a total until you know what they make.** `forge
  setup` prints the bill for their answer, and `--dry-run` prints it
  without touching anything. For reference, per backend: `trellis2` ~20 GB
  and `ardy` ~35 GB and `skintokens` ~3 GB (environments and clones —
  estimates), `acestep` 10.03 GB, `moss_sfx` 10.46 GB, `moss_tts` 10.56 GB
  (voice design + tokenizer), `moss_speech` 12.33 GB (speech + tokenizer),
  the `comfy` host ~2 GB for its venv and clone. The
  weights figures are each backend's own `[[models]] gb`; use the current
  setup screen instead of adding historical estimates by hand.

## Steps

### 1. Answer the three questions — `forge init`

The door is `forge init` in the project directory (an agent's door is the
MCP `init_project`). It asks once, on a terminal, and writes the answers to
`forge.toml`:

1. **What will you make here?** `props`, `characters`, `clips`, `sfx`,
   `music`, `voice` — default `props,characters,clips`.
2. **What card is this?** Detected from `nvidia-smi --query-gpu=memory.total`
   — ≥ 22 GB is `full`, ≥ 14 GB is `lean`, none is `fake` — and **offered,
   not assumed**: the detected one is the default and you can override it.
3. **Where is ComfyUI?** Asked only when a chosen kind runs in the comfy
   executor. Another machine's URL is fine.

The same three as flags, which is what to use in a script or under an
agent:

```
forge init --make props,characters,clips --tier full --comfy-url http://127.0.0.1:8188 --yes
forge init --make all          # every kind
forge init --make none         # nothing yet; every doctor row reads off
```

**With no terminal and no flags it takes the defaults and prints one line
naming each assumption.** It never hangs on a prompt — that is the trap
`hf auth login` taught this repo, and it is why every branch of the
question code returns an answer.

What the answers change downstream:

- `[make]` decides which backends `forge setup` installs and which doctor
  rows are `off`.
- `[hardware] tier` changes **registers and variants, never features**:
  both `lean` and `full` lift at 1024³. That lift measured 4.7 GB; reducing
  it to 512³ loses facial detail. Current speech uses the same pinned
  `moss_speech` model on either tier; voice design stays on `moss_tts`.
  `fake` sets `FORGE_FAKE=1` as a first-class answer: every `forge gen`
  writes a branded placeholder through the same doors and validators.

To re-answer for a project that already exists, edit `[make]`/`[hardware]`
by hand, or call `init_project` with `adopt: true` — which rewrites only
those two tables and leaves every other line, comments included, alone.

### 2. Read the screen, then accept by name — `forge setup`

The door is `forge setup [kind…]` (an agent's doors are `licences`, then
`setup` with `accept`). **It prints one screen before a byte downloads**:
per chosen kind the backends, what each costs on disk, the total, and every
licence fact those carry — each in the words a human is asked to accept,
not a summary of them.

```
forge setup --dry-run          # the screen, and nothing else happens
forge setup                    # the screen, then one question, on a terminal
forge setup --yes nvdiffrast --yes dinov3      # accept by name; repeatable
forge setup sfx voice          # only these kinds, whatever [make] says
forge setup --no-models        # make the envs now; weights on first generate
```

**A bare `--yes` is refused.** The ids are what is being agreed to, and a
blanket yes to a list nobody read is exactly what this gate exists to
prevent. The five ids:

| id | what | backend | needs your yes |
|---|---|---|---|
| `nvdiffrast` | NVIDIA Source Code License (1-Way Commercial) — **non-commercial only** | `trellis2` | **yes** |
| `dinov3` | DINOv3 License (Meta), gated behind a token only a human holds | `trellis2` | **yes** |
| `llama3` | Llama 3 Community License — "Built with Meta Llama 3" | `ardy` | **yes** |
| `skintokens_encoder` | the Michelangelo encoder question (upstream issue #9) | `skintokens` | no — a warning |
| `comfyui_gpl` | GPL-3.0-or-later, driven over HTTP from a separate process | `comfy` | no — a fact |

**Say what a `--yes` accepts before passing it.** It is the user's licence
decision, not yours. nvdiffrast is the one that matters: TRELLIS.2's
texture bake runs through it, every lift record carries
`texture_baker: "nvdiffrast (NVIDIA Source Code License, non-commercial)"`,
and a commercial project cannot ship a lifted texture until a replacement
baker exists. Declining leaves an env that does everything but bake:
doctor says `partial`, `forge gen mesh` exits 6.

What was accepted is appended to `$FORGE_BACKENDS_HOME/licences.json` —
beside the installs, because the install is what is licensed, and never in
`forge.toml`, which is hand-edited and would let an acceptance be *typed*
rather than *given*. The receipt records who accepted (`human`, or
`agent:claude` through the current MCP implementation), when, and at
which door. That MCP actor string is fixed in the server; it does not detect
the client identity.

**The DINOv3 login is a human's job and nothing gets past it.** Two
commands, in this order:

```
# 1. accept on the model page, with the account the token belongs to:
#    https://huggingface.co/facebook/dinov3-vitl16-pretrain-lvd1689m
# 2. then, and NEVER the interactive form — there is no TTY under an agent:
hf auth login --token <tok>
```

`$HF_TOKEN` exported is the zero-file alternative. The token is the user's;
ask for it, do not hunt for it.

**Resumable.** `forge setup` asks doctor first and skips every backend it
already calls `ok` with one line saying so — a stronger question than "does
the receipt match the pin", because a receipt says an env was *made* and
`ok` says the weights are there too. Re-run it as often as you like.

### 2b. One backend at a time — `just setup <backend>`

The backend-shaped door under `forge setup`, for installing or adopting
one: `just setup trellis2 --yes`. `just setup` with no name runs every
`backends/*/install.sh` in turn after printing a disk bill and refusing
without `--yes`. Installers are idempotent (`set -euo pipefail`, a finished
env is a no-op) and every trap they encode is dated in
`designs/hosting.md`. Flags:

| Flag | Does |
|---|---|
| `--prefix DIR` | where the env and clone go (default `$FORGE_BACKENDS_HOME/<name>`) |
| `--adopt-env DIR` | link an existing interpreter prefix instead of making one |
| `--adopt-checkout DIR` | link an existing upstream clone instead of cloning |
| `--adopt-text-encoders DIR` | `ardy`: link an assembled text-encoder directory |
| `--adopt-checkpoints DIR` | `acestep` or `moss_speech`: adopt a checkpoints directory |
| `--no-models` | skip the weight downloads; doctor says `partial` until the first run |
| `--yes` | accept the installer's own licence prompts without a TTY; the text prints either way |
| `--no-service` | `comfy`: skip the systemd `--user` unit |

**A machine that already has the envs** (one that ran the previous
toolkit, or your own): do not rebuild, link. Adopting writes the
`.env`/`.checkout` links and `installed.json` (`"adopted": true`), runs the
probe, installs nothing:

```
just setup trellis2 --adopt-env ~/anaconda3/envs/trellis2 --adopt-checkout ~/src/TRELLIS.2
just setup ardy     --adopt-env ~/src/ardy/.venv --adopt-checkout ~/src/ardy --adopt-text-encoders ~/src/text-encoders
just setup skintokens --adopt-env ~/src/SkinTokens/.venv --adopt-checkout ~/src/SkinTokens
```

An adopted env without nvdiffrast is warned about, not refused. For one
shell and no links at all, `FORGE_BACKEND_<NAME>_PYTHON=<prefix or python>`
names the interpreter directly and wins over the link.

### 3. Read the table — `just doctor`

The door is `just doctor` (`--json` for a machine, `--quick` to skip the
in-env probes; the MCP tool is `doctor`). **Exit 0 when every *chosen*
backend is `ok`.** Five words per row:

| Status | An `env` backend | A `comfy` backend | Do |
|---|---|---|---|
| `ok` | toml parses, checkout at its pin, the env python is the declared version, the probe imports everything and sees CUDA, every weight is on disk | the service answers at `comfy_url`, is at its pinned commit, lists every node class the description and its workflows name, every pack clone is at its pin, every weight is on disk | nothing |
| `partial` | the env runs but a `FAIL` line names a weight, an import or CUDA that is missing | it answers and the packs are right, but a node class or a weight is absent — named, with its GB | the `hint:` under it is the exact command |
| `missing` | no `.env` link: not installed | nothing is listening | `forge setup`, or `just setup <name>` |
| `broken` | present but unusable: bad toml, no probe, the probe fails, the checkout gone | it answers as **another commit** than pinned, a pack is off its pin, or a tracked workflow names a class that does not exist | the check on the row says why; for a comfy row, `systemctl --user status forge-comfy.service` |
| `off` | `[make]` did not choose the kind | the same | **nothing.** It was not probed and it is not a defect |

An `off` row is printed with the line that turned it off — `off — [make]
music = false` — and **never votes on the exit code**. A project at tier
`fake` chooses nothing, so every row reads `off` and doctor exits 0. Each
row also names its executor: `[env]`, `[comfy]` or `[tool]`.

Lines under a row:

- `warn checkout: <sha>; dirty (N tracked files modified)` — the clone is
  not at the pinned commit, or has edits. A **warning, not an error**.
- `warn notice: …` — a licence fact, printed every time on purpose. It is
  not a defect to fix; it is a decision that was made and now travels.
- `FAIL model:<id>: gated and absent: …` followed by two hints — the
  DINOv3 login from step 2. Do exactly those two, in that order.
- `hint: the env's torch cannot see the GPU: driver, CUDA build of torch,
  or another process holding the card` — `nvidia-smi` first, then `just gpu`.
- `warn env:<KEY>: the shell's <KEY>=… shadows backend.toml's …` — an
  ambient shell variable is overriding a plain `[env]` value. A warning;
  the values a run cannot work without are in `[env.force]` and cannot be
  shadowed.
- `gpu … warn: GPU busy: pid N <name> X GB` — somebody holds more than
  2 GB. Believe it.

A command that never reached the GPU says the same thing as doctor, in
about 100 ms:

```
forge-gen: missing_backend: ardy is not installed — generation through it is off
forge-gen: hint: bash backends/ardy/install.sh  (or --adopt-env <prefix> --adopt-checkout <clone>)
```

Exit 3.

### 4. The GPU — `just gpu`

```
gpu       NVIDIA GeForce RTX 4090  3413 / 24564 MiB in use, 21151 MiB free
holding   pid 140003 2.5 GB  <process>
largest   ardy needs 17 GB (17408 MiB): does NOT fit — stop what holds the card before a generate
```

Exit 1 when the largest chosen backend would not fit in what is free,
naming who holds the rest. One card, a desktop resident (~0.8 GB), and
these do not share it. **Peaks measured 2026-08-30, `nvidia-smi` at 10 Hz**
— the rows marked *budget* are estimates nobody has sampled, and a
`backend.toml`'s `vram_gb` is always a budget:

| Backend | VRAM | Resident after the call? |
|---|---|---|
| `ardy` sweep | **15.4 GB measured** | no |
| `trellis2` at 1024³ | **4.7 GB measured** | no |
| `skintokens` skin-only | **3.3–4.4 GB measured** | no |
| retired Comfy speech (1.7B), historical | **7.1 GB measured**, on top of the designer's 5.4 GB if one just ran | **yes, 7.3 GB** |
| `moss_speech` isolated speech | **14 GB budget**, no measured peak recorded here | no |
| `moss_tts` voice design | **5.3 GB measured** | **yes, 5.4 GB** |
| `moss_sfx` | **10.0 GB measured** | **yes, 9.1 GB** |
| `acestep` | **13.1 GB measured** | **no** — the card comes back by itself |
| the `comfy` unit, idle | ~0.4 GB of CUDA context | **yes**, until the unit stops |
| studio viewer on the real adapter | small; not measured | while open |

**Giving the card back.** `forge gpu --free` is the door. It calls
`POST /free`, which returns the card for **native** models (ACE-Step needs
even that only rarely) and does **nothing** for what TTS-Audio-Suite
loaded: the pack registers no unload node at this pin, and 9.1 GB stayed on
the card after an effect. The lever that works there is `systemctl --user
restart forge-comfy` — 4.4 s, measured 2026-08-30 — which the daemon's own
release ladder does for you, and if even that does not free the card the
lease is **withheld** until something proves it is free. `forge gpu --free`
says "the card is back" only when free VRAM reaches the card's idle floor,
never merely because it matches what the call started with. Otherwise:
close the studio. Never two generates at once; never one while a studio
window with a model loaded is up on the real adapter; never 1536³ on
24 GB.

## The traps (the why is in `designs/hosting.md`, dated; read it before fighting one)

- **Python 3.11, not 3.10** — a 3.10 conda build crashed in torch's
  hipify regex. `forge_gen` requires ≥ 3.11 for the same reason.
- **`PYTHONNOUSERSITE=1` everywhere** — a `~/.local` full of old `.pth`
  hooks is imported by any same-versioned interpreter; the launcher sets
  it for every inner process.
- **One env per backend** — the pins contradict (`transformers` 4.57 for
  the lift, 5.8 for motion; `numpy<2` for motion only). Sharing one is how
  an upgrade breaks a neighbour.
- **`trellis2`:** pip → `typing-extensions` → `torch==2.6.0+cu124` in that
  order; the CUDA toolkit from `nvidia/label/cuda-12.4.1` with
  `--override-channels` (the bare metapackage floats to 13.x); gcc 13 in
  the env exported as `CC/CXX/CUDAHOSTCXX` at **runtime** too, because
  nvdiffrast JIT-compiles on first use; flash-attn needs
  `--no-build-isolation` with `psutil` preinstalled, else
  `ATTN_BACKEND=sdpa` (slower, correct); FlexGEMM via `bdist_wheel`;
  o-voxel cloned `--recursive`; `transformers==4.57.6`; no WebP on export.
- **`ardy`:** `transformers==5.8.1` exactly, `numpy<2`; the text encoder is
  hand-assembled (Llama-3-8B-Instruct from an ungated mirror, the LLM2Vec
  MNTP adapter merged, the supervised adapter's base path rewritten) —
  `assemble_text_encoder.py` does all three; ~31 GB.
- **Audio uses two execution paths.** `acestep`, `moss_sfx` and voice design
  (`moss_tts`) run in ComfyUI. The MOSS pack has no unload node at this pin;
  `forge gpu --free` uses the managed restart when `/free` cannot release it.
  Spoken lines use `moss_speech`, an isolated interpreter pinned to
  Transformers 5.0.0 and torch 2.9.1+cu128. This restored speech on
  2026-09-05 without changing the shared host. Its installer can adopt existing
  model directories with `--adopt-checkpoints`. The retired Comfy speech graph
  remains diagnostic evidence; do not restore it by shimming or downgrading
  the host. See `forge-audio` for sampling, reference and end-token checks.
- **`comfy`:** a systemd `--user` unit on `127.0.0.1:8188`, started with
  `--base-directory` (without it the service writes into the clone and
  finds no models), `--disable-api-nodes` (no node can call a paid API) and
  `--enable-manager`. `snapshot.json` beside `backend.toml` is the
  Manager's own answer — re-fetch it, never hand-edit it. A pack clone off
  its pin, or a workflow naming a class the service does not list, is
  `broken`, not `partial`: no download fixes either.
- **Blender:** only rig, export and prop-normalize need it; `just ci`
  does not. `*.blend1` is a backup, not a source. glTF export is not
  byte-stable across versions — bodies and models claim integrity only.

## What is never installed

- **`briaai/RMBG-2.0`** — gated and commercially restrictive, for a job the
  pipeline does not need: references are flat-background by contract, the
  launcher keys alpha itself and stubs the `BiRefNet` class before the
  pipeline is built. It fails loudly on a border that is not flat; that is
  the reference's defect.
- **nvdiffrec `renderutils`** — NVIDIA, non-commercial, texturing-only;
  nothing here needs it. Do not add it.

## Seen → consequence → fix

| Seen | Consequence | Fix |
|---|---|---|
| `missing_backend: <name> is not installed`, exit 3 | no GPU work attempted | `forge setup`, `just setup <name>`, or the `--adopt-*` line in the hint |
| `refused: nothing was installed. N licence(s) here need accepting by name` | `setup` did nothing at all | read them (`forge setup --dry-run`, or the MCP `licences`), then `--yes <id>` / `accept: [<id>]` for each |
| a bare `--yes` refused | intentional | name the ids: `--yes nvdiffrast --yes dinov3` |
| a row reads `off` | the kind was not chosen; it was never probed | nothing, unless they meant to choose it — then `[make]` in `forge.toml`, or `init_project` with `adopt: true` |
| doctor exits 0 with rows that are not `ok` | those rows are `off` | correct. `off` never votes |
| a comfy row reads `missing` | the service is not listening | `systemctl --user start forge-comfy`, then `systemctl --user status forge-comfy` |
| a comfy row reads `broken` naming a commit | the running clone is not the pinned one | `git -C <clone> checkout <pin>`, or update `backend.toml`'s commit knowingly |
| `<link> has no bin/python — the install did not finish, or the environment moved` | `broken` | re-run the installer, or `--adopt-env` the env's new home |
| `<name> runs from its upstream checkout, and … is not there` | `broken` | `--adopt-checkout`, or re-run the installer |
| `this needs consent: re-run with --yes after reading the text above (no TTY to ask)` | the nvdiffrast or Llama 3 prompt with no TTY | tell the user what it accepts; `--yes` only when they say so |
| `FAIL model:… gated and absent` | a lift cannot start | the two hints: accept on the page, `hf auth login --token <tok>` |
| `the env's torch cannot see the GPU` | driver, torch build or the card is held | `nvidia-smi`, then `just gpu` |
| `warn checkout: … is not the pinned …` | the clone moved; doctor warns, generation runs | fine for a knowing user; the record carries the commit it ran at |
| `GPU busy: pid N …` / `does NOT fit` | the next generate OOMs, not queues | stop the holder (`systemctl --user stop forge-comfy`, close the studio); wait for the other generate |
| `hint: set FORGE_BACKENDS or run from a toolkit checkout` | run from a project that names no backends dir | `forge.toml [backends] dir`, or `FORGE_BACKENDS=<checkout>/backends` |
| doctor `partial` after `--no-models` | intended | the first generate downloads; or re-run the installer without it |

## Commit set

Nothing under `backends/<name>/` that the installer wrote: `.env`,
`.checkout`, `.text-encoders`, `.checkpoints`, `installed.json` are
gitignored and machine-local, as is `backends.local.toml`. What *is*
committed: a new trap, under its backend in `designs/hosting.md`, dated the
day it cost something, before the installer learns it; an installer fix
beside it. Commit only when the user asks.

## Known limits (say them, don't fight them)

- Verified on one machine: Linux, one RTX 4090, no system CUDA root,
  system gcc 14. Another machine's numbers differ; the order of operations
  should not.
- nvdiffrast is non-commercial and the lift does not work without it; the
  replacement baker is a follow-up, not a flag.
- Weights download from Hugging Face at whatever speed the link has;
  `--no-models` defers, it does not avoid.
- `just doctor` describes this machine; it is not part of `just ci`, and a
  CI runner is not this machine.
- The disk figures are read out of `backends/README.md` and
  `backends/comfy/backend.toml`; the VRAM figures marked *measured* are
  `nvidia-smi` at 10 Hz on one 24 GB card on 2026-08-30. **No `vram_gb` in
  any `backend.toml` is a measurement** — it is a budget `just gpu` sizes
  the card against, kept conservative. Never re-quote one as a fact.
- No number here was taken on a real 16 GB part. The `lean` column was
  measured under a cap on a 24 GB card, and a real 16 GB part has roughly
  15.0–15.5 GB usable once its own context and a desktop are resident;
  `ardy` at 15.4 GB is marginal on one.
