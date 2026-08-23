//! The headless renders: a clip on a body as a contact sheet, and a mesh from
//! the angles a reviewer would walk to.
//!
//! Both run the same [`crate::stage`] through a [`CaptureApp`] with no window,
//! frame the subject from its bounds once, and compose the captured cells with
//! [`forge_capture::compose`]. What differs is what moves between cells: the
//! playhead for a sheet, the camera for a views render. See the crate docs
//! for why each exists.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use bevy::{
    animation::{AnimationClip, AnimationTargetId},
    asset::LoadState,
    math::UVec2,
    prelude::*,
    render::renderer::RenderAdapterInfo,
    world_serialization::{WorldAsset, WorldAssetRoot},
};
use forge_capture::{CaptureApp, CaptureSettings, SheetCell, SheetLayout, compose, mean_abs_diff};

use crate::binding::{ClipDiff, SkeletonPaths, attach_player, install_animation_targets};
use crate::stage::{
    CAMERA_FOV_DEG, FRAME_PADDING, find_animation_root, ground_under, seek, spawn_stage,
    stylize_gltf_default_material, uncull_materials, world_bounds,
};
use crate::views::{Facing, HeadView, View};

/// Frames to wait for assets before declaring them stuck.
const MAX_LOAD_FRAMES: u32 = 900;
/// Below this mean per-pixel difference, two frames are "the same picture".
const FROZEN_EPSILON: f32 = 0.002;

/// Which model to inspect, and where its assets live.
#[derive(Debug, Clone)]
pub struct Stage {
    /// Directory Bevy treats as the asset root.
    pub asset_root: PathBuf,
    /// Model path, relative to `asset_root`, forward-slashed.
    pub model: String,
    /// Scene index within the model file.
    pub scene_index: usize,
}

impl Stage {
    /// A stage for one model under an asset root.
    pub fn new(asset_root: impl Into<PathBuf>, model: impl Into<String>) -> Self {
        Self {
            asset_root: asset_root.into(),
            model: model.into(),
            scene_index: 0,
        }
    }

    /// A stage for a `.glb` anywhere on disk: its directory becomes the
    /// asset root and its file name the model. This is how a raw lift under
    /// `out/` is looked at before it is anybody's asset.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::AssetLoad`] when `path` has no file name or
    /// does not exist — checked here because Bevy would only report it
    /// hundreds of frames later, as a timeout.
    pub fn from_path(path: &Path) -> Result<Self, RenderError> {
        let absolute = std::path::absolute(path)
            .map_err(|e| RenderError::AssetLoad(format!("{}: {e}", path.display())))?;
        if !absolute.is_file() {
            return Err(RenderError::AssetLoad(format!(
                "{} is not a file",
                path.display()
            )));
        }
        let (Some(parent), Some(name)) = (absolute.parent(), absolute.file_name()) else {
            return Err(RenderError::AssetLoad(format!(
                "{} has no file name",
                path.display()
            )));
        };
        Ok(Self::new(parent, name.to_string_lossy().into_owned()))
    }

    /// The asset root as an absolute path.
    ///
    /// Bevy treats a relative asset root as relative to the *executable*,
    /// which under `cargo run` is somewhere in `target/`; a plain `assets`
    /// would silently become `target/debug/assets` and every load would fail
    /// as "path not found".
    #[must_use]
    pub fn absolute_root(&self) -> PathBuf {
        std::path::absolute(&self.asset_root).unwrap_or_else(|_| self.asset_root.clone())
    }

    /// The asset path Bevy loads the scene by.
    #[must_use]
    pub fn scene_path(&self) -> String {
        format!("{}#Scene{}", self.model, self.scene_index)
    }
}

/// A clip on the stage's model, sampled across a window, from one or more views.
#[derive(Debug, Clone)]
pub struct SheetRequest {
    /// Clip file, relative to the asset root.
    pub clip_file: String,
    /// Animation index within the clip file.
    pub clip_index: usize,
    /// Poses sampled across the window.
    pub frames: u32,
    /// One band of cells per view.
    pub views: Vec<View>,
    /// Cell size in pixels, before the vision-budget clamp.
    pub cell: UVec2,
    /// Cells per row.
    pub columns: u32,
    /// Window start, as a fraction of clip length.
    pub t0: f32,
    /// Window end, as a fraction of clip length.
    pub t1: f32,
    /// Append a band of head close-ups under the view bands.
    ///
    /// A whole-body cell frames 1.8 m into 512 pixels, which leaves a face
    /// about two dozen pixels tall — enough to see that a head is there and
    /// nothing about whether it reads as a face. Generated bodies made that
    /// worth a camera of its own. Off by default: a prop and a garment have no
    /// head, and a close-up of where one would be is three cells of nothing.
    pub head_row: bool,
}

impl SheetRequest {
    /// The documented defaults: 8 frames, three-quarter view, 4 columns,
    /// 384x512 cells — a 1536x1080 sheet, just under the downscale threshold.
    pub fn new(clip_file: impl Into<String>) -> Self {
        Self {
            clip_file: clip_file.into(),
            clip_index: 0,
            frames: 8,
            views: vec![View::ThreeQuarter],
            cell: UVec2::new(384, 512),
            columns: 4,
            t0: 0.0,
            t1: 1.0,
            head_row: false,
        }
    }
}

/// The stage's model from the angles a reviewer would walk to, no clip and
/// no skeleton required.
#[derive(Debug, Clone)]
pub struct ViewsRequest {
    /// The whole-figure views, one cell each.
    pub views: Vec<View>,
    /// Cell size in pixels, before the vision-budget clamp.
    pub cell: UVec2,
    /// Cells per row.
    pub columns: u32,
    /// Append the three head close-ups — the top of the bounds from the
    /// front, from behind, and from behind-and-above. Off for a prop.
    pub head_row: bool,
    /// Switch back-face culling off on every material, so a missing rear
    /// surface shows as the inside of the front one instead of as background.
    pub cull_off: bool,
    /// Hold a pose while looking: a clip file relative to the asset root and
    /// a time in seconds into it. Needs a skeleton; the rest pose otherwise.
    pub pose: Option<(String, f32)>,
}

impl Default for ViewsRequest {
    /// Front, back, left, right and the three head close-ups, culling on,
    /// the rest pose, in the sheet's 384x512 cells four to a row.
    fn default() -> Self {
        Self {
            views: View::WALK_AROUND.to_vec(),
            cell: UVec2::new(384, 512),
            columns: 4,
            head_row: true,
            cull_off: false,
            pose: None,
        }
    }
}

/// Where the head sits, as a fraction of total height down from the top,
/// and how much of the height a head close-up frames.
///
/// Measured from the bounds rather than from a `Head` bone because the thing
/// being looked at is very often not rigged yet — that is the point of
/// looking. A prop's "head" is whatever is at its top, and the cells honestly
/// show it.
const HEAD_DROP: f32 = 0.08;
/// See [`HEAD_DROP`].
const HEAD_FRAME: f32 = 0.30;

/// A rendered sheet plus everything needed to explain it.
pub struct Shot {
    /// The composed contact sheet.
    pub sheet: Image,
    /// Clip length in seconds; zero when no clip was involved.
    pub duration: f32,
    /// Sampled times, in seconds. One entry for a posed views render, none
    /// for a rest-pose one.
    pub times: Vec<f32>,
    /// Final cell size after any vision-budget clamp.
    pub cell: UVec2,
    /// Whether the cell size was reduced to fit the budget.
    pub clamped: bool,
    /// How many cells the sheet holds.
    pub cells: usize,
    /// GPU that rendered it.
    pub adapter: String,
    /// Which skeleton bones the clip actually drove, when there was a clip.
    pub diff: Option<ClipDiff>,
    /// True when every sampled pose was identical.
    pub frozen: bool,
    /// World-space bounds the subject was framed from, metres.
    pub bounds: (Vec3, Vec3),
}

impl Shot {
    /// True when the clip drives none of the skeleton's bones — the pose on
    /// the sheet is the rest pose, and the render is a failure however good
    /// it looks. Never true without a clip.
    #[must_use]
    pub fn is_total_mismatch(&self) -> bool {
        self.diff.as_ref().is_some_and(ClipDiff::is_total_mismatch)
    }

    /// True when every sampled pose was the same picture.
    #[must_use]
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// Why a command should exit non-zero, if it should: a total mismatch
    /// first, a frozen clip second. `None` is a sheet worth looking at.
    #[must_use]
    pub fn failure(&self) -> Option<&'static str> {
        if self.is_total_mismatch() {
            Some("the clip drives none of this skeleton's bones")
        } else if self.is_frozen() {
            Some("every sampled pose is identical")
        } else {
            None
        }
    }

    /// Write the sheet as a PNG.
    ///
    /// # Errors
    ///
    /// As [`forge_capture::save_png`].
    pub fn save_png(&self, path: impl AsRef<Path>) -> Result<(), forge_capture::SaveError> {
        forge_capture::save_png(&self.sheet, path)
    }

    /// A short human- and agent-readable report.
    #[must_use]
    pub fn summary(&self) -> String {
        let (lo, hi) = self.bounds;
        let size = hi - lo;
        let mut out = format!(
            "{} cells, {}x{} each{}\nadapter: {}\nbounds:  {:.2} x {:.2} x {:.2} m, lowest y {:.3}\n",
            self.cells,
            self.cell.x,
            self.cell.y,
            if self.clamped {
                " (clamped to fit)"
            } else {
                ""
            },
            self.adapter,
            size.x,
            size.y,
            size.z,
            lo.y,
        );
        if let Some(diff) = &self.diff {
            let _ = write!(
                out,
                "clip:    {:.3}s, {} sampled\ntimes:   {}\nbones:   {} driven, {} at rest, {} orphaned\n",
                self.duration,
                self.times.len(),
                self.times
                    .iter()
                    .map(|t| format!("{t:.2}"))
                    .collect::<Vec<_>>()
                    .join(" "),
                diff.bound.len(),
                diff.unbound.len(),
                diff.orphaned,
            );
        }
        if self.is_total_mismatch() {
            out.push_str(
                "WARNING: the clip drives none of this skeleton's bones — \
                 the pose you are looking at is the rest pose.\n",
            );
        } else if self.frozen {
            out.push_str(
                "WARNING: every sampled pose is identical — the clip may have no motion.\n",
            );
        }
        out
    }
}

/// Why a shot could not be produced.
#[derive(Debug)]
pub enum RenderError {
    /// An asset never finished loading, or was never there.
    AssetLoad(String),
    /// The model spawned no named hierarchy to animate.
    NoSkeleton,
    /// Capture failed.
    Capture(String),
    /// Sheet composition failed.
    Sheet(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AssetLoad(msg) => write!(f, "{msg}"),
            Self::NoSkeleton => f.write_str(
                "the model spawned no named node hierarchy — is it a rigged glTF scene?",
            ),
            Self::Capture(msg) => write!(f, "capture failed: {msg}"),
            Self::Sheet(msg) => write!(f, "could not compose sheet: {msg}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Render `request` on `stage` and return the sheet.
///
/// # Errors
///
/// Returns [`RenderError`] if assets fail to load, the model has no skeleton, or
/// rendering or composition fails.
pub fn render_clip_sheet(stage: &Stage, request: &SheetRequest) -> Result<Shot, RenderError> {
    let cells_total =
        request.frames * request.views.len().max(1) as u32 + u32::from(request.head_row) * 3;
    let layout = SheetLayout {
        columns: request.columns.max(1),
        ..SheetLayout::default()
    };
    let cell = layout.fit_to_budget(cells_total, request.cell);
    let clamped = cell != request.cell;

    let mut app = capture_app(stage, cell, false);
    let loaded = load_subject(
        &mut app,
        stage,
        Some((&request.clip_file, request.clip_index)),
    )?;
    let anim_root = loaded.anim_root.ok_or(RenderError::NoSkeleton)?;
    let clip_handle = loaded
        .clip
        .ok_or_else(|| RenderError::AssetLoad(String::from("clip vanished after loading")))?;

    let paths = SkeletonPaths::from_world(app.world_mut(), anim_root);
    let (duration, diff) = {
        let clips = app.world_mut().resource::<Assets<AnimationClip>>();
        let clip = clips
            .get(&clip_handle)
            .ok_or_else(|| RenderError::AssetLoad(String::from("clip vanished after loading")))?;
        (clip.duration(), ClipDiff::new(clip, &paths))
    };

    // Wire a player onto the animation root. The graph must round-trip through
    // an asset event before any pose applies, which the warm-up absorbs.
    let node = attach_player(app.world_mut(), anim_root, clip_handle);

    let times = sample_times(duration, request.frames, request.t0, request.t1);
    let (lo, hi) = frame_subject(&mut app, anim_root, node, &times);
    let (centre, radius) = enclosing(lo, hi);
    let mut cells = capture_poses(&mut app, anim_root, node, request, &times, centre, radius)?;
    if request.head_row {
        let (focus, head_radius) = head_sphere(&mut app, lo, hi);
        cells.extend(capture_head_row(
            &mut app,
            anim_root,
            node,
            times.first().copied().unwrap_or(0.0),
            focus,
            head_radius,
        )?);
    }

    let frozen = cells.len() > 1
        && cells
            .windows(2)
            .all(|w| mean_abs_diff(&w[0].image, &w[1].image) < FROZEN_EPSILON);

    let header = format!(
        "{} / {}  {:.2}S  {} BONES DRIVEN",
        short(&stage.model),
        short(&request.clip_file),
        duration,
        diff.bound.len()
    );
    let count = cells.len();
    let sheet = compose(&cells, &layout, &header).map_err(|e| RenderError::Sheet(e.to_string()))?;

    Ok(Shot {
        sheet,
        duration,
        times,
        cell,
        clamped,
        cells: count,
        adapter: adapter_name(&mut app),
        diff: Some(diff),
        frozen,
        bounds: (lo, hi),
    })
}

/// Render the stage's model from every requested angle.
///
/// The framing comes from the mesh bounds — a raw lift is a unit cube about
/// the origin, a body stands on y = 0, and both are handled by measuring
/// rather than assuming — and the ground is moved to the bottom of them so
/// half of a lift is not under the floor. A subject that cannot be framed
/// (nothing with bounds spawned) is a failure, not an empty sheet.
///
/// # Errors
///
/// Returns [`RenderError`] if the model fails to load, a pose was asked for
/// and the model has no skeleton or the clip does not load, or rendering or
/// composition fails.
pub fn render_views(stage: &Stage, request: &ViewsRequest) -> Result<Shot, RenderError> {
    let cells_total = request.views.len() as u32 + u32::from(request.head_row) * 3;
    let layout = SheetLayout {
        columns: request.columns.max(1),
        ..SheetLayout::default()
    };
    let cell = layout.fit_to_budget(cells_total, request.cell);
    let clamped = cell != request.cell;

    let mut app = capture_app(stage, cell, request.cull_off);
    let clip_ref = request
        .pose
        .as_ref()
        .map(|(file, _)| (file.as_str(), 0usize));
    let loaded = load_subject(&mut app, stage, clip_ref)?;

    let (mut duration, mut times, mut diff) = (0.0, Vec::new(), None);
    if let (Some((_, t)), Some(clip_handle)) = (&request.pose, loaded.clip) {
        let anim_root = loaded.anim_root.ok_or(RenderError::NoSkeleton)?;
        let paths = SkeletonPaths::from_world(app.world_mut(), anim_root);
        {
            let clips = app.world_mut().resource::<Assets<AnimationClip>>();
            let clip = clips.get(&clip_handle).ok_or_else(|| {
                RenderError::AssetLoad(String::from("clip vanished after loading"))
            })?;
            duration = clip.duration();
            diff = Some(ClipDiff::new(clip, &paths));
        }
        let node = attach_player(app.world_mut(), anim_root, clip_handle);
        let t = t.clamp(0.0, duration.max(0.0));
        times.push(t);
        // One frame for the graph to bind, one for the pose to apply.
        app.update();
        seek(app.world_mut(), anim_root, node, t);
        app.update();
    } else {
        // Transforms and bounds propagate in the frame after the spawn lands.
        app.update();
    }

    let (lo, hi) = world_bounds(app.world_mut()).ok_or_else(|| {
        RenderError::AssetLoad(String::from(
            "the model spawned nothing with bounds — does the scene hold a mesh?",
        ))
    })?;
    ground_under(app.world_mut(), lo.y);
    let (centre, radius) = enclosing(lo, hi);

    // A rest pose faces the contract's +Z; a subject held in a clip pose was
    // turned 180° by the bake and faces −Z. The labels follow the subject.
    let facing = if request.pose.is_some() {
        Facing::MinusZ
    } else {
        Facing::PlusZ
    };
    let mut cells = Vec::with_capacity(cells_total as usize);
    for &view in &request.views {
        let transform =
            view.camera_transform(facing, centre, radius, CAMERA_FOV_DEG, FRAME_PADDING);
        cells.push(capture_cell(
            &mut app,
            transform,
            view.name().replace('_', " ").to_ascii_uppercase(),
        )?);
    }
    if request.head_row {
        let height = (hi.y - lo.y).max(0.05);
        let focus = Vec3::new(centre.x, hi.y - HEAD_DROP * height, centre.z);
        let head_radius = HEAD_FRAME * height * 0.5;
        for view in HeadView::ALL {
            let transform =
                view.camera_transform(facing, focus, head_radius, CAMERA_FOV_DEG, FRAME_PADDING);
            cells.push(capture_cell(
                &mut app,
                transform,
                view.name().replace('_', " ").to_ascii_uppercase(),
            )?);
        }
    }

    let size = hi - lo;
    let header = format!(
        "{}  {} VIEWS  CULL {}  {:.2} x {:.2} x {:.2} M{}",
        short(&stage.model),
        cells.len(),
        if request.cull_off { "OFF" } else { "ON" },
        size.x,
        size.y,
        size.z,
        request
            .pose
            .as_ref()
            .map(|(file, t)| format!("  {} @ {t:.2}S", short(file)))
            .unwrap_or_default(),
    );
    let count = cells.len();
    let sheet = compose(&cells, &layout, &header).map_err(|e| RenderError::Sheet(e.to_string()))?;

    Ok(Shot {
        sheet,
        duration,
        times,
        cell,
        clamped,
        cells: count,
        adapter: adapter_name(&mut app),
        diff,
        frozen: false,
        bounds: (lo, hi),
    })
}

/// The windowless app with the shared stage on it.
fn capture_app(stage: &Stage, cell: UVec2, cull_off: bool) -> CaptureApp {
    CaptureApp::new(
        CaptureSettings {
            width: cell.x,
            height: cell.y,
            asset_root: Some(stage.absolute_root().to_string_lossy().into_owned()),
            ..CaptureSettings::default()
        },
        |app| {
            app.add_systems(Startup, spawn_stage)
                .add_systems(Update, stylize_gltf_default_material);
            if cull_off {
                app.add_systems(
                    Update,
                    uncull_materials.after(stylize_gltf_default_material),
                );
            }
        },
    )
}

/// The model on the stage, and the clip beside it when one was asked for.
struct Loaded {
    /// The glTF root node, when the model has a named hierarchy.
    anim_root: Option<Entity>,
    /// The clip, loaded.
    clip: Option<Handle<AnimationClip>>,
}

/// Spawn the stage's model, load `clip` if given, and wait for both.
///
/// Animation targets are installed on the root when the importer left none —
/// a body exported without animations of its own has nothing for a clip to
/// bind to otherwise.
fn load_subject(
    app: &mut CaptureApp,
    stage: &Stage,
    clip: Option<(&str, usize)>,
) -> Result<Loaded, RenderError> {
    let (scene, clip_handle) = {
        let server = app.world_mut().resource::<AssetServer>().clone();
        (
            server.load::<WorldAsset>(stage.scene_path()),
            clip.map(|(file, index)| {
                server.load::<AnimationClip>(format!("{file}#Animation{index}"))
            }),
        )
    };
    let model_entity = app.world_mut().spawn(WorldAssetRoot(scene.clone())).id();
    wait_for_assets(app, model_entity, &scene, clip_handle.as_ref())?;

    let anim_root = find_animation_root(app.world_mut(), model_entity);
    if let Some(root) = anim_root
        && app.world_mut().get::<AnimationTargetId>(root).is_none()
    {
        install_animation_targets(app.world_mut(), root);
    }
    Ok(Loaded {
        anim_root,
        clip: clip_handle,
    })
}

/// One capture from `transform`, labelled.
fn capture_cell(
    app: &mut CaptureApp,
    transform: Transform,
    label: String,
) -> Result<SheetCell, RenderError> {
    let camera = place_camera(app, transform);
    let image = app
        .capture()
        .map_err(|err| RenderError::Capture(err.to_string()))?;
    app.world_mut().entity_mut(camera).despawn();
    Ok(SheetCell { image, label })
}

/// A camera on the stage's lens, aimed at the capture target.
fn place_camera(app: &mut CaptureApp, transform: Transform) -> Entity {
    let camera = app.spawn_camera_target(transform, Msaa::Sample4);
    app.world_mut()
        .entity_mut(camera)
        .insert(Projection::from(PerspectiveProjection {
            fov: CAMERA_FOV_DEG.to_radians(),
            ..default()
        }));
    camera
}

/// Union the subject's bounds across every pose.
///
/// Framing once for the whole clip rather than per frame is deliberate: a
/// camera refitted each pose makes the subject grow and shrink between cells,
/// which destroys exactly the comparison a contact sheet exists to support.
///
/// The bounds come back rather than only a centre and radius, because the head
/// row needs the *top* of the subject and a mid-point cannot be un-averaged.
fn frame_subject(
    app: &mut CaptureApp,
    anim_root: Entity,
    node: AnimationNodeIndex,
    times: &[f32],
) -> (Vec3, Vec3) {
    let mut bounds: Option<(Vec3, Vec3)> = None;
    for &t in times {
        seek(app.world_mut(), anim_root, node, t);
        app.update();
        if let Some((lo, hi)) = world_bounds(app.world_mut()) {
            bounds = Some(match bounds {
                Some((l, h)) => (l.min(lo), h.max(hi)),
                None => (lo, hi),
            });
        }
    }
    bounds.unwrap_or((Vec3::ZERO, Vec3::splat(1.0)))
}

/// The centre and radius of a sphere enclosing `lo..hi`.
fn enclosing(lo: Vec3, hi: Vec3) -> (Vec3, f32) {
    ((lo + hi) * 0.5, ((hi - lo).length() * 0.5).max(0.05))
}

/// Fraction of a standing subject's height taken up by the head.
///
/// The classical eight-heads figure, used only when there is no `Head` bone to
/// ask — see [`head_sphere`].
const HEAD_FRACTION: f32 = 0.125;

/// Where a head close-up should look, and how wide to frame it.
///
/// **The `Head` bone decides, not the bounding box.** Deriving the focus from
/// the whole-body bounds put it 5 cm behind the face: the clip-posed subject
/// faces −Z and the Z extent is dominated by the *feet*, so the mid-point sits
/// behind the skull and a camera aimed there renders a head that is too close
/// and cropped. The bone is exactly where the head is, and the mesh's top gives
/// its size — so the frame is measured on both axes rather than assumed on
/// either, and it follows whatever pose the clip is in.
///
/// The fallback matters for a garment or a prop, which have no head at all: it
/// keeps the sheet renderable rather than refusing, and the cells honestly show
/// whatever is at the top of the subject.
fn head_sphere(app: &mut CaptureApp, lo: Vec3, hi: Vec3) -> (Vec3, f32) {
    let world = app.world_mut();
    let mut bones = world.query::<(&Name, &GlobalTransform)>();
    let head = bones
        .iter(world)
        .find(|(name, _)| name.as_str() == "Head")
        .map(|(_, at)| at.translation());

    let Some(head) = head else {
        let span = (hi.y - lo.y).max(0.1) * HEAD_FRACTION;
        return (
            Vec3::new((lo.x + hi.x) * 0.5, hi.y - span * 0.5, (lo.z + hi.z) * 0.5),
            span * 0.62,
        );
    };
    // How far the crown sits above the joint — the only head measure bones
    // can give. The stylized register pins the crown and grows the skull
    // DOWN (crown ~1.80, joint ~1.68, chin near the shoulder line), so the
    // joint sits in the upper skull, not mid-head: treating
    // crown-to-joint as the half height (the previous rule) framed a bare
    // forehead. Taking the head as ~2.5× the crown-to-joint distance holds
    // for the stylized register and merely over-frames a realistic head a
    // little — and a loose close-up beats a cropped chin.
    let above = (hi.y - head.y).max(0.03);
    let half = above * 1.25;
    (
        // Centre between crown and estimated chin, nudged a little forward
        // (−Z is the way a clip-posed subject faces) so the frame centres on
        // the face rather than on the skull's axis.
        Vec3::new(head.x, hi.y - half, head.z - half * 0.15),
        // Margin past the half-height, so the frame holds the whole head
        // plus a little collar — which is what makes a jaw line judgeable.
        half * 1.3,
    )
}

/// Three close-ups of the subject's head, as one extra band.
///
/// Front, three-quarter and profile — the same three the portrait example
/// settled on, and the same [`View`] transforms the body bands use, only
/// framed on a sphere a tenth the size. Nothing here is a second camera
/// implementation: a close-up is the ordinary framing pointed somewhere else.
fn capture_head_row(
    app: &mut CaptureApp,
    anim_root: Entity,
    node: AnimationNodeIndex,
    time: f32,
    focus: Vec3,
    radius: f32,
) -> Result<Vec<SheetCell>, RenderError> {
    const FACES: [View; 3] = [View::Front, View::ThreeQuarter, View::Left];
    let mut cells = Vec::with_capacity(FACES.len());
    seek(app.world_mut(), anim_root, node, time);
    for view in FACES {
        // The subject is posed by a baked clip, which plays facing −Z.
        let transform =
            view.camera_transform(Facing::MinusZ, focus, radius, CAMERA_FOV_DEG, FRAME_PADDING);
        cells.push(capture_cell(
            app,
            transform,
            format!("FACE {}", view.name().to_ascii_uppercase()),
        )?);
    }
    Ok(cells)
}

/// Capture every pose from every requested view, one band per view.
fn capture_poses(
    app: &mut CaptureApp,
    anim_root: Entity,
    node: AnimationNodeIndex,
    request: &SheetRequest,
    times: &[f32],
    centre: Vec3,
    radius: f32,
) -> Result<Vec<SheetCell>, RenderError> {
    let mut cells = Vec::with_capacity(times.len() * request.views.len().max(1));
    for &view in &request.views {
        // Sheets always pose a clip, and a baked clip plays facing −Z.
        let transform = view.camera_transform(
            Facing::MinusZ,
            centre,
            radius,
            CAMERA_FOV_DEG,
            FRAME_PADDING,
        );
        let camera = place_camera(app, transform);

        for (index, &t) in times.iter().enumerate() {
            seek(app.world_mut(), anim_root, node, t);
            let image = app
                .capture()
                .map_err(|err| RenderError::Capture(err.to_string()))?;
            cells.push(SheetCell {
                image,
                label: format!("#{index} {t:.2}S {}", view.name().to_ascii_uppercase()),
            });
        }
        app.world_mut().entity_mut(camera).despawn();
    }
    Ok(cells)
}

/// Evenly spaced sample times across a fraction of the clip.
fn sample_times(duration: f32, frames: u32, t0: f32, t1: f32) -> Vec<f32> {
    let frames = frames.max(1);
    let (a, b) = (t0.clamp(0.0, 1.0), t1.clamp(0.0, 1.0));
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    let start = duration * a;
    let end = duration * b;
    if frames == 1 {
        return vec![start];
    }
    (0..frames)
        .map(|i| start + (end - start) * (i as f32 / (frames - 1) as f32))
        .collect()
}

fn wait_for_assets(
    app: &mut CaptureApp,
    model_entity: Entity,
    scene: &Handle<WorldAsset>,
    clip: Option<&Handle<AnimationClip>>,
) -> Result<(), RenderError> {
    for _ in 0..MAX_LOAD_FRAMES {
        app.update();
        let (scene_state, clip_state) = {
            let server = app.world_mut().resource::<AssetServer>();
            (
                server.get_load_state(scene),
                clip.and_then(|c| server.get_load_state(c)),
            )
        };
        for (what, state) in [("model", &scene_state), ("clip", &clip_state)] {
            if let Some(LoadState::Failed(err)) = state {
                return Err(RenderError::AssetLoad(format!(
                    "{what} failed to load: {err}"
                )));
            }
        }
        let spawned = app
            .world_mut()
            .get::<Children>(model_entity)
            .is_some_and(|c| !c.is_empty());
        let clip_ready = clip.is_none() || matches!(clip_state, Some(LoadState::Loaded));
        if spawned && clip_ready {
            return Ok(());
        }
    }
    Err(RenderError::AssetLoad(format!(
        "assets did not load within {MAX_LOAD_FRAMES} frames"
    )))
}

fn adapter_name(app: &mut CaptureApp) -> String {
    app.world_mut()
        .get_resource::<RenderAdapterInfo>()
        .map_or_else(|| String::from("<unknown>"), |info| info.name.clone())
}

fn short(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_times_span_the_window_inclusively() {
        let times = sample_times(2.0, 5, 0.0, 1.0);
        assert_eq!(times.len(), 5);
        assert!((times[0]).abs() < 1e-6);
        assert!((times[4] - 2.0).abs() < 1e-6);
        // A reversed or out-of-range window is put right rather than refused.
        let times = sample_times(2.0, 2, 1.5, 0.25);
        assert!((times[0] - 0.5).abs() < 1e-6);
        assert!((times[1] - 2.0).abs() < 1e-6);
        assert_eq!(sample_times(2.0, 0, 0.5, 0.5), [1.0]);
    }

    #[test]
    fn a_stage_from_a_path_splits_root_and_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let glb = dir.path().join("lift.glb");
        std::fs::write(&glb, b"glb").expect("write");
        let stage = Stage::from_path(&glb).expect("a file");
        assert_eq!(stage.model, "lift.glb");
        assert_eq!(stage.scene_path(), "lift.glb#Scene0");
        assert!(stage.absolute_root().is_absolute());
        assert!(Stage::from_path(&dir.path().join("missing.glb")).is_err());
    }

    #[test]
    fn a_shot_fails_on_mismatch_before_frozen_and_never_without_a_clip() {
        let blank = |diff: Option<ClipDiff>, frozen: bool| Shot {
            sheet: Image::default(),
            duration: 0.0,
            times: Vec::new(),
            cell: UVec2::new(1, 1),
            clamped: false,
            cells: 0,
            adapter: String::new(),
            diff,
            frozen,
            bounds: (Vec3::ZERO, Vec3::ONE),
        };
        assert!(blank(None, false).failure().is_none());
        assert!(
            blank(None, true)
                .failure()
                .is_some_and(|f| f.contains("identical"))
        );
        let mismatch = ClipDiff::default();
        assert!(
            blank(Some(mismatch), true)
                .failure()
                .is_some_and(|f| f.contains("none"))
        );
        let bound = ClipDiff {
            bound: vec![String::from("Armature/Hips")],
            ..ClipDiff::default()
        };
        assert!(blank(Some(bound), false).failure().is_none());
    }
}
