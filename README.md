# asset-forge

A local toolkit for **agentic game asset creation**: meshes and rigged
characters, animation clips, and sound — generated on your own GPU from a
reference image or a prompt, judged by a human *and* an AI agent, and
shipped as engine-agnostic files (glTF, WAV/OGG, PNG, JSON) under one
manifest. Nothing is modelled, animated or recorded by hand; the repo is one
path from "describe it" to "it is in the library", with a gate at every step
that fails loudly.

The generating half is the easy half — plenty of models produce meshes,
motion and audio. The judging half is the point. An agent cannot orbit a
mesh, watch an animation or listen to a sound, so this turns all three into
things it *can* inspect: rendered contact sheets, plots, and numbers that
fail loudly. Claude Code drives it through committed skills and an MCP
server; a terminal drives it through one binary, `forge`, and a `justfile`.

Every generator runs locally. The one thing this repo does not make is the
reference image: bring a PNG, and the record claims the file's integrity
rather than pretending it could paint it again.

```
make                                judge                               ship
PNG → TRELLIS.2 → auto-rig          views, rig check, the walk sheet    promote body → sidecar + manifest
PNG → TRELLIS.2 → prop normalize    views, the studio                   promote model → sidecar + manifest
ARDY motion → native bake           review table, strip on the body     promote clip → sidecar + manifest
ACE-Step, MOSS → audio              waveform, spectrogram, loudness     promote audio → sidecar + manifest
```

## Quickstart

(P5)

## Hardware

| Need | What |
|---|---|
| OS | Linux. Everything is tested on one machine; nothing is tested elsewhere yet |
| GPU | NVIDIA, ≥ 16 GB for clips and audio; 24 GB for 1024³ lifts (the body and prop registers) |
| CUDA | 12.4 (the TRELLIS.2 env pins its own toolkit; see `designs/hosting.md`) |
| Blender | ≥ 4.2, headless, only for the rig, export and prop-normalize steps |
| Judging and the viewer | CPU is enough: headless sheets and views need a wgpu adapter and llvmpipe qualifies; no display server |

## The reference image

(P5)

## The rig profile

(P5)

## Records

(P5)

## Why the judging half has to exist

(P5)

## The studio

(P5)

## For agents

(P5)

## Backends & licences

**The texture baker is non-commercial.** TRELLIS.2 bakes its textures
through nvdiffrast 0.4.0, which ships under the NVIDIA Source Code License —
research and evaluation only, no commercial use. Everything else in the lift
(TRELLIS.2 code and weights, CuMesh, FlexGEMM, utils3d) is MIT, but a mesh
textured through this pipeline passed through nvdiffrast. The installer
requires explicit consent with the licence printed, `forge doctor` warns
while it is installed, every lift record carries
`texture_baker: "nvdiffrast (non-commercial)"`, and a replacement baker is an
open follow-up. Decide whether that fits your project before you lift
anything you mean to sell.

(P5)

## Crates

(P5)

## Layout

(P5)

## Using it from your game

(P5)

## Licence

MIT OR Apache-2.0, at your option.

The sample library under `assets/` and `assets-src/` is shipped so the tools
have something to show on a fresh clone. Its meshes were lifted with
TRELLIS.2 (MIT, code and weights) out of reference images made with a cloud
image model before this repository existed; its clips come from
[ARDY](https://github.com/nv-tlabs/ardy) (code Apache-2.0, checkpoints under
the NVIDIA Open Model License; outputs are usable); its sounds from MOSS and
ACE-Step (Apache-2.0 and MIT). The per-image record, the licence answer for
the reference images, and what each record is and is not allowed to claim
are in [`assets-src/SOURCES.md`](assets-src/SOURCES.md). Every lifted texture
in the sample passed through nvdiffrast; see above.
