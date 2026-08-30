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

A backend is one generator — or, since Phase 0, one *host* a generator runs
inside — kept in its own environment and described by one directory under
`backends/`:

```
backends/<name>/
  backend.toml        what it is: upstream, pinned commit, licence, env kind, entry module,
                      the [env] the launcher exports, the [[models]] it needs, its [[notices]]
  install.sh          makes the env and the clone under $PREFIX, or adopts ones you have
  probe.py            run inside the env by doctor: imports, torch, CUDA, one JSON line
  patches/            (acestep, skintokens) what the clone needs changed before it runs
  workflows/          (comfy) API-format graphs the executor patches and posts
  snapshot.json       (comfy) what the Manager says is installed: commit, node packs, pips
  .env        ->      gitignored symlink to the interpreter prefix (what the launcher execs)
  .checkout   ->      gitignored symlink to the upstream clone at the pinned commit
  .text-encoders ->   (ardy) gitignored symlink to the assembled text encoders
  .checkpoints ->     (acestep) gitignored symlink to the checkpoint directory
  installed.json      gitignored receipt: commit, python, torch, date, adopted
```

The seven the toolkit knows, in the order doctor lists them — five
generators, then the two the Phase 0 spikes stood up:

| backend | role | upstream | env | entry |
|---|---|---|---|---|
| `trellis2` | image → textured mesh | microsoft/TRELLIS.2 @ `75fbf018` | conda, python 3.11, CUDA 12.4 | `forge gen mesh` |
| `ardy` | prompt → motion take | nv-tlabs/ardy @ `693f74d1` | venv, python 3.12 | `forge gen motion sweep\|keys` |
| `acestep` | prompt → music | ACE-Step 1.5, native to the pinned ComfyUI | **none of its own** — runs on the `comfy` host | `forge gen music` |
| `moss_sfx` | prompt → sound effect | MOSS-SoundEffect-v2.0 through TTS-Audio-Suite @ `fab00263` | **none of its own** — runs on the `comfy` host | `forge gen sfx` |
| `moss_tts` | text → speech; description → voice | MOSS-TTS-Local-Transformer (1.7B) and MOSS-VoiceGenerator through TTS-Audio-Suite @ `fab00263` | **none of its own** — runs on the `comfy` host | `forge gen speech`, `forge gen voice` |
| `comfy` | **host**, not a generator: the service the `comfy` executor will drive over HTTP | comfyanonymous/ComfyUI @ `169fcf35` (+ two node packs, `city96/ComfyUI-GGUF` @ `6ea2651e` and `diodiogod/TTS-Audio-Suite` @ `fab00263`) | venv, python 3.12, torch cu130, run as `forge-comfy.service`; doctor probes the service on `127.0.0.1:8188`, never the env | none — nothing execs a host; `backends/comfy/workflows/*.api.json` are what it is sent |
| `skintokens` | mesh + armature → skin weights | VAST-AI-Research/SkinTokens @ `273b691d` (two patches under `patches/`) | venv, python 3.11, CUDA 12.8 | `forge gen skin` — Phase 2; today `python/forge_gen/spike_skin.py` |

Doctor prints an eighth row, `blender`, between the two groups: it is
described by a `backend.toml` like the rest so its version and licence
notice have somewhere to live, but it is a host tool, not a backend (below).

**`executor` is the field that says who runs a backend**, and doctor renders
its row from it: `env` is the per-backend interpreter and checkout the
launcher execs; `comfy` is a workflow posted to the ComfyUI service at
`[hardware] comfy_url`, so the backend has no interpreter of its own and its
weights live under the host's model folders; `tool` is a host program
(Blender) or a service described so it has a row (`comfy` itself). A
`comfy` backend's five words are judged against the service — see
"What doctor's words mean" below.

**Which backends a project even has rows for** comes from `[make]` in its
`forge.toml`. A kind that was not chosen reads `off`: not probed, printed
with the line that turned it off, and never a reason to exit 1. The map is
one fact in one place — `props → trellis2, qwen_image`; `characters → +
skintokens`; `clips → ardy`; `sfx → moss_sfx`; `music → acestep`; `voice →
moss_tts`; anything `comfy` adds the `comfy` host, and a mesh kind adds
Blender.

`moss_sfx` and `moss_tts` share the ComfyUI host, not a clone: since the
three MOSS models moved onto TTS-Audio-Suite neither has an environment,
a torch pin or a checkout of its own, and their installers only check that
the host has the pack at its pin. `moss_tts` hosts two models:
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

Added 2026-08-30, with the two Phase 0 backends:

| component | licence | note |
|---|---|---|
| SkinTokens code + weights | MIT | |
| SkinTokens `src/model/michelangelo/` | **open question** | derived from NeuralCarver/Michelangelo, GPL-3.0 upstream, shipped by SkinTokens under MIT; the authors have not answered (issue #9). It runs in its own process, nothing of it ships inside an asset, and every rig record names the `skinner`. |
| ComfyUI | GPL-3.0-or-later | A service driven over HTTP from a separate process; nothing of it is linked into the toolkit, and what it writes is the project's own. |
| `city96/ComfyUI-GGUF` | Apache-2.0 | The one custom node pack: `UnetLoaderGGUF`, which is the only way to load the lean tier's Q4 image model. |
| Qwen-Image (+ `Comfy-Org` fp8 repack, `city96` Q4_K_M GGUF) | Apache-2.0 | |
| InstantX Qwen-Image ControlNet-Union | Apache-2.0 | The pose ControlNet the reference door will use; trained on the model it conditions. |
| FLUX.1-schnell + `flux_text_encoders` + its VAE | Apache-2.0 | The VAE repo is gated `auto`: an `hf auth login --token` is needed even so. |
| `Shakker-Labs/FLUX.1-dev-ControlNet-Union-Pro-2.0` | **FLUX.1-dev Non-Commercial License** | On disk only because the Phase 0 spike had to condition both candidates on a pose. Trained on FLUX.1-dev, applied off-base to schnell. The spike chose Qwen-Image, so **no shipped record may name it**; `install.sh` asks before fetching it and `--no-flux-controlnet` declines. |

A licence fact in a record is not a detail. Keep `texture_baker` in every
lift record, and keep the Llama 3 notice in `ardy/backend.toml`.

## VRAM and co-residency

One 24 GB card with a desktop resident (~0.8 GB). Approximate peaks; two
rows never share the card. `just gpu` before any generate, and give the
card back before a lift — the audio models are the host's now, and it is
the host that holds them. **`forge gpu --free` is the door; for the MOSS
pack only `systemctl --user restart forge-comfy` returns the card
(measured 2026-08-30, 4.4 s):** `POST /free` unloads native models —
ACE-Step gives its card back with no intervention at all — and does
nothing at all for what TTS-Audio-Suite loaded. There is no unload node
in the pack at this pin. Doctor says who is holding the card
(`GPU busy: pid … 8.1 GB`); believe it. The measured figures and how they
were sampled are in `designs/hosting.md` § GPU co-residency.

| backend | VRAM | resident after the call? |
|---|---|---|
| `trellis2` at 1024³ | **4.7 GB measured** (2026-08-30) — the ~22 GB this row carried for a week was a budget nobody had sampled | no |
| `trellis2` at 512³ | **3.1 GB measured** (2026-08-30) | no |
| `ardy` sweep | **15.4 GB measured** (2026-08-30; one model load covers a batch) | no |
| ACE-Step 1.5 in the host (`acestep`) | **13.1 GB measured** (2026-08-30, a 30 s track at 96 bpm); `vram_gb = 14` | **no** — 22.8 → 22.6 GB free with no intervention; native models honour ComfyUI's own manager |
| `moss_tts` speech (Local-Transformer 1.7B, in the host) | **7.1 GB measured** (2026-08-30) *with the designer's 5.4 GB already on the card*; `vram_gb = 13` is the pair | **yes, 7.3 GB** — `POST /free` does nothing for it; `systemctl --user restart forge-comfy` |
| `moss_tts` voice design (MOSS-VoiceGenerator 1.7B) | **5.3 GB measured** (2026-08-30, a 6 s audition) | **yes, 5.4 GB** — same lever |
| `moss_sfx` (in the host) | **10.0 GB measured** (2026-08-30, 3 s at 100 steps, over a 1.2 GB floor); `vram_gb = 11` | **yes, 9.1 GB** — same lever |
| `skintokens` skin-only | **3.3–4.4 GB measured** (2026-08-30) — not the 14 GB upstream and `backend.toml` claim | no |
| Qwen-Image fp8 + ControlNet at 1024², in `comfy` | **23.3 GB measured** (2026-08-30) — **alone** | no, `POST /free` returns it |
| Qwen-Image Q4_K_M GGUF + ControlNet, the lean form | **16.2 GB measured** (2026-08-30) — **alone** | no, `POST /free` returns it |
| the `comfy` unit idle, nothing loaded | ~0.4 GB, creeping to ~0.7 GB after several model swaps | **yes**, until the unit stops |
| studio viewer on the real adapter | small; not measured | while open |

The rows marked *measured* are `nvidia-smi` at 10 Hz on one 24 GB card;
the rows marked *budget* are estimates, and so is every `backend.toml`'s
`vram_gb` — `just gpu` sizes the card against those, which is why they stay
conservative. **Never re-quote a `vram_gb` as a measurement.** The
image model, not the lift, is the thing that wants the whole card.

Never 1536³ on 24 GB. The 8B MOSS-TTS Delay model OOMs with the audio
tokenizer loaded, which is why `speech.api.json` states the 1.7B.

**Three of these budgets were raised on 2026-08-30 because the first real
run measured past them** — `moss_sfx` 8 → 11, `acestep` 12 → 14,
`moss_tts` 12 → 13 (the designer and the cloner co-reside; the pack
unloads neither). A budget under its own peak is worse than no budget: it
is what `card_is_held` admits a job against, so it turns
blocked-rather-than-OOM into OOM.

## Installing

Each installer is idempotent (`set -euo pipefail`, sources
`_lib/common.sh`), and every trap it encodes is dated in
`designs/hosting.md`. Order matters only for disk and time:

1. `bash backends/ardy/install.sh` — venv, `transformers==5.8.1`,
   `numpy<2`; assembles the Llama-3 + LLM2Vec text encoder under `$PREFIX`
   (the Llama 3 notice prints; `--yes` accepts it without a TTY).
2. `bash backends/comfy/install.sh` — venv (python 3.12, torch cu130), the
   pinned ComfyUI clone, **two** node packs (`ComfyUI-GGUF` for the lean
   tier's Q4 reference, `TTS-Audio-Suite` for the three MOSS models), a
   systemd `--user` unit on `127.0.0.1:8188`, ~74 GB of image weights and
   ACE-Step's 10.03 GB checkpoint. It asks before fetching the FLUX pose
   ControlNet, which is **non-commercial**; `--no-flux-controlnet`
   declines, `--no-service` skips systemd. Every audio kind runs on this
   host, so it comes before them.
3. `bash backends/moss_sfx/install.sh`, `bash backends/moss_tts/install.sh`
   and `bash backends/acestep/install.sh` — none of which install anything.
   Each checks that the host is there, that its pack is at the pin this
   backend names and that the node classes the tracked graph needs are
   registered, and names the fix when one is not. The MOSS weights download
   on the node's first run into the **host's own base directory** —
   `$PREFIX/data/models/TTS/moss_soundeffect_v2/` (10.46 GB) and
   `.../moss_tts/` (5.72 + 3.95 + 6.61 GB: the 1.7B, the voice designer and
   the audio tokenizer) — measured there 2026-08-30, and not into the HF
   cache the other backends fill.
4. `bash backends/trellis2/install.sh --yes` — conda (python 3.11, CUDA
   12.4.1 from the label channel, gcc 13), torch cu124, the CUDA extensions,
   and **nvdiffrast after the licence prompt**. DINOv3 is gated: accept on
   the model page and `hf auth login --token <tok>` first, or doctor will
   tell you to.
5. `bash backends/skintokens/install.sh` — venv (python 3.11, torch
   cu128), the two patches under `patches/` applied to the clone, ~1.6 GB
   of weights under `$PREFIX/weights`.

### What doctor's words mean

Then `forge doctor` (or `python3 python/forge_gen doctor`): every **chosen**
backend should read `ok`.

| word | an `env` backend | a `comfy` backend |
|---|---|---|
| `ok` | the env runs, the probe imports, the weights are cached | the service answers at `comfy_url`, is at its pinned commit, lists every node class the description and its workflows name, every pack clone is at its pin, every weight is on disk |
| `partial` | the env runs but a weight, an import or CUDA is missing | it answers and the packs are right, but a node class or a weight is absent — named, with its GB |
| `missing` | not installed | nothing is listening (hint: `systemctl --user status forge-comfy.service`) |
| `broken` | present but unusable: bad toml, no probe, probe fails, checkout gone | it answers as another commit than pinned, a pack is off its pin, or a tracked workflow names a class that does not exist — none of which a download fixes |
| `off` | `[make]` did not choose the kind. Never probed, never a vote | the same |

**Exit 1 only while a *chosen* backend is not ok.** A project at tier `fake`
chooses nothing, so every row reads `off` and doctor exits 0.

`GET /object_info` is fetched **once per doctor run** and shared by every
comfy backend: it is the whole node surface, and six backends asking six
times on a cold host is six waits for one answer that cannot differ.

Common flags (all installers): `--prefix DIR`, `--no-models` (skip weight
downloads; doctor says `partial`), `--yes` (every licence prompt).

The kind-shaped door above these is **`forge setup [kind…]`**: it prints one
screen — per chosen kind the backends, their disk, the total, and every
licence fact in full — before a byte downloads, asks once, appends what you
accepted to `$FORGE_BACKENDS_HOME/licences.json` (beside the installs,
because the install is what is licensed), and skips every backend doctor
already calls `ok`, so it is safe to re-run. `--yes nvdiffrast --yes llama3`
accepts **by name** and is repeatable; a bare `--yes` is refused, because a
blanket yes to a list nobody read is what the gate exists to prevent. The
licence ids are `nvdiffrast`, `dinov3`, `llama3`, `skintokens_encoder` and
`comfyui_gpl`; the first three need your yes, the last two are facts you are
told.

## Adopting an install you already have

If an env and a clone exist — a machine that ran the previous toolkit, or
your own — do not rebuild them. Link them:

```sh
bash backends/trellis2/install.sh --adopt-env ~/anaconda3/envs/trellis2 \
    --adopt-checkout ~/src/TRELLIS.2
bash backends/ardy/install.sh --adopt-env ~/src/ardy/.venv --adopt-checkout ~/src/ardy \
    --adopt-text-encoders ~/src/text-encoders
bash backends/comfy/install.sh --adopt-env ~/src/ComfyUI/.venv \
    --adopt-checkout ~/src/ComfyUI
```

There is nothing to adopt for `acestep`, `moss_sfx` or `moss_tts`: they have
no env and no checkout of their own since the audio kinds moved onto the
host. Adopt the host, and their installers will find the pack there.

Adopting writes the `.env`/`.checkout` links and `installed.json`
(`"adopted": true`), runs the probe, and installs nothing. A checkout at
another commit than the pinned one is a doctor warning, not an error; a
dirty checkout is noted (ACE-Step's is expected to be: the soundfile patch
and a trimmed `pyproject`). `FORGE_BACKEND_<NAME>_PYTHON` is the
zero-install alternative for one shell: no links, no receipt, the
interpreter named directly.
