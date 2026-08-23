//! Audio in the studio: hearing a file, seeing it, and reading what it measures.
//!
//! The plot and the numbers tell you what is wrong. This tells you whether it
//! *sounds* wrong, which is the one judgement no measurement makes for you —
//! and it plays through per-bus gains so a bark is auditioned at the level it
//! will actually ship at, not solo at full scale.
//!
//! # Why decoding is lazy
//!
//! The window this replaces decoded the entire library at startup, which was
//! fine at eleven files and is seconds of stalled window at a few hundred. The
//! rows do not need samples anyway: duration and loudness come out of
//! [`forge_library::metrics_cache`], which is keyed on the file's size and
//! mtime and survives between runs. Samples are needed only to *draw* the
//! file, so it is decoded the first time it is selected, and what that decode
//! measures is written back to the cache for the next run.
//!
//! # Why the picture is the same one `forge audio` draws
//!
//! The centre of the window shows [`forge_audio::render`]'s plot — waveform
//! over log-frequency spectrogram, the numbers across the top — rendered to
//! an image at decode time. The old view drew its own envelope out of UI
//! nodes, which was a second waveform implementation with its own idea of
//! what clipping looks like; an agent judging a file from the headless plot
//! and a person judging it in the window now look at one picture, and each
//! can be used to check the other — the same reason the stage is shared.
//!
//! # Why the centre panel is opaque and registers as UI
//!
//! It sits over the 3D stage, where a drag orbits the camera. A translucent
//! panel there would make a moving character read as noise behind the plot,
//! and a panel that does not register with [`crate::orbit::PointerOverUi`] would
//! let a drag *on the waveform* spin the model behind it.

use std::path::{Path, PathBuf};

use bevy::{
    asset::RenderAssetUsages,
    audio::{AudioSink, AudioSinkPlayback, PlaybackSettings, Volume},
    ecs::hierarchy::ChildSpawnerCommands,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use forge_audio::PlotLayout;
use forge_library::{
    AssetRecord, Catalog, Project, Sidecar,
    metrics_cache::{AudioMeasurement, MetricsCache},
    schema::Kind,
};

use crate::{
    studio::{
        library::{AssetKey, FileStamp, Selection},
        playback::Playback,
        shell::CentreSlot,
        widgets,
    },
    theme,
};

/// Extensions Bevy's audio stack can actually decode, given the features
/// enabled in the workspace manifest.
///
/// This list exists because `bevy_audio` builds its rodio decoder with
/// `.unwrap()`: handing it a format it was not compiled for panics a task pool,
/// taking the whole app down, rather than returning an error anyone can show.
/// `forge_audio` decodes a wider set than Bevy plays, so a file can be
/// perfectly inspectable and still unplayable — the studio says so instead of
/// dying.
const PLAYABLE: [&str; 4] = ["wav", "ogg", "mp3", "flac"];

/// Consecutive full-scale samples before a file counts as clipped.
///
/// Mirrors `forge_audio`'s own threshold, which is private to it. Isolated
/// full-scale samples are what peak normalisation looks like; a *run* of them
/// is audible distortion.
const CLIPPED_RUN: usize = 3;

/// Fraction of the panel's width the plot occupies, as a percentage.
const PLOT_WIDTH_PCT: f32 = 94.0;

/// The plot as the window draws it: narrower than the headless default so
/// the text stays legible once it is scaled to the panel, and at text scale 1
/// for the same reason — the numbers are on the metadata panel anyway.
fn plot_layout() -> PlotLayout {
    PlotLayout {
        width: 1024,
        wave_height: 220,
        spec_height: 260,
        header_height: 40,
        text_scale: 1,
        ..PlotLayout::default()
    }
}

/// A gain group, standing in for a mixer bus.
///
/// Bevy has no bus concept, so the group comes from the asset's kind — which is
/// how the library is already organised, and means auditioning a sound applies
/// the same relative level the game will.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bus {
    /// Music.
    Music,
    /// Sound effects.
    Sfx,
    /// Dialogue.
    Voice,
}

impl Bus {
    /// Every bus, in the order the transport shows them.
    const ALL: [Self; 3] = [Self::Music, Self::Sfx, Self::Voice];

    /// The bus a kind of asset plays on.
    #[must_use]
    pub const fn of_kind(kind: Kind) -> Self {
        match kind {
            Kind::Music => Self::Music,
            Kind::Voice => Self::Voice,
            // A clip and a mesh have no bus; neither ever reaches a sink.
            Kind::Sfx | Kind::Clip | Kind::Body | Kind::Model => Self::Sfx,
        }
    }

    /// The label shown on its gain buttons.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Music => "MUSIC",
            Self::Sfx => "SFX",
            Self::Voice => "VOICE",
        }
    }

    const fn slot(self) -> usize {
        match self {
            Self::Music => 0,
            Self::Sfx => 1,
            Self::Voice => 2,
        }
    }
}

/// One audio asset, and whatever is known about it so far.
pub struct AudioAsset {
    /// What names it in the selection.
    pub key: AssetKey,
    /// File stem, for display.
    pub name: String,
    /// Path relative to the asset root, which is also what the asset server is
    /// asked for and what the metrics cache is keyed on.
    pub rel_path: String,
    /// Absolute path, for decoding.
    pub path: PathBuf,
    /// Which library it belongs to, from where it lives.
    pub kind: Kind,
    /// The loaded source, or a handle that will resolve to one.
    pub handle: Handle<AudioSource>,
    /// False when Bevy cannot decode this format; it is still measurable.
    pub playable: bool,
    /// The record beside it, when there is one.
    pub sidecar: Option<Sidecar>,
    /// What it measures. From the cache at startup, from a decode after that.
    pub measurement: Option<AudioMeasurement>,
    /// The rendered plot, once a decode has drawn it.
    pub plot: Option<Handle<Image>>,
    /// Whether this file has been decoded in this session.
    pub decoded: bool,
    /// Why it could not be decoded, when it could not.
    pub decode_error: Option<String>,
    /// The file as it was when last scanned, so a rescan can tell a rebake.
    stamp: Option<FileStamp>,
}

impl AudioAsset {
    /// Which gain applies to it.
    #[must_use]
    pub const fn bus(&self) -> Bus {
        Bus::of_kind(self.kind)
    }

    /// Length in seconds, or zero while nothing has measured it.
    #[must_use]
    pub fn duration(&self) -> f32 {
        self.measurement.as_ref().map_or(0.0, |m| m.duration)
    }

    /// The cached measurements, in the space a browser row has for them.
    #[must_use]
    pub fn detail(&self) -> String {
        self.measurement.as_ref().map_or_else(String::new, |m| {
            format!("{:.1}s {:.0} LUFS", m.duration, m.loudness_lufs)
        })
    }

    /// Whether what was measured is a fault rather than a fact.
    ///
    /// Only the two that mean the file is broken — silence and flat-topping.
    /// The softer warnings (a quiet master, a long lead-in) belong in the panel
    /// where there is room to say what they mean, not as a colour on a row that
    /// would then be orange half the library over.
    #[must_use]
    pub fn is_faulty(&self) -> bool {
        self.measurement
            .as_ref()
            .is_some_and(|m| m.silent || m.longest_clip_run >= CLIPPED_RUN)
    }

    /// Everything worth a human's attention about this file.
    ///
    /// Rebuilt from the measurement rather than stored, so a file served
    /// entirely out of the cache still reports its problems without decoding.
    #[must_use]
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(error) = &self.decode_error {
            out.push(error.clone());
        }
        if let Some(measurement) = &self.measurement {
            out.extend(as_metrics(measurement).warnings());
        }
        if !self.playable {
            out.push(format!(
                "cannot play {} in-engine (measurement still works)",
                Path::new(&self.rel_path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("this format")
            ));
        }
        out
    }

    /// Forget everything derived from the bytes, because the bytes changed.
    fn reset(&mut self) {
        self.measurement = None;
        self.plot = None;
        self.decoded = false;
        self.decode_error = None;
    }
}

/// Every sound the studio can play, and the per-bus gains it plays them at.
#[derive(Resource)]
pub struct AudioLibrary {
    assets: Vec<AudioAsset>,
    /// Per-bus gain, indexed by [`Bus::slot`].
    gains: [f32; 3],
    /// The measured-metrics cache, absent until the first scan.
    cache: Option<MetricsCache>,
}

impl Default for AudioLibrary {
    fn default() -> Self {
        Self {
            assets: Vec::new(),
            // Where the shipped audio actually sits at 0.7 rather than 1.0:
            // music under a bark is a mix decision, and auditioning at unity
            // would flatter every track.
            gains: [0.7, 1.0, 1.0],
            cache: None,
        }
    }
}

impl AudioLibrary {
    /// Everything found, in path order.
    #[must_use]
    pub fn assets(&self) -> &[AudioAsset] {
        &self.assets
    }

    /// The asset `key` names.
    #[must_use]
    pub fn get(&self, key: &AssetKey) -> Option<&AudioAsset> {
        self.assets.iter().find(|asset| asset.key == *key)
    }

    /// The asset currently selected, if the selection names one.
    #[must_use]
    pub fn selected<'a>(&'a self, selection: &Selection) -> Option<&'a AudioAsset> {
        selection.key().and_then(|key| self.get(key))
    }

    /// How many assets have a measurement, cached or decoded.
    ///
    /// The browser watches this: a row gains its duration and loudness the
    /// moment the file is first measured, and that has to redraw the list.
    #[must_use]
    pub fn measured(&self) -> usize {
        self.assets
            .iter()
            .filter(|asset| asset.measurement.is_some())
            .count()
    }

    /// The gain a bus plays at, 0.0..=1.5.
    #[must_use]
    pub fn gain(&self, bus: Bus) -> f32 {
        self.gains[bus.slot()]
    }

    fn nudge(&mut self, bus: Bus, delta: f32) {
        let next = (self.gain(bus) + delta).clamp(0.0, 1.5);
        self.gains[bus.slot()] = next;
    }

    /// Add a sound, returning the key naming it.
    ///
    /// The stem is made unique here rather than at the call site for the reason
    /// [`crate::studio::library::ClipLibrary::push`] does it: `bark` under
    /// `sfx/` and `bark` under `voice/` are two files, and a key that matched
    /// both would make the selection ambiguous.
    fn push(&mut self, record: &AssetRecord, server: &AssetServer) -> AssetKey {
        if let Some(err) = &record.sidecar_error {
            warn!("{}: {err}", record.name);
        }
        let key = AssetKey::audio(self.unique_stem(&record.name));
        let measurement = self
            .cache
            .as_ref()
            .and_then(|c| c.get(&record.rel_path, &record.path))
            .copied();
        self.assets.push(AudioAsset {
            key: key.clone(),
            name: record.name.clone(),
            handle: server.load(record.rel_path.clone()),
            playable: is_playable(&record.rel_path),
            rel_path: record.rel_path.clone(),
            path: record.path.clone(),
            kind: record.kind,
            sidecar: record.sidecar.clone(),
            measurement,
            plot: None,
            decoded: false,
            decode_error: None,
            stamp: FileStamp::of(&record.path),
        });
        key
    }

    fn unique_stem(&self, base: &str) -> String {
        if !self.assets.iter().any(|asset| asset.key.stem == base) {
            return base.to_owned();
        }
        (2..=self.assets.len() + 1)
            .map(|n| format!("{base}#{n}"))
            .find(|stem| !self.assets.iter().any(|asset| asset.key.stem == *stem))
            .unwrap_or_else(|| base.to_owned())
    }
}

/// Play every audio asset in turn, then quit.
///
/// Exists because the obvious way to "verify" a player — screenshot it — proves
/// nothing about playback: the window can be paused, so a decoder that panics
/// on a format never gets the chance. This actually spawns a sink per file.
#[derive(Resource, Debug, Clone)]
pub struct AudioSelfTest {
    /// Frames to hold each file before moving on.
    pub frames_per_file: u32,
}

// ---------------------------------------------------------------- markers ---

/// The plot image in the centre of the window.
#[derive(Component)]
pub(super) struct PlotImage;
/// The line showing where playback has reached.
#[derive(Component)]
pub(super) struct Playhead;
/// A per-bus gain stepper: which bus, and by how much.
#[derive(Component)]
pub(super) struct BusButton(Bus, f32);
/// The label showing one bus's current gain.
#[derive(Component)]
pub(super) struct BusReadout(Bus);
/// The entity holding the sink that is currently sounding.
#[derive(Component)]
pub(super) struct NowPlaying;

// ------------------------------------------------------------------ setup ---

/// Bring the sounds in step with what the catalog found, and say whether
/// anything changed.
///
/// Nothing is decoded here. That is the whole point: a library of hundreds
/// answers this in the time it takes to stat the files, with the numbers the
/// cache already holds. Called once at startup and again on every rescan: a
/// sound that arrived is listed, one whose bytes changed is reloaded under
/// its handle and forgets what it measured, and one that is gone leaves.
pub(super) fn absorb_catalog(
    library: &mut AudioLibrary,
    catalog: &Catalog,
    project: &Project,
    server: &AssetServer,
) -> bool {
    let first = library.cache.is_none();
    if first {
        library.cache = Some(MetricsCache::load(project));
    }
    let mut changed = false;
    let mut seen: Vec<String> = Vec::new();
    for record in catalog.records().iter().filter(|r| r.kind.is_audio()) {
        seen.push(record.rel_path.clone());
        let stamp = FileStamp::of(&record.path);
        if let Some(asset) = library
            .assets
            .iter_mut()
            .find(|a| a.rel_path == record.rel_path)
        {
            if asset.stamp != stamp {
                info!("{} changed on disk: reloading", record.rel_path);
                server.reload(record.rel_path.clone());
                asset.stamp = stamp;
                asset.reset();
                asset.sidecar.clone_from(&record.sidecar);
                changed = true;
            } else if asset.sidecar != record.sidecar {
                asset.sidecar.clone_from(&record.sidecar);
                changed = true;
            }
            continue;
        }
        if !first {
            info!("{} arrived", record.rel_path);
        }
        library.push(record, server);
        changed = true;
    }
    let before = library.assets.len();
    library
        .assets
        .retain(|asset| seen.contains(&asset.rel_path));
    changed |= library.assets.len() != before;

    // A cache that has seen a year of sweeps should not carry a year of
    // ghosts; this is the only place that knows what still exists.
    if first && let Some(cache) = &mut library.cache {
        cache.retain(&seen);
    }
    changed
}

/// Whether Bevy can be trusted to decode this file without panicking.
fn is_playable(rel_path: &str) -> bool {
    Path::new(rel_path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PLAYABLE.contains(&e.to_ascii_lowercase().as_str()))
}

/// Fill the centre slot with the plot, hidden until a sound is selected.
pub(super) fn build_panel(mut commands: Commands, slot: Query<Entity, With<CentreSlot>>) {
    let Ok(slot) = slot.single() else {
        return;
    };
    let layout = plot_layout();
    let aspect = layout.width as f32 / layout.total_height() as f32;
    commands
        .entity(slot)
        .insert((
            Node {
                display: Display::None,
                flex_grow: 1.0,
                // The plot is a texture with a size of its own, and a flex
                // item's floor is its content's size unless told otherwise:
                // without these three the slot grew to the image and pushed
                // the metadata column off the window.
                flex_basis: Val::Px(0.0),
                min_width: Val::Px(0.0),
                overflow: Overflow::clip(),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            // Opaque, and darker than the panels either side: the plot is
            // the subject here, not a label over a scene.
            BackgroundColor(Color::srgb(0.05, 0.05, 0.07)),
            widgets::UiPanel,
        ))
        .with_children(|centre| {
            centre
                .spawn(Node {
                    width: Val::Percent(PLOT_WIDTH_PCT),
                    aspect_ratio: Some(aspect),
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    ..default()
                })
                .with_children(|frame| {
                    frame.spawn((
                        PlotImage,
                        // Stretched to the frame rather than sized from the
                        // texture: the frame already has the plot's aspect,
                        // and the texture's own pixel count is not a layout
                        // request.
                        ImageNode::default().with_mode(NodeImageMode::Stretch),
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.09, 0.09, 0.12)),
                    ));
                    frame.spawn((
                        Playhead,
                        Node {
                            position_type: PositionType::Absolute,
                            width: Val::Px(2.0),
                            height: Val::Percent(100.0),
                            left: Val::Percent(0.0),
                            top: Val::Px(0.0),
                            ..default()
                        },
                        BackgroundColor(theme::WARN),
                    ));
                });
        });
}

/// The per-bus gain steppers, for the transport's context cluster.
///
/// Stepped buttons rather than sliders: the point is to audition at the mix
/// level, not to dial in a final value.
pub(super) fn build_bus_cluster(bar: &mut ChildSpawnerCommands) {
    for bus in Bus::ALL {
        bar.spawn((
            BusButton(bus, -0.1),
            widgets::button(
                format!("{}-", bus.name()),
                theme::FONT_SMALL,
                theme::TEXT,
                widgets::chip(),
            ),
        ));
        bar.spawn((
            BusReadout(bus),
            Node {
                width: Val::Px(34.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            children![theme::label("", theme::FONT_SMALL, theme::TEXT_DIM)],
        ));
        bar.spawn((
            BusButton(bus, 0.1),
            widgets::button("+", theme::FONT_SMALL, theme::TEXT, widgets::chip()),
        ));
    }
}

// ------------------------------------------------------------------ input ---

/// The gain steppers.
pub(super) fn bus_buttons(
    buttons: Query<(&Interaction, &BusButton), Changed<Interaction>>,
    mut library: ResMut<AudioLibrary>,
) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            library.nudge(button.0, button.1);
        }
    }
}

// --------------------------------------------------------------- decoding ---

/// Decode the selected sound, once, the first time it is looked at.
///
/// Synchronous on purpose. One file is tens of milliseconds — a frame, maybe
/// two — where decoding the whole library up front was seconds of dead window,
/// and a worker thread here would buy a stutter's worth of latency at the cost
/// of a second copy of the "is it ready yet" problem the rig already has.
pub(super) fn decode_selected(
    selection: Res<Selection>,
    mut library: ResMut<AudioLibrary>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(index) = selection
        .key()
        .and_then(|key| library.assets.iter().position(|asset| asset.key == *key))
    else {
        return;
    };
    if library.assets[index].decoded {
        return;
    }
    library.assets[index].decoded = true;

    let path = library.assets[index].path.clone();
    let rel_path = library.assets[index].rel_path.clone();
    let name = library.assets[index].name.clone();
    match forge_audio::decode(&path) {
        Ok(audio) => {
            let metrics = forge_audio::measure(&audio);
            let measurement = AudioMeasurement::from(&metrics);
            let plot = images.add(plot_image(&audio, &metrics, &name));
            if let Some(cache) = &mut library.cache {
                cache.insert(&rel_path, &path, measurement);
                // Losing the cache costs only time, so a failure to write it is
                // not worth interrupting anyone over.
                if let Err(err) = cache.save() {
                    debug!("could not write the metrics cache: {err}");
                }
            }
            let asset = &mut library.assets[index];
            asset.measurement = Some(measurement);
            asset.plot = Some(plot);
        }
        // Recorded rather than logged and forgotten: a file the studio cannot
        // decode is exactly what the metadata panel is for.
        Err(err) => library.assets[index].decode_error = Some(err.to_string()),
    }
}

/// The headless plot, as a texture the UI can show.
fn plot_image(audio: &forge_audio::Audio, metrics: &forge_audio::Metrics, name: &str) -> Image {
    let canvas = forge_audio::render(audio, metrics, &name.to_ascii_uppercase(), &plot_layout());
    let (width, height) = (canvas.width(), canvas.height());
    Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        canvas.into_data(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

/// The cache's numbers as `forge_audio` would report them, so a file served
/// out of the cache can still be asked what is wrong with it without being
/// decoded again.
///
/// Field for field, and deliberately hand-written: the library crate owns the
/// one direction (`From<&Metrics>`) and this crate, the one with the decoder,
/// owns the way back.
fn as_metrics(measurement: &AudioMeasurement) -> forge_audio::Metrics {
    forge_audio::Metrics {
        duration: measurement.duration,
        sample_rate: measurement.sample_rate,
        channels: measurement.channels,
        peak: measurement.peak,
        peak_db: measurement.peak_db,
        rms_db: measurement.rms_db,
        loudness_lufs: measurement.loudness_lufs,
        crest_db: measurement.crest_db,
        full_scale_samples: measurement.full_scale_samples,
        longest_clip_run: measurement.longest_clip_run,
        lead_silence: measurement.lead_silence,
        tail_silence: measurement.tail_silence,
        dc_offset: measurement.dc_offset,
        silent: measurement.silent,
        loop_seam_db: measurement.loop_seam_db,
    }
}

// --------------------------------------------------------------- playback ---

/// Drive the sink from the shared transport state.
///
/// The same [`Playback`] the clip half uses, so PLAY, the scrub bar, the clock
/// and the space bar work on a sound without a second copy of any of them. What
/// differs is only where the time comes from: a clip is seeked to the studio's
/// clock, and a sink *is* the clock — reading its position back is the only way
/// the playhead can be honest about buffering and speed.
pub(super) fn drive_playback(
    mut commands: Commands,
    library: Res<AudioLibrary>,
    selection: Res<Selection>,
    mut playback: ResMut<Playback>,
    // Two queries over the same entities, and deliberately: the sink is
    // attached a frame or more *after* the player, once the source has
    // resolved, so asking "is anything sounding" through `&AudioSink` answers
    // no while a file is still loading — and a restart keyed on that answer
    // spawns a fresh player every frame until they all arrive at once.
    sounding: Query<Entity, With<NowPlaying>>,
    mut sinks: Query<&mut AudioSink, With<NowPlaying>>,
) {
    let silence = |commands: &mut Commands, sounding: &Query<Entity, With<NowPlaying>>| {
        for entity in sounding {
            commands.entity(entity).despawn();
        }
    };

    let Some(asset) = library.selected(&selection) else {
        // Selecting a clip has to silence whatever was sounding, or a track
        // keeps playing under an animation nobody asked to hear it with.
        silence(&mut commands, &sounding);
        return;
    };

    playback.duration = asset.duration();
    let volume = Volume::Linear(library.gain(asset.bus()));

    // A new selection, or PLAY pressed after the last one ran out.
    if playback.dirty || (playback.playing && sounding.is_empty()) {
        silence(&mut commands, &sounding);
        playback.dirty = false;
        playback.time = 0.0;
        // Never hand Bevy a format it will unwrap-panic on.
        if playback.playing && asset.playable {
            commands.spawn((
                NowPlaying,
                AudioPlayer(asset.handle.clone()),
                PlaybackSettings::ONCE.with_volume(volume),
            ));
        }
        return;
    }

    let mut finished = false;
    for mut sink in &mut sinks {
        sink.set_volume(volume);
        if playback.playing {
            sink.play();
        } else {
            sink.pause();
        }

        let position = sink.position().as_secs_f32();
        // A playhead that has been dragged is a seek request: the transport
        // writes the time it wants, and the gap between that and where the sink
        // actually is says a human moved it. Anything smaller is the clock.
        if (position - playback.time).abs() > 0.2 {
            let wanted = std::time::Duration::from_secs_f32(playback.time.max(0.0));
            if sink.try_seek(wanted).is_err() {
                // Snapping back is the honest answer: this format cannot seek,
                // and leaving the playhead where it was dropped would claim it
                // did.
                playback.time = position;
            }
        } else {
            playback.time = position;
        }

        // `position > 0` guards the frame a sink is created in, where it can
        // report itself empty before the first samples are queued — and where
        // taking that at face value restarts a loop forever.
        finished |= sink.empty() && position > 0.0;
    }

    if finished {
        if playback.looping {
            playback.dirty = true;
        } else {
            playback.playing = false;
            playback.time = playback.duration;
            silence(&mut commands, &sounding);
        }
    }
}

// ---------------------------------------------------------------- refresh ---

/// Show the plot, move the playhead, and keep the gain readouts current.
pub(super) fn refresh(
    library: Res<AudioLibrary>,
    selection: Res<Selection>,
    playback: Res<Playback>,
    mut plots: Query<&mut ImageNode, With<PlotImage>>,
    mut playhead: Query<&mut Node, With<Playhead>>,
    readouts: Query<(&BusReadout, &Children)>,
    mut texts: Query<&mut Text>,
) {
    for (bus, children) in &readouts {
        widgets::set_child_text(
            &mut texts,
            Some(children),
            &format!("{:.0}%", library.gain(bus.0) * 100.0),
        );
    }

    let Some(asset) = library.selected(&selection) else {
        return;
    };
    // The plot is swapped by handle rather than redrawn: a file that has not
    // been decoded yet shows the blank frame, and one that has shows the
    // picture it drew then.
    let wanted = asset.plot.clone().unwrap_or_default();
    for mut plot in &mut plots {
        if plot.image != wanted {
            plot.image = wanted.clone();
        }
    }

    let fraction = if playback.duration > 0.0 {
        (playback.time / playback.duration).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let left = Val::Percent(fraction * 100.0);
    for mut node in &mut playhead {
        if node.left != left {
            node.left = left;
        }
    }
}

// -------------------------------------------------------------- self-test ---

/// Step through every asset, playing each, then exit.
pub(super) fn self_test(
    test: Option<Res<AudioSelfTest>>,
    library: Res<AudioLibrary>,
    mut selection: ResMut<Selection>,
    mut playback: ResMut<Playback>,
    mut frame: Local<u32>,
    mut played: Local<usize>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(test) = test else {
        return;
    };
    *frame += 1;
    // Give the asset server a moment before the first sink is built.
    if *frame < 60 {
        return;
    }
    let step = ((*frame - 60) / test.frames_per_file.max(1)) as usize;
    let Some(asset) = library.assets.get(step) else {
        println!(
            "self-test: played {} file(s) without panicking",
            library.assets.len()
        );
        exit.write(AppExit::Success);
        return;
    };
    if *played == step && *frame != 60 {
        return;
    }
    *played = step;
    println!(
        "self-test: {} {}",
        if asset.playable {
            "playing"
        } else {
            "skipping (unplayable)"
        },
        asset.rel_path
    );
    selection.select(asset.key.clone());
    playback.playing = true;
    playback.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `bevy_audio` unwraps its decoder, so handing it a format the build does
    /// not include takes the whole app down. The gate is the only thing between
    /// an agent dropping an `.aiff` into the library and a crash.
    #[test]
    fn only_formats_bevy_can_decode_are_played() {
        assert!(is_playable("audio/sfx/bark.WAV"));
        assert!(is_playable("audio/music/theme.ogg"));
        assert!(!is_playable("audio/sfx/bark.aiff"));
        assert!(!is_playable("audio/sfx/bark"));
    }

    /// The cache and `forge_audio` describe the same fifteen numbers, and the
    /// only thing keeping the two lists in step is this pair of conversions.
    #[test]
    fn a_measurement_survives_the_round_trip() {
        let measurement = AudioMeasurement {
            duration: 1.5,
            sample_rate: 48_000,
            channels: 2,
            peak: 0.9,
            peak_db: -0.9,
            rms_db: -18.0,
            loudness_lufs: -20.0,
            crest_db: 17.1,
            full_scale_samples: 4,
            longest_clip_run: 3,
            lead_silence: 0.01,
            tail_silence: 0.05,
            dc_offset: 0.002,
            silent: false,
            loop_seam_db: 1.2,
        };
        assert_eq!(
            AudioMeasurement::from(&as_metrics(&measurement)),
            measurement
        );
    }

    /// A row goes orange for the two faults that mean the file is unusable,
    /// and stays plain for the softer warnings — which the panel explains.
    #[test]
    fn only_real_faults_colour_a_row() {
        let asset = |silent: bool, run: usize, peak_db: f32| AudioAsset {
            key: AssetKey::audio("bark"),
            name: "bark".to_owned(),
            rel_path: "audio/sfx/bark.wav".to_owned(),
            path: PathBuf::from("/nowhere/bark.wav"),
            kind: Kind::Sfx,
            handle: Handle::default(),
            playable: true,
            sidecar: None,
            measurement: Some(AudioMeasurement {
                duration: 1.0,
                sample_rate: 48_000,
                channels: 1,
                peak: 1.0,
                peak_db,
                rms_db: -18.0,
                loudness_lufs: -20.0,
                crest_db: 17.0,
                full_scale_samples: run,
                longest_clip_run: run,
                lead_silence: 0.0,
                tail_silence: 0.0,
                dc_offset: 0.0,
                silent,
                loop_seam_db: 0.0,
            }),
            plot: None,
            decoded: true,
            decode_error: None,
            stamp: None,
        };
        assert!(asset(true, 0, -1.0).is_faulty(), "silence");
        assert!(asset(false, 4, -1.0).is_faulty(), "clipping");
        assert!(!asset(false, 1, -1.0).is_faulty(), "one normalised peak");
        // Quiet is a warning, not a fault.
        let quiet = asset(false, 0, -24.0);
        assert!(!quiet.is_faulty());
        assert!(!quiet.warnings().is_empty());
        // And a format the engine cannot play says so, measurement or not.
        let mut aiff = asset(false, 0, -1.0);
        aiff.playable = false;
        aiff.rel_path = String::from("audio/sfx/bark.aiff");
        assert!(aiff.warnings().iter().any(|w| w.contains("aiff")));
    }

    /// The plot the window shows is the one `forge audio` draws, at a width
    /// the panel can scale without the text turning to mush.
    #[test]
    fn the_plot_renders_to_a_texture_of_the_layouts_size() {
        let audio = forge_audio::Audio {
            samples: (0..48_000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect(),
            sample_rate: 48_000,
            channels: 1,
        };
        let metrics = forge_audio::measure(&audio);
        let image = plot_image(&audio, &metrics, "tone");
        let layout = plot_layout();
        assert_eq!(image.width(), layout.width);
        assert_eq!(image.height(), layout.total_height());
    }
}
