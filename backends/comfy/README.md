# comfy — the ComfyUI host

Not a generator. ComfyUI is a **host**: a service that loads models and runs
graphs, driven over HTTP by the `comfy` executor `forge serve` grows in
Phase 1. Nothing here is execed by the launcher, nothing of ComfyUI is
imported into the toolkit, and its GPL-3.0 stays on its own side of a
socket. What lives in this directory is the description, the installer, the
probe, the unit file, the paths config and the Manager snapshot — the same
five things every other backend directory holds, minus an interpreter the
toolkit enters.

Installed by Phase 0 prep on 2026-08-30 so the reference-image spike
(Qwen-Image vs FLUX.1-schnell under pose conditioning) has somewhere to run.

## What is where

```
$PREFIX = ${FORGE_BACKENDS_HOME:-~/.cache/asset-forge/backends}/comfy

  $PREFIX/ComfyUI/       the clone at 169fcf35 (v0.34.2), plus the one file
                         it is allowed to carry: extra_model_paths.yaml
  $PREFIX/venv/          python 3.12, torch 2.13.0+cu130, comfyui_manager 4.2.2
  $PREFIX/data/          --base-directory: models/ input/ output/ user/ custom_nodes/
  $PREFIX/snapshot.json  what the Manager says is installed, as of the last install

backends/comfy/
  backend.toml           the pin, the licence, [server], and the models as
                         ordinary [[models]] with `store = "comfy:models/<dir>"`,
                         resolved under $PREFIX/<base_directory>/ — one model
                         list, the same one doctor, install.sh and probe.py read
  install.sh             venv, clone, Manager, unit, paths, weights — idempotent
  probe.py               GET /system_stats + /object_info; doctor's one JSON line
  forge-comfy.service    the systemd --user unit, %h-relative
  extra_model_paths.yaml copied into the clone by install.sh
  snapshot.json          the committed Manager snapshot
  .env      ->           $PREFIX/venv     (gitignored)
  .checkout ->           $PREFIX/ComfyUI  (gitignored)
```

## Running it

```sh
bash backends/comfy/install.sh --yes          # ~55 GB of weights; see below
systemctl --user status forge-comfy
curl -s http://127.0.0.1:8188/system_stats | head
python3 python/forge_gen doctor --backend comfy --no-host
```

`--no-service` installs everything but the unit (start it by hand from the
clone). `--no-models` skips the weights; doctor then says `partial`.
`--no-flux-controlnet` skips the one non-open-source download.

## Why the flags are the flags

The unit runs

```
python main.py --listen 127.0.0.1 --port 8188 --disable-auto-launch \
    --disable-api-nodes --base-directory $PREFIX/data --cache-none --enable-manager
```

* `--base-directory` — models, output, user data and custom nodes live
  beside the clone, never inside it. The clone is a checkout pinned at a
  commit; a checkout that accumulates state is one nobody can move.
* `--cache-none` — ComfyUI caches node outputs between runs, so a second
  prompt with the same inputs returns the first one's image without
  executing. A re-roll that never ran is exactly the failure `forge2.md`
  names ("Node-result caching returns a stale output"), and the honest fix
  at this stage is to turn the cache off. When Phase 1's job records carry
  `cached: true, same_as`, this can be revisited.
* `--disable-api-nodes` — no node in any graph can call a paid API or reach
  the internet.
* `--listen 127.0.0.1` — loopback only. The daemon is the sole client.
* `--enable-manager` — ComfyUI-Manager is a pip package now
  (`comfyui_manager`, pinned by the clone's own `manager_requirements.txt`),
  not a `custom_nodes` clone. This flag is what turns it on, and it is what
  makes `GET /v2/snapshot/get_current` answer.

## The models, and what each licence says

Fetched into `$PREFIX/data/models/<folder>/`. Sizes are the download.

| folder | file | from | GB | licence |
|---|---|---|---|---|
| `diffusion_models` | `qwen_image_fp8_e4m3fn.safetensors` | Comfy-Org/Qwen-Image_ComfyUI | 20.43 | Apache-2.0 |
| `text_encoders` | `qwen_2.5_vl_7b_fp8_scaled.safetensors` | Comfy-Org/Qwen-Image_ComfyUI | 9.38 | Apache-2.0 |
| `vae` | `qwen_image_vae.safetensors` | Comfy-Org/Qwen-Image_ComfyUI | 0.25 | Apache-2.0 |
| `controlnet` | `Qwen-Image-InstantX-ControlNet-Union.safetensors` | Comfy-Org/Qwen-Image-InstantX-ControlNets | 3.54 | Apache-2.0 |
| `checkpoints` | `flux1-schnell-fp8.safetensors` | Comfy-Org/flux1-schnell | 17.24 | Apache-2.0 |
| `text_encoders` | `clip_l.safetensors` | comfyanonymous/flux_text_encoders | 0.25 | Apache-2.0 |
| `text_encoders` | `t5xxl_fp8_e4m3fn.safetensors` | comfyanonymous/flux_text_encoders | 4.89 | Apache-2.0 |
| `vae` | `ae.safetensors` | black-forest-labs/FLUX.1-schnell | 0.34 | Apache-2.0, **gated "auto"** — needs an HF token |
| `controlnet` | `FLUX.1-dev-ControlNet-Union-Pro-2.0.safetensors` | Shakker-Labs/FLUX.1-dev-ControlNet-Union-Pro-2.0 | 4.28 | **FLUX.1-dev Non-Commercial** |

A backend that runs *on* this host brings its own row and its own installer:
`backends/acestep` adds `checkpoints/ace_step_1.5_turbo_aio.safetensors`
(Comfy-Org/ace_step_1.5_ComfyUI_files, 10.03 GB, Apache-2.0) through
`bash backends/acestep/install.sh`, which refuses until this host is
installed. Its `[[models]]` row carries the same `comfy:models/<dir>` store,
so doctor reads every weight on this card's tree out of one list.

Two facts the spike has to carry, not bury:

1. **The two sides are not symmetric on licence.** Qwen-Image's pose
   ControlNet (InstantX Union: canny, depth, pose, soft edge) is Apache-2.0
   and trained on the model it conditions. Every maintained pose ControlNet
   in the FLUX family is under the FLUX.1-dev Non-Commercial License and
   trained on FLUX.1-dev, not on the Apache-2.0 schnell it would be applied
   to. XLabs' Apache-friendly FLUX ControlNets are canny, depth and HED
   only — there is no pose one. So a FLUX.1-schnell reference door either
   ships without pose conditioning or ships under a non-commercial licence
   on an off-base adapter.
2. **The fp8 form differs per model.** ComfyUI documents Qwen-Image as
   three split files (diffusion model, text encoder, VAE) and
   FLUX.1-schnell's fp8 as one all-in-one checkpoint loaded with
   `CheckpointLoaderSimple`; the split FLUX diffusion model exists only in
   bf16 at 23.8 GB. The clip_l / t5xxl-fp8 / ae files are here anyway,
   because the lean tier's GGUF path will need them.

**One custom node pack is installed, and only one.** Both image models,
both ControlNets and every loader the fp8 templates need are native to
ComfyUI v0.34.2 — verified against `/object_info`: `UNETLoader`,
`CLIPLoader`, `VAELoader`, `ControlNetLoader`, `CheckpointLoaderSimple`,
`TextEncodeQwenImageEdit`, `ModelSamplingFlux`, `ControlNetApplyAdvanced`.
The exception is the lean tier's image model: a Q4_K_M GGUF, which no
native loader reads. So `city96/ComfyUI-GGUF` @ `6ea2651e` (Apache-2.0) is
cloned into `$PREFIX/data/custom_nodes/` with `gguf` and `protobuf` in the
venv, it contributes exactly one node this repo uses (`UnetLoaderGGUF`, in
`reference_qwen_gguf.api.json`), and it is written down in three places
that must agree: `backend.toml`'s `[[comfy.packs]]`, `install.sh`, and
`snapshot.json`, which is the Manager's own answer and not written by hand.
`probe.py` holds the clone to the pinned commit the way it holds ComfyUI's.

## What doctor says

`comfy` is described with `env_kind = "none"`, so doctor takes the tool
path: it runs `probe.py` under the system python (stdlib only) rather than
entering an environment, because the thing to check is a service, not an
interpreter. The probe asks `/system_stats` and `/object_info` and reports
`ok` when the service answers, the clone is at the pinned commit, the unit
carries the flags above, the node classes are there, the GGUF pack is at
its own pin and ComfyUI lists all ten model files; `partial` when it
answers but something is absent or adrift; `missing` when nothing is
listening.

The model check goes through `/object_info`'s loader enums, not `os.stat`:
a weight ComfyUI cannot see in its folders is a weight that is not
installed, whatever is on disk. When a file *is* on disk and still not
listed, the probe says so — that is a paths problem, not a download one.

`$FORGE_COMFY_URL` points the probe at another machine's ComfyUI (what
`forge.toml`'s `[hardware] comfy_url` will name in Phase 1).

## The card

The service is up across calls, unlike every other backend, and holds about
0.4 GB of the 24 GB for its CUDA context with nothing loaded. A model it has
loaded stays resident until the graph's unload node or `POST /free`. Stop
the unit before a 1024³ lift:

```sh
systemctl --user stop forge-comfy      # and `start` when the lift is done
```

## Lingering

`systemctl --user enable` is only half of "it starts itself": a `--user`
unit runs while the user has a session, so on a headless machine it never
comes up at boot and stops when the last shell logs out. `install.sh` warns
when lingering is off; the fix, which is the user's to make, is

```sh
loginctl enable-linger $USER
```

`designs/hosting.md` § ComfyUI has the pins and the exact download list.
