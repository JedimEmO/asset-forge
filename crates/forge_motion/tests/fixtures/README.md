# Fixtures: the frozen bake oracle

Five clips — take (`.npz`), sidecar (`.json`) and baked `.glb` — pinned so
`tests/bake_matches_shipped.rs` can hold today's writer to output that was
produced before it existed. Nothing here is regenerated; a live library is
re-baked by this very crate, so reading any live file would compare the
writer against itself.

## Where they came from, and on what terms

The takes are **ARDY** output (NVIDIA,
[github.com/nv-tlabs/ardy](https://github.com/nv-tlabs/ardy)): code
Apache-2.0, `ARDY-Core-RP-20FPS-Horizon40` checkpoints under the NVIDIA Open
Model License, **outputs usable** — the same terms as the sample library's
clips, stated in `assets-src/SOURCES.md`. They were generated in the
repository this one was distilled from, between 2026-07-30 and 2026-08-02,
at ARDY commit `693f74d`.

- `blender/gen_{walk,roll,pistol_shoot,rifle_idle}.*` — takes plus the
  `.glb`s the retired Blender pipeline baked from them: the oracle that
  proves the channel convention.
- `rust/gen_jump.*` — baked by this very writer's previous home; the one
  fixture held to byte equality, and the one that exercises `y_mode`.

## The records are frozen too

The `.json` sidecars are old-format records kept exactly as the era that
baked them wrote them — the tests read only the `recipe` block (as untyped
JSON, deliberately), so keys today's schema does not define (`review`) are
part of the freeze, not a format anything current writes.
