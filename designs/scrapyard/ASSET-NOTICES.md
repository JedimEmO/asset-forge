# Scrapline asset notices

This private playable build combines original game code, authored procedural
geometry and audio, and AI-generated character, weapon, pickup and effect art.

The armed scavenger and rusher are consumer assemblies of the project's accepted
body and animation assets. Their original references, generation settings,
validation reports and content hashes remain in `assets-src/`, asset sidecars,
and `designs/scrapyard/batch-02/` in the source checkout.

The character texture generation used a baker with a non-commercial notice
(nvdiffrast). These generated assets are not represented as cleared for commercial
distribution. Resolve the relevant upstream terms or replace the affected assets
before commercial release. The Rust source's MIT/Apache licenses do not change
those asset terms.

The seven sound effects and combat loop are original deterministic synthesis,
retained as `assets-src/audio/scrapyard_synth.py` and `scrapyard_music.py`.
They were promoted through Forge with source records; rejected model and audio
generations are not included in the game package.

The VFX PNG sources are unchanged. At runtime the renderer discards alpha below
4/255 as specified by their manifest, then plays four-frame sheets with their
recorded pivots. Character foot grounding applies measured offsets to visual
roots without changing the source clips or gameplay colliders.

See `manifest.json` in a packaged build for the exact bundled file hashes.
