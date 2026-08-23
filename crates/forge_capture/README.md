# forge_capture

Windowless offscreen frame capture and labelled contact-sheet composition for
Bevy. Every headless render in the toolkit — clip sheets, mesh views, the rig
check strip, the `forge doctor` adapter smoke — goes through this crate. It
needs a wgpu adapter (llvmpipe is enough) and no display server.

It is deliberately **not** animation-aware: it captures whatever the app renders
in whatever state the caller has put the world into, which makes it equally
useful for golden-image regression testing.

## What it does that `App::run()` does not

Getting deterministic single-frame output out of Bevy needs three things that
are easy to miss, and all three fail *silently* — a black frame, a stale frame,
or a frame from the wrong pose rather than an error:

| Trap | What `CaptureApp` does |
|---|---|
| `WinitPlugin` panics without a display server | disabled; the caller owns the loop |
| `PipelinedRenderingPlugin` moves rendering to its own thread, so `update()` returns before the frame has rendered | disabled |
| The first frame renders with missing pipelines | `synchronous_pipeline_compilation: true` |
| Bevy quits the moment it notices it has no windows | `primary_window: None` + `ExitCondition::DontExit` |
| A screenshot scheduled against a target that has never been drawn to comes back transparent black | three warm-up frames before the first capture, one before every later one |
| Readback latency is not a fixed number of frames | pumps `update()` until `ScreenshotCaptured` fires |
| A capture that can never complete (no camera on the target) would spin forever | `max_frames_per_capture` — a deadlock guard, not a tuning knob |

## Use

```rust,no_run
use bevy::prelude::*;
use forge_capture::{CaptureApp, CaptureSettings, save_png};

let mut app = CaptureApp::new(CaptureSettings::default(), |app| {
    app.add_systems(Startup, |mut commands: Commands| {
        commands.spawn((
            DirectionalLight::default(),
            Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
    });
});
app.spawn_camera_target(
    Transform::from_xyz(3.5, 3.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    Msaa::Sample4,
);
let frame = app.capture().expect("capture failed");
save_png(&frame, "out/frame.png").expect("png saves");
```

`save_png` keeps alpha, unlike Bevy's own `save_to_disk` observer, which drops
it via `to_rgb8` — a surprise if you are compositing cut-out figures.

`ink_fraction` and `mean_abs_diff` are the two cheap sanity checks the sheet
renderers use: a frame whose ink is near zero rendered nothing (no camera, no
lights, a target never drawn to), and a sheet whose consecutive frames differ
by ~0.0 has a playhead that never moved.

## Contact sheets

`sheet::compose` tiles same-sized frames into a grid with a header line and a
caption burnt into each cell's corner (via `forge_raster`'s 5×7 font on a dark
plate, so light text stays readable over a light render).

The defaults — 4 columns, 384×512 cells, 8 frames — exist to land under the
**1568 px long edge** that vision models downscale past
(`sheet::VISION_LONG_EDGE`). Overshooting is worse than rendering smaller: you
pay for pixels and then lose them to a resample, which turns burnt-in labels to
mush. `SheetLayout::fit_to_budget` shrinks an oversized cell request to fit;
callers should report the final resolution either way, because a silently
shrunk sheet reads as a deliberate choice when it was actually a clamp. A
standing figure is portrait, so cells default to 3:4 rather than square — a
square cell spends roughly 40% of itself on empty air.

## Examples

```sh
# Proves the capture path needs neither X11 nor Wayland, and names the adapter
# it rendered on — "it rendered" and "it rendered on the GPU you expected" are
# different claims; a silent fall back to llvmpipe still produces a valid PNG.
env -u DISPLAY -u WAYLAND_DISPLAY cargo run -p forge_capture --example smoke [out.png]

# Front, three-quarter and profile head close-ups of a character glb, matte.
cargo run -p forge_capture --example portrait -- model.glb out_prefix
```

The workspace `bevy` dependency enables `tonemapping_luts` on purpose: without
it the default tonemapper runs on a placeholder LUT and produces wrong colours
with no warning, and a capture crate whose whole job is faithful pixels cannot
ship that.
