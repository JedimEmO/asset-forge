//! The studio: one window that is both viewer and monitor for a library.
//!
//! It opens on the library, plays what you pick — a clip on the shared stage, a
//! sound through its bus — and keeps watching the library while it is open,
//! so a promote from the CLI or an export from Blender shows up in the column
//! without a restart. What it does *not* do is make anything: generating,
//! editing and shipping go through `forge` and the skills, and the window is
//! where a person looks at the result — one door per job, and this window is
//! the looking door.
//!
//! # Layout
//!
//! [`shell`] owns the furniture: the library column, the transport bar with
//! its context cluster, the status line, and the slot [`audio_view`] fills.
//! Everything it draws is built from [`widgets`], so a button behaves the
//! same wherever it appears.
//!
//! The middle is the one part that changes shape. For a clip it holds no UI at
//! all, so a drag there orbits the camera over the stage; for a sound it holds
//! the plot, which is opaque and registers as UI so that dragging *it* does
//! not spin a model nobody can see. [`metadata`] describes whichever is
//! selected, from the same typed record either way.
//!
//! # State
//!
//! Ten resources, deliberately small and separately borrowable, because a
//! Bevy system that needs the playhead should not have to take a write lock on
//! the whole application:
//!
//! * [`rig::Rig`] — the model, and the facts only readable while it loads.
//! * [`rig::ActiveModel`] — which model that is, and how many have stood up.
//! * [`rig::StageContract`] — the profile's contract the model is judged by.
//! * [`RigFindings`](crate::rig_findings::RigFindings) — what the contract
//!   makes of it, re-read every time one stands up.
//! * [`library::ModelLibrary`] — the bodies and models there are to choose
//!   between.
//! * [`library::ClipLibrary`] — everything playable, shipped or made here.
//! * [`audio_view::AudioLibrary`] — every sound, and what it measures.
//! * [`library::Selection`] — which of it is showing, named rather than
//!   numbered, and across both libraries.
//! * [`library::LibraryView`] — what the browser is filtered to and showing.
//! * [`playback::Playback`] — the playhead, for a clip and for a sound alike.
//!
//! # System order
//!
//! Each panel's systems are `.chain()`ed in one order throughout: input, then
//! state, then playback, then refresh. Reading input after drawing would show
//! the previous frame's answer to this frame's click, and the two-frame lag is
//! plainly visible on a slider.

pub mod audio_view;
pub mod library;
pub mod metadata;
pub mod playback;
pub mod rig;
pub mod shell;
pub mod widgets;

use std::path::PathBuf;

use bevy::{
    asset::io::{AssetSourceBuilder, file::FileAssetReader},
    input_focus::tab_navigation::TabNavigationPlugin,
    prelude::*,
};
use forge_library::Project;

use crate::{
    orbit::{PointerOverUi, orbit_camera},
    rig_findings,
    stage::{self, spawn_stage},
    viewer::{AutoScreenshot, auto_screenshot, window_title},
};

/// The asset source the project's `out/` directory is registered as, so a
/// file there can stand on the stage as `out://fixture/mannequin.glb`.
///
/// Bevy loads nothing from outside an asset root — an absolute path or a
/// `../` never spawns and never fails, it just sits there — and the window's
/// root is the library. A raw export, or the fixture mannequin a project
/// with no body falls back to, lives under `out/`, which is the one other
/// place a thing worth looking at comes from.
pub const OUT_SOURCE: &str = "out";

/// What the studio should open.
#[derive(Resource, Debug, Clone)]
pub struct StudioConfig {
    /// The project whose library the window shows. Its asset directory is
    /// Bevy's asset root, so every path the window loads is relative to it.
    pub project: Project,
    /// The body or model to open on: a name, a file name or a path relative
    /// to the asset root, as the browser resolves them; or `out://<path>`
    /// for a file under the project's `out/` ([`OUT_SOURCE`]).
    ///
    /// `None` opens on the project's `stage_body`, then the first body, then
    /// the first model. A library with no meshes at all opens on an empty
    /// stage — demanding a rigged character before a bark can be auditioned
    /// would be a requirement invented by the implementation.
    pub model: Option<String>,
    /// Open on the first sound rather than the first clip.
    pub audio: bool,
    /// A raw take to put on the stage body, beside the shipped clips.
    ///
    /// What makes a generated candidate previewable the instant it exists,
    /// with no Blender in the loop: the take is materialised through
    /// [`crate::npz_clip`] on the real body, selected, and plays.
    pub take: Option<PathBuf>,
    /// A recipe to apply to the take once, at load — a bare
    /// [`forge_library::ClipRecipe`] or a sidecar holding one. Ignored
    /// without `take`.
    pub recipe: Option<PathBuf>,
    /// Capture the window to this PNG once the scene has settled, then quit.
    pub screenshot: Option<PathBuf>,
    /// Play every audio asset in turn, then quit.
    pub selftest: bool,
}

/// Runs the studio window.
pub struct StudioPlugin;

impl Plugin for StudioPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<TabNavigationPlugin>() {
            // Not in DefaultPlugins. Without it Tab does nothing and, more
            // importantly, clicking a text field never focuses it. Focus is
            // the shell's business; the filter is the one field that needs it.
            app.add_plugins(TabNavigationPlugin);
        }
        app.init_resource::<rig::Rig>()
            .init_resource::<rig::ActiveModel>()
            .init_resource::<rig::StageContract>()
            // The stage's mesh is swappable at runtime; see `rig` for the whole
            // contract. Registered here rather than in the panel that will ask
            // for a swap, because the message is the studio's, not a panel's.
            .add_message::<rig::SwapModel>()
            .init_resource::<rig_findings::RigFindings>()
            .init_resource::<library::ClipLibrary>()
            .init_resource::<library::ModelLibrary>()
            .init_resource::<library::Selection>()
            .init_resource::<library::LibraryView>()
            .init_resource::<audio_view::AudioLibrary>()
            .init_resource::<playback::Playback>()
            .init_resource::<shell::StatusLine>()
            .init_resource::<PointerOverUi>()
            .add_systems(
                Startup,
                (
                    spawn_stage,
                    rig::spawn_camera,
                    rig::load_contract,
                    library::discover,
                    rig::open_model,
                    library::focus_first,
                    shell::build,
                    library::build_browser,
                    audio_view::build_panel,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    // Ahead of the model standing up: a material-free
                    // primitive wears the loader's glossy default until this
                    // restyles it, and the stage exists to show what ships.
                    stage::stylize_gltf_default_material,
                    // A swap asked for last frame starts before the model in
                    // flight is driven, so a request costs one frame at most.
                    // The contract findings are read in the same frame the rig
                    // stands up and before anything animates it, which is a
                    // requirement and not a preference: see `rig_findings`.
                    (
                        rig::request_swap,
                        rig::finish_loading,
                        rig::refresh_findings,
                        rig::draw_skeleton,
                    )
                        .chain(),
                    // The four phases, each chained internally and to the next.
                    // Bevy's tuples cap out at twenty, and the grouping is
                    // what the order means anyway.
                    (
                        widgets::track_pointer_over_ui,
                        widgets::colour_clickables,
                        widgets::scroll_lists,
                        orbit_camera,
                        shell::transport_buttons,
                        shell::scrub,
                        library::row_buttons,
                        library::collapse_headers,
                        library::tag_chips,
                        audio_view::bus_buttons,
                        playback::keyboard,
                        audio_view::self_test,
                    )
                        .chain(),
                    (
                        library::filter_field,
                        library::open_take,
                        audio_view::decode_selected,
                        library::rescan_models,
                        library::notice_changes,
                        // After everything that can move the selection, and
                        // before the rows are rebuilt: a fold that opened this
                        // frame has to be open in the list the reveal then
                        // looks through.
                        library::expand_to_selection,
                    )
                        .chain(),
                    (
                        playback::follow_selection,
                        playback::advance,
                        audio_view::drive_playback,
                    )
                        .chain(),
                    (
                        shell::refresh_context,
                        library::rebuild_rows,
                        library::refresh,
                        library::stage_status,
                        library::reveal_selection,
                        metadata::refresh,
                        audio_view::refresh,
                        shell::refresh_transport,
                        auto_screenshot,
                    )
                        .chain(),
                )
                    .chain(),
            );
    }
}

/// Frames the window is given to load the model and every clip before a
/// `--screenshot` is taken; the viewer shows a loading hint until then.
const SCREENSHOT_AFTER_FRAMES: u32 = 180;
/// Frames each sound is held for under `--selftest`.
const SELFTEST_FRAMES_PER_FILE: u32 = 30;

/// Open the studio window and run it until it is closed.
///
/// A separate `App` from the headless path rather than a shared one: this
/// needs winit and a real window, which is exactly what
/// [`forge_capture::CaptureApp`] disables. Exits non-zero when the project's
/// asset directory does not exist, because Bevy would otherwise open an empty
/// window and report every load as "path not found".
#[must_use]
pub fn run_studio(config: StudioConfig) -> std::process::ExitCode {
    // Bevy resolves a *relative* asset root against the executable's own
    // location, not the shell's working directory — so `assets` run from a
    // project root silently becomes `target/debug/assets` and every load
    // fails with a confusing "path not found". The project's paths are
    // absolute whenever it was discovered from an absolute start, and this
    // makes them so regardless.
    let assets = std::path::absolute(&config.project.assets)
        .unwrap_or_else(|_| config.project.assets.clone());
    if !assets.is_dir() {
        eprintln!("error: asset root {} is not a directory", assets.display());
        return std::process::ExitCode::FAILURE;
    }

    let title = window_title(&config.project.name, config.model.as_deref());
    let screenshot = config.screenshot.clone();
    let selftest = config.selftest;
    let out =
        std::path::absolute(&config.project.out).unwrap_or_else(|_| config.project.out.clone());

    let mut app = App::new();
    // Sources have to exist before AssetPlugin builds them. `out/` is read
    // only: nothing the window does writes there.
    app.register_asset_source(
        OUT_SOURCE,
        AssetSourceBuilder::new(move || Box::new(FileAssetReader::new(out.clone()))),
    );
    app.add_plugins(
        DefaultPlugins
            .set(bevy::asset::AssetPlugin {
                file_path: assets.to_string_lossy().into_owned(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title,
                    resolution: (1280u32, 800u32).into(),
                    ..default()
                }),
                ..default()
            }),
    )
    .insert_resource(config)
    .add_plugins(StudioPlugin);

    if let Some(path) = screenshot {
        app.insert_resource(AutoScreenshot {
            path,
            after_frames: SCREENSHOT_AFTER_FRAMES,
        });
    }
    if selftest {
        app.insert_resource(audio_view::AudioSelfTest {
            frames_per_file: SELFTEST_FRAMES_PER_FILE,
        });
    }

    app.run();
    std::process::ExitCode::SUCCESS
}
