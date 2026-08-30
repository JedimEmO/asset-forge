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
  resident (~0.8 GB). The full-run peak at 1024³ was unpinned until
  2026-08-30, when a sampler finally ran over one: **4.7 GB**, not the
  ~22 GB this file and `backend.toml` had budgeted — see *Lean tier* below
  before quoting either number. 2026-08-18, measured 2026-08-30.

**A lift can die in the glb export with a JSON TypeError, and re-running
the same seed fixes it.** Seen 2026-08-28 lifting a character at the 1024
cascade: the pipeline finished (`Get 2085 clusters after fast clustering`,
`Done`), then `forge_gen.mesh` exited 5 with `TypeError: keys must be str,
int, float, bool or None, not str`, raised from `json.dumps` deep inside
trimesh 5.0.0's glTF exporter (`trimesh/exchange/gltf/__init__.py`, the
material/accessor dedup hash). Re-running the identical command with the
identical default seed 42 succeeded and produced a *different* mesh (1845
clusters, not 2085). Two things follow, and the second is the important
one: the immediate fix is to run it again, and the reason it works is that
TRELLIS.2 is not bit-reproducible for a fixed seed — CUDA nondeterminism
reaches all the way to the topology. That is exactly why a body's record
claims integrity (sha256 of what shipped) and never regeneration, and
anyone tempted to add a `reproduces` claim for bodies should read this
entry first. If it ever stops being transient, the suspect is a key in the
exporter's blob dict whose class is named `str` but is not `str`.

**Prop lifts fail intermittently, in three different ways, and the fix is
to run them again.** Seen 2026-08-29 across seven prop references on an
idle card: one `RuntimeError: super(): bad __class__ cell` raised inside
the prop path, three `exited -11` (SIGSEGV during model load), two silent
failures — and one that simply worked. Retried, they succeed. This is the
same flakiness the character lift shows (see the trimesh TypeError above):
TRELLIS.2 on this machine fails a minority of runs for reasons that do not
reproduce, and a lift script should therefore carry a retry loop of two or
three attempts rather than treat a first failure as a defect. Do not
conclude from a batch of failures that the preset is broken until a retry
has failed too — the first read of this on 2026-08-29 wrongly recorded the
prop path as systematically broken, on evidence that included a success.

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
- **This whole section is the retired path.** Since 2026-08-30 ACE-Step 1.5
  runs natively inside the ComfyUI host (§ ComfyUI): one tracked graph, one
  10.03 GB checkpoint under the host's `models/checkpoints`, no venv, no
  patch, no pid file and no `--stop-server`. The three traps above are kept
  because the venv is still on disk and still in git history until a real
  track has come out of the new path — nothing is deleted on a promise.
  2026-08-30.

## MOSS-TTS and MOSS-SoundEffect (`backends/moss_tts`, `backends/moss_sfx`, commit `58b20a0`)

**The retired path, kept while the venvs are.** Since 2026-08-30 all three
MOSS models reach the card through TTS-Audio-Suite inside the ComfyUI host
(§ ComfyUI). The entries below describe the venvs, which are still on disk
and still in git history until a real sound has come out of the new path.

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

- **The pack does not offer the checkpoint the venv ran.** TTS-Audio-Suite's
  MOSS-TTS variants at `fab00263` are 1.7B
  (`OpenMOSS-Team/MOSS-TTS-Local-Transformer`), the 8B Delay checkpoints and
  community fine-tunes of those; `MOSS-TTS-Local-Transformer-v1.5` is not
  among them, and the 8B Delay model is the one this file records OOM-ing on
  24 GB with the audio tokenizer loaded. `speech.api.json` therefore states
  1.7B, and **a line cloned after the move is not the same voice as one
  cloned before it at the same reference** (24 kHz where the old path was
  48 kHz stereo). The voice designer is unaffected: the pack's "Voice Design
  1.7B" is the same MOSS-VoiceGenerator weights. The speech reference now
  travels as an uploaded file (`POST /upload/image` takes any file,
  `LoadAudio` reads it, `CharacterVoicesNode.opt_audio_input` carries it to
  `UnifiedTTSTextNode.opt_narrator`) and never as a path — which is the
  structural answer to the 2026-08-23 torchcodec trap, made plausible by the
  host's venv carrying PyAV 18.1.0, and **still a hypothesis**: one real
  speech through the host must confirm it before `moss_tts`'s venv is
  deleted. 2026-08-30.

## Blender (`BLENDER_BIN`, ≥ 4.2 headless; 5.2 is the reference)

- Only the rig, export and prop-normalize steps need it; nothing in `just
  ci` does. `$BLENDER_BIN` or `blender` on PATH, run `--background
  --factory-startup --python … --`. 2026-08-18.
- **`*.blend1` backups are gitignored.** Blender writes one beside every
  save; they are not sources. 2026-08-22.
- **glTF export is not byte-stable across Blender versions.** This is why
  bodies and models claim integrity and not regeneration, and why `forge
  rebake` skips them loudly. 2026-08-20.

## ComfyUI (`backends/comfy`, host, systemd `--user`, commit `169fcf35`)

Installed 2026-08-30 for the Phase 0 reference-image spike. Not a
generator: a service on `127.0.0.1:8188` that the `comfy` executor will
drive over HTTP. `backends/comfy/README.md` says what is where and why the
flags are the flags.

**The pins.**

| what | pin |
|---|---|
| ComfyUI | `comfyanonymous/ComfyUI` @ `169fcf35a2fc163fec31338b816503ddac0d3fcf` (v0.34.2, 2026-08-27) |
| python | 3.12 venv under `$PREFIX/venv` |
| torch | `2.13.0` from PyPI — the linux wheel is `2.13.0+cu130` |
| ComfyUI-Manager | pip package `comfyui_manager==4.2.2`, pinned by the clone's own `manager_requirements.txt`, switched on with `--enable-manager`. Not a `custom_nodes` clone. |
| frontend | `comfyui-frontend-package==1.49.6` (from `requirements.txt`) |
| custom node packs | **two:** `city96/ComfyUI-GGUF` @ `6ea2651e7df66d7585f6ffee804b20e92fb38b8a` (Apache-2.0, `gguf==0.19.0` + `protobuf==7.36.0` in the venv) — it exists for one node, `UnetLoaderGGUF`, the only way to load the lean tier's Q4_K_M image model; and `diodiogod/TTS-Audio-Suite` @ `fab00263fbdcdaddd4c721d1b560e1a08b6025ea` (v5.8.7, MIT, 185 MB, **no pips**), which is how the three MOSS models reach the host. Both cloned into `$PREFIX/data/custom_nodes/`; every other loader the fp8 templates and ACE-Step 1.5 need is native to v0.34.2. `backends/comfy/snapshot.json` records both. |
| unit | `forge-comfy.service`, `Restart=on-failure`, `--listen 127.0.0.1 --port 8188 --disable-auto-launch --disable-api-nodes --base-directory $PREFIX/data --cache-none --enable-manager` |

**The exact download list.** Into `$PREFIX/data/models/<folder>/`; sizes are
the download, 73.7 GB in total.

| folder | file | repo | GB | licence |
|---|---|---|---|---|
| `diffusion_models` | `qwen_image_fp8_e4m3fn.safetensors` | `Comfy-Org/Qwen-Image_ComfyUI` @ `split_files/diffusion_models/` | 20.43 | Apache-2.0 |
| `text_encoders` | `qwen_2.5_vl_7b_fp8_scaled.safetensors` | `Comfy-Org/Qwen-Image_ComfyUI` @ `split_files/text_encoders/` | 9.38 | Apache-2.0 |
| `vae` | `qwen_image_vae.safetensors` | `Comfy-Org/Qwen-Image_ComfyUI` @ `split_files/vae/` | 0.25 | Apache-2.0 |
| `controlnet` | `Qwen-Image-InstantX-ControlNet-Union.safetensors` | `Comfy-Org/Qwen-Image-InstantX-ControlNets` @ `split_files/controlnet/` | 3.54 | Apache-2.0 |
| `checkpoints` | `flux1-schnell-fp8.safetensors` | `Comfy-Org/flux1-schnell` | 17.24 | Apache-2.0 |
| `text_encoders` | `clip_l.safetensors` | `comfyanonymous/flux_text_encoders` | 0.25 | Apache-2.0 |
| `text_encoders` | `t5xxl_fp8_e4m3fn.safetensors` | `comfyanonymous/flux_text_encoders` | 4.89 | Apache-2.0 |
| `vae` | `ae.safetensors` | `black-forest-labs/FLUX.1-schnell` | 0.34 | Apache-2.0, but the repo is gated `auto`: an `hf auth login --token` is required or it 401s |
| `diffusion_models` | `qwen-image-Q4_K_M.gguf` | `city96/Qwen-Image-gguf` @ `e77babc5` | 13.07 | Apache-2.0, and needs the ComfyUI-GGUF pack above — the lean tier's image model |
| `controlnet` | `FLUX.1-dev-ControlNet-Union-Pro-2.0.safetensors` | `Shakker-Labs/FLUX.1-dev-ControlNet-Union-Pro-2.0` (`diffusion_pytorch_model.safetensors`, renamed) | 4.28 | **FLUX.1-dev Non-Commercial License**, and trained on FLUX.1-dev rather than the schnell it conditions. `install.sh` asks before fetching it; `--no-flux-controlnet` declines. |

The fp8 form differs per model, and the difference is upstream's, not a
choice: ComfyUI documents Qwen-Image as three split files and
FLUX.1-schnell's fp8 as one all-in-one checkpoint loaded with
`CheckpointLoaderSimple`. There is no fp8 UNET-only schnell file — the
split diffusion model exists only in bf16 at 23.8 GB, which the budget had
no room for. The FLUX `clip_l` / `t5xxl_fp8_e4m3fn` / `ae` files are
fetched anyway, because the lean tier's GGUF path will need them.
2026-08-30.

**A pack the host carries and the tracked files deny is a lie with a
13 GB attachment.** ComfyUI-GGUF was cloned mid-spike, by hand, for the
lean-tier image model, and for an afternoon `snapshot.json` said
`git_custom_nodes: {}`, `backend.toml` said `packs = []`, `install.sh`
never fetched it and this table's pins row said "custom node packs: none"
— while `workflows/reference_qwen_gguf.api.json`, tracked, could not run
without it. A stranger following the installer would have got a template
that fails at `POST /prompt`. So a pack is written in four places at once
or it is not installed: `[[comfy.packs]]` (repo, commit, dir, pips, the
nodes it contributes), `install.sh` (cloned at its pin *before* the unit
starts — packs are scanned once at startup), `snapshot.json` (re-fetched
from `GET /v2/snapshot/get_current`, never hand-edited), and this row.
`probe.py` holds the clone to its pinned commit the way it holds ComfyUI's
own, and `UnetLoaderGGUF` is in its wanted-node list, so an absent pack is
a doctor line and not a runtime surprise. 2026-08-30.

**The reference spike: Qwen-Image wins, and the pose was never the close
part.** 2026-08-30, the card otherwise idle. Two API-format templates —
`backends/comfy/workflows/reference_qwen.api.json` and
`reference_flux.api.json`, stable node ids, five patched inputs (prompt,
negative prompt, pose image, seed, filename prefix) — driven by
`python/forge_gen/spike_reference.py` over `POST /upload/image`, `POST
/prompt`, `GET /history/{id}`, `GET /view`, `POST /free`, `GET
/system_stats`. One prompt for both models: the style-guide line ("flat
matte painted texture with the lighting painted in, posterized colour
steps") in front of one courier description, the format sentences the
reference door will carry, on `out/spike/tpose_pose.png` (the profile's own
rest pose as an OpenPose figure). Four seeds each: 7, 11, 42, 1234.

**Both sides ran with a ControlNet**, which is the only reason the
comparison is fair and also the reason it is not free: Qwen-Image on
InstantX ControlNet-Union (Apache-2.0, trained on the model it conditions),
FLUX.1-schnell on Shakker-Labs FLUX.1-dev-ControlNet-Union-Pro-2.0 (**FLUX.1-dev
Non-Commercial**, trained on dev and applied off-base to schnell). Both
through native `ControlNetLoader` → `ControlNetApplyAdvanced` with the VAE
connected, strength 0.85, 0.0–0.85 of the schedule; no `SetUnionControlNetType`
(the Qwen union infers, and Union Pro 2.0 dropped the mode embedding).

| measured | Qwen-Image fp8 | FLUX.1-schnell fp8 |
|---|---|---|
| seconds per 1024² image, mean of 4 | **113.6 s** (20 steps, cfg 2.5, euler/simple, shift 3.1) | **30.8 s** (4 steps, cfg 1.0 — schnell's own register; 35.0 s on the first seed, 29 s after) |
| peak on the card, `nvidia-smi` at 10 Hz | **23 902 MiB (23.3 GB)** | **23 522 MiB (23.0 GB)** |
| baseline before each run | 1 090–1 360 MiB | 1 280–1 525 MiB |
| weights it moves per image | 20.43 + 9.38 + 0.25 + 3.54 GB | 17.24 + 0.34 + 4.28 GB |
| strict T-pose, arms within 2° of horizontal | **4 of 4** (0.2°, 1.6°, 0.6°, 0.3°) | 0 of 4 (3.1°, 3.6°, 3.0°, 6.4°) |
| flat keyable background (`mesh.py`'s own keyer) | 4 of 4 pass, but a floor band in 2 and a contact shadow in 2 | 3 of 4 pass; seed 11 refused, border spread 45 > 24 |
| all five criteria (pose, single subject, flat ground, clean silhouette, ≥ 7 heads) | **2 of 4** (11, 42) | **0 of 4** |
| the style line obeyed | 4 of 4 flat and posterized | 0 of 4 — photoreal every seed |

**`POST /free` gave the card back both times**, so the unit-restart path
`forge2.md` names as the fallback was never needed: 22.43 → 22.22 GB free
across the Qwen run, 22.24 → 22.09 across the FLUX one. What does creep is
the resident floor — 1.09 GB before the first run, 1.53 GB after seven
model swaps — the CUDA context plus allocator residue, not a leaked model.
Believe `/system_stats` for *free*, and `nvidia-smi` for *peak*: ComfyUI
reports what torch has allocated now, which is nowhere near the peak of a
run.

**Both image models are alone-on-the-card jobs, exactly like a 1024³
lift.** 23.3 and 23.0 GB peak on a 24 GB card is the whole card; the four
weights of a Qwen image do not co-reside, so with `--cache-none` every
prompt re-encodes the text through the 9.38 GB Qwen2.5-VL encoder and
re-loads the 20.43 GB diffusion model, and that swap is most of the 113 s.
The number to weigh in Phase 1 is not "Qwen is four times slower than
schnell" but "the cache-off decision costs a model swap per image"; it is
still the right decision while a job record cannot say `cached: true`.

**The Qwen picture that shipped carries a floor the prompt forbade.** "no
floor, no shadow, no gradient" in the prompt, and Qwen drew a lit ground
plane in seeds 7 and 1234 and a soft contact shadow under the shoes in 11
and 42. On seed 42 — the one lifted — the shadow's core is more than the
keyer's 28-level tolerance from the backdrop, so it survives keying,
connects to the shoes, and TRELLIS.2 lifts it as **a slab under both feet**
(visible in `out/spike/views_courier_qwen.png`). The fit gate passes it
anyway (it measures arms, not plinths) and the dust filter cannot drop it
(it is connected, not an island). Phase 3's `generate_reference` needs the
pose image framed with air under the feet, or a shadow check on the drawn
PNG, or both.

**A garment the value of the backdrop is unkeyable.** FLUX seed 7 drew a
cream jacket on a 185-grey ground; `mesh.py`'s border flood reached through
the antialiased sleeve edges and punched 1 771- and 1 053-pixel holes out
of the torso and arms. The keyer is not at fault and a looser tolerance
would be worse. It is an argument for saying the backdrop value in the
prompt *against* the character ("light grey" beside a dark courier), and
for running the keyer on the drawn PNG before a GPU minute is spent on a
lift — which `import_reference` already promises to do and
`generate_reference` must do too.

**The lifts both passed, and that is the least interesting result.**
`forge gen mesh --preset character` on each winner, first attempt, no retry
needed (104.1 s Qwen, 83.0 s FLUX), then `prepare_spike.py`: Qwen reach
1.21 of wrist span, arm tips 22 mm under the wrists; FLUX reach 1.20, tips
35 mm under. Both inside the gate's 0.80–1.45 and 0.15 m. So the fit gate
does **not** separate these two models — a 3–6° arm droop is well inside
what it tolerates — and anyone choosing an image model on "which one passes
the gate" would have learned nothing. What separates them is the style
line, the hands and the background, and all three are read by eye off the
contact sheet.

**A pack whose requirements you must not install.** TTS-Audio-Suite asks
for `numpy>=1.26.4,<2.3.0`, which would downgrade the host's 2.5.2 under
every image template, plus transformers, keras, funasr, faiss and forty
more. None of it was installed and none of it was needed: the pack
registers each node behind its own `try`/`except`, so all 58 of its classes
came up on the first restart with torch 2.13.0, torchaudio 2.11.0 and numpy
2.5.2 exactly as they were. The Manager's config has
`allow_pip_install = False`, which is why dropping a clone into
`custom_nodes` cannot run the pack's own `install.py` behind your back — if
that ever flips, this install stops being free. The Manager writes this
pack's hash into `snapshot.json` with a trailing newline inside the JSON
string; that is its answer, it is committed as fetched, and tidying it by
hand is hand-editing a snapshot. The snapshot endpoint is
`GET /v2/snapshot/get_current`; `/api/manager/...` is 404 on this build.
2026-08-30.

**ComfyUI v0.34.2 cannot write a WAV.** `SaveAudio` saves FLAC and
`SaveAudioAdvanced` offers flac, mp3 and opus — read off the node schemas
on the running host, not from memory. Every audio record in this library
measures a PCM WAV, so every comfy audio template ends in `SaveAudio` and
the Python half transcodes the FLAC to 16-bit PCM before `measure_wav` or
`add_output` see a file. FLAC is lossless, so nothing is lost — but sfx,
speech and voice gain an **ffmpeg** dependency they did not have when they
wrote the WAV themselves with soundfile, and a missing ffmpeg is refused as
exit 6 before the card is leased. 2026-08-30.

**`diffusers` is the pip TTS-Audio-Suite does need, and the host has not
got it.** "No pips" is true for the pack's *registration* — all 58 classes
come up without one — and false the moment `MossSoundEffectV2EngineNode`
actually loads a model:
`engines/adapters/moss_soundeffect_v2_adapter.py` → `unified_model_interface`
→ `ModuleNotFoundError: No module named 'diffusers'`, on the host, at
`POST /prompt` time, after the graph has been accepted and the card leased.
Measured 2026-08-30 on this machine (`forge gen sfx`, prompt `a door`,
prompt id `d490a5e9`), with the venv confirmed to hold torch 2.13.0,
torchaudio 2.11.0, transformers 5.16.1, numpy 2.5.2, PyAV 18.1.0, gguf
0.19.0, protobuf 7.36.0 — and no `diffusers` at all. So the pack's four
places are right about the clone and wrong about `pips = []`, and this is
the first thing the real sfx run has to settle: which `diffusers` the
adapter wants, and whether installing it moves transformers or numpy under
the image templates, which is the whole reason the pack's own
`requirements.txt` was refused. Until that is measured, `moss_sfx` is
`partial` at best and its installer should say so rather than let a
stranger find out at `POST /prompt`. **Nothing has been installed into the
host on the strength of this entry.** 2026-08-30.

**The pack ships no unload node.** `designs/serve.md` names
`TTSAudioSuiteUnload`; no such class exists among the 58 at this pin. So
`unload_node` is `null` for `acestep`, `moss_sfx` and `moss_tts` alike, and
`POST /free` plus the unit restart are the only levers the card has — which
makes `forge_serve`'s ladder load-bearing rather than a safety net. Whether
`/free` returns the card after a wrapper pack has loaded its own models is
**unmeasured**, and is the first thing the real run should record.
2026-08-30.

**The sound-effect engine `torch.compile`s its DiT, and the unit does not
stop it.** `MossSoundEffectV2EngineNode`'s own tooltip: the DiT uses
`torch.compile` automatically, the first generation can spend several
minutes compiling, and compiled artifacts are cached and reused across
ComfyUI sessions. That is what `moss_sfx`'s `TORCHDYNAMO_DISABLE=1` existed
to prevent, and that `[env]` value died with the venv. The choice is the
host's now: `Environment=TORCHDYNAMO_DISABLE=1` in `forge-comfy.service`
keeps the old behaviour at the cost of every effect being slower forever,
or leave it on and record on the first real run how long the compile takes
and whether the inductor cache survives a restart. Either way it belongs in
the unit file, because it is now a property of the host. 2026-08-30.

## Onboarding, doctor and the licence gate

**`nvidia-smi` is the tier detector, and its absence is an answer.**
`forge init` reads `--query-gpu=memory.total --format=csv,noheader,nounits`
and takes the largest card: ≥ 22 GB is `full`, ≥ 14 GB is `lean`, anything
else — including no `nvidia-smi` on PATH, a driver that does not answer, and
a machine with no GPU — is `fake`. It is **offered, not assumed**: the
detected tier is the prompt's default and `--tier` overrides it, because the
card that is busy today is still the card this project runs on. 2026-08-30.

**A prompt with no TTY in front of it hangs, and that is the trap this repo
already knew.** `hf auth login` taught it (see Common, above); `forge init`
must not repeat it. With no terminal *and* no `--make`, init takes the
defaults and prints **one line naming each assumption** rather than waiting
on stdin — which is what makes it safe inside `ci-fake`, inside
`mcp-session`, and inside any agent's shell. Every branch of the question
code returns an answer; none of them can block. 2026-08-30.

**`GET /object_info` is fetched once per doctor run.** It is ComfyUI's whole
node surface — megabytes on a host with packs, and slow on a cold service —
and every backend the `comfy` executor hosts asks it the same question. Six
comfy backends probing independently would be six fetches for one answer
that cannot differ, so the view (`/system_stats` and `/object_info`) is
built once and shared. The base directory is read from the running service's
own `--base-directory` argv rather than guessed, because a service started
by hand without it writes into the clone and finds no models. 2026-08-30.

**The five words for a comfy backend, and which of them is `broken`.**
`ok` the service answers, is at the pinned commit, lists every node class
the backend's `[comfy] nodes` and its tracked workflows name, every pack
clone is at its pin and every weight is on disk. `partial` it answers and
the packs are right, but a class or a weight is absent — the weight is named
with its GB, because "partial" without the number is a shrug. `missing`
nothing is listening. `broken` it answers as *another* commit than pinned,
or a pack is off its pin, or a tracked workflow names a class the service
does not have: none of those is fixed by downloading anything, which is what
separates `broken` from `partial`. The hint on a service that is not
answering is `systemctl --user status forge-comfy.service`. 2026-08-30.

**`process-wrap ^9.0` is not resolvable from this machine's crates.io
index**, so rmcp's `transport-child-process` feature cannot be enabled here.
`mcp-session`'s stdio leg spawns the server itself and hands rmcp the
child's own pipes — `(ChildStdout, ChildStdin)` implements `IntoTransport`
under `transport-async-rw` — which is the same protocol over the same bytes
with one fewer dependency. If the index ever carries it, `TokioChildProcess`
is a drop-in. 2026-08-30.

## GPU co-residency

Approximate peaks on one 24 GB card with a desktop resident (~0.8 GB);
rows marked not measured are exactly that. Two rows do not share the
card; `just gpu` before any generate, and give the card back with `forge
gpu --free` before a lift.

| Backend | VRAM | Resident after the call? |
|---|---|---|
| TRELLIS.2 at 1024³ | **4.7 GB measured** (2026-08-30, `vex_runner`) — the ~22 GB this row carried for a week was a budget nobody had run a sampler over; see the lean-tier section | no |
| TRELLIS.2 at 512³ | **3.1 GB measured** (2026-08-30, the same reference) | no |
| ARDY sweep | **15.4 GB measured** (2026-08-30, one prompt, two samples) — the ~16 GB this row carried was right | no |
| ACE-Step 1.5 in the ComfyUI host | ~8 GB (budget) | held by the host until `POST /free`, `forge gpu --free` or the unit stops — there is no resident ACE-Step server any more |
| MOSS-TTS (4B) | ~12 GB | no |
| MOSS-SoundEffect | ~6–8 GB | no |
| Qwen-Image fp8 + InstantX ControlNet at 1024² | **23.3 GB measured** (2026-08-30) — **alone** | no, `POST /free` returns it |
| FLUX.1-schnell fp8 + Union-Pro ControlNet at 1024² | **23.0 GB measured** (2026-08-30) — **alone** | no, `POST /free` returns it |
| Qwen-Image **Q4_K_M GGUF** + InstantX ControlNet at 1024², `--reserve-vram 8` | **16.2 GB measured** (2026-08-30) — **alone**; the lean tier's form | no, `POST /free` returns it |
| SkinTokens skin-only | **3.3–4.4 GB measured** (2026-08-30) — not the 14 GB upstream and `backend.toml` claim | no |
| ComfyUI unit idle, nothing loaded | ~0.4 GB, creeping to ~0.7 GB after several model swaps | **yes**, until the unit stops |
| studio viewer on the real adapter | small; not measured | while open |

## SkinTokens (`backends/skintokens`, venv, commit `273b691d`)

Phase 0 prep, 2026-08-30. The pins that were settled while standing the
backend up; the traps that earned them are the spike's to write under this
heading.

- **Upstream** `https://github.com/VAST-AI-Research/SkinTokens` at
  `273b691d35989d71cd17ff2895fdc735097b92d1` (HEAD on 2026-08-30, "modify
  post-sampling strategy", authored 2026-05-12). MIT code, MIT weights.
- **Env** venv, python **3.11** (upstream asks ≥ 3.11), torch
  **2.7.0+cu128** with torchvision 0.22.0 and torchaudio 2.7.0 from
  `https://download.pytorch.org/whl/cu128`, then upstream's
  `requirements.txt` unpinned as written — resolved here to transformers
  5.16.1, diffusers 0.40.0, lightning 2.6.5, bpy 5.0.1, trimesh 5.0.0,
  open3d 0.19.0, fast-simplification 0.2.0, bottle 0.13.4, tornado,
  numpy 1.26.4.
- **flash-attn is not installed.** `patches/0001-sdpa-instead-of-flash-attn.patch`
  rewrites both hard-coded `attn_implementation="flash_attention_2"` sites
  (`src/model/tokenrig.py`, `src/server/spec.py`) to `"sdpa"` and gives the
  two bare `flash_attn` imports a `scaled_dot_product_attention` fallback in
  the same (B, L, H, D) layout that `src/model/skin_vae/attention_processor.py`
  already uses upstream.
- **`patches/0002-make_asset-sons-counted-once.patch`** is upstream issue #8,
  one line: `make_asset()` appended every child twice into `sons`.
- **Weights** `python download.py --model` run inside `$PREFIX/weights`
  (~1.6 GB): `experiments/skin_vae_2_10_32768/last.ckpt`,
  `experiments/articulation_xl_quantization_256_token_4/grpo_1400.ckpt`,
  and `models/Qwen3-0.6B` (config and tokenizer only). The two directory
  names are upstream's and are not ours to change. `.checkpoints` links
  the directory; `SKINTOKENS_WEIGHTS` and `SKINTOKENS_CHECKOUT` carry it
  into the env.
- **VRAM** `vram_gb = 14`, upstream's own "at least 14 GB". Not measured
  here — the spike measures it. Never beside TRELLIS.2, ARDY, MOSS or the
  ACE-Step server.
- **`demo.py` starts its own `bpy_server.py`, in its own process group, and
  cleans it up from an `atexit` hook** (`preexec_fn=os.setsid`, port 59876
  from `src/server/spec.py`). So a run that is killed or times out leaves
  that server alive, and the *next* run's `wait_for_bpy_server` pings it,
  finds it healthy and quietly talks to the orphan instead of starting one.
  `spike_skin.py` refuses to start while the port answers and says so again
  if one survives its own run; find it with `ss -lptn 'sport = :59876'` and
  kill it by PID, never `pkill -f`. 2026-08-30.
- **The run only works from the checkout.** `demo.py` launches
  `bpy_server.py` by bare name and its `--model_ckpt` default is
  `experiments/…` — both relative to the working directory, which is what
  `cwd = "checkout"` and the two weight symlinks in the clone are for.
  2026-08-30.

**`--use_skeleton` gives back our skeleton, relabelled and quantised, so
take the weights and leave the rig.** First run of the Phase 0 skin spike,
2026-08-30: `vex_runner`'s mesh with the profile's 55-bone armature as a
sibling and no vertex groups (`prepare_spike.py`), through `demo.py
--use_skeleton --use_transfer --use_postprocess`, 8 s on an idle card. What
came back: **55 joints, in the order they went in, with an identical parent
array**, every one of the 55 contract bones carrying weight, 0 of 24 119
vertices weightless, at most 4 influences — and **the names gone**
(`bone_0…bone_54`, since a skeleton token carries geometry and not a label)
and **every joint moved: 7.6 mm on average, 12.1 mm at worst**
(`LeftHandEnd`). The contract's `rest_tolerance_m` is 0.1 mm and every clip
in the library is baked against the frozen rest pose, so the returned
*skeleton* is unusable as a rig by construction — the displacement is what
the checkpoint's name says it is
(`articulation_xl_quantization_256_…`: joint positions are tokenised on a
256-level grid, and the error accumulates down a chain in world space). The
weights are the deliverable and they are good; the rig they came back on is
not. Anything downstream must re-attach the returned per-vertex joint
indices — which are skin-order indices, so the mapping is by position in
the joint list, not by name — onto the profile's own untouched armature.

**Standing the env up, the traps worth the words.** All 2026-08-30.

- **Without flash-attn the repository does not import at all**, and the
  try/except that looks like it handles that does not: `src/model/tokenrig.py`
  and `src/model/skin_vae_model.py` wrap `from flash_attn_interface import
  flash_attn_func` in a `try`, and the *except branch imports flash-attn
  again* (`from flash_attn.flash_attn_interface import …`). With no wheel,
  `import src.model.tokenrig` raises. Two other call sites —
  `src/model/skin_vae/attention_processor.py` and
  `.../autoencoders/miche_transformer_blocks.py` — already carry correct SDPA
  fallbacks upstream and are not patched. That is why `patches/0001` touches
  four files and why doctor prints `attention=sdpa, patch:sdpa=applied`.
- **`src` is a plain directory in the clone; nothing pip-installs it.**
  `cwd = "checkout"` covers the launcher's `python -m forge_gen.<entry>`, but
  doctor runs `probe.py` *by path*, so `sys.path[0]` is `backends/skintokens/`
  and `import src` fails. Hence `[env] SKINTOKENS_CHECKOUT = "${CHECKOUT}"`
  and a probe that puts it on `sys.path` itself; anything else that reaches
  into the clone needs the same.
- **The checkout is permanently dirty** — four tracked files carry the two
  patches, so doctor reads `273b691d3598; dirty (4 tracked files modified)`.
  Expected, exactly the way ACE-Step's soundfile patch is.
- **`bpy` from PyPI resolves to 5.0.1 on python 3.11** — an entire Blender
  inside the venv, unrelated to `$BLENDER_BIN` and the toolkit's Blender 5.2.
  It is the mesh loader and exporter SkinTokens talks to over HTTP through
  its own `bpy_server.py`, which is why `bottle` and `tornado` are in
  `requirements.txt`. Do not try to point it at the system Blender.
- **`miche_transformer_blocks.py` prints `use flash attention 2.` to stdout
  at import time.** Anything whose contract is "the last stdout line is JSON"
  — the probe, the launcher's `--json` — must write its object after every
  import and flush. The probe does.
- **transformers is unpinned (`>=4.57.0`) and resolved to 5.16.1, and it
  works.** Unlike TRELLIS.2, which is pinned to 4.57.6 because 5.x
  restructured DINOv3, SkinTokens imports and builds its Qwen3 config fine on
  5.16.1 here. Recorded as a fact of this env, not as a pin; 4.57.6 is the
  fallback if a real run ever disagrees.
- **`download.py` uses `hf_hub_download`'s `local_dir` mode**, so it leaves a
  `.cache/huggingface/` bookkeeping tree inside the weights directory.
  Harmless, and it is why the directory measures a little over the stated
  1.6 GB.

**The spike's second half: the weights are good, and by-order re-attachment
is the whole fix.** 2026-08-30, the card idle and held by nothing but the
ComfyUI unit's 0.4 GB context.

`out/prepare/vex_runner.glb` (`prepare_spike.py` off
`assets-src/blender/vex_runner.blend` — 55 contract bones as a sibling
armature, 0 vertex groups) → `spike_skin.py` → `spike_reattach.py`
(rename each returned `bone_i` to the name at index *i* of the skin that
went in, discard the returned skeleton whole, bind to `rigs/humanoid/rig.blend`'s
own armature) → **`forge gen export`** → **`forge rig check`**.

| measured | value |
|---|---|
| wall clock, whole `spike_skin.py` process | **27.5 s** (25.8 s on a second run); upstream's own sampling loop is 7.3 s of it, the rest is import and model load |
| **peak VRAM, the SkinTokens process** | **4474 MiB (4.4 GB)**, twice, sampled at 10 Hz — **not the 14 GB `backend.toml` and upstream claim**. Peak on the card was 5800 MiB with a 1200 MiB idle baseline. |
| `forge rig check` on the returned glb, unmodified | 6 passed, **56 failed** — 0 bones driven, 27 orphaned curves, every contract bone "missing" |
| `forge rig check` after by-order re-attachment | **10 passed, 0 failed**: 55 bones at contract depth, rest rotations match, **the walk drives 27 of 27 with 0 orphaned**, feet at y=0.000, stature 1.80 m |
| `forge gen export`'s rest-pose gate | passed — the profile's rest pose is exact, because it is the profile's own file and none of the returned skeleton survives |
| unweighted vertices | **0 of 24 119** (`unweighted_abort_fraction` is 0.20) |
| influences | max 4, against `[export] max_influences = 4` |

**Detached shells are where SkinTokens beats the bone-heat ladder, and the
picture says so.** `vex_runner` has twelve connected islands; the two big
ones (776 and 744 vertices, x +0.10..+0.40, y 1.36..1.72) are the left
spiked pauldron. SkinTokens binds each one **rigidly to a single bone**
(LeftArm 100 %, and the right-side shell RightArm 88 %). The shipped
bone-heat body blends the same shell across three (LeftArm 50 %,
LeftShoulder 36 %, LeftForeArm 14 %), and on `pistol_shoot` — where the
shoulders counter-rotate hardest — that shell shears: the spikes fan out
into the air and the head disappears inside the shoulder mass. The
SkinTokens body keeps the head, the visor and the mohawk readable through
the same four frames. Rigid-per-shell is what `rig.py`'s `shells_rigid`
rescue was written to force; SkinTokens does it natively, and it is the
first thing to look at on any plated body.

**There is no seed, and the skin is genuinely not reproducible.** Two runs
of the same input, same knobs, minutes apart: identical output vertex count,
identical 0 unweighted, identical skeleton displacement to five decimals
(the skeleton half converges under `num_beams = 10`), **different glb
hashes**, and per-bone weighted-vertex counts drifting by up to 807 vertices
(`RightShoulder`). Both re-attached bodies pass `forge rig check` 10/10 and
read the same on the strip, so the spread is under the eye's threshold here —
but a rig claims **integrity, never reproduction**, and `seed: null` in the
record means unknown, not zero.

**What the export gate does not catch, and now must.** `forge rig check` on
the raw SkinTokens output said `ok: rest rotations match the contract` while
every joint sat up to 12.1 mm from where the contract puts it: the check
compares rest *rotations*, and nothing compares rest joint *translations* in
the glb. `forge gen export` does compare positions, but only against a
`.blend`. A Phase 2 that skins in Rust and never passes through Blender needs
that translation check moved into `forge_rig`.

## Lean tier (16 GB, measured under a cap)

2026-08-30, Phase 0 spike 3. `forge.toml`'s `[hardware] tier = "lean"`
shipped three unmeasured cells in `forge2.md`'s table — "512³ (unmeasured)",
"+ ~14 GB (tight)", "~16 GB (unmeasured on 16)" — and nobody here owns a
16 GB card. These are what a 24 GB card says with the run held under a
ceiling. **Every number below is approximate and the approximation is
named**; none of it is a certificate that a 16 GB part is enough.

**The cap, exactly.** Two different mechanisms, because the two executors
are two different animals.

*For an `env` backend* — TRELLIS.2, SkinTokens, ARDY — the inner half calls
`torch.cuda.set_per_process_memory_fraction(16 GiB / 23.496 GiB) = 0.6810`
before the first weight moves, from a new opt-in module
`python/forge_gen/vram_cap.py`. It is switched on by `FORGE_VRAM_CAP_GB`
and by nothing else: unset — every ordinary run — and not a byte of
behaviour changes. Three entry points honour it: `forge_gen.mesh`'s inner
half and `forge_gen.motion.sweep`'s call `vram_cap.apply()` after
`import torch`; SkinTokens' `demo.py` is upstream's and not ours to edit,
so `spike_skin.py` runs it as
`python -m forge_gen.vram_cap demo.py …`, which sets the ceiling and then
runs the script under its own `__main__` with `sys.argv` untouched. Every
capped run in this section also exported
`PYTORCH_CUDA_ALLOC_CONF=expandable_segments:True`, which is what keeps a
tight ceiling failing on size rather than on fragmentation.

    FORGE_VRAM_CAP_GB=16 PYTORCH_CUDA_ALLOC_CONF=expandable_segments:True \
        forge gen mesh ref.png --preset character --resolution 512 …

**What that ceiling is not.** `set_per_process_memory_fraction` bounds
*torch's caching allocator in that process*. Outside it sit the CUDA
context itself (~300–400 MiB), cuBLAS/cuDNN workspaces, and nvdiffrast's
own device allocations during the texture bake. So "peaked at 15.4 GB under
a 16 GB ceiling" means the torch allocator stayed under 16 GB, not that a
16 GB card would have survived — a real 16 GB part has roughly 15.0–15.5 GB
usable once its context and a desktop are resident.

*For ComfyUI* the unit was restarted with `--reserve-vram 8` added through a
systemd drop-in (`~/.config/systemd/user/forge-comfy.service.d/`), which
tells ComfyUI's model manager to keep 8 GB of the 24 free and plan against
~16. That is a coarser approximation still: it bounds what the *model
manager* will load and offload, not what a node may allocate around it. The
drop-in was removed and the unit restarted on its tracked flags when the
spike ended.

**Peaks** are `nvidia-smi` at 10 Hz, sampled both card-wide
(`--query-gpu=memory.used`) and per compute app
(`--query-compute-apps`), so a run's own peak is separable from the
baseline. The card baseline through all of it was 1 131–1 236 MiB: the
desktop plus the idle ComfyUI unit's 386 MiB CUDA context.

| what | under the cap | peak, the process | peak, the whole card | seconds |
|---|---|---|---|---|
| **TRELLIS.2 512³**, `vex_runner`, `--preset character` seed 7, 1024² texture | **completes**, first attempt, no retry | **3 200 MiB (3.1 GB)** | 4 482 MiB | **73.3 s** |
| the same at `--texture 512` | completes, first attempt | 3 200 MiB (3.1 GB) | 4 435 MiB | 74.3 s |
| **SkinTokens** skin-only + transfer + postprocess, `out/prepare/vex_runner.glb` | **completes** | **3 408 MiB (3.3 GB)** | 4 543 MiB | **30.9 s** |
| **Qwen-Image Q4_K_M GGUF** + InstantX ControlNet-Union, 1024², seed 42, `--reserve-vram 8` | **completes** | n/a (the unit's own process) | **16 609 MiB (16.2 GB)**, i.e. ~15.1 GB of run over a 1 122 MiB baseline | **111.2 s** |
| **ARDY** sweep, one prompt × one seed × two samples | **completes** | **15 808 MiB (15.4 GB)** | 17 041 MiB | **49.5 s** |

**Four of four complete. Two of the four are marginal and one is not close
to marginal at all.** TRELLIS.2 and SkinTokens finish with three quarters
of a 16 GB card unused; ARDY at 15.4 GB and the quantised image model at
~15.1 GB of run each sit within a few hundred megabytes of what a real
16 GB part has left after its own context. The lean tier's constraint is
**motion and the reference image**, and it always was — the lift never was.

**The 22 GB that was never there.** For comparison the same reference was
lifted at **1024³ uncapped, same seed, the same afternoon**: 89.6 s, process
peak **4 790 MiB (4.7 GB)**, card peak 5 880 MiB. Every table in this repo
had TRELLIS.2 at 1024³ down as "~22 GB — alone", and `backends/trellis2/backend.toml`
budgets `vram_gb = 22`; this file said in so many words that the peak "was
not pinned down". It is pinned now and it is **4.5× smaller than the
budget**. The same is true one size down for SkinTokens: upstream says "at
least 14 GB", `backend.toml` says `vram_gb = 14`, and it peaks at 3.3–4.4 GB.
ARDY's ~16 GB is the one row that was right. One caveat kept honest: this
is one reference whose keyed subject covers 18 % of the frame, and
TRELLIS.2's cost follows the occupied voxels, so a fatter subject will cost
more — but not four times more. **Do not re-quote a `vram_gb` as a
measurement; it is a budget, and `just gpu` sizes the card against it.**

**Reading the two lift sheets, which is the part no number settles.**
`out/spike/lean/views_512.png` against `out/spike/lean/views_1024.png`,
nine views each, culling off. Both are **closed** — no hole in the back of
the skull at either register, the failure the seed sweep exists for — both
hold the T-pose, and both land the same bounds (1.00 × 0.94 × 0.28 m) and
the same vertex budget (21 828 vs 25 211). Where 512³ loses, it loses the
same way the 1 500-vertex register once did, more mildly: **the face goes**
(the 1024 head has a brow, a nose and a mouth line under the visor band;
the 512 head is a soft doughy mask), **the fingers fuse** (the 1024 top view
has separated fingers, the 512 a mitt), the boot soles lose their treads —
and the **colours drift**: the reference's magenta visor bakes out purple
and the scalp bakes orange at 512³ where 1024³ keeps them. The teal
forearm circuitry is actually clearer at 512³, so it is not uniformly
worse. Read as a rigging input it is fine; read as a hero face it is not.
**Lean lifting at 512³ is a real register for crowd and background bodies
and a downgrade for anything the camera walks up to** — and since a lean
card has plenty of room at 1024³ (4.7 GB), the honest lean default is
1024³ and 512³ is a speed knob, not a memory one.

**The image model in its lean form.** Pack
`city96/ComfyUI-GGUF` @ `6ea2651e7df66d7585f6ffee804b20e92fb38b8a`
(2026-01-12), cloned into `$PREFIX/data/custom_nodes/`; `gguf 0.19.0` and
`protobuf 7.36.0` into the comfy venv (`uv pip install --python
$PREFIX/venv/bin/python`, because that venv has no `pip`). Weights
`city96/Qwen-Image-gguf` @ `e77babc55af111419e1714a7a0a848b9cac25db7`,
file `qwen-image-Q4_K_M.gguf`, **13.07 GB**, into
`$PREFIX/data/models/diffusion_models/`. The template is
`backends/comfy/workflows/reference_qwen_gguf.api.json` — the fp8 template
with node `1` swapped from `UNETLoader` to `UnetLoaderGGUF` and nothing
else moved, so the five patched inputs and every node id stay where
`spike_reference.py` expects them. **The text encoder is not quantised**:
the fp8 Qwen2.5-VL 7B (9.38 GB) still does the encoding, because
`--cache-none` means it loads, encodes and goes before the diffusion model
arrives, and 9.38 GB alone fits the lean budget.

The picture, seed 42, the same prompt the fp8 comparison used
(`out/spike/lean/reference/qwen_image_gguf_42.png` beside
`out/spike/reference/qwen_image_42.png`): the style line is still obeyed —
flat, posterized, painted lighting — the T-pose is strict, and
`mesh.py`'s own keyer passes it with the fingertip line **0.00° off
horizontal** (fp8: 0.03°) and the subject 94 % of the frame height. It is
**cleaner at the feet than the fp8 image was**: the keyed alpha four pixels
above the lowest subject pixel is 43 px on the GGUF draw and 299 px on the
fp8 one, which is the contact shadow that lifted as a slab under both feet
in the reference spike. One seed of one model is not evidence that Q4
draws fewer shadows; what it is evidence of is that **Q4 costs about
nothing in style adherence or pose and about nothing in time** — 111.2 s
against fp8's 113.6 s, because with `--cache-none` most of both numbers is
the model swap, not the sampling. `POST /free` gave the card back
(22.40 → 22.28 GB free), as it did for the fp8 run.

**What this section does not cover.** MOSS-TTS 1.7B on the lean tier
(`forge2.md`'s table says ~5 GB) was not run; neither was a lean-tier
prop lift or a lean `moss_sfx`. The gap this section left open on the day
— `snapshot.json`, `backend.toml` and `install.sh` all saying the host
carried no custom node packs while ComfyUI-GGUF sat in `custom_nodes/` —
is closed: the pack, its two pips and the 13.07 GB Q4 file are recorded in
all four places, and the trap that earned it is under § ComfyUI above.
