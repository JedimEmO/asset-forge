//! Look at an asset before you trust it: headless renders for an agent, a
//! window for a person, and the checks that say whether a clip is wired to a
//! body at all.
//!
//! # Two renders, one stage
//!
//! [`render_clip_sheet`] is the visual counterpart to a numeric animation
//! gate: checks that a clip drives the right bones tell you it is wired up,
//! not that it *reads* as the action it claims to be. Bad silhouettes, feet
//! through the floor and poses that pop are visual defects, and only a
//! picture finds them.
//!
//! [`render_views`] is the gate before the rig: a raw lift from the angles a
//! reviewer would otherwise walk to by hand — front, back, both sides, and
//! three close-ups of the top — with back-face culling switched off so what
//! the sheet shows is geometry. A hollow skull passed every downstream gate
//! once; this is the look that would have caught it before the GPU minute
//! and the rig minute were spent.
//!
//! Both run on the same [`stage`] — the same lights, ground and lens as the
//! viewer window — so an agent's sheet and a human's viewport agree about the
//! same asset and each can be used to check the other.
//!
//! ```no_run
//! use forge_studio::{SheetRequest, Stage, ViewsRequest, render_clip_sheet, render_views};
//! let stage = Stage::new("assets", "bodies/vex_runner.glb");
//! let shot = render_clip_sheet(&stage, &SheetRequest::new("clips/walk.glb")).unwrap();
//! println!("{}", shot.summary());
//! let lift = Stage::from_path(std::path::Path::new("out/lifts/hero/hero.glb")).unwrap();
//! let views = ViewsRequest { cull_off: true, ..ViewsRequest::default() };
//! render_views(&lift, &views).unwrap().save_png("out/views/hero.png").unwrap();
//! ```
//!
//! # Without a GPU
//!
//! [`binding`] answers the question Bevy never will — does this clip bind to
//! this skeleton — on an app with no renderer at all, and [`npz_clip`] turns
//! a raw take into a clip the stage can play the instant it exists.

pub mod audit;
pub mod binding;
pub mod catalog;
pub mod npz_clip;
pub mod orbit;
pub mod render;
pub mod rig_check;
pub mod rig_findings;
pub mod stage;
pub mod studio;
pub mod theme;
pub mod viewer;
pub mod views;

pub use binding::{BonesReport, ClipDiff, SkeletonPaths, bones_report};
pub use render::{
    RenderError, SheetRequest, Shot, Stage, ViewsRequest, render_clip_sheet, render_views,
};
pub use views::{HeadView, View};

/// The one Bevy type a caller needs to name to fill a request — a cell
/// size — re-exported so the `forge` binary can build a [`SheetRequest`]
/// without a Bevy dependency of its own.
pub use bevy::math::UVec2;
