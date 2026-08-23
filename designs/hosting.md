# Hosting the backends

The install-trap log. Every backend under `backends/<name>/` has an
`install.sh` that encodes what is written here; this file is the *why*, so
the next rebuild of an env is twenty minutes and not an evening. Each
bullet is dated the day it cost something. A new trap goes under its
backend, dated, before the installer learns it.

Verified on one machine: Linux, one RTX 4090 (24 GB), no system CUDA root,
system gcc 14. Your numbers will differ; the order of operations should
not.

## Common

- **Python 3.11, not 3.10.** A conda 3.10 build crashed in `sre_compile`
  on torch's hipify trie regex and showed other interpreter flakiness;
  3.11's rewritten `re` engine is fine. `forge_gen` requires ≥ 3.11 for
  the same reason. 2026-08-18.
- **`PYTHONNOUSERSITE=1` everywhere.** A `~/.local/lib/pythonX.Y` that has
  seen a few years of ML experiments carries `.pth` startup hooks, and any
  same-versioned interpreter imports the lot. Every launcher sets it for
  the inner process too. 2026-08-18.
- **One env per backend.** The pins below contradict each other
  (`transformers` 4.57 for the lift, 5.8 for motion; `numpy<2` for motion
  only). Sharing an env is how one backend's upgrade breaks another's at
  three in the morning. 2026-08-18.
- **`hf auth login --token …`, never the interactive login.** Under an
  agent's shell there is no TTY; the interactive prompt hangs or exits
  silently with nothing stored. Gated weights then fail on first load,
  after the env built fine. 2026-08-18.
- **Resolve absence before the GPU.** A missing env exits 3 in about
  100 ms; the launcher never imports torch to find out. 2026-08-23.
- **The ambient shell can shadow a backend's `[env]` — and on this machine
  it did.** The launcher applies a plain `[env]` value with `setdefault`,
  so a variable already exported by the shell wins: an anaconda-base
  `CC`/`CXX` in the ambient environment shadowed trellis2's gcc-13 trio
  and fed nvdiffrast's JIT a mixed CUDA host toolchain. Load-bearing
  values now go in `[env.force]` in `backend.toml` (applied
  unconditionally; overlap with `[env]` is refused) — trellis2 forces
  `CC`, `CXX`, `CUDAHOSTCXX`, `CUDA_HOME`, `PYTHONNOUSERSITE` — and
  `forge doctor` prints a `warn env:<KEY> …` row for every ambient value
  that shadows a remaining plain `[env]` one. A backend variable the run
  cannot work without belongs in `[env.force]`, not `[env]`. 2026-08-23.

## TRELLIS.2 (`backends/trellis2`, conda, commit `75fbf018`)

- **Install order: pip, then `typing-extensions`, then
  `torch==2.6.0+cu124`.** The cu124 wheel index serves a
  `typing_extensions` wheel whose metadata name the stock pip
  mis-normalizes, and the sdist fallback cannot see `flit_core` because
  `--index-url` replaced PyPI. 2026-08-18.
- **CUDA toolkit via the label channel.** `conda install -c nvidia
  cuda-toolkit=12.4` installs the 12.4.1 *metapackage* and floats every
  component to 13.x. What held: remove the whole `cuda-*` tree and
  reinstall with `--override-channels -c nvidia/label/cuda-12.4.1
  cuda-toolkit`. `CUDA_HOME=$CONDA_PREFIX`. 2026-08-18.
- **gcc 13 in the env, exported at runtime.** CUDA 12.4's `host_config.h`
  refuses gcc > 13 and the system has 14. `gcc_linux-64=13 gxx_linux-64=13`
  from conda-forge, exported as `CC`, `CXX`, `CUDAHOSTCXX` — and not only
  at build time: nvdiffrast JIT-compiles its kernels on first use, so the
  launcher exports the same trio on every run. 2026-08-18.
- **flash-attn needs `--no-build-isolation` and `psutil` preinstalled**
  (its metadata imports torch). If it is absent the launcher falls back to
  `ATTN_BACKEND=sdpa`: slower, correct. 2026-08-18.
- **FlexGEMM via `python setup.py bdist_wheel`.** The `pip install` path
  dies in metadata generation (a str-subclass JSON key deep in the hooks);
  building the wheel and installing it works. Its CUDA lives in triton/JIT,
  so the wheel is cheap. 2026-08-18.
- **o-voxel needs `--recursive`.** It vendors eigen as a submodule; a
  shallow clone builds nothing and says so late. 2026-08-18.
- **`transformers==4.57.6`.** 5.x restructured `DINOv3ViTModel` and the
  pipeline indexes `model.layer` directly. 2026-08-18.
- **The image conditioner is gated.** `facebook/dinov3-vitl16-pretrain-lvd1689m`
  needs access granted on the model page and a token login (see Common).
  `forge doctor` prints the accept URL and the exact login line; a fresh
  clone cannot lift until both are done. 2026-08-18.
- **`briaai/RMBG-2.0` is never downloaded.** Gated and commercially
  licensed, for a job this pipeline does not need: references are
  flat-background by contract, and upstream's own preprocess skips
  background removal when the input carries real alpha. The launcher keys
  alpha itself — border-connected flood on the flat backdrop — and
  replaces the `BiRefNet` class with a stub before pipeline init. It fails
  loudly when the border is not flat; that is the reference's defect.
  2026-08-18.
- **nvdiffrast 0.4.0 is under the NVIDIA Source Code License —
  non-commercial.** It is the texture baker, and nothing in the lift works
  without it. The installer requires `--yes` or interactive consent with
  the licence printed, `forge doctor` warns while it is installed, and
  every lift record carries `texture_baker: "nvdiffrast (NVIDIA Source
  Code License, non-commercial)"` — the exact string `mesh.py` writes.
  A replacement baker is a follow-up. 2026-08-23.
- **No `extension_webp` on export.** Upstream demos embed WebP textures;
  `bevy_gltf` does not decode them. A plain `.export()` on the trimesh
  embeds PNG. 2026-08-18.
- **`decimation_target` counts vertices, not faces.** ~900 vertices is
  ~1 800 triangles on a closed surface; the registers in `profile.toml`
  are stated in vertices for this reason. 2026-08-18.
- **Never 1536³ on 24 GB.** 512³ and 1024³ both complete with a desktop
  resident (~0.8 GB); the full-run peak at 1024³ was not pinned down.
  Measure one before being tempted. 2026-08-18.

## ARDY (`backends/ardy`, venv 3.12, commit `693f74d`)

- **`transformers==5.8.1` exactly, `numpy<2`.** Neither floats; the text
  encoder assembly below is written against that transformers. 2026-08.
- **`TEXT_ENCODERS_DIR` holds a hand-assembled encoder.** Upstream expects
  Llama-3-8B-Instruct (a mirror without the gated download — NousResearch
  — carries the same weights) with the LLM2Vec MNTP adapter *merged into a
  full model*, then the supervised PEFT adapter on top, whose
  `adapter_config.json` base path must be rewritten to where the merged
  model actually lives. `assemble_text_encoder.py` does all three under the
  backend prefix. Llama 3's Community License requires attribution; the
  backend's notice carries it. 2026-08.
- **~16 GB VRAM per sweep.** One model load covers a batch, so sixteen
  samples cost little more than four. 2026-08.

## ACE-Step (`backends/acestep`, venv, commit `82252c2`)

- **torchaudio/torchcodec collide with the system glib.** Loading audio
  through torchaudio segfaults; `patches/0001-audio_utils-soundfile.patch`
  routes `acestep/audio_utils.py` through `soundfile`, and the client
  renders WAV and transcodes with ffmpeg afterwards. 2026-08.
- **It is a server.** `acestep.api_server` on `127.0.0.1:8001`, `/health`
  to probe; first start loads models for minutes. It stays resident at
  ~8 GB until `forge gen music --stop-server`, and the co-residency matrix
  below is why that flag exists. 2026-08.
- **`ACESTEP_CHECKPOINTS_DIR`** points the server at the minimal model set
  (~7.5 GB); without it the server downloads into its own tree. 2026-08.

## MOSS-TTS and MOSS-SoundEffect (`backends/moss_tts`, `backends/moss_sfx`, commit `58b20a0`)

- **One clone, two venvs.** The sound-effect model lives in a subdirectory
  with its own requirements; sharing the venv pins the wrong torch for one
  of them. 2026-08.
- **`TORCHDYNAMO_DISABLE=1` for sound effects.** The first call otherwise
  `torch.compile`s the DiT for minutes, and the compiled kernel is lost
  with the process; for one-shot generation the compile never pays back.
  2026-08.
- **`torch.backends.cuda.enable_cudnn_sdp(False)`** for speech — a broken
  kernel, per the model card. 2026-08.
- **The Local-Transformer 4B fits 24 GB; the 8B Delay model OOMs** with
  the audio tokenizer loaded. `MOSS_TTS_MODEL` overrides if a low-VRAM path
  ever exists. 2026-08.
- **Never hand the processor a reference as a path.** `torchaudio.load`
  in this env goes through torchcodec, whose ffmpeg libraries do not load
  beside the system glib (`Could not load libtorchcodec`, exit 5, after the
  4B has loaded) — the same collision that made soundfile the writer. The
  speech inner half reads the clip with soundfile and tokenizes it through
  `processor.encode_audios_from_wav` (resampling is torchaudio's pure
  torch kernel), then passes the codes tensor as the reference. Found on
  the first real `just speech`, 2026-08-23.
- **MOSS-VoiceGenerator runs in the same env** (`forge gen voice`): its
  `generate` takes no `do_sample` — the Delay model samples when
  `audio_temperature > 0` — and its processor wants `normalize_inputs=True`.
  The first load after a download spent ~80 s before the GPU saw anything
  (a cold 4 GB safetensors); warm loads are ~20 s. The same seed gave the
  same bytes on two runs on this card. 2026-08-23.

## Blender (`BLENDER_BIN`, ≥ 4.2 headless; 5.2 is the reference)

- Only the rig, export and prop-normalize steps need it; nothing in `just
  ci` does. `$BLENDER_BIN` or `blender` on PATH, run `--background
  --factory-startup --python … --`. 2026-08-18.
- **`*.blend1` backups are gitignored.** Blender writes one beside every
  save; they are not sources. 2026-08-22.
- **glTF export is not byte-stable across Blender versions.** This is why
  bodies and models claim integrity and not regeneration, and why `forge
  rebake` skips them loudly. 2026-08-20.

## GPU co-residency

Approximate peaks on one 24 GB card with a desktop resident (~0.8 GB);
rows marked not measured are exactly that. Two rows do not share the
card; `just gpu` before any generate, and stop the ACE-Step server before
a lift.

| Backend | VRAM | Resident after the call? |
|---|---|---|
| TRELLIS.2 at 1024³ | ~22 GB | no |
| TRELLIS.2 at 512³ | completes beside the desktop; peak not measured | no |
| ARDY sweep | ~16 GB | no |
| ACE-Step server | ~8 GB | **yes**, until `--stop-server` |
| MOSS-TTS (4B) | ~12 GB | no |
| MOSS-SoundEffect | ~6–8 GB | no |
| studio viewer on the real adapter | small; not measured | while open |
