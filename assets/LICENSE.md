# Licence of the sample library

The repository's MIT OR Apache-2.0 licence covers the **code**. The sample
assets under `assets/` and `assets-src/` are shipped so the tools have
something to show on a fresh clone, and each kind carries its own terms.
Per-file provenance — where every file came from, on what terms, with what
seed — is in [`assets-src/SOURCES.md`](../assets-src/SOURCES.md); this file
is the summary a reuse decision can be made from.

| Sample | Files | Terms |
|---|---|---|
| Body and models | `bodies/vex_runner.glb`, `models/sword.glb`, `models/barrel.glb` (+ `.json` sidecars, the `.blend`) | Meshes lifted with TRELLIS.2 (MIT, code and weights). **Their textures were baked through nvdiffrast 0.4.0, which ships under the NVIDIA Source Code License — non-commercial use only. The sample textures are therefore NOT licensed for commercial use or commercial redistribution.** Demonstration and evaluation only; for a commercial project, regenerate from your own references once a replacement baker exists, or ship the geometry with your own textures. |
| Reference images | `assets-src/refs/**/*.png` | Generated with xAI's grok (cloud image model); xAI's consumer Terms of Service state the user owns the output, commercial use included. That reading is recorded in `SOURCES.md` as confirmed through search excerpts, not a direct page retrieval — verify it yourself before relying on it commercially. Images with no human authorship are likely not copyrightable by anyone, which cuts both ways: weak exclusivity, and no third-party claim. |
| Clips | `clips/*.glb` (+ takes under `assets-src/takes/`) | Baked from ARDY takes: code Apache-2.0, checkpoints under the NVIDIA Open Model License — outputs are usable under that licence's output terms. |
| Audio | `audio/sfx/*`, `audio/music/*`, `audio/voice/*`, the designed voice under `assets-src/voices/` | Rendered here: MOSS-SoundEffect / MOSS-TTS / MOSS-VoiceGenerator outputs (models Apache-2.0) and ACE-Step outputs (MIT). The rendered files are the project's own. |

The one encumbrance that matters is the first row: **nothing textured
through this sample pipeline is for sale**. Every lift record names the
baker (`texture_baker: "nvdiffrast (NVIDIA Source Code License,
non-commercial)"`), so the fact travels with the asset.
