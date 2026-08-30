# Workflow templates

API-format ComfyUI graphs (`POST /prompt`'s `prompt` object: a flat map of
node id → `{class_type, inputs, _meta}`). They are *pure graphs* — no forge
metadata is smuggled into the file, because anything at the top level of the
object is read by ComfyUI as another node. What a caller may change is
marked **inside the graph**: a node's `_meta.title` carries one
`PATCH:<key>` token per patchable input (`PATCH:seed`, or
`pose conditioning; PATCH:strength PATCH:end_percent`), and everything else
in the title is prose for whoever opens the graph in the UI. There is no
sidecar manifest of node ids — that would be one fact in two files that
nothing holds together — and `python/forge_gen/comfy.py::patch_points`
builds the map by reading the markers. A template missing a key the verb
requires is a refusal before the GPU, naming the key and the file.

Node ids happen to be stable across the three reference templates, which is
convenient for a human diffing them, but nothing reads an id any more.

| id | node | patch |
|---|---|---|
| `5` | `CLIPTextEncode` | `inputs.text` — the style prefix plus the description |
| `6` | `CLIPTextEncode` | `inputs.text` — the negative prompt (inert on schnell, which samples at cfg 1.0) |
| `7` | `LoadImage` | `inputs.image` — the pose image's **name in ComfyUI's input directory**, put there with `POST /upload/image` (`overwrite=true`), not a path |
| `9` | `ControlNetApplyAdvanced` | `inputs.strength`, `start_percent`, `end_percent` |
| `10` | `EmptySD3LatentImage` | `inputs.width`, `inputs.height` |
| `11` | `KSampler` | `inputs.seed` — always patched, never left at the file's `0` |
| `13` | `SaveImage` | `inputs.filename_prefix` |

Everything else — the model files, the sampler, the step count, the shift —
is the template's own statement of the register, and changing one is a new
template, not a flag. The measured cost of each and why Qwen-Image was
chosen are in `designs/hosting.md` § ComfyUI.

## `reference_qwen.api.json`

Qwen-Image fp8 as three split files (`UNETLoader` + `CLIPLoader type
qwen_image` + `VAELoader`), `ModelSamplingAuraFlow` shift 3.1, 20 steps at
cfg 2.5, euler/simple. Pose conditioning through **InstantX
Qwen-Image ControlNet-Union** — Apache-2.0, and trained on the model it
conditions. No `SetUnionControlNetType`: ComfyUI's Qwen InstantX loader
infers the control type from the image.

## `reference_flux.api.json`

FLUX.1-schnell fp8 as the one all-in-one checkpoint
(`CheckpointLoaderSimple`, which is where its CLIP and VAE come from), 4
steps at cfg 1.0, euler/simple — schnell's own register. Pose conditioning
through **Shakker-Labs FLUX.1-dev-ControlNet-Union-Pro-2.0**, which is under
the **FLUX.1-dev Non-Commercial License** and was trained on FLUX.1-dev, not
on the Apache-2.0 schnell it is applied to. Both facts are why this template
exists only to have been measured: no reference record may name it. Union
Pro 2.0 dropped the mode embedding, so it too needs no
`SetUnionControlNetType`.

## `reference_qwen_gguf.api.json`

The lean tier's form of `reference_qwen.api.json`: node `1` swapped from
`UNETLoader` to **`UnetLoaderGGUF`** loading `qwen-image-Q4_K_M.gguf`
(13.07 GB), and nothing else moved — same node ids, same five patched
inputs, same sampler, shift, steps and ControlNet. The text encoder is *not* quantised:
the fp8 Qwen2.5-VL 7B still encodes, because `--cache-none` means it
loads, encodes and goes before the diffusion model arrives, and 9.38 GB
alone fits the lean budget.

`UnetLoaderGGUF` is the one node in this directory that is not native to
ComfyUI v0.34.2: it comes from the `city96/ComfyUI-GGUF` pack that
`backend.toml`'s `[[comfy.packs]]` pins and `install.sh` clones. On a host
without that pack this template fails at `POST /prompt` with an unknown
node class, and `forge doctor` says so first.
