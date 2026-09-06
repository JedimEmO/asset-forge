# Scrapline verification

Verified locally on 2026-09-05, using Bevy 0.19, Rust 1.96.1 and the NVIDIA RTX 4090 Vulkan driver on Linux. This is a private playable build; asset terms are recorded separately in ASSET-NOTICES.md.

## Code and source checks

- `cargo test --workspace`: 566 passed, 0 failed; 2 existing documentation examples ignored.
- `cargo test -p scrapyard_arena --bin scrapline`: 32 passed, 0 ignored.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo fmt --all -- --check`: clean.
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`: clean.
- Python toolchain suite: 249 passed.
- `forge verify`: 83 accepted asset integrity checks passed.
- Batch verifier: 3 VFX atlases, 12 contained frames, 3 self-contained character packages, unchanged attachment inputs and 5 measured grounding tracks passed.

The game tests include five complete ordinary combat-bot runs, without health injection. They clear 1,010 machines and finish in 706–744 seconds across optimized and ordinary upgrade choices. Other tests cover loss, restart, pause, swept collision order, spawn immunity, horde limits, upgrade diversity/caps, high-refresh dash buffering, audio limits, atlas playback and UI interaction.

Independent focused reviews covered simulation, rendering, interface, audio, packaging and launch paths. The optional `/simplify` slash command was unavailable in this runtime; it is not claimed as executed.

## Actual application checks

`tools/verify_runtime.py` sends native X11 events to an isolated game window. Start, movement, mouse aim, dash, pause/resume and clicking an upgrade all pass. The chosen upgrade appears in the resulting state, and the selection click does not leak into a shot.

`tools/verify_layout.py` captures the real window at 800×600 and 1920×1080. Title, upgrade and pause layouts fit, remain readable and return no renderer errors. Title, upgrade, pause, victory and defeat screens were also reviewed at 1280×720.

The explicitly labeled showcase and boss scenarios test authored visual states. Their inspection invulnerability is separate from ordinary gameplay; they do not count as completed runs. Normal restart clears their overrides. Captures and autoplay are silent and never update the player's record.

The packaged executable was launched from `/tmp`, loaded its own assets and rendered successfully. An explicit relative `--asset-root` was also exercised from another working directory. All 23 packaged file hashes match. Repackaging through a fresh staging directory excludes stale files and preserves the previous package.

Runtime screenshots, state reports and logs are retained under `out/scrapline/`. The packaged build is `out/scrapline-linux/`; launch it with `./PLAY.sh`.

## Complete rendered run

An ordinary autoplay run completed all ten waves and reached Victory with 1,010 kills, 80,690 score and level 32 in 751.38 simulation seconds (12:31). The renderer processed 46,800 frames at 59.68 FPS on average over 784.14 wall-clock seconds. This run used no inspection overrides, generated no renderer errors, and exercised the full live simulation, animation, UI and asset pipeline. Silent mode left the user's records unchanged.

The final package received subsequent cosmetic improvements to robot/pickup meshes and menu layering. Those changes were separately captured and reviewed; the gameplay rules were unchanged. The FPS figure is a local verification result on the hardware above, not a cross-device performance guarantee.
