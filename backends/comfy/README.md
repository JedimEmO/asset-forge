# comfy — the ComfyUI host

Not a generator. ComfyUI is a **host**: a service that loads models and runs
graphs, driven over HTTP by the `comfy` executor behind `forge serve`. Nothing here is execed by the launcher, nothing of ComfyUI is
imported into the toolkit, and its GPL-3.0 stays on its own side of a
socket. What lives in this directory is the description, the installer, the
probe, the unit file, the paths config and the Manager snapshot — the same
five things every other backend directory holds, minus an interpreter the
toolkit enters.

Installed by Phase 0 prep on 2026-08-30 so the reference-image spike
(Qwen-Image vs FLUX.1-schnell under pose conditioning) had somewhere to run.
That spike is over and its models have left: **the reference image is
brought, not generated** (`designs/decisions.md`, "The reference image stays
brought", 2026-08-30), and what the host runs today is the three audio
backends: `acestep` for music, `moss_sfx` for effects and `moss_tts` for
voice design. Spoken lines use the isolated `moss_speech` interpreter; they
do not run inside this host.

## What is where

```
$PREFIX = ${FORGE_BACKENDS_HOME:-~/.cache/asset-forge/backends}/comfy

  $PREFIX/ComfyUI/       the clone at 169fcf35 (v0.34.2), plus the one file
                         it is allowed to carry: extra_model_paths.yaml
  $PREFIX/venv/          python 3.12, torch 2.13.0+cu130, comfyui_manager 4.2.2
  $PREFIX/data/          --base-directory: models/ input/ output/ user/ custom_nodes/
  $PREFIX/snapshot.json  what the Manager says is installed, as of the last install

backends/comfy/
  backend.toml           the pin, the licence, [server] and the one node pack.
                         No [[models]]: the host owns no weights, and each
                         backend it runs names its own with
                         `store = "comfy:models/<dir>"`, resolved under
                         $PREFIX/<base_directory>/ — one model list, the same
                         one doctor, install.sh and probe.py read
  install.sh             venv, clone, Manager, pack, unit, paths — idempotent,
                         and it downloads no weights at all
  probe.py               GET /system_stats + /object_info; doctor's one JSON line
  forge-comfy.service    the systemd --user unit, %h-relative
  extra_model_paths.yaml copied into the clone by install.sh
  snapshot.json          the committed Manager snapshot
  .env      ->           $PREFIX/venv     (gitignored)
  .checkout ->           $PREFIX/ComfyUI  (gitignored)
```

## Running it

```sh
bash backends/comfy/install.sh --yes          # no weights: see below
systemctl --user status forge-comfy
curl -s http://127.0.0.1:8188/system_stats | head
python3 python/forge_gen doctor --backend comfy --no-host
```

`--no-service` installs everything but the unit (start it by hand from the
clone). `--no-models` is accepted and does nothing — there is nothing here to
skip. `--models` and `--no-flux-controlnet` are gone with the group they
selected, and the installer says so by name rather than failing as an unknown
flag.

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
  is to turn the cache off. Any future cache must record reuse explicitly
  rather than presenting a cached output as a fresh generation.
* `--disable-api-nodes` — disable ComfyUI's API-node integration. This is
  not a network sandbox; model downloads and custom-node code can still
  use the network.
* `--listen 127.0.0.1` — loopback only. The daemon is the sole client.
* `--enable-manager` — ComfyUI-Manager is a pip package now
  (`comfyui_manager`, pinned by the clone's own `manager_requirements.txt`),
  not a `custom_nodes` clone. This flag is what turns it on, and it is what
  makes `GET /v2/snapshot/get_current` answer.

## The models: none of them are the host's

**This installer downloads nothing.** A host is a place for models, not an
owner of any, and every weight on this card's tree is named by the backend
that runs it: `backends/acestep` brings
`checkpoints/ace_step_1.5_turbo_aio.safetensors` (Comfy-Org/ace_step_1.5_
ComfyUI_files, 10.03 GB, Apache-2.0) through its own installer, and the three
MOSS models come down on the TTS-Audio-Suite node's first run into
`$PREFIX/data/models/TTS/`. Each of those rows carries the same
`comfy:models/<dir>` store, so doctor reads every weight on this card's tree
out of one list.

Until 2026-08-30 the host also fetched ~60 GB of image weights for the
reference spike — Qwen-Image fp8 as three split files plus its Q4_K_M GGUF,
FLUX.1-schnell's checkpoint, `clip_l`, `t5xxl_fp8` and `ae`, the InstantX
Qwen ControlNet-Union and the Shakker-Labs FLUX.1-dev ControlNet-Union-Pro-2.0
(non-commercial). They went with the reference door. **Nothing deletes them**
— a weight is the user's, and an installer that removes files it did not just
write is one nobody can re-run safely — so the installer closes by naming
what is now unread:

```
$PREFIX/data/models/{diffusion_models,controlnet}
$PREFIX/data/models/{text_encoders,vae,checkpoints}   (the FLUX/Qwen files in them)
$PREFIX/data/custom_nodes/ComfyUI-GGUF
```

`designs/hosting.md` § ComfyUI keeps the measurements and the licence
reasoning as the record of what was weighed.

**One custom node pack is installed, and only one:**
`diodiogod/TTS-Audio-Suite` @ `fab00263` (MIT), which registers the six node
classes the three audio graphs use. ACE-Step 1.5 is native to ComfyUI
v0.34.2 and needs no pack. `city96/ComfyUI-GGUF` was the second and existed
for one node, `UnetLoaderGGUF`, which read the lean tier's Q4 image model;
nothing left on this host reads a `.gguf`. A pack is written down in four
places that must agree: `backend.toml`'s `[[comfy.packs]]`, `install.sh`,
`snapshot.json` (the Manager's own answer, never written by hand) and
`hosting.md`'s pins row. `probe.py` holds each clone to its pinned commit the
way it holds ComfyUI's, and checks that the classes the pack claims are
actually registered.

## What doctor says

`comfy` is described with `env_kind = "none"`, so doctor takes the tool
path: it runs `probe.py` under the system python (stdlib only) rather than
entering an environment, because the thing to check is a service, not an
interpreter. The probe asks `/system_stats` and `/object_info` and reports
`ok` when the service answers, the clone is at the pinned commit, the unit
carries the flags above, the pack is at its own pin and every node class it
claims to register is in `/object_info`; `partial` when it answers but
something is absent or adrift; `missing` when nothing is listening.

The wanted-node list is **derived from the packs the description names**, not
written out. Until the image models left it was the eight loaders the
reference templates used; the host asserts nothing about ComfyUI's native
surface now — a guest backend checks the classes its own graphs name — and
what is left for the host to hold is its own claim: a pack that cloned but
did not register, which is the one failure a commit check cannot see.

The host states no `[[models]]`, so its model check has nothing to read; the
loop stays because it is the general one, and it goes through
`/object_info`'s loader enums rather than `os.stat`: a weight ComfyUI cannot
see in its folders is a weight that is not installed, whatever is on disk.
When a file *is* on disk and still not listed, the probe says so — that is a
paths problem, not a download one.

`$FORGE_COMFY_URL` points the probe at another machine's ComfyUI.
Projects name their service in `forge.toml` under `[hardware] comfy_url`.

## The card

The service is up across calls, unlike every other backend, and holds about
0.4 GB of the 24 GB for its CUDA context with nothing loaded. A model it has
loaded stays resident until the graph's unload node or `POST /free` — and
TTS-Audio-Suite ships neither at this pin, so `systemctl --user restart
forge-comfy` (4.4 s, measured 2026-08-30) is the only lever that returns what
the MOSS models took. Use the managed release door before another generator:

```sh
forge gpu --free
just gpu
```

The release path restarts the managed service when the MOSS pack cannot unload.
Do not start a lift until the GPU check confirms the required budget fits.

## Lingering

`systemctl --user enable` is only half of "it starts itself": a `--user`
unit runs while the user has a session, so on a headless machine it never
comes up at boot and stops when the last shell logs out. `install.sh` warns
when lingering is off; the fix, which is the user's to make, is

```sh
loginctl enable-linger $USER
```

`designs/hosting.md` § ComfyUI has the pins and the exact download list.
