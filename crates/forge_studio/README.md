# forge_studio

Look at an asset before you trust it. Two headless renders for an agent, a
window for a person, and the checks that say whether a clip is wired to a
body at all. Not published: this is the toolkit's own workflow, and the
engine-free crates beneath it are the library surface.

## What is here

| Module | What it is for |
|---|---|
| `render` | `render_clip_sheet` — a clip on a body as a labelled contact sheet; `render_views` — a mesh from the seven angles a reviewer would walk to, culling off on request |
| `stage` | The set both renders and the viewer share: 30° lens, two lights, ground plane, cast shadow, the matte restyle of glTF's default material, the cull-off system |
| `views` | `View` (three-quarter, front, back, left, right, top) and `HeadView` (the three head close-ups), each a camera transform around a sphere |
| `binding` | `SkeletonPaths`, `ClipDiff`, `install_animation_targets` — does this clip bind to this skeleton, by name; `headless_app` + `bones_report` run it with no GPU |
| `npz_clip` | A raw ARDY take as a Bevy `AnimationClip` in the rig's bone frames, the instant it exists |
| `catalog` | The clip walk, reading `forge_library` sidecars, and the derived numbers a browser row shows |
| `orbit`, `theme`, `viewer` | The window's camera, colours and screenshot |
| `studio`, `rig_findings`, `rig_check`, `audit` | The viewer panels and the rig checks (other P3 agents) |

## The two renders

```rust,no_run
use forge_studio::{SheetRequest, Stage, ViewsRequest, render_clip_sheet, render_views};

// A clip on a body: eight poses, three-quarter view, a head row if asked.
let stage = Stage::new("assets", "bodies/vex_runner.glb");
let shot = render_clip_sheet(&stage, &SheetRequest::new("clips/walk.glb")).unwrap();
println!("{}", shot.summary());
if let Some(why) = shot.failure() {
    eprintln!("not a sheet worth looking at: {why}");
}

// A raw lift, before the rig: front/back/left/right plus three head close-ups,
// culling off so a missing rear surface shows as the inside of the front one.
let lift = Stage::from_path(std::path::Path::new("out/lifts/hero/hero.glb")).unwrap();
let views = ViewsRequest { cull_off: true, ..ViewsRequest::default() };
render_views(&lift, &views).unwrap().save_png("out/views/hero.png").unwrap();
```

Both return a `Shot`: the composed `Image`, the cell size after the
vision-budget clamp, the adapter that drew it, the bounds the subject was
framed from, and — when a clip was involved — the `ClipDiff` and whether every
sampled pose was the same picture. `Shot::failure()` is the exit-code rule:
a total mismatch (the clip drives none of the skeleton's bones — what you are
looking at is the rest pose) first, a frozen clip second.

### Why the sheet frames once

The camera is fitted to the union of the subject's bounds across every
sampled pose, not per frame. A camera refitted each pose makes the subject
grow and shrink between cells, which destroys exactly the comparison a
contact sheet exists to support. The head row is the same framing pointed at
the `Head` bone — the bounding box puts the focus behind the face, because
the Z extent is dominated by the feet.

### Why the views render exists

TRELLIS lifts a single front view, and a seed can leave the rear of the skull
simply absent. Every downstream gate — fit check, bone heat, rig check, the
walk sheet — passed that mesh; only a human orbiting the studio camera caught
it, three re-rigs later. `render_views` is that orbit, run before the GPU
minute and the rig minute are spent. The head close-ups are framed on the top
30 % of the bounds rather than on a bone because the thing being looked at is
usually not rigged yet, and with `cull_off` every `StandardMaterial` is set to
`cull_mode: None` + `double_sided` so what the sheet shows is geometry: a hole
reads as the inside of the face, not as dark hair. The ground is moved to the
bottom of the bounds, because a raw lift is a unit cube about the origin and a
floor through its middle hides the half you need. The sheet does *not* do
that: a foot through the floor is a defect it exists to show.

## Without a GPU

`binding::headless_app` is `MinimalPlugins` plus assets, transforms, the glTF
loader, the world spawner and animation — no renderer, no window, no adapter.
The spawner writes a loaded scene into the world through reflection, so every
component the importer emits is registered by hand there; `DefaultPlugins`
would register them all and demand a GPU for the privilege. `bones_report`
and `tests/npz_fidelity.rs` run on it, which is what makes `forge bones` a CI
gate rather than an eye-render.

`AnimationTargetId` is a one-way hash of a bone's full name path, and the
hashing changed in Bevy 0.19 (names are length-prefixed now). Never persist
the ids; `SkeletonPaths` is the table that maps them back to names at runtime.

## Tests

- `cargo test -p forge_studio` — unit tests plus `npz_fidelity` (preview ==
  native bake to under a millimetre, on the fixture mannequin, no GPU).
- `env -u DISPLAY -u WAYLAND_DISPLAY cargo test -p forge_studio --test render`
  — the two renders on a real adapter; skips with a printed reason when
  `request_adapter` finds nothing. Writes `out/p3/test_sheet.png` and
  `out/p3/test_views.png` for a person to look at.

Nothing here depends on the sample library: the body is written from the rig
contract by `forge_rig::fixture`, clips are baked by `forge_motion::bake` from
fixture takes, and the lift is `tests/fixtures/testbox.glb`, a unit cube
TRELLIS made from a test image.
