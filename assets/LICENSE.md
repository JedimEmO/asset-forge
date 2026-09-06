# Sample asset notices

The repository's MIT OR Apache-2.0 licence covers the code. Sample assets
under `assets/` and their sources under `assets-src/` carry separate provenance
and notices. Use the [source ledger](../assets-src/SOURCES.md), reference
records and individual sidecars for each file's recorded origin.

| Material | Recorded source and scope |
| --- | --- |
| Bodies and static models | TRELLIS.2 lifts and Forge preparation. The recorded nvdiffrast texture baker carries a non-commercial notice; retain that restriction with the generated textures. |
| Reference PNGs | Brought from external image tools, including Grok and OpenAI image generation. The ledger and `.ref.json` identify the source for each image; no single provider description applies to the whole library. |
| Animation clips | ARDY takes and explicit Forge bake recipes. Keep selected takes and records for reproduction checks. ARDY code and checkpoint notices are distinct. |
| Audio | MOSS and ACE-Step generation, plus deterministic authored Scrapyard synthesis. Individual records identify the source or explicitly record unknown backend provenance. The authored scripts and source manifests remain in the repository. |
| Voice references | Recorded auditions under `assets-src/voices/`, with their own voice records. |

Bodies, models and audio claim integrity; clips additionally support the
recorded reproduction audit. These claims describe verification, not a grant
of rights. Code licensing does not override a model, reference or asset notice.

The Relay Run delivery has [separate notices](../demos/relay-runner/web/NOTICES.md)
for its selected runtime assets, including experimental Pixal3D exports. The
older [Scrapyard notices](../designs/scrapyard/ASSET-NOTICES.md) are preserved
unchanged alongside the shared source material; its game and review history
are available in the [archive](../designs/scrapyard/README.md).
