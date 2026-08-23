# Backends

**Read this first: one of the backends depends on non-commercial code.**
TRELLIS.2's texture bake calls **nvdiffrast 0.4.0**, which ships under the
NVIDIA Source Code License (1-Way Commercial) — *non-commercial use only*.
Nothing in the lift works without it. `backends/trellis2/install.sh` prints
the licence and refuses to fetch it without `--yes` or an interactive "y";
`forge doctor` warns for as long as it is installed; every lift record
carries `texture_baker: "nvdiffrast (NVIDIA Source Code License,
non-commercial)"` — quoted here exactly as `mesh.py` writes it — so the
fact travels with the asset. If your project is commercial, a lifted mesh's texture came
through software you are not licensed to use for it. A replacement baker is
a follow-up; until then, decide before you lift.

## What a backend directory is

A backend is one generator, hosted in its own environment, described by one
directory under `backends/`:

```
backends/<name>/
  backend.toml        what it is: upstream, pinned commit, licence, env kind, entry module,
                      the [env] the launcher exports, the [[models]] it needs, its [[notices]]
  install.sh          makes the env and the clone under $PREFIX, or adopts ones you have
  probe.py            run inside the env by doctor: imports, torch, CUDA, one JSON line
  patches/            (acestep) what the clone needs changed before it runs
  .env        ->      gitignored symlink to the interpreter prefix (what the launcher execs)
  .checkout   ->      gitignored symlink to the upstream clone at the pinned commit
  .text-encoders ->   (ardy) gitignored symlink to the assembled text encoders
  .checkpoints ->     (acestep) gitignored symlink to the checkpoint directory
  installed.json      gitignored receipt: commit, python, torch, date, adopted
```

The five the toolkit knows, in the order doctor lists them:

| backend | role | upstream | env | entry |
|---|---|---|---|---|
| `trellis2` | image → textured mesh | microsoft/TRELLIS.2 @ `75fbf018` | conda, python 3.11, CUDA 12.4 | `forge gen mesh` |
| `ardy` | prompt → motion take | nv-tlabs/ardy @ `693f74d1` | venv, python 3.12 | `forge gen motion sweep\|keys` |
| `acestep` | prompt → music | ACE-Step/ACE-Step-1.5 @ `82252c24` | venv, python 3.12 | `forge gen music` (a resident server) |
| `moss_sfx` | prompt → sound effect | OpenMOSS/MOSS-TTS @ `58b20a0d`, `moss_soundeffect_v2/` | venv, python 3.12 | `forge gen sfx` |
| `moss_tts` | text → speech; description → voice | OpenMOSS/MOSS-TTS @ `58b20a0d` | venv, python 3.12 | `forge gen speech`, `forge gen voice` |

`moss_sfx` and `moss_tts` share one clone and keep two venvs: the
sound-effect model pins a different torch. `moss_tts` hosts two models:
MOSS-TTS clones a line from a 5–15 s reference clip, and MOSS-VoiceGenerator
designs that clip from a description (`forge gen voice`) so a project never
has to bring a voice it does not own. Nothing heavy lives in this tree. Envs and clones go under `$PREFIX` — `${FORGE_BACKENDS_HOME:-~/.cache/
asset-forge/backends}/<name>` by default — and the directory here holds only
links to them. No absolute path is ever written into a tracked file.

Blender is a host tool, not a backend: `$BLENDER_BIN` or `blender` on PATH,
≥ 4.2, run `--background --factory-startup`. So is ffmpeg.

## How the launcher finds an interpreter

`forge gen <cmd>` is `python3 python/forge_gen <cmd> --json`. The outer
half runs under the system python (≥ 3.11, stdlib only, never imports
torch) and resolves the backend's interpreter in this order:

1. `$FORGE_BACKEND_<NAME>_PYTHON` — a python binary or an env prefix
   (`FORGE_BACKEND_TRELLIS2_PYTHON=~/anaconda3/envs/trellis2`);
2. `backends/<name>/.env/bin/python` — the symlink `install.sh` wrote;
3. otherwise **exit 3** (`missing_backend`) with the install line as the
   hint, in about 100 ms, before any GPU work.

It then execs `<python> -m forge_gen.<entry> --inner …` with this
checkout's `python/` on `PYTHONPATH`, from the upstream checkout when
`backend.toml` says `cwd = "checkout"`, with every `[env]` entry exported
(`${PREFIX}`, `${CHECKOUT}`, `${TEXT_ENCODERS}`, `${CHECKPOINTS}` expanded)
— as defaults, so a value you exported yourself wins — and
`PYTHONNOUSERSITE=1` everywhere, because a `~/.local` that has seen years of
experiments carries `.pth` hooks.

The exception is an `[env.force]` table: those entries are exported
**unconditionally**, shell or no shell. They are for values the backend
does not work without — trellis2 forces `CC`/`CXX`/`CUDAHOSTCXX`/
`CUDA_HOME`/`PYTHONNOUSERSITE` because nvdiffrast JIT-compiles at run time
and an anaconda-base `CC` left in a login shell once fed it a mixed CUDA
host toolchain. When the shell *does* shadow a plain `[env]` value,
`forge doctor` prints a warn row naming both values. The backends directory itself is
`$FORGE_BACKENDS` or `<checkout>/backends`; a project's `forge.toml` may
name another in `[backends] dir`.

## Licences

Verified from the files on disk, 2026-08-23. The toolkit's own code is MIT
OR Apache-2.0; none of the below is vendored, all of it is fetched by the
installers.

| component | licence | note |
|---|---|---|
| **nvdiffrast 0.4.0** | **NVIDIA Source Code License (1-Way Commercial) — NON-COMMERCIAL** | TRELLIS's texture bake (`o_voxel/postprocess.py` `to_glb`) uses it. Consent-gated install; doctor warns; the lift record names it. |
| nvdiffrec `renderutils` | NVIDIA, non-commercial | **Not installed.** Texturing-only; nothing here needs it. Do not add it. |
| TRELLIS.2 code | MIT | |
| `microsoft/TRELLIS.2-4B` weights | MIT | |
| `facebook/dinov3-vitl16-pretrain-lvd1689m` | DINOv3 License (Meta) — **gated** | Accept on the model page, then `hf auth login --token <tok>`. Doctor prints both. |
| `briaai/RMBG-2.0` | commercially restrictive | **Never downloaded.** Stubbed at runtime: references are flat-background by contract, and the launcher keys alpha itself. |
| CuMesh, FlexGEMM, utils3d | MIT | CuMesh `12289e10`, FlexGEMM `6dd94a85`, utils3d `9a4eb15e` — pinned in `trellis2/install.sh` to what the verified env runs. |
| flash-attn | BSD-3 | Optional; `ATTN_BACKEND=sdpa` fallback when absent. |
| ARDY code | Apache-2.0 | |
| `nvidia/ARDY-Core-RP-20FPS-Horizon40` weights | NVIDIA Open Model License | |
| Meta-Llama-3-8B-Instruct (ARDY's text encoder base) | Llama 3 Community License | Attribution required: **"Built with Meta Llama 3"**. The backend's notice carries it. |
| LLM2Vec | MIT | |
| ACE-Step 1.5 code + weights | MIT | |
| MOSS-TTS family | Apache-2.0 | |
| `OpenMOSS-Team/MOSS-VoiceGenerator` (1.7B, MossTTSDelay) | Apache-2.0 | the voice designer behind `forge gen voice`; the same env as MOSS-TTS |
| MOSS-SoundEffect-v2 | Apache-2.0 | |
| Blender | GPL | A tool; nothing of it ships in an asset. |

A licence fact in a record is not a detail. Keep `texture_baker` in every
lift record, and keep the Llama 3 notice in `ardy/backend.toml`.

## VRAM and co-residency

One 24 GB card with a desktop resident (~0.8 GB). Approximate peaks; two
rows never share the card. `just gpu` before any generate, and stop the
ACE-Step server before a lift. Doctor says who is holding the card
(`GPU busy: pid … 8.1 GB`); believe it.

| backend | VRAM | resident after the call? |
|---|---|---|
| `trellis2` at 1024³ | ~22 GB — **alone** | no |
| `trellis2` at 512³ | completes beside the desktop; peak not measured | no |
| `ardy` sweep | ~16 GB (one model load covers a batch) | no |
| `acestep` server | ~8 GB | **yes**, until `forge gen music --stop-server` |
| `moss_tts` (Local-Transformer 4B) | ~12 GB | no |
| `moss_tts` voice design (MOSS-VoiceGenerator 1.7B) | ~12 GB measured at the peak of a 7 s audition — the generation loop, not the weights | no |
| `moss_sfx` | ~6–8 GB | no |
| studio viewer on the real adapter | small; not measured | while open |

Never 1536³ on 24 GB. The 8B MOSS-TTS Delay model OOMs with the audio
tokenizer loaded; the 4B fits.

## Installing

Each installer is idempotent (`set -euo pipefail`, sources
`_lib/common.sh`), and every trap it encodes is dated in
`designs/hosting.md`. Order matters only for disk and time:

1. `bash backends/ardy/install.sh` — venv, `transformers==5.8.1`,
   `numpy<2`; assembles the Llama-3 + LLM2Vec text encoder under `$PREFIX`
   (the Llama 3 notice prints; `--yes` accepts it without a TTY).
2. `bash backends/moss_sfx/install.sh` and `bash backends/moss_tts/install.sh`
   — one clone, two venvs; weights download on first run (~11 GB / ~8 GB,
   plus ~4 GB for the voice designer).
3. `bash backends/acestep/install.sh` — venv, the soundfile patch,
   `ACESTEP_CHECKPOINTS_DIR` at the minimal ~7.5 GB model set.
4. `bash backends/trellis2/install.sh --yes` — conda (python 3.11, CUDA
   12.4.1 from the label channel, gcc 13), torch cu124, the CUDA extensions,
   and **nvdiffrast after the licence prompt**. DINOv3 is gated: accept on
   the model page and `hf auth login --token <tok>` first, or doctor will
   tell you to.

Then `forge doctor` (or `python3 python/forge_gen doctor`): every backend
should read `ok`; `partial` names the weight, import or CUDA that is
missing; `missing` is not installed; `broken` is present but unusable, with
the check that says why. Exit 1 while any is not ok.

Common flags (all installers): `--prefix DIR`, `--no-models` (skip weight
downloads; doctor says `partial`), `--yes` (every licence prompt).

## Adopting an install you already have

If an env and a clone exist — a machine that ran the previous toolkit, or
your own — do not rebuild them. Link them:

```sh
bash backends/trellis2/install.sh --adopt-env ~/anaconda3/envs/trellis2 \
    --adopt-checkout ~/src/TRELLIS.2
bash backends/ardy/install.sh --adopt-env ~/src/ardy/.venv --adopt-checkout ~/src/ardy \
    --adopt-text-encoders ~/src/text-encoders
bash backends/acestep/install.sh --adopt-env ~/src/ACE-Step-1.5/.venv \
    --adopt-checkout ~/src/ACE-Step-1.5 --adopt-checkpoints ~/src/ACE-Step-1.5/checkpoints
bash backends/moss_tts/install.sh --adopt-env ~/src/MOSS-TTS/.venv --adopt-checkout ~/src/MOSS-TTS
bash backends/moss_sfx/install.sh --adopt-env ~/src/MOSS-TTS/moss_soundeffect_v2/.venv \
    --adopt-checkout ~/src/MOSS-TTS/moss_soundeffect_v2
```

Adopting writes the `.env`/`.checkout` links and `installed.json`
(`"adopted": true`), runs the probe, and installs nothing. A checkout at
another commit than the pinned one is a doctor warning, not an error; a
dirty checkout is noted (ACE-Step's is expected to be: the soundfile patch
and a trimmed `pyproject`). `FORGE_BACKEND_<NAME>_PYTHON` is the
zero-install alternative for one shell: no links, no receipt, the
interpreter named directly.
