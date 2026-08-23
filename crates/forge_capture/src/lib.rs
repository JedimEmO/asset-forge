//! Windowless offscreen frame capture for Bevy.
//!
//! This crate drives a Bevy [`App`] with no window and no display server,
//! renders to an offscreen texture, and hands back the pixels as an [`Image`].
//! It is deliberately **not** animation-aware: it captures whatever the app
//! renders in whatever state the caller has put the world into, which makes it
//! equally useful for golden-image regression testing.
//!
//! ```no_run
//! use forge_capture::{CaptureApp, CaptureSettings};
//! # use bevy::prelude::*;
//! let mut app = CaptureApp::new(CaptureSettings::default(), |app| {
//!     app.add_systems(Startup, |mut commands: Commands| {
//!         commands.spawn((Camera3d::default(), Transform::default()));
//!     });
//! });
//! let frame = app.capture().expect("capture failed");
//! ```
//!
//! # Why this is not just `App::run()`
//!
//! Getting deterministic single-frame output out of Bevy needs three things
//! that are easy to miss, and all three fail *silently* — you get a black
//! frame, a stale frame, or a frame from the wrong pose rather than an error:
//!
//! * [`PipelinedRenderingPlugin`] is in `DefaultPlugins` and moves rendering to
//!   its own thread, so `update()` returns *before* the frame has rendered.
//!   [`CaptureApp`] disables it.
//! * Without `synchronous_pipeline_compilation`, the first frame renders with
//!   missing pipelines. [`CaptureApp`] turns it on.
//! * Readback latency is not a fixed number of frames. [`CaptureApp`] pumps
//!   until the capture observer actually fires rather than guessing a count.

pub mod sheet;

pub use forge_raster::SaveError;
pub use sheet::{SheetCell, SheetError, SheetLayout, compose};

use std::sync::{Arc, Mutex};

use bevy::{
    app::SubApps,
    asset::{AssetPlugin, RenderAssetUsages},
    camera::RenderTarget,
    image::Image,
    prelude::*,
    render::{
        RenderPlugin,
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{Extent3d, PollType, TextureDimension, TextureFormat, TextureUsages},
        renderer::RenderDevice,
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::ExitCondition,
    winit::WinitPlugin,
};

/// Configuration for the offscreen render target and the capture loop.
#[derive(Debug, Clone)]
pub struct CaptureSettings {
    /// Width of the render target, in pixels.
    ///
    /// Widths that are a multiple of 64 avoid GPU row padding entirely for
    /// RGBA8 targets (64 x 4 bytes = the 256-byte copy alignment).
    pub width: u32,
    /// Height of the render target, in pixels.
    pub height: u32,
    /// How many `update()` calls a single capture may take before giving up.
    ///
    /// This is a deadlock guard, not a tuning knob — a healthy capture
    /// completes in two or three updates. Raising it will not make a broken
    /// capture work.
    pub max_frames_per_capture: u32,
    /// Directory Bevy loads assets from. `None` keeps Bevy's default.
    pub asset_root: Option<String>,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            width: 384,
            height: 512,
            max_frames_per_capture: 32,
            asset_root: None,
        }
    }
}

/// Why a capture did not produce a frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    /// The observer never fired within [`CaptureSettings::max_frames_per_capture`].
    TimedOut {
        /// How many updates were pumped before giving up.
        frames: u32,
    },
    /// The capture completed but carried no pixel data.
    NoPixelData,
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimedOut { frames } => write!(
                f,
                "screenshot observer did not fire within {frames} frames — \
                 is a camera targeting the capture image?"
            ),
            Self::NoPixelData => f.write_str("captured image carried no pixel data"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Frames rendered before the very first capture, covering pipeline
/// compilation and assets that only became available during `Startup`.
const WARMUP_FRAMES: u32 = 3;

/// A Bevy app that renders offscreen and hands back frames on demand.
///
/// The app owns no window and never calls [`App::run`] — the caller drives it
/// one frame at a time via [`CaptureApp::capture`].
pub struct CaptureApp {
    subapps: SubApps,
    settings: CaptureSettings,
    target: Handle<Image>,
    warmed: bool,
}

impl CaptureApp {
    /// Build a headless app around `build`, which receives the [`App`] before
    /// the render target exists so it can register plugins and systems.
    ///
    /// The caller is responsible for spawning a camera whose `RenderTarget` is
    /// [`CaptureApp::target`]; [`CaptureApp::spawn_camera_target`] is the easy
    /// way to get one.
    pub fn new(settings: CaptureSettings, build: impl FnOnce(&mut App)) -> Self {
        let mut app = App::new();
        let plugins = DefaultPlugins.set(AssetPlugin {
            file_path: settings
                .asset_root
                .clone()
                .unwrap_or_else(|| AssetPlugin::default().file_path),
            ..default()
        });
        app.add_plugins(
            plugins
                .set(WindowPlugin {
                    // A lot of Bevy expects WindowPlugin to be present even
                    // when there is no window, so it is configured away rather
                    // than removed. DontExit stops the app quitting the moment
                    // it notices it has no windows.
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(RenderPlugin {
                    synchronous_pipeline_compilation: true,
                    ..default()
                })
                // Winit panics without a display server, and we own the loop.
                .disable::<WinitPlugin>()
                // Without this, update() returns before the frame has rendered.
                .disable::<PipelinedRenderingPlugin>(),
        );

        build(&mut app);

        // No runner is ever started, so finish/cleanup are ours to call.
        app.finish();
        app.cleanup();

        let target = {
            let mut image = Image::new_uninit(
                Extent3d {
                    width: settings.width,
                    height: settings.height,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            );
            image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
            app.world_mut().resource_mut::<Assets<Image>>().add(image)
        };

        Self {
            subapps: std::mem::take(app.sub_apps_mut()),
            settings,
            target,
            warmed: false,
        }
    }

    /// The offscreen image every capture reads from.
    #[must_use]
    pub fn target(&self) -> &Handle<Image> {
        &self.target
    }

    /// A [`RenderTarget`] pointing at [`CaptureApp::target`], ready to put on a camera.
    #[must_use]
    pub fn render_target(&self) -> RenderTarget {
        self.target.clone().into()
    }

    /// The main world, for setting up the scene and stepping it between captures.
    pub fn world_mut(&mut self) -> &mut World {
        self.subapps.main.world_mut()
    }

    /// Spawn a 3D camera already pointed at the capture target.
    ///
    /// `Msaa` is set explicitly because Bevy adds `Msaa::Sample4` to every
    /// camera via required components, and silently inheriting that would make
    /// output depend on a default rather than on the caller's intent.
    pub fn spawn_camera_target(&mut self, transform: Transform, msaa: Msaa) -> Entity {
        let target = self.render_target();
        self.world_mut()
            .spawn((Camera3d::default(), target, transform, msaa))
            .id()
    }

    /// Advance the app by one frame and block until the GPU has caught up.
    pub fn update(&mut self) {
        self.subapps.update();
        // Completes any pending map_async so readbacks can resolve.
        let device = self.subapps.main.world().resource::<RenderDevice>().clone();
        // A failure here means the device is lost; there is nothing useful to
        // do about it at this layer, and the capture will time out and say so.
        let _ = device.wgpu_device().poll(PollType::Wait {
            submission_index: None,
            timeout: None,
        });
    }

    /// Render one frame and return its pixels.
    ///
    /// Pumps [`CaptureApp::update`] until the screenshot observer fires, rather
    /// than assuming a fixed readback latency.
    ///
    /// # Errors
    ///
    /// Returns [`CaptureError::TimedOut`] if no frame arrives within
    /// [`CaptureSettings::max_frames_per_capture`] updates — most often because
    /// no camera targets the capture image.
    pub fn capture(&mut self) -> Result<Image, CaptureError> {
        // Render the current state at least once before asking for a copy of
        // it. A screenshot scheduled against a target that has never been drawn
        // to resolves against the uninitialised texture and comes back as
        // transparent black — a silent, plausible-looking blank frame.
        //
        // The first capture additionally absorbs pipeline compilation and any
        // asset that only became ready during Startup.
        let warmup = if self.warmed { 1 } else { WARMUP_FRAMES };
        for _ in 0..warmup {
            self.update();
        }
        self.warmed = true;

        let slot: Arc<Mutex<Option<Image>>> = Arc::new(Mutex::new(None));
        let sink = Arc::clone(&slot);

        let target = self.target.clone();
        self.world_mut().spawn(Screenshot::image(target)).observe(
            move |event: On<ScreenshotCaptured>| {
                if let Ok(mut guard) = sink.lock() {
                    *guard = Some(event.image.clone());
                }
            },
        );

        for _ in 0..self.settings.max_frames_per_capture {
            self.update();
            let captured = slot.lock().ok().and_then(|mut guard| guard.take());
            if let Some(image) = captured {
                return if image.data.is_some() {
                    Ok(image)
                } else {
                    Err(CaptureError::NoPixelData)
                };
            }
        }

        Err(CaptureError::TimedOut {
            frames: self.settings.max_frames_per_capture,
        })
    }
}

/// Write a captured RGBA frame to a PNG.
///
/// Alpha is preserved, unlike Bevy's own `save_to_disk` observer, which drops
/// it via `to_rgb8` — a surprise if you are compositing cut-out figures.
///
/// # Errors
///
/// Returns an error if the frame carries no pixel data, is not 8-bit RGBA, or
/// cannot be written to `path`.
pub fn save_png(image: &Image, path: impl AsRef<std::path::Path>) -> Result<(), SaveError> {
    let data = image.data.as_ref().ok_or(SaveError::UnexpectedLayout {
        got: 0,
        expected: (image.width() as usize) * (image.height() as usize) * 4,
    })?;
    forge_raster::save_png(image.width(), image.height(), data, path)
}

/// Fraction of pixels that differ from the image's most common colour.
///
/// A sheet where this is near zero rendered nothing — no camera, no lights, or
/// a target that was never drawn to. Stride-samples, so it costs microseconds.
#[must_use]
pub fn ink_fraction(image: &Image) -> f32 {
    const STRIDE: usize = 16;
    let Some(data) = image.data.as_ref() else {
        return 0.0;
    };
    if data.len() < 4 {
        return 0.0;
    }
    // The background is whatever the corner pixel is; anything that differs
    // from it by more than a rounding error counts as ink.
    let bg = [data[0], data[1], data[2]];
    let mut total = 0usize;
    let mut ink = 0usize;
    for px in data.chunks_exact(4).step_by(STRIDE) {
        total += 1;
        let d = (i32::from(px[0]) - i32::from(bg[0])).abs()
            + (i32::from(px[1]) - i32::from(bg[1])).abs()
            + (i32::from(px[2]) - i32::from(bg[2])).abs();
        if d > 12 {
            ink += 1;
        }
    }
    if total == 0 {
        0.0
    } else {
        ink as f32 / total as f32
    }
}

/// Mean absolute per-channel difference between two same-sized frames, 0.0..=1.0.
///
/// Used to catch a contact sheet whose playhead never moved: every consecutive
/// pair comes back at ~0.0 and the clip is frozen.
#[must_use]
pub fn mean_abs_diff(a: &Image, b: &Image) -> f32 {
    const STRIDE: usize = 16;
    let (Some(da), Some(db)) = (a.data.as_ref(), b.data.as_ref()) else {
        return 0.0;
    };
    if da.len() != db.len() || da.is_empty() {
        return 0.0;
    }
    let mut total = 0usize;
    let mut sum = 0u64;
    for (pa, pb) in da
        .chunks_exact(4)
        .step_by(STRIDE)
        .zip(db.chunks_exact(4).step_by(STRIDE))
    {
        total += 1;
        for c in 0..3 {
            sum += u64::from(pa[c].abs_diff(pb[c]));
        }
    }
    if total == 0 {
        0.0
    } else {
        sum as f32 / (total as f32 * 3.0 * 255.0)
    }
}
