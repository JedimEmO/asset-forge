---
name: forge-setup
description: Get the backends healthy — read the doctor table, install or adopt one backend at a time under backends/<name>/, accept the licence prompts knowingly, log in for the gated weights, and free the GPU. Use when a command exits 3 saying a backend is missing, when `just doctor` is red, or on a fresh machine.
---

# Setup: `just doctor` → `just setup <backend>` → `just doctor` again

Every generator lives in its own environment under a `$PREFIX` outside the
tree (`~/.cache/asset-forge/backends/<name>` by default), and
`backends/<name>/` holds only gitignored links to it. `just doctor` is
where every diagnosis starts and where it ends; nothing in this skill
guesses at what the table can say.

## Prerequisites (check, don't assume)

- Linux, an NVIDIA card, `nvidia-smi` on PATH. 24 GB for the 1024³ lifts;
  16 GB is enough for clips and audio.
- Rust via rustup (`rust-toolchain.toml` pins 1.96.1 and rustup fetches
  it), `just`, and Bevy's headers — Debian/Ubuntu:
  `sudo apt install libasound2-dev libudev-dev pkg-config`. Every recipe
  builds `./target/debug/forge` first (minutes the first time — it links
  Bevy), and `.mcp.json` launches that binary: on a fresh clone, run any
  recipe once before the MCP server can start.
- A system `python3` ≥ 3.11 (the launcher is stdlib-only and never imports
  torch; an older interpreter is refused with exit 6 naming the version
  and the path — put a newer python3 first on PATH); `conda` for
  `trellis2` only (the other four are venvs).
- Blender ≥ 4.2 on PATH or `$BLENDER_BIN`, and `ffmpeg` — host tools, not
  backends; doctor lists them and nothing installs them.
- Disk, per backend, before you start (~80 GB and change for all five;
  `just setup` with no name prints this bill and refuses without `--yes`):

  | Backend | Env + clone | Weights | Where |
  |---|---|---|---|
  | `trellis2` | conda env (py 3.11, CUDA 12.4.1, gcc 13, torch cu124, flash-attn, nvdiffrast) + clone | `microsoft/TRELLIS.2-4B` + gated `facebook/dinov3-vitl16-pretrain-lvd1689m` | HF cache |
  | `ardy` | venv 3.12 + clone | `nvidia/ARDY-Core-RP-20FPS-Horizon40` in the HF cache; the Llama-3 + LLM2Vec text encoder assembled in-env — **~16 GB downloaded, ~31 GB written** | `.text-encoders` |
  | `acestep` | venv + patched clone | the minimal set, ~7.3 GB (`--all-models` adds ~38 GB nobody asks for) | `.checkpoints` |
  | `moss_sfx` | venv + the `moss_soundeffect_v2/` subdirectory of the MOSS-TTS clone | `OpenMOSS-Team/MOSS-SoundEffect-v2.0`, ~11 GB | HF cache |
  | `moss_tts` | venv + the MOSS-TTS clone (shared with `moss_sfx`) | `OpenMOSS-Team/MOSS-TTS-Local-Transformer-v1.5`, ~8 GB; `OpenMOSS-Team/MOSS-VoiceGenerator`, ~4 GB (the voice designer) | HF cache |

## Steps

### 1. Read the table — `just doctor`

Exit 0 only when every backend is `ok`; `--quick` skips the in-env probes
(seconds each) and says only `found / missing / broken` from the directory;
`--json` for a machine. The table on this machine, 2026-08-23, trimmed:

```
project   asset-forge at … (forge.toml)
rig       humanoid v1: 55 bones (27 driven by cskel27), 5 socket(s), rigs/humanoid — glb sha ok, blend sha ok, no drift
library   N clip, N body, N model, N sfx, N music, N voice; manifest current
gpu       NVIDIA GeForce RTX 4090  3437 / 24564 MiB in use
          warn: GPU busy: pid 140003 … 2.5 GB
blender   5.2.0 /snap/bin/blender  ok
ffmpeg    ffmpeg version 6.1.1 …
python3   3.12.7 …
backends  …/backends (forge.toml [backends] dir)
  trellis2   ok       torch 2.6.0+cu124 cu12.4, cuda yes, imports 9/9; attn_backend=flash_attn, nvcc=12.4, nvdiffrast=0.4.0
             warn notice: nvdiffrast is non-commercial: the texture bake runs through nvdiffrast 0.4.0 (NVIDIA Source Code License, 1-Way Commercial): non-commercial use only. …
             warn notice: briaai/RMBG-2.0 is never fetched: …
  ardy       ok       torch 2.13.0+cu130 cu13.0, cuda yes, imports 11/11; … load_model=True, text_encoders=ok
             warn notice: Llama 3: Built with Meta Llama 3 — the text encoder only; …
  acestep    ok       … checkpoints=…, lm=acestep-5Hz-lm-0.6B
             warn checkout: 82252c2418de; dirty (5 tracked files modified)
             warn notice: resident server: the ACE-Step API server stays on the GPU (~8 GB) after a track until `forge gen music --stop-server`; …
  moss_sfx   ok       torch 2.9.0+cu128 cu12.8, cuda yes, imports 3/3; …
  moss_tts   partial  torch 2.9.1+cu128 cu12.8, cuda yes, imports 3/3; …
             FAIL model:OpenMOSS-Team/MOSS-TTS-Local-Transformer-v1.5: absent: not in …/.cache/huggingface/hub
             hint: the first run downloads OpenMOSS-Team/MOSS-TTS-Local-Transformer-v1.5; or: bash backends/moss_tts/install.sh
  blender    ok       5.2.0 at /snap/bin/blender (build fbe6228777e7)
             warn notice: Blender: GPL-licensed tool; … glTF export is not byte-stable across Blender versions: bodies and models claim integrity (sha256), never regeneration.
toolkit   … (python/forge_gen)
doctor: moss_tts partial (exit 1)
```

How to read a backend row:

| Status | Means | Do |
|---|---|---|
| `ok` | toml parses, checkout at the pinned commit, env python is the declared version, the probe imports everything and sees CUDA, every weight is on disk | nothing |
| `partial` | the env runs but a `FAIL` line names a weight, an import or CUDA that is missing; the first generate through it would download for minutes (or fail) | the `hint:` under it is the exact command |
| `missing` | no `.env` link: not installed | `just setup <name>` (step 2) |
| `broken` | present but unusable: bad toml, no probe, the probe fails, the checkout gone | the check that says why is on the row; usually re-run the installer, or `--adopt-*` the thing that moved |

Lines under a row:

- `warn checkout: <sha>; dirty (N tracked files modified)` — the clone is
  not at the pinned commit, or has edits. A **warning, not an error**;
  ACE-Step's is expected to be dirty (the soundfile patch and a trimmed
  `pyproject`).
- `warn notice: …` — a licence fact, printed every time on purpose.
  **`nvdiffrast is non-commercial`** is the one that matters: TRELLIS.2's
  texture bake runs through nvdiffrast 0.4.0 under the NVIDIA Source Code
  License (1-Way Commercial), every lift record carries
  `texture_baker: "nvdiffrast (NVIDIA Source Code License,
  non-commercial)"`, and a commercial project
  cannot ship a lifted texture until a replacement baker exists. The line
  stays for as long as it is installed; it is not a defect to fix, it is a
  decision to make before lifting. `Built with Meta Llama 3` is ARDY's
  attribution; it ships in no asset.
- `FAIL model:<id>: gated and absent: …` followed by two hints — the DINOv3
  image conditioner, on a fresh `trellis2`:

  ```
  hint: accept the licence for facebook/dinov3-vitl16-pretrain-lvd1689m at https://huggingface.co/facebook/dinov3-vitl16-pretrain-lvd1689m
  hint: hf auth login --token <tok>   (no token at …/.cache/huggingface/token; never the interactive login — no TTY under an agent)
  ```

  Do exactly those two, in that order: accept on the model page with the
  account the token belongs to, then `hf auth login --token <tok>` —
  **never the interactive `hf auth login`**: under an agent's shell there
  is no TTY, the prompt hangs or exits with nothing stored, and the env
  builds fine only to fail on first load. `$HF_TOKEN` exported is the
  zero-file alternative. The token is the user's; ask for it, do not
  hunt for it.
- `hint: the env's torch cannot see the GPU: driver, CUDA build of torch,
  or another process holding the card` — `nvidia-smi` first; then `just gpu`.
- A `hint:` under a row that still reads `ok` is a hint, not a gate: the
  generate will run. Act on it when it recurs — it usually names a link or
  editable install that moved.
- `warn env:<KEY>: the shell's <KEY>=… shadows backend.toml's …` — an
  ambient shell variable is overriding one of the backend's plain `[env]`
  values for the inner process. A warning, not a failure; the values a run
  cannot work without (trellis2's `CC`/`CXX`/`CUDAHOSTCXX`/`CUDA_HOME`)
  are in `[env.force]` and cannot be shadowed. Unset the ambient variable
  if the run misbehaves — `designs/hosting.md` has the trap, dated.
- `gpu … warn: GPU busy: pid N <name> X GB` — somebody holds more than
  2 GB. Believe it.

A command that never reached the GPU says the same thing as doctor, in
about 100 ms:

```
forge-gen: missing_backend: ardy is not installed — generation through it is off
forge-gen: hint: bash backends/ardy/install.sh  (or --adopt-env <prefix> --adopt-checkout <clone>)
```

Exit 3. That hint is step 2.

### 2. Install or adopt — `just setup <backend> [flags]`

`just setup` with no name runs every `backends/*/install.sh` in turn with
the same flags — after printing the ~80 GB disk bill and, without `--yes`,
refusing with exit 2 (it names `--no-models` and the one-backend
alternative). One at a time is easier to read. Installers are idempotent
(`set -euo pipefail`, a finished env is a no-op) and every trap they encode
is dated in `designs/hosting.md`. Common flags:

| Flag | Does |
|---|---|
| `--prefix DIR` | where the env and clone go (default `$FORGE_BACKENDS_HOME` or `~/.cache/asset-forge/backends/<name>`) |
| `--adopt-env DIR` | link an existing interpreter prefix (a conda env or a venv) instead of making one |
| `--adopt-checkout DIR` | link an existing upstream clone instead of cloning |
| `--adopt-text-encoders DIR` | `ardy`: link an assembled text-encoder directory |
| `--adopt-checkpoints DIR` | `acestep`: link a checkpoints directory |
| `--no-models` | skip the weight downloads; doctor says `partial` until the first run fetches them |
| `--yes` | accept every licence prompt without a TTY — nvdiffrast (`trellis2`), Llama 3 (`ardy`). The text prints either way |
| `--all-models` | `acestep` only: the two XL DiTs, ~38 GB, that nothing here asks for |

**Fresh machine**, in the order that fails fastest and downloads least:

```
just setup ardy --yes
just setup moss_sfx
just setup moss_tts
just setup acestep
just setup trellis2 --yes
```

`trellis2` last: it is the conda env, the CUDA toolkit from the label
channel, gcc 13, torch cu124, four CUDA extensions and — after the licence
is printed — nvdiffrast. Without `--yes` and without a TTY the installer
stops at that prompt on purpose: `this needs consent: re-run with --yes
after reading the text above`. Declining leaves the env without a texture
bake: doctor says `partial`, `forge gen mesh` exits 6. **Say what `--yes`
accepts before passing it**; it is the user's licence decision, not yours.
Then the gated login from step 1.

**A machine that already has the envs** (one that ran the previous
toolkit, or your own): do not rebuild, link. Adopting writes the
`.env`/`.checkout` links and `installed.json` (`"adopted": true`), runs the
probe, installs nothing:

```
just setup trellis2 --adopt-env ~/anaconda3/envs/trellis2 --adopt-checkout ~/src/TRELLIS.2
just setup ardy     --adopt-env ~/src/ardy/.venv --adopt-checkout ~/src/ardy --adopt-text-encoders ~/src/text-encoders
just setup acestep  --adopt-env ~/src/ACE-Step-1.5/.venv --adopt-checkout ~/src/ACE-Step-1.5 --adopt-checkpoints ~/src/ACE-Step-1.5/checkpoints
just setup moss_tts --adopt-env ~/src/MOSS-TTS/.venv --adopt-checkout ~/src/MOSS-TTS
just setup moss_sfx --adopt-env ~/src/MOSS-TTS/moss_soundeffect_v2/.venv --adopt-checkout ~/src/MOSS-TTS/moss_soundeffect_v2
```

An adopted env without nvdiffrast is warned about, not refused
(`the adopted env has no nvdiffrast — the texture bake is unavailable`).
For one shell and no links at all, `FORGE_BACKEND_<NAME>_PYTHON=<prefix or
python>` names the interpreter directly and wins over the link.

### 3. Read the table again — `just doctor`

Every row `ok`, the nvdiffrast and Llama 3 notices still there (they do
not go away; they are not supposed to), exit 0. A `partial` that names a
weight with `the first run downloads …` is allowed to stay partial until
the first generate if the user would rather pay then.

### 4. The GPU — `just gpu`

```
gpu       NVIDIA GeForce RTX 4090  3413 / 24564 MiB in use, 21151 MiB free
holding   pid 140003 2.5 GB  <process>
largest   trellis2 needs 22 GB (22528 MiB): does NOT fit — stop what holds the card before a generate
```

Exit 1 when the largest backend would not fit in what is free, naming who
holds the rest. The free card reads `769 / 24564 MiB in use, 23795 MiB
free`, `holding   nobody`, `largest   trellis2 needs 22 GB (22528 MiB):
fits`, exit 0. One 24 GB card, a desktop resident (~0.8 GB), and these do
not share it:

| Backend | VRAM | Resident after the call? |
|---|---|---|
| `trellis2` at 1024³ | ~22 GB — alone | no |
| `trellis2` at 512³ | completes beside the desktop; peak not measured | no |
| `ardy` sweep | ~16 GB | no |
| `acestep` server | ~8–10 GB | **yes**, until `forge gen music --stop-server` |
| `moss_tts` (4B) | ~12 GB | no |
| `moss_sfx` | ~6–8 GB | no |
| studio viewer on the real adapter | small; not measured | while open |

`holding pid N 10.4 GB …/backends/acestep/.env/bin/python` is the usual
answer and `target/debug/forge gen music --stop-server` the usual fix.
Never two generates at once; never one while a studio window with a model
loaded is up on the real adapter; never 1536³ on 24 GB.

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
- **`acestep`:** torchaudio segfaults against the system glib, so the
  clone is patched to `soundfile` (the dirty checkout doctor notes); it is
  a server on `127.0.0.1:8001`, `/health` to probe, first start loads for
  minutes; `ACESTEP_CHECKPOINTS_DIR` or it downloads into its own tree.
- **`moss_sfx` / `moss_tts`:** one clone, two venvs (the effect model pins a
  different torch); `TORCHDYNAMO_DISABLE=1` for effects or the first call
  compiles the DiT for minutes and loses it with the process;
  `enable_cudnn_sdp(False)` for speech (a broken kernel); the 8B Delay
  model OOMs, the 4B is what runs.
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
| `missing_backend: <name> is not installed`, exit 3 | no GPU work attempted | `just setup <name>` or the `--adopt-*` line in the hint |
| `<link> has no bin/python — the install did not finish, or the environment moved` | `broken` | re-run the installer, or `--adopt-env` the env's new home |
| `<name> runs from its upstream checkout, and … is not there` | `broken` | `--adopt-checkout`, or re-run the installer |
| `this needs consent: re-run with --yes after reading the text above (no TTY to ask)` | the nvdiffrast or Llama 3 prompt with no TTY | tell the user what it accepts; `--yes` only when they say so |
| `FAIL model:… gated and absent` | a lift cannot start | the two hints: accept on the page, `hf auth login --token <tok>` |
| `the env's torch cannot see the GPU` | driver, torch build or the card is held | `nvidia-smi`, then `just gpu` |
| `warn checkout: … is not the pinned …` | the clone moved; doctor warns, generation runs | fine for a knowing user; the record carries the commit it ran at |
| `GPU busy: pid N …` / `does NOT fit` | the next generate OOMs, not queues | stop the holder (`--stop-server`, close the studio); wait for the other generate |
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
