//! What the studio can play, which of it is selected, and the browser over it.
//!
//! # Why the selection is a name and not an index
//!
//! The library grows while the app runs: a promote lands a clip, an export
//! lands a body, `--take` materialises a candidate beside the shipped clips.
//! Anything holding a position in that list has to be reconciled every time it
//! grows, and that reconciliation has already been got wrong once — a panel
//! decided it had already handled clip 0 before clip 0 existed, because index
//! 0 is also the value an empty selection starts at. A key names a thing;
//! there is nothing to reconcile.
//!
//! # Why the browser is sectioned and filtered
//!
//! A flat list built once at startup works at thirty assets and stops working
//! somewhere before three hundred. Sections give the eye somewhere to start,
//! the filter is how you find the one you remember the *prompt* of rather than
//! the name of, and the dim measurement after each name is what makes a wrong
//! asset visible without opening it — a bark that is four seconds long, a clip
//! that moves at 0.02 m/s.
//!
//! Rows are rebuilt on a generation bump rather than every frame: thirty rows
//! of three entities each, rebuilt at 60 Hz, is a lot of despawning to keep a
//! list that changes when somebody types.
//!
//! # Why the sections fold, and why the tags are a strip
//!
//! Sections that are always open are a flat list with headings in it: at forty
//! assets the audio is already below the fold, and the shape of the library —
//! what kinds of thing are in it, how much of each — cannot be seen at all.
//! Folding is how a browser shows its own shape, so both levels fold, and a
//! shut fold keeps saying how many rows it holds.
//!
//! Tags were the opposite problem: they were searchable from the day the schema
//! grew them and invisible in the window, so the only person who could filter
//! by `loop` was somebody who already knew there was a `loop` tag. The strip
//! says what there is to filter by, with a library-wide count beside each, and
//! clicking one is the same question as typing it — except that two clicks mean
//! *both*, which is a thing the text field cannot ask.
//!
//! # Why the meshes are listed here and are not selected
//!
//! The stage's mesh is a property of the *studio*, not of the clip: a reviewer
//! judging a walk wants to see it on the slim body as well as the blocky one,
//! without losing their place. So BODIES and MODELS are sections of the same
//! browser — discovered, folded and filtered like the rest — but clicking a
//! mesh row writes a [`SwapModel`] and deliberately leaves the [`Selection`]
//! where it was, so the clip under review keeps playing on the new body.
//! Everything particular to that is `RowId`, the two-variant thing a row names.
//!
//! # Why the library is rescanned while the window is open
//!
//! The catalog is derived by scanning and never persisted, so the browser's
//! question — what is in the library *now* — is the same question every
//! other reader asks, and it costs milliseconds. An artist iterating on a mesh
//! exports, alt-tabs and expects to click it; a promote from the CLI or the
//! MCP server lands a clip beside the ones on screen; a rebake replaces the
//! bytes under a name already loaded. All three show within a poll tick, and
//! the last one goes through `AssetServer::reload` on the labelled path —
//! which is the one way Bevy swaps the motion under a handle already in hand.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    path::Path,
    time::SystemTime,
};

use bevy::{
    animation::{AnimationClip, graph::AnimationGraph},
    asset::LoadState,
    input_focus::tab_navigation::TabGroup,
    prelude::*,
    text::EditableText,
};

use forge_library::{AssetRecord, Catalog, Sidecar, schema::Kind};
use forge_motion::Take;

use crate::{
    catalog::ClipEntry,
    npz_clip,
    studio::{
        StudioConfig,
        audio_view::{self, AudioLibrary},
        rig::{ActiveModel, Rig, SwapModel},
        shell::LibraryPanel,
        widgets,
    },
    theme,
};

/// Which library an asset lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssetKind {
    /// An animation clip.
    Clip,
    /// An audio file.
    Audio,
}

/// What the studio is showing, named rather than numbered.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssetKey {
    /// Which library it lives in.
    pub kind: AssetKind,
    /// File stem, unique within that library.
    pub stem: String,
}

impl AssetKey {
    /// A key for an animation clip.
    #[must_use]
    pub fn clip(stem: impl Into<String>) -> Self {
        Self {
            kind: AssetKind::Clip,
            stem: stem.into(),
        }
    }

    /// A key for an audio file.
    #[must_use]
    pub fn audio(stem: impl Into<String>) -> Self {
        Self {
            kind: AssetKind::Audio,
            stem: stem.into(),
        }
    }

    /// Whether this names something in the audio library.
    #[must_use]
    pub fn is_audio(&self) -> bool {
        self.kind == AssetKind::Audio
    }
}

/// Where a clip in the library came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipOrigin {
    /// Loaded from the shipped asset library on disk.
    Shipped,
    /// Materialised from a raw take in this session; gone when the app closes.
    Candidate,
}

/// Size and modification time of a file, which is how a rescan tells a
/// rebake from a clip it has already loaded.
///
/// Deliberately not a hash: the poll runs every couple of seconds over every
/// clip in the library, and reading them all to hash them would turn a stat
/// into a disk scan. Size plus mtime is the same signature the metrics cache
/// trusts, and a bake that lands byte-identical and in the same second was
/// not a change anybody could see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FileStamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl FileStamp {
    pub(super) fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self {
            len: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// One playable clip, however it got here.
pub struct ClipItem {
    /// Where it came from on disk, and whatever its sidecar says.
    pub entry: ClipEntry,
    /// The clip asset. Replaced in place when a rebake changes it.
    pub handle: Handle<AnimationClip>,
    /// Its node in the shared graph, once the graph exists.
    pub node: Option<AnimationNodeIndex>,
    /// Shipped, or made here.
    pub origin: ClipOrigin,
    /// What names it. Unique across the library.
    pub key: AssetKey,
    /// The file as it was when last scanned; `None` for a candidate.
    stamp: Option<FileStamp>,
}

/// Every clip the studio can play.
#[derive(Resource, Default)]
pub struct ClipLibrary {
    items: Vec<ClipItem>,
}

impl ClipLibrary {
    /// Every item, in the order they were added.
    #[must_use]
    pub fn items(&self) -> &[ClipItem] {
        &self.items
    }

    /// The item `key` names.
    #[must_use]
    pub fn get(&self, key: &AssetKey) -> Option<&ClipItem> {
        self.items.iter().find(|item| item.key == *key)
    }

    /// The clip asset `key` names.
    #[must_use]
    pub fn handle(&self, key: &AssetKey) -> Option<Handle<AnimationClip>> {
        self.get(key).map(|item| item.handle.clone())
    }

    /// The item currently selected, if the selection still names one.
    #[must_use]
    pub fn selected<'a>(&'a self, selection: &Selection) -> Option<&'a ClipItem> {
        selection.key().and_then(|key| self.get(key))
    }

    /// Add a clip, returning the key that names it from now on.
    ///
    /// The stem is made unique rather than the caller being asked for a unique
    /// name: a take materialised beside the shipped clip it was promoted from
    /// deliberately produces a second item with the *same* display name, and
    /// a key that matched both would make the selection ambiguous.
    pub fn push(
        &mut self,
        entry: ClipEntry,
        handle: Handle<AnimationClip>,
        node: Option<AnimationNodeIndex>,
        origin: ClipOrigin,
    ) -> AssetKey {
        let key = AssetKey::clip(self.unique_stem(&entry.name));
        self.items.push(ClipItem {
            entry,
            handle,
            node,
            origin,
            key: key.clone(),
            stamp: None,
        });
        key
    }

    /// Attach graph nodes to the items, in order.
    ///
    /// Only meaningful at the one moment the graph is built from every loaded
    /// clip at once; clips added afterwards carry the node they were given.
    pub fn assign_nodes(&mut self, nodes: impl IntoIterator<Item = AnimationNodeIndex>) {
        for (item, node) in self.items.iter_mut().zip(nodes) {
            item.node = Some(node);
        }
    }

    /// Take on a re-baked sidecar for the shipped item at `asset_path`, and say
    /// which item that was.
    ///
    /// `None` means no shipped item loads from that path, i.e. the path is new
    /// to the library. The handle and graph node are deliberately kept: they
    /// name the asset by path, so a reload of that path is what refreshes the
    /// motion, and swapping them here would orphan whatever is mid-play.
    pub fn refresh_shipped(
        &mut self,
        asset_path: &str,
        sidecar: Option<Sidecar>,
    ) -> Option<AssetKey> {
        let item = self.items.iter_mut().find(|item| {
            item.origin == ClipOrigin::Shipped && item.entry.asset_path == asset_path
        })?;
        item.entry.sidecar = sidecar;
        Some(item.key.clone())
    }

    /// Forget the shipped item at `asset_path`, because the file is gone.
    fn remove_shipped(&mut self, asset_path: &str) -> Option<AssetKey> {
        let index = self.items.iter().position(|item| {
            item.origin == ClipOrigin::Shipped && item.entry.asset_path == asset_path
        })?;
        Some(self.items.remove(index).key)
    }

    fn unique_stem(&self, base: &str) -> String {
        if !self.items.iter().any(|item| item.key.stem == base) {
            return base.to_owned();
        }
        // Bounded, and the bound is enough: with n items in hand at most n of
        // these suffixes can be taken, so one of the first n + 1 is free.
        (2..=self.items.len() + 1)
            .map(|n| format!("{base}#{n}"))
            .find(|stem| !self.items.iter().any(|item| item.key.stem == *stem))
            .unwrap_or_else(|| base.to_owned())
    }
}

/// One mesh that can be put on the stage: a body or a model.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelEntry {
    /// File stem, for display.
    pub name: String,
    /// Path relative to the asset root, which is what a [`SwapModel`] names and
    /// what [`ActiveModel::path`] is compared against.
    pub rel_path: String,
    /// Which family it belongs to — [`Kind::Body`] or [`Kind::Model`] — from
    /// which directory the catalog found it in. A mesh with no record is still
    /// a mesh you can stand up and look at.
    pub kind: Kind,
    /// The record beside it, when there is one that parses.
    pub sidecar: Option<Sidecar>,
}

impl ModelEntry {
    fn from_record(record: &AssetRecord) -> Self {
        if let Some(err) = &record.sidecar_error {
            warn!("{}: {err}", record.name);
        }
        Self {
            name: record.name.clone(),
            rel_path: record.rel_path.clone(),
            kind: record.kind,
            sidecar: record.sidecar.clone(),
        }
    }
}

/// How often the library is rescanned while the window is open.
///
/// An artist iterating on a mesh exports, alt-tabs, and expects to click it —
/// a restart between every export would make `just check-mesh` the better
/// viewer, which is backwards. Rows move only when the library actually
/// changed, which is exactly the moment the mover is wanted.
const POLL_SECONDS: f32 = 2.0;

/// Every mesh the studio can put on the stage, rescanned while it runs.
#[derive(Resource)]
pub struct ModelLibrary {
    models: Vec<ModelEntry>,
    poll: Timer,
}

impl Default for ModelLibrary {
    fn default() -> Self {
        Self {
            models: Vec::new(),
            poll: Timer::from_seconds(POLL_SECONDS, TimerMode::Repeating),
        }
    }
}

impl ModelLibrary {
    /// Every mesh found, bodies first, in the order the browser lists them.
    #[must_use]
    pub fn models(&self) -> &[ModelEntry] {
        &self.models
    }

    /// The entry standing at `rel_path`, if the library lists one there.
    #[must_use]
    pub fn get(&self, rel_path: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.rel_path == rel_path)
    }

    /// The entry `wanted` names: an exact relative path, a file name, or a
    /// stem, case-insensitively — the same three spellings the catalog
    /// accepts, so `--model vex_runner` and `--model bodies/vex_runner.glb`
    /// open the same thing. Bodies are listed first, so a body and a model
    /// sharing a stem resolve to the body.
    #[must_use]
    pub fn resolve(&self, wanted: &str) -> Option<&ModelEntry> {
        let wanted = wanted.trim().trim_start_matches("./").replace('\\', "/");
        if wanted.is_empty() {
            return None;
        }
        let file_name = |path: &str| path.rsplit('/').next().unwrap_or(path).to_owned();
        let stem = Path::new(&wanted)
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        self.models
            .iter()
            .find(|m| m.rel_path == wanted)
            .or_else(|| {
                self.models
                    .iter()
                    .find(|m| file_name(&m.rel_path).eq_ignore_ascii_case(&wanted))
            })
            .or_else(|| {
                self.models
                    .iter()
                    .find(|m| m.name.eq_ignore_ascii_case(&stem))
            })
    }

    /// Record a mesh, which is only ever done by the library scans.
    pub fn push(&mut self, name: impl Into<String>, rel_path: impl Into<String>, kind: Kind) {
        self.models.push(ModelEntry {
            name: name.into(),
            rel_path: rel_path.into(),
            kind,
            sidecar: None,
        });
    }
}

/// What the studio is showing.
#[derive(Resource, Default)]
pub struct Selection {
    key: Option<AssetKey>,
}

impl Selection {
    /// What is selected, if anything.
    #[must_use]
    pub fn key(&self) -> Option<&AssetKey> {
        self.key.as_ref()
    }

    /// Show this instead.
    pub fn select(&mut self, key: AssetKey) {
        self.key = Some(key);
    }

    /// Show nothing.
    pub fn clear(&mut self) {
        self.key = None;
    }
}

/// One of the browser's top-level sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Section {
    /// The rigged bodies, one of which is standing on the stage.
    Bodies,
    /// The props and other unrigged meshes.
    Models,
    /// The shipped clip library.
    Clips,
    /// Candidates materialised in this session.
    Takes,
    /// The audio library.
    Audio,
}

impl Section {
    /// What its header says.
    const fn title(self) -> &'static str {
        match self {
            Self::Bodies => "BODIES",
            Self::Models => "MODELS",
            Self::Clips => "CLIPS",
            Self::Takes => "TAKES",
            Self::Audio => "AUDIO",
        }
    }
}

/// What one foldable heading names: a section, or a subgroup inside one.
///
/// The subgroup is unique only *within* a section — `sfx` under AUDIO is a
/// different fold from anything another section might call `sfx`, and would
/// be the same one if the section were not part of the identity.
pub(super) type GroupId = (Section, Option<String>);

/// The browser's own state: what is typed in the filter, and what is on screen.
#[derive(Resource, Default)]
pub struct LibraryView {
    /// The filter text, mirrored from its field each frame.
    pub filter: String,
    /// Tags a row must carry to be listed, on top of the filter text.
    active_tags: BTreeSet<String>,
    /// The headings that are folded shut; empty means everything is open.
    ///
    /// Session state on purpose. Which parts of the library you folded away
    /// while hunting for one clip is not a preference worth restoring a week
    /// later, and a restored fold means opening the studio onto a library that
    /// looks half empty for a reason nobody remembers giving.
    collapsed: HashSet<GroupId>,
    /// Bumped whenever the rows need drawing again.
    generation: u32,
    drawn_generation: u32,
    /// The keys currently listed, in the order they appear.
    ///
    /// Kept because the arrow keys move through *what is visible*: stepping
    /// through the whole library while a filter hides most of it would select
    /// rows nobody can see. Folded-away rows are absent for the same reason,
    /// which is why this is assigned from what was actually spawned rather than
    /// from everything that passed the filter.
    rows: Vec<AssetKey>,
}

impl LibraryView {
    /// Ask for the rows to be drawn again.
    pub fn touch(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// Whether this heading is folded shut.
    fn is_collapsed(&self, group: &GroupId) -> bool {
        self.collapsed.contains(group)
    }

    /// Fold this heading shut, or open it again.
    fn toggle_collapsed(&mut self, group: GroupId) {
        if !self.collapsed.remove(&group) {
            self.collapsed.insert(group);
        }
        self.touch();
    }

    /// Open this heading and the section it sits in, if either was shut.
    fn expand(&mut self, group: &GroupId) {
        let mut opened = self.collapsed.remove(&(group.0, None));
        opened |= self.collapsed.remove(group);
        if opened {
            self.touch();
        }
    }

    /// Filter by this tag as well, or stop filtering by it.
    fn toggle_tag(&mut self, tag: &str) {
        if !self.active_tags.remove(tag) {
            self.active_tags.insert(tag.to_owned());
        }
        self.touch();
    }

    /// Step `delta` places through the visible rows, wrapping.
    #[must_use]
    pub fn step(&self, from: Option<&AssetKey>, delta: isize) -> Option<AssetKey> {
        let count = self.rows.len();
        if count == 0 {
            return None;
        }
        let current = from
            .and_then(|key| self.rows.iter().position(|row| row == key))
            .unwrap_or(0);
        let count_i = isize::try_from(count).unwrap_or(isize::MAX);
        let next = (isize::try_from(current).unwrap_or(0) + delta).rem_euclid(count_i);
        self.rows.get(usize::try_from(next).unwrap_or(0)).cloned()
    }

    /// Whether an asset is listed at all: it must carry every switched-on tag
    /// and match whatever is typed.
    fn matches(&self, name: &str, sidecar: Option<&Sidecar>) -> bool {
        self.tagged(sidecar) && self.matches_text(name, sidecar)
    }

    /// Whether an asset carries every tag the strip has switched on.
    ///
    /// AND rather than OR because that is what a strip of chips is for: each
    /// one you click narrows what is on screen. Chips that meant "either" would
    /// widen it instead, and nothing left in the browser could then ask for the
    /// intersection — which is the question a second tag is clicked to ask.
    fn tagged(&self, sidecar: Option<&Sidecar>) -> bool {
        if self.active_tags.is_empty() {
            return true;
        }
        let Some(sidecar) = sidecar else {
            return false;
        };
        self.active_tags
            .iter()
            .all(|wanted| sidecar.tags.iter().any(|tag| tag == wanted))
    }

    /// Whether an asset matches the typed filter, on its name, prompt or tags.
    ///
    /// All three because you remember an asset by whichever of them stuck: the
    /// name `roll`, the prompt "dives forward into a roll", or the fact that
    /// it was tagged as a loop.
    fn matches_text(&self, name: &str, sidecar: Option<&Sidecar>) -> bool {
        let needle = self.filter.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return true;
        }
        if name.to_ascii_lowercase().contains(&needle) {
            return true;
        }
        let Some(sidecar) = sidecar else {
            return false;
        };
        sidecar
            .prompt
            .as_deref()
            .is_some_and(|p| p.to_ascii_lowercase().contains(&needle))
            || sidecar
                .tags
                .iter()
                .any(|t| t.to_ascii_lowercase().contains(&needle))
    }
}

// ------------------------------------------------------------------ setup ---

/// Scan the library once: start every clip loading, list every mesh, and
/// hand the sounds to the audio view.
///
/// The clip loads are kicked off here rather than when the rig is ready
/// because nothing about them depends on the rig, and the graph cannot be
/// built until they have all resolved anyway. Nothing is loaded for a mesh:
/// a model costs a scene spawn and a rig wire-up, and standing all of them up
/// to list their names would be seconds of stalled window to show three rows.
/// The one that is wanted is loaded by [`crate::studio::rig`] when asked.
pub fn discover(
    config: Res<StudioConfig>,
    server: Res<AssetServer>,
    mut library: ResMut<ClipLibrary>,
    mut models: ResMut<ModelLibrary>,
    mut audio: ResMut<AudioLibrary>,
) {
    let catalog = Catalog::scan(&config.project);
    for record in catalog.records() {
        match record.kind {
            Kind::Clip => {
                let entry = ClipEntry::from_record(record);
                let handle = server.load(format!("{}#Animation0", entry.asset_path));
                let stamp = FileStamp::of(&record.path);
                library.push(entry, handle, None, ClipOrigin::Shipped);
                if let Some(item) = library.items.last_mut() {
                    item.stamp = stamp;
                }
            }
            Kind::Body | Kind::Model => models.models.push(ModelEntry::from_record(record)),
            Kind::Sfx | Kind::Music | Kind::Voice => {}
        }
    }
    // Bodies first, then models, each in the catalog's sorted order: the
    // browser's order must not depend on the order the filesystem happens to
    // hand entries back in, and a body is what a clip is judged on.
    models
        .models
        .sort_by_key(|m| (m.kind != Kind::Body, m.rel_path.clone()));
    if models.models.is_empty() {
        // Said out loud rather than left as an empty section: a studio whose
        // BODIES section is missing has either no bodies or no directory, and
        // those are not the same problem.
        warn!(
            "no bodies or models to list under {}",
            config.project.assets.display()
        );
    }
    audio_view::absorb_catalog(&mut audio, &catalog, &config.project, &server);
}

/// Keep the browser in step with the library while the window runs.
///
/// A mesh that vanished stays swappable-away-from: the stage keeps whatever
/// is standing, and clicking a listed-but-deleted file is refused loudly by
/// the swap. A clip whose bytes changed is reloaded under the handle already
/// in hand, and a clip whose file is gone leaves the list — along with the
/// selection, if it was the one showing, rather than a row that plays motion
/// nobody can find on disk any more.
pub fn rescan_models(
    time: Res<Time>,
    config: Res<StudioConfig>,
    server: Res<AssetServer>,
    rig: Res<Rig>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut models: ResMut<ModelLibrary>,
    mut library: ResMut<ClipLibrary>,
    mut audio: ResMut<AudioLibrary>,
    mut selection: ResMut<Selection>,
    mut view: ResMut<LibraryView>,
) {
    models.poll.tick(time.delta());
    if !models.poll.just_finished() {
        return;
    }
    let catalog = Catalog::scan(&config.project);

    // Meshes: replace the list wholesale when it differs. An unreadable
    // directory during a poll is not the startup case — the scan may be
    // racing an export — and the catalog answers "nothing" for it, which is
    // why a listing that went from something to nothing is kept as it was.
    let mut found: Vec<ModelEntry> = catalog
        .records()
        .iter()
        .filter(|r| r.kind.is_mesh())
        .map(ModelEntry::from_record)
        .collect();
    found.sort_by_key(|m| (m.kind != Kind::Body, m.rel_path.clone()));
    let blanked = found.is_empty() && !models.models.is_empty();
    if found != models.models && !blanked {
        info!(
            "meshes changed: {} listed (was {})",
            found.len(),
            models.models.len()
        );
        models.models = found;
        view.touch();
    }

    let changed = sync_clips(
        &catalog,
        &server,
        &rig,
        &mut graphs,
        &mut library,
        &mut selection,
    );
    let audio_changed = audio_view::absorb_catalog(&mut audio, &catalog, &config.project, &server);
    if audio_changed
        && let Some(key) = selection.key().filter(|key| key.is_audio())
        && audio.get(key).is_none()
    {
        selection.clear();
    }
    if changed || audio_changed {
        view.touch();
    }
}

/// Bring the clip list up to date with what the catalog found.
///
/// New clips are loaded and, when the shared graph already exists, grafted
/// onto it so they play on the body standing right now; a clip whose file
/// changed is reloaded by its labelled path, which is the one path Bevy
/// tracks a sub-asset under; a clip that is gone leaves the list.
fn sync_clips(
    catalog: &Catalog,
    server: &AssetServer,
    rig: &Rig,
    graphs: &mut Assets<AnimationGraph>,
    library: &mut ClipLibrary,
    selection: &mut Selection,
) -> bool {
    let mut changed = false;
    let mut seen: HashSet<String> = HashSet::new();
    for record in catalog.records().iter().filter(|r| r.kind == Kind::Clip) {
        seen.insert(record.rel_path.clone());
        let stamp = FileStamp::of(&record.path);
        let existing = library.items.iter_mut().find(|item| {
            item.origin == ClipOrigin::Shipped && item.entry.asset_path == record.rel_path
        });
        if let Some(item) = existing {
            if item.stamp != stamp {
                info!("{} changed on disk: reloading", record.rel_path);
                // The reload must name the *labelled* path. The server tracks
                // handles by the exact path they were loaded under, and
                // nothing ever loaded the bare source — so reloading it finds
                // no handle and quietly does nothing.
                server.reload(format!("{}#Animation0", record.rel_path));
                item.stamp = stamp;
                item.entry = ClipEntry::from_record(record);
                changed = true;
            } else if item.entry.sidecar != record.sidecar {
                item.entry = ClipEntry::from_record(record);
                changed = true;
            }
            continue;
        }
        info!("{} arrived: loading", record.rel_path);
        let entry = ClipEntry::from_record(record);
        let handle: Handle<AnimationClip> = server.load(format!("{}#Animation0", entry.asset_path));
        // Grafted onto the live graph if there is one; otherwise the graph is
        // built from every item once the rig stands, this one included.
        let node = rig
            .graph()
            .and_then(|graph| graphs.get_mut(graph))
            .map(|mut graph| {
                let root = graph.root;
                graph.add_clip(handle.clone(), 1.0, root)
            });
        library.push(entry, handle, node, ClipOrigin::Shipped);
        if let Some(item) = library.items.last_mut() {
            item.stamp = stamp;
        }
        changed = true;
    }
    let gone: Vec<String> = library
        .items
        .iter()
        .filter(|item| item.origin == ClipOrigin::Shipped && !seen.contains(&item.entry.asset_path))
        .map(|item| item.entry.asset_path.clone())
        .collect();
    for path in gone {
        info!("{path} is gone from the library");
        if let Some(key) = library.remove_shipped(&path)
            && selection.key() == Some(&key)
        {
            selection.clear();
        }
        changed = true;
    }
    changed
}

/// Open on something: the first clip, or the first sound when asked for audio.
///
/// Runs after the library is discovered because "audio first" has to be able
/// to fail over to a clip — a project with no audio at all opened with
/// `--audio` should still show its clips rather than an empty window.
///
/// When the stage subject is a static model, no clip is selected at all: a
/// clip on a barrel binds nothing, and the panel would open on "NOTHING
/// BOUND" about a mesh that was never meant to move. A sound still is —
/// there is nothing model-shaped about listening.
pub fn focus_first(
    config: Res<StudioConfig>,
    library: Res<ClipLibrary>,
    audio: Res<AudioLibrary>,
    models: Res<ModelLibrary>,
    active: Res<ActiveModel>,
    mut selection: ResMut<Selection>,
) {
    let stage_is_model = models
        .get(&active.path)
        .is_some_and(|entry| entry.kind == Kind::Model);
    let clip = library
        .items
        .first()
        .filter(|_| !stage_is_model)
        .map(|item| item.key.clone());
    let sound = audio.assets().first().map(|asset| asset.key.clone());
    let first = if config.audio {
        sound.or(clip)
    } else {
        clip.or(sound)
    };
    if let Some(key) = first {
        selection.select(key);
    }
}

/// Put the `--take` on the stage body the moment the rig can carry it.
///
/// Runs every frame until the rig stands, then once: the take is read, the
/// `--recipe` applied — the identity edit when there is none, because
/// [`forge_motion::Edit::apply`] is also where a take turns to face the rig's
/// way — and the result materialised as a candidate and selected, since the
/// take is what the window was opened to watch. A take that will not read is
/// an error in the log and a window that opens on the library instead, not a
/// window that never opens.
pub(super) fn open_take(
    config: Res<StudioConfig>,
    rig: Res<Rig>,
    mut library: ResMut<ClipLibrary>,
    mut clips: ResMut<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut selection: ResMut<Selection>,
    mut done: Local<bool>,
) {
    let Some(path) = config.take.as_deref().filter(|_| !*done) else {
        return;
    };
    if !rig.is_ready() || rig.graph().is_none() {
        return;
    }
    *done = true;
    let name = path.file_stem().map_or_else(
        || String::from("take"),
        |s| s.to_string_lossy().into_owned(),
    );
    let take = match Take::read(path) {
        Ok(take) => take,
        Err(err) => {
            error!("cannot read the take {}: {err}", path.display());
            return;
        }
    };
    let recipe = match config.recipe.as_deref().map(read_recipe) {
        Some(Ok(recipe)) => Some(recipe),
        Some(Err(err)) => {
            error!("{err}; previewing the take unedited");
            None
        }
        None => None,
    };
    let edit = match recipe.as_ref().map(|r| r.to_edit(take.fps)).transpose() {
        Ok(edit) => edit.unwrap_or_default(),
        Err(err) => {
            error!("the recipe does not resolve: {err}; previewing the take unedited");
            forge_motion::Edit::default()
        }
    };
    let edited = edit.apply(&take);
    let mut entry =
        ClipEntry::bare(path.to_string_lossy().replace('\\', "/"), name).with_prompt(&take.prompt);
    if let (Some(recipe), Some(sidecar)) = (recipe, entry.sidecar.as_mut()) {
        sidecar.recipe = Some(recipe);
    }
    let materialised = materialise_take(
        &mut library,
        &mut clips,
        &mut graphs,
        &rig,
        &edited,
        entry,
        ClipOrigin::Candidate,
    );
    if let Some(key) = materialised {
        info!(
            "{} on the stage ({} frames)",
            path.display(),
            edited.frames()
        );
        selection.select(key);
    } else {
        error!("the rig stood up without a graph, so the take cannot play");
    }
}

/// Read a `--recipe` file: a bare [`forge_library::ClipRecipe`] object, or a
/// whole sidecar whose `recipe` block is the one wanted.
///
/// Both, because both are what somebody has to hand: the CLI writes a recipe
/// on its own, and a shipped clip's sidecar is where the recipe that made it
/// lives. A sidecar read as a bare recipe would parse — every field defaults —
/// and silently preview the identity edit under the name of a real one.
fn read_recipe(path: &Path) -> Result<forge_library::ClipRecipe, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("{} is not JSON: {e}", path.display()))?;
    let recipe = match value.get("recipe") {
        Some(inner) => inner.clone(),
        None => value,
    };
    serde_json::from_value(recipe).map_err(|e| format!("{} is not a recipe: {e}", path.display()))
}

// --------------------------------------------------------------- browsing ---

/// What one row of the browser names.
///
/// Two things end up in the same list because they are found, folded and
/// filtered the same way, and they answer two different clicks: an asset row
/// moves the [`Selection`], a mesh row swaps the model under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RowId {
    /// A clip or a sound, named the way the selection names it.
    Asset(AssetKey),
    /// A mesh, by the asset-root-relative path a [`SwapModel`] wants.
    Model(String),
}

impl RowId {
    /// The asset this row names, if it names one.
    ///
    /// Also the answer to "can the arrow keys land here" — see where
    /// [`rebuild_rows`] fills [`LibraryView::rows`].
    fn asset(&self) -> Option<&AssetKey> {
        match self {
            Self::Asset(key) => Some(key),
            Self::Model(_) => None,
        }
    }

    /// Whether the browser marks this row: the asset that is showing, or the
    /// mesh that is standing.
    fn is_marked(&self, selection: &Selection, active: &ActiveModel) -> bool {
        match self {
            Self::Asset(key) => selection.key() == Some(key),
            // [`ActiveModel::path`] is rewritten when a rig finishes standing
            // up and never when a swap is asked for, so the mark lands with the
            // mesh rather than with the click — and stays where it is if the
            // load fails, which is the one case where a marker that moved
            // eagerly would be pointing at something nobody can see.
            Self::Model(path) => active.path == *path,
        }
    }
}

/// Names the row for one thing, by what it names and not by position.
#[derive(Component)]
pub(super) struct LibraryRow(RowId);

/// The name inside a row, carrying the colour it rests at.
///
/// The resting colour is per row — a clipped sound stays orange whether or not
/// it is selected — so it is remembered here rather than recomputed by the
/// refresh, which would have to reach back into the audio library to do it.
#[derive(Component)]
pub(super) struct RowName(Color);

/// The filter box over the library list.
#[derive(Component)]
pub(super) struct FilterField;

/// The scrolling list of rows, inside the library column.
#[derive(Component)]
pub(super) struct LibraryList;

/// The strip of filter-by-tag chips over the list.
#[derive(Component)]
pub(super) struct TagStrip;

/// The fixed-height line under the list that carries [`Rig::status`].
///
/// This is the only in-window account of a swap that did not happen: a refused
/// mesh leaves the previous one standing, so without this line clicking a
/// mesh that will not load is indistinguishable from clicking nothing.
#[derive(Component)]
pub(super) struct StageStatus;

/// What fits on the [`StageStatus`] line at the browser's width.
const STATUS_CHARS: usize = 36;

/// Names the tag one chip switches on.
#[derive(Component)]
pub(super) struct TagToggle(String);

/// Names the heading one caret folds.
#[derive(Component)]
pub(super) struct CollapseToggle(GroupId);

/// One section of the browser: its own fold, and the groups under it.
struct SectionView {
    id: Section,
    groups: Vec<Group>,
    collapsed: bool,
}

impl SectionView {
    /// How many rows it holds, folded or not — which is what its header says.
    fn count(&self) -> usize {
        self.groups.iter().map(|group| group.rows.len()).sum()
    }

    /// The rows that are actually on screen, and so actually steppable.
    fn visible_rows(&self) -> impl Iterator<Item = &Row> {
        self.groups
            .iter()
            .filter(|group| !self.collapsed && !group.collapsed)
            .flat_map(|group| group.rows.iter())
    }
}

/// One group of rows under one heading.
struct Group {
    /// What names this fold: the section, plus the subgroup when its rows have
    /// one. Rows with no second heading carry the section's own id, because
    /// a heading nobody can see is not one anybody can reopen.
    id: GroupId,
    rows: Vec<Row>,
    collapsed: bool,
}

/// One line of the browser.
struct Row {
    id: RowId,
    name: String,
    /// The cached measurements, already formatted. Empty when nothing has been
    /// measured yet, which for audio is the normal state until it is selected.
    detail: String,
    /// True when what was measured is a fault: a silent file, a clipped one.
    warn: bool,
}

/// Build the filter box and the list it filters.
pub(super) fn build_browser(mut commands: Commands, panel: Query<Entity, With<LibraryPanel>>) {
    let Ok(panel) = panel.single() else {
        return;
    };
    commands
        .entity(panel)
        .insert(TabGroup::default())
        .with_children(|column| {
            // The field goes above the scrolling area, not inside it: a filter
            // that scrolls away is a filter you cannot clear without finding
            // it again. It is labelled because an empty box is not obviously
            // a search — and with nothing typed in it, it looks like nothing.
            column.spawn((
                Node {
                    margin: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(8.0), Val::Px(2.0)),
                    ..default()
                },
                widgets::section_header("FILTER  name, prompt, tag"),
            ));
            column.spawn((
                FilterField,
                widgets::text_field(
                    "",
                    0,
                    Node {
                        margin: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(0.0), Val::Px(4.0)),
                        ..default()
                    },
                ),
            ));
            // Between the field and the list, because it is the same question
            // asked a faster way: the strip is what says which tags exist at
            // all, and a tag nobody can see is one only its author can search
            // for. It sits outside the scrolling area for the same reason the
            // field does — a control that scrolls away is one you have to go
            // back and find before you can undo it.
            column.spawn((
                TagStrip,
                Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(4.0),
                    row_gap: Val::Px(4.0),
                    margin: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(0.0), Val::Px(4.0)),
                    ..default()
                },
            ));
            column.spawn((LibraryList, widgets::scroll_column()));
            // Under the list and outside it, at a fixed height, so a message
            // appearing cannot reflow the rows a reviewer is about to click.
            column.spawn((
                StageStatus,
                Node {
                    height: Val::Px(22.0),
                    margin: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(2.0), Val::Px(2.0)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                TextLayout::no_wrap(),
                theme::label("", theme::FONT_SMALL, theme::TEXT_DIM),
            ));
        });
}

/// Keep the stage-status line in step with the rig.
///
/// Progress and notes — "loading …", "… on stage", the bare rig's "no visible
/// mesh" — read dim; a refusal reads in the warn colour, because the stage
/// keeps the previous model and colour is what stops "it refused" looking
/// like "nothing happened".
pub(super) fn stage_status(
    rig: Res<Rig>,
    label: Query<Entity, With<StageStatus>>,
    mut texts: Query<&mut Text>,
    mut colours: Query<&mut TextColor>,
) {
    let Ok(entity) = label.single() else {
        return;
    };
    // Clamped to the column, keeping the head: "cannot load bodies/x.glb"
    // says everything, and the full reason is in the log. Text on the label's
    // own entity is not clipped by its Node, so an unclamped line would run
    // out of the column and over the viewport's keyboard hint.
    let status = rig.status();
    let line: String = if status.chars().count() <= STATUS_CHARS {
        status.to_owned()
    } else {
        let mut head: String = status.chars().take(STATUS_CHARS - 3).collect();
        head.push_str("...");
        head
    };
    widgets::set_text(&mut texts, Some(entity), &line);
    if let Ok(mut colour) = colours.get_mut(entity) {
        colour.0 = if rig.is_troubled() {
            theme::WARN
        } else {
            theme::TEXT_DIM
        };
    }
}

/// Keep the filter field and the browser state in step, in both directions.
pub(super) fn filter_field(
    mut fields: Query<&mut EditableText, With<FilterField>>,
    mut view: ResMut<LibraryView>,
    mut seen: Local<String>,
) {
    let Ok(mut field) = fields.single_mut() else {
        return;
    };
    let before = view.filter.clone();
    widgets::bind_field(&mut field, &mut view.filter, &mut seen);
    if view.filter != before {
        view.touch();
    }
}

/// Notice that the library grew, or that something finished being measured.
///
/// A take is materialised into [`ClipLibrary`] without the browser being
/// told, and audio measures itself lazily when first selected. Both change
/// what the rows should say, and comparing a cheap signature is more precise
/// than `Res::is_changed` — which also fires when the graph hands out node
/// indices, and would rebuild every row for that.
pub(super) fn notice_changes(
    library: Res<ClipLibrary>,
    audio: Res<AudioLibrary>,
    mut view: ResMut<LibraryView>,
    mut seen: Local<Option<(usize, usize, usize)>>,
) {
    let signature = (library.items.len(), audio.assets().len(), audio.measured());
    if *seen == Some(signature) {
        return;
    }
    *seen = Some(signature);
    view.touch();
}

/// Rebuild the rows and the tag strip, sectioned, folded and filtered.
pub(super) fn rebuild_rows(
    mut commands: Commands,
    library: Res<ClipLibrary>,
    audio: Res<AudioLibrary>,
    models: Res<ModelLibrary>,
    mut view: ResMut<LibraryView>,
    list: Query<Entity, With<LibraryList>>,
    mut strip: Query<(Entity, &mut Node), With<TagStrip>>,
) {
    if view.generation == view.drawn_generation {
        return;
    }
    let Ok(list) = list.single() else {
        return;
    };
    view.drawn_generation = view.generation;

    let sections = build_sections(&library, &audio, &models, &view);
    // Assets only, which is what makes the meshes click-only. The arrow keys
    // move the [`Selection`], and a selection is what the transport plays and
    // what [`crate::studio::metadata`] describes — a mesh answers to neither,
    // so stepping onto one would blank the right half of the window and stop
    // the playhead to say "you are on a mesh now". A mesh is picked once and
    // then reviewed *through*, so a click is the whole interaction it needs.
    view.rows = sections
        .iter()
        .flat_map(SectionView::visible_rows)
        .filter_map(|row| row.id.asset().cloned())
        .collect();
    // What is *listed*, not what is on screen: a shut fold is not an empty
    // library, and telling somebody who folded AUDIO away that nothing matches
    // would be the browser lying about the state it was just put in.
    let empty = sections.iter().all(|section| section.count() == 0);
    let filtered = !view.filter.trim().is_empty() || !view.active_tags.is_empty();

    if let Ok((strip, mut node)) = strip.single_mut() {
        rebuild_tag_strip(
            &mut commands,
            strip,
            &mut node,
            tag_counts(&library, &audio, &models),
            view.active_tags.clone(),
        );
    }

    commands.entity(list).despawn_children();
    commands.entity(list).with_children(move |list| {
        for section in sections {
            let held = section.count();
            list.spawn((
                CollapseToggle((section.id, None)),
                widgets::collapse_header(
                    section.id.title(),
                    held,
                    section.collapsed,
                    Node::default(),
                ),
            ));
            if section.collapsed {
                continue;
            }
            for group in section.groups {
                // Absent for rows with no second heading of their own.
                if let Some(heading) = group.id.1.clone() {
                    list.spawn((
                        CollapseToggle(group.id.clone()),
                        widgets::collapse_header(
                            heading,
                            group.rows.len(),
                            group.collapsed,
                            Node {
                                padding: UiRect::left(Val::Px(8.0)),
                                ..default()
                            },
                        ),
                    ));
                }
                if group.collapsed {
                    continue;
                }
                for row in group.rows {
                    let resting = if row.warn { theme::WARN } else { theme::TEXT };
                    list.spawn((LibraryRow(row.id), widgets::list_row_frame(3.0)))
                        .with_children(|line| {
                            line.spawn((
                                RowName(resting),
                                // Long names are clipped rather than wrapped:
                                // a row that grows to two lines moves every
                                // row under it out from under the cursor.
                                Node {
                                    flex_shrink: 1.0,
                                    min_width: Val::Px(0.0),
                                    overflow: Overflow::clip_x(),
                                    ..default()
                                },
                                TextLayout::no_wrap(),
                                theme::label(row.name, theme::FONT, resting),
                            ));
                            if !row.detail.is_empty() {
                                line.spawn((
                                    Node {
                                        flex_shrink: 0.0,
                                        ..default()
                                    },
                                    TextLayout::no_wrap(),
                                    theme::label(row.detail, theme::FONT_SMALL, theme::TEXT_DIM),
                                ));
                            }
                        });
                }
            }
        }
        if empty {
            list.spawn(theme::label(
                if filtered {
                    "nothing matches"
                } else {
                    "no assets found"
                },
                theme::FONT,
                theme::WARN,
            ));
        }
    });
}

/// The whole browser as headings and rows, filter applied and folds marked.
///
/// Folded rows are still built: a shut heading has to say how many rows it
/// holds, and the caller is the one that decides not to spawn them.
fn build_sections(
    library: &ClipLibrary,
    audio: &AudioLibrary,
    models: &ModelLibrary,
    view: &LibraryView,
) -> Vec<SectionView> {
    let mut sections: Vec<SectionView> = Vec::new();

    // The meshes first: a body is what everything else is judged on, and the
    // accent on its name is what says which one is standing. Each family is
    // one group of its own rather than a heading per directory — a subgroup
    // called "bodies" under a section already saying BODIES says nothing.
    for (section, kind) in [
        (Section::Bodies, Kind::Body),
        (Section::Models, Kind::Model),
    ] {
        let mut meshes = Vec::new();
        for model in &models.models {
            // Filtered by the same question everything else is asked, which
            // for a mesh with no record means the text matches its name and a
            // tag chip takes the whole section away — honestly, since none of
            // them carry that tag.
            if model.kind != kind || !view.matches(&model.name, model.sidecar.as_ref()) {
                continue;
            }
            meshes.push(Row {
                id: RowId::Model(model.rel_path.clone()),
                name: model.name.clone(),
                detail: mesh_detail(model.sidecar.as_ref()),
                warn: false,
            });
        }
        if !meshes.is_empty() {
            sections.push(fold(
                view,
                section,
                vec![Group {
                    id: (section, None),
                    rows: meshes,
                    collapsed: false,
                }],
            ));
        }
    }

    let mut clips = Vec::new();
    for item in &library.items {
        if item.origin != ClipOrigin::Shipped
            || !view.matches(&item.entry.name, item.entry.sidecar.as_ref())
        {
            continue;
        }
        clips.push(Row {
            id: RowId::Asset(item.key.clone()),
            name: item.entry.name.clone(),
            detail: clip_detail(&item.entry),
            warn: false,
        });
    }
    if !clips.is_empty() {
        sections.push(fold(
            view,
            Section::Clips,
            vec![Group {
                id: (Section::Clips, None),
                rows: clips,
                collapsed: false,
            }],
        ));
    }

    // Session takes are their own section rather than being mixed in with the
    // library: what is shipped and what is a candidate is the single most
    // important distinction in the window, and interleaving them by name would
    // hide it.
    let mut takes = Vec::new();
    for item in &library.items {
        if item.origin != ClipOrigin::Candidate
            || !view.matches(&item.entry.name, item.entry.sidecar.as_ref())
        {
            continue;
        }
        takes.push(Row {
            id: RowId::Asset(item.key.clone()),
            name: item.entry.name.clone(),
            detail: String::new(),
            warn: false,
        });
    }
    if !takes.is_empty() {
        sections.push(fold(
            view,
            Section::Takes,
            vec![Group {
                id: (Section::Takes, None),
                rows: takes,
                collapsed: false,
            }],
        ));
    }

    let mut sounds: Vec<Group> = Vec::new();
    for asset in audio.assets() {
        if !view.matches(&asset.name, asset.sidecar.as_ref()) {
            continue;
        }
        push_row(
            &mut sounds,
            (Section::Audio, Some(asset.kind.as_str().to_owned())),
            Row {
                id: RowId::Asset(asset.key.clone()),
                name: asset.name.clone(),
                detail: asset.detail(),
                warn: asset.is_faulty(),
            },
        );
    }
    if !sounds.is_empty() {
        sections.push(fold(view, Section::Audio, sounds));
    }
    sections
}

/// Add a row to its group, keeping the groups in first-seen order.
fn push_row(groups: &mut Vec<Group>, id: GroupId, row: Row) {
    match groups.iter_mut().find(|group| group.id == id) {
        Some(group) => group.rows.push(row),
        None => groups.push(Group {
            id,
            rows: vec![row],
            collapsed: false,
        }),
    }
}

/// Mark a section and its groups with the folds the view is holding shut.
///
/// Applied here rather than while the rows are gathered so that the state lives
/// in one place: every group in the browser is asked the same question once,
/// and a section that is shut does not have to be pushed down into its groups.
fn fold(view: &LibraryView, id: Section, mut groups: Vec<Group>) -> SectionView {
    for group in &mut groups {
        group.collapsed = view.is_collapsed(&group.id);
    }
    SectionView {
        id,
        collapsed: view.is_collapsed(&(id, None)),
        groups,
    }
}

/// Every tag in the library, and how many assets carry it.
///
/// Counted across the whole library rather than across what is currently shown,
/// so the numbers hold still while you type: a count that fell to zero as the
/// filter narrowed would make a chip look spent at exactly the moment it is the
/// thing that would find what you are after.
fn tag_counts(
    library: &ClipLibrary,
    audio: &AudioLibrary,
    models: &ModelLibrary,
) -> BTreeMap<String, usize> {
    let clips = library
        .items
        .iter()
        .filter_map(|item| item.entry.sidecar.as_ref());
    let sounds = audio
        .assets()
        .iter()
        .filter_map(|asset| asset.sidecar.as_ref());
    let meshes = models
        .models
        .iter()
        .filter_map(|model| model.sidecar.as_ref());

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for sidecar in clips.chain(sounds).chain(meshes) {
        for tag in &sidecar.tags {
            *counts.entry(tag.clone()).or_default() += 1;
        }
    }
    counts
}

/// Redraw the tag strip, and hide it outright when there is nothing to show.
///
/// Hidden rather than left empty because an empty flex node still holds its
/// margins open, and eight pixels of unexplained gap over a list is the kind of
/// thing somebody later spends an afternoon looking for.
fn rebuild_tag_strip(
    commands: &mut Commands,
    strip: Entity,
    node: &mut Node,
    counts: BTreeMap<String, usize>,
    active: BTreeSet<String>,
) {
    let display = if counts.is_empty() {
        Display::None
    } else {
        Display::Flex
    };
    if node.display != display {
        node.display = display;
    }
    commands.entity(strip).despawn_children();
    commands.entity(strip).with_children(move |strip| {
        for (tag, count) in counts {
            let on = active.contains(&tag);
            // The count reads as part of the label rather than as a second
            // chip: "loop 9" is one thing to click, and two nodes there would
            // wrap independently once the strip has a few tags in it.
            strip.spawn((
                TagToggle(tag.clone()),
                widgets::toggle_chip(format!("{tag} {count}"), on),
            ));
        }
    });
}

/// What a clip's sidecar measured, in the space a row has for it.
fn clip_detail(entry: &ClipEntry) -> String {
    match (entry.duration_s(), entry.avg_speed_mps()) {
        (Some(duration), Some(speed)) => format!("{duration:.1}s {speed:.1}m/s"),
        (Some(duration), None) => format!("{duration:.1}s"),
        (None, Some(speed)) => format!("{speed:.1}m/s"),
        (None, None) => String::new(),
    }
}

/// What a mesh's sidecar measured, in the space a row has for it: the
/// triangle budget and the height, which are the two numbers a lift is
/// gated on. Empty when nothing measured it.
fn mesh_detail(sidecar: Option<&Sidecar>) -> String {
    sidecar
        .and_then(|s| s.measured.as_ref())
        .and_then(|m| m.mesh.as_ref())
        .map_or_else(String::new, |mesh| {
            let tris = if mesh.triangles >= 1000 {
                format!("{:.1}k", mesh.triangles as f32 / 1000.0)
            } else {
                mesh.triangles.to_string()
            };
            format!("{tris} tris {:.2}m", mesh.height())
        })
}

// ------------------------------------------------------------------ input ---

/// Clicking a row shows that asset, or puts that mesh on the stage.
///
/// The two are deliberately not the same gesture in disguise: a mesh row
/// leaves the selection exactly where it is, so the clip being reviewed keeps
/// playing and the panel keeps describing it, on a different body. Swapping to
/// the mesh already standing is allowed through — [`SwapModel`] treats it as a
/// reload, which is how a mesh edited on disk is seen again.
pub(super) fn row_buttons(
    rows: Query<(&Interaction, &LibraryRow), Changed<Interaction>>,
    mut selection: ResMut<Selection>,
    mut swaps: MessageWriter<SwapModel>,
) {
    for (interaction, row) in &rows {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match &row.0 {
            RowId::Asset(key) => selection.select(key.clone()),
            RowId::Model(path) => {
                swaps.write(SwapModel::new(path.clone()));
            }
        }
    }
}

/// Clicking a heading folds it away, or opens it again.
pub(super) fn collapse_headers(
    headers: Query<(&Interaction, &CollapseToggle), Changed<Interaction>>,
    mut view: ResMut<LibraryView>,
) {
    for (interaction, header) in &headers {
        if *interaction == Interaction::Pressed {
            view.toggle_collapsed(header.0.clone());
        }
    }
}

/// Clicking a tag chip filters by that tag as well, or stops.
pub(super) fn tag_chips(
    chips: Query<(&Interaction, &TagToggle), Changed<Interaction>>,
    mut view: ResMut<LibraryView>,
) {
    for (interaction, chip) in &chips {
        if *interaction == Interaction::Pressed {
            view.toggle_tag(&chip.0);
        }
    }
}

/// Open whatever fold the selection has just moved into.
///
/// Somebody else moves the selection as often as you do — `--take` lands on
/// the candidate it just made, the self-test walks every sound — and it can
/// land inside a fold that is shut. Without this the row is never spawned, so
/// [`reveal_selection`] spends its handful of attempts looking for a row that
/// does not exist and then gives up without saying anything: the panel on the
/// right describes an asset the list does not show.
pub(super) fn expand_to_selection(
    selection: Res<Selection>,
    library: Res<ClipLibrary>,
    audio: Res<AudioLibrary>,
    mut view: ResMut<LibraryView>,
    mut last: Local<Option<AssetKey>>,
) {
    let showing = selection.key().cloned();
    if *last == showing {
        return;
    }
    last.clone_from(&showing);
    let Some(group) = showing.and_then(|key| group_of(&key, &library, &audio)) else {
        return;
    };
    view.expand(&group);
}

/// The fold an asset's row lives in, when it has one at all.
fn group_of(key: &AssetKey, library: &ClipLibrary, audio: &AudioLibrary) -> Option<GroupId> {
    if key.is_audio() {
        let asset = audio.get(key)?;
        return Some((Section::Audio, Some(asset.kind.as_str().to_owned())));
    }
    let item = library.get(key)?;
    Some(match item.origin {
        ClipOrigin::Shipped => (Section::Clips, None),
        ClipOrigin::Candidate => (Section::Takes, None),
    })
}

// ---------------------------------------------------------------- refresh ---

/// Scroll the browser so the selected row is on screen.
///
/// Without this the arrow keys walk the selection off the bottom of the list
/// and nothing appears to happen: the clip changes, the panel changes, and the
/// row that says which one is showing is below the fold.
///
/// The attempt is retried for a few frames because the rows are rebuilt by
/// [`rebuild_rows`] in the same schedule and are not laid out until the frame
/// after, so the first look finds a row of zero height.
pub(super) fn reveal_selection(
    selection: Res<Selection>,
    rows: Query<(&LibraryRow, &ComputedNode, &UiGlobalTransform)>,
    mut lists: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<LibraryList>>,
    mut wanted: Local<Option<(AssetKey, u32)>>,
    mut last: Local<Option<AssetKey>>,
) {
    let showing = selection.key().cloned();
    if *last != showing {
        last.clone_from(&showing);
        // A handful of frames: enough for a rebuild to be laid out, few enough
        // that a row the filter is hiding stops being looked for.
        *wanted = showing.map(|key| (key, 8));
    }
    let Some((key, attempts)) = wanted.as_mut() else {
        return;
    };
    *attempts -= 1;
    let expired = *attempts == 0;
    let key = key.clone();

    if let Ok((list_node, list_at, mut scroll)) = lists.single_mut()
        && let Some((_, row_node, row_at)) =
            rows.iter().find(|(row, ..)| row.0.asset() == Some(&key))
        && row_node.size().y > 0.0
    {
        widgets::scroll_into_view((list_node, list_at), (row_node, row_at), &mut scroll);
        *wanted = None;
        return;
    }
    if expired {
        *wanted = None;
    }
}

/// Mark the selected row, and the mesh standing on the stage.
///
/// Colour rather than a background: thirty rows of block highlight is a wall,
/// and the accent reads at a glance against dim text. Only the name is
/// recoloured — the measurement stays dim, because it is a fact about the
/// asset and not a statement about which one is showing.
///
/// One mark for both questions because there is one of each on screen and they
/// are in different sections: the accent under CLIPS says "playing", the
/// accent under BODIES says "standing". Done here, per frame, rather than
/// baked into the rows, so a swap landing moves the mark without a rebuild.
pub(super) fn refresh(
    selection: Res<Selection>,
    active: Res<ActiveModel>,
    rows: Query<(&LibraryRow, &Children)>,
    mut names: Query<(&RowName, &mut TextColor)>,
) {
    for (row, children) in &rows {
        let marked = row.0.is_marked(&selection, &active);
        for child in children {
            let Ok((name, mut colour)) = names.get_mut(*child) else {
                continue;
            };
            let wanted = if marked { theme::ACCENT } else { name.0 };
            if colour.0 != wanted {
                colour.0 = wanted;
            }
        }
    }
}

// ------------------------------------------------------------ materialise ---

/// Turn a raw take into a playable clip and add it to the library.
///
/// This is the whole path from bytes to something the transport can scrub:
/// build the clip in the rig's bone frames, give it a node in the shared graph,
/// and record it. Sharing it is the point — a `--take` on the command line and
/// a candidate a test materialises become playable the same way, so playback,
/// orbit and metadata need no new code for either of them.
///
/// `None` when the rig is not ready yet, which is not an error: the caller
/// should say so and try again.
pub fn materialise_take(
    library: &mut ClipLibrary,
    clips: &mut Assets<AnimationClip>,
    graphs: &mut Assets<AnimationGraph>,
    rig: &Rig,
    take: &Take,
    entry: ClipEntry,
    origin: ClipOrigin,
) -> Option<AssetKey> {
    let rest_frames = rig.rest_frames()?;
    let graph_handle = rig.graph()?;
    let mut graph = graphs.get_mut(graph_handle)?;
    let graph_root = graph.root;
    let handle = clips.add(npz_clip::build(take, rest_frames));
    let node = graph.add_clip(handle.clone(), 1.0, graph_root);
    Some(library.push(entry, handle, Some(node), origin))
}

/// Whether every shipped clip has either loaded or failed for good.
///
/// The graph cannot be built until the answer is yes: a graph missing nodes
/// would silently renumber the indices the library holds. A clip that
/// *failed* counts as resolved — it will never play, and a library with one
/// corrupt file in it is still a library with a rig worth standing up.
pub(super) fn clips_resolved(
    library: &ClipLibrary,
    clips: &Assets<AnimationClip>,
    server: &AssetServer,
) -> bool {
    library.items.iter().all(|item| {
        clips.get(&item.handle).is_some()
            || matches!(
                server.get_load_state(&item.handle),
                Some(LoadState::Failed(_))
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> ClipEntry {
        ClipEntry::bare(format!("clips/{name}.glb"), name)
    }

    /// The keys the browser would spawn rows for, in the order it spawns them.
    ///
    /// The same expression [`rebuild_rows`] assigns to `view.rows`, which is
    /// what the arrow keys step through — so a fold that hides a row here is a
    /// fold the arrow keys skip over there, and a mesh is absent from it
    /// wherever it appears on screen.
    fn listed_by(library: &ClipLibrary, view: &LibraryView) -> Vec<AssetKey> {
        steppable(&build_sections(
            library,
            &AudioLibrary::default(),
            &ModelLibrary::default(),
            view,
        ))
    }

    /// What the arrow keys can reach, out of sections already built.
    fn steppable(sections: &[SectionView]) -> Vec<AssetKey> {
        sections
            .iter()
            .flat_map(SectionView::visible_rows)
            .filter_map(|row| row.id.asset().cloned())
            .collect()
    }

    /// A body and a prop, in the order they are found.
    fn two_meshes() -> ModelLibrary {
        let mut models = ModelLibrary::default();
        models.push("vex_runner", "bodies/vex_runner.glb", Kind::Body);
        models.push("barrel", "models/barrel.glb", Kind::Model);
        models
    }

    /// Every row one section holds, by name.
    fn names_in(sections: &[SectionView], id: Section) -> Vec<String> {
        sections
            .iter()
            .filter(|section| section.id == id)
            .flat_map(SectionView::visible_rows)
            .map(|row| row.name.clone())
            .collect()
    }

    /// A record carrying nothing but these tags.
    fn tagged(tags: &[&str]) -> Sidecar {
        let mut sidecar = Sidecar::new(Kind::Clip, "roll");
        sidecar.tags = tags.iter().map(|tag| (*tag).to_owned()).collect();
        sidecar
    }

    /// A take materialised beside the clip it was promoted from deliberately
    /// produces a second item with the same display name. If both answered to
    /// the same key the selection would be ambiguous.
    #[test]
    fn two_clips_with_one_name_still_get_one_key_each() {
        let mut library = ClipLibrary::default();
        let first = library.push(entry("roll"), Handle::default(), None, ClipOrigin::Shipped);
        let second = library.push(
            entry("roll"),
            Handle::default(),
            None,
            ClipOrigin::Candidate,
        );

        assert_ne!(first, second);
        assert_eq!(
            library.get(&first).map(|i| i.origin),
            Some(ClipOrigin::Shipped)
        );
        assert_eq!(
            library.get(&second).map(|i| i.origin),
            Some(ClipOrigin::Candidate)
        );
        // The name the panels show is untouched by the disambiguation.
        assert_eq!(
            library.get(&second).map(|i| i.entry.name.as_str()),
            Some("roll")
        );
    }

    /// A rebake refreshes the item in place — same key, new sidecar — rather
    /// than growing a second row with the same name. Only the *shipped* item
    /// at that path answers: a candidate with the same name is its own row.
    #[test]
    fn a_rebake_refreshes_the_shipped_item_rather_than_duplicating_it() {
        let mut library = ClipLibrary::default();
        let shipped = library.push(entry("roll"), Handle::default(), None, ClipOrigin::Shipped);
        library.push(
            entry("roll"),
            Handle::default(),
            None,
            ClipOrigin::Candidate,
        );

        let refreshed = library.refresh_shipped("clips/roll.glb", Some(tagged(&["oneshot"])));

        assert_eq!(refreshed, Some(shipped.clone()));
        assert_eq!(
            library
                .get(&shipped)
                .and_then(|i| i.entry.sidecar.as_ref())
                .map(|s| s.tags.clone()),
            Some(vec![String::from("oneshot")])
        );
        // A path new to the library is the caller's cue to push instead.
        assert_eq!(library.refresh_shipped("clips/new.glb", None), None);
        // And a file that is gone takes its row with it, shipped only.
        assert_eq!(library.remove_shipped("clips/roll.glb"), Some(shipped));
        assert_eq!(library.items().len(), 1);
        assert_eq!(library.remove_shipped("clips/roll.glb"), None);
    }

    #[test]
    fn stepping_wraps_through_what_is_visible() {
        let view = LibraryView {
            rows: vec![AssetKey::clip("a"), AssetKey::audio("b")],
            ..LibraryView::default()
        };
        let a = AssetKey::clip("a");
        let b = AssetKey::audio("b");

        assert_eq!(view.step(Some(&a), 1).as_ref(), Some(&b));
        assert_eq!(view.step(Some(&b), 1).as_ref(), Some(&a));
        assert_eq!(view.step(Some(&a), -1).as_ref(), Some(&b));
        // A key the filter has hidden is not a position in the visible list, so
        // stepping from it starts at the top rather than at nothing.
        assert_eq!(
            view.step(Some(&AssetKey::clip("gone")), 1).as_ref(),
            Some(&b)
        );
        assert_eq!(LibraryView::default().step(None, 1), None);
    }

    /// The filter is how an asset gets found by what it *is* rather than by
    /// what it is called, so the prompt and the tags have to be searched too.
    #[test]
    fn the_filter_matches_name_prompt_and_tags() {
        let mut sidecar = Sidecar::new(Kind::Clip, "roll");
        sidecar.prompt = Some("A person dives forward into a roll.".to_owned());
        sidecar.tags = vec!["loop".to_owned()];

        let matching = |needle: &str| {
            let view = LibraryView {
                filter: needle.to_owned(),
                ..LibraryView::default()
            };
            view.matches("roll", Some(&sidecar))
        };

        assert!(matching(""), "an empty filter hides nothing");
        assert!(matching("ROLL"), "the name, case-insensitively");
        assert!(matching("dives"), "the prompt");
        assert!(matching("loop"), "a tag");
        assert!(!matching("sword"));
        // Without a record there is nothing but the name to go on, and that
        // must not become "matches everything".
        let view = LibraryView {
            filter: "dives".to_owned(),
            ..LibraryView::default()
        };
        assert!(!view.matches("roll", None));
    }

    /// A fold is not a way of hiding rows from the eye only: the arrow keys
    /// step through what the browser listed, so a row inside a shut fold must
    /// be absent from that list — otherwise pressing Down walks the selection
    /// into a group nobody can see and the window changes for no visible
    /// reason.
    #[test]
    fn a_shut_fold_is_a_row_the_arrow_keys_cannot_reach() {
        let mut library = ClipLibrary::default();
        let roll = library.push(entry("roll"), Handle::default(), None, ClipOrigin::Shipped);
        let take = library.push(
            entry("roll"),
            Handle::default(),
            None,
            ClipOrigin::Candidate,
        );

        let mut view = LibraryView::default();
        assert_eq!(listed_by(&library, &view), vec![roll.clone(), take.clone()]);

        // One section shut leaves the other one listed...
        view.toggle_collapsed((Section::Clips, None));
        assert_eq!(listed_by(&library, &view), vec![take.clone()]);

        // ...and clicking the heading a second time puts it back as it was.
        view.toggle_collapsed((Section::Clips, None));
        assert_eq!(listed_by(&library, &view), vec![roll, take]);
    }

    /// The stage's mesh is picked from the browser, so what was found on disk
    /// has to reach it: one row per mesh, bodies in their own section ahead
    /// of the props, because a body is what every clip is judged on.
    #[test]
    fn bodies_and_models_are_listed_first_and_are_click_only() {
        let mut library = ClipLibrary::default();
        library.push(entry("roll"), Handle::default(), None, ClipOrigin::Shipped);

        let view = LibraryView::default();
        let sections = build_sections(&library, &AudioLibrary::default(), &two_meshes(), &view);

        assert_eq!(
            sections.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![Section::Bodies, Section::Models, Section::Clips]
        );
        assert_eq!(names_in(&sections, Section::Bodies), vec!["vex_runner"]);
        assert_eq!(names_in(&sections, Section::Models), vec!["barrel"]);
        assert_eq!(Section::Bodies.title(), "BODIES");
        // Nothing measured these, so nothing is invented for the column.
        let meshes: Vec<&Row> = sections
            .iter()
            .filter(|section| section.id != Section::Clips)
            .flat_map(SectionView::visible_rows)
            .collect();
        assert!(meshes.iter().all(|row| row.detail.is_empty() && !row.warn));

        // Click-only: the meshes are on screen and the arrow keys walk past
        // them, because stepping moves the selection and a mesh is not one.
        assert_eq!(
            steppable(&sections),
            vec![AssetKey::clip("roll")],
            "a mesh row is not somewhere the arrow keys can land"
        );

        // And a project with no meshes has no sections rather than empty ones.
        assert!(
            build_sections(
                &library,
                &AudioLibrary::default(),
                &ModelLibrary::default(),
                &view
            )
            .iter()
            .all(|section| section.id == Section::Clips)
        );
    }

    /// The mark says what is *standing*, which is not the same as what was
    /// clicked: the swap takes as long as it takes, and a failed one leaves the
    /// previous mesh on the stage. A marker that moved on the click would
    /// spend that time — or the rest of the session, after a failure — naming
    /// a mesh nobody can see.
    #[test]
    fn the_marked_mesh_is_the_one_on_the_stage_and_not_the_one_asked_for() {
        let barrel = RowId::Model(String::from("models/barrel.glb"));
        let body = RowId::Model(String::from("bodies/vex_runner.glb"));
        let selection = Selection::default();

        // What the window opens on, before anything has stood up.
        let mut active = ActiveModel {
            path: String::from("bodies/vex_runner.glb"),
            generation: 0,
        };
        assert!(body.is_marked(&selection, &active));
        assert!(!barrel.is_marked(&selection, &active));

        // A swap has been asked for. Nothing about the mark changes until the
        // rig stands up, which is what rewrites the path and bumps the count.
        assert!(!barrel.is_marked(&selection, &active));
        active = ActiveModel {
            path: String::from("models/barrel.glb"),
            generation: 1,
        };
        assert!(barrel.is_marked(&selection, &active));
        assert!(!body.is_marked(&selection, &active));

        // And the selection has no say in it, either way round.
        let mut selection = Selection::default();
        selection.select(AssetKey::clip("roll"));
        assert!(barrel.is_marked(&selection, &active));
        assert!(!RowId::Asset(AssetKey::clip("walk")).is_marked(&selection, &active));
        assert!(RowId::Asset(AssetKey::clip("roll")).is_marked(&selection, &active));
    }

    /// The whole point of picking a mesh from the browser is to keep reviewing
    /// the clip you were reviewing, on a different body. A click that also
    /// moved the selection would restart the window on some other asset.
    #[test]
    fn clicking_a_mesh_asks_for_a_swap_and_leaves_the_selection_alone() {
        #[derive(Resource, Default)]
        struct Asked(Vec<String>);

        fn collect(mut requests: MessageReader<SwapModel>, mut asked: ResMut<Asked>) {
            asked.0.extend(requests.read().map(|r| r.path.clone()));
        }

        let mut app = App::new();
        app.init_resource::<Selection>()
            .init_resource::<Asked>()
            .add_message::<SwapModel>()
            .add_systems(Update, (row_buttons, collect).chain());
        let showing = AssetKey::clip("roll");
        app.world_mut()
            .resource_mut::<Selection>()
            .select(showing.clone());

        let model = app
            .world_mut()
            .spawn((
                LibraryRow(RowId::Model(String::from("models/barrel.glb"))),
                Interaction::Pressed,
            ))
            .id();
        app.update();

        assert_eq!(
            app.world().resource::<Asked>().0,
            vec![String::from("models/barrel.glb")]
        );
        assert_eq!(
            app.world().resource::<Selection>().key(),
            Some(&showing),
            "picking a mesh must not change what is playing on it"
        );

        // An asset row still selects, and asks for no swap.
        app.world_mut().entity_mut(model).despawn();
        app.world_mut().spawn((
            LibraryRow(RowId::Asset(AssetKey::clip("walk"))),
            Interaction::Pressed,
        ));
        app.update();
        assert_eq!(
            app.world().resource::<Selection>().key(),
            Some(&AssetKey::clip("walk"))
        );
        assert_eq!(app.world().resource::<Asked>().0.len(), 1);
    }

    /// BODIES folds like everything else. It has no subgroups, so the
    /// section's own heading is the only one there is, and a section that
    /// could not be folded would be permanent rows between the top of the
    /// list and the clips.
    #[test]
    fn the_mesh_sections_fold_shut_like_the_rest() {
        let models = two_meshes();
        let mut view = LibraryView::default();
        let listed = |view: &LibraryView| {
            names_in(
                &build_sections(
                    &ClipLibrary::default(),
                    &AudioLibrary::default(),
                    &models,
                    view,
                ),
                Section::Bodies,
            )
        };
        assert_eq!(listed(&view).len(), 1);

        view.toggle_collapsed((Section::Bodies, None));
        assert!(listed(&view).is_empty(), "shut, so nothing is on screen");
        // Shut is not empty: the header still says how many it is holding.
        let sections = build_sections(
            &ClipLibrary::default(),
            &AudioLibrary::default(),
            &models,
            &view,
        );
        let section = sections
            .iter()
            .find(|section| section.id == Section::Bodies)
            .expect("BODIES is still a section when it is folded");
        assert!(section.collapsed);
        assert_eq!(section.count(), 1);

        view.toggle_collapsed((Section::Bodies, None));
        assert_eq!(listed(&view).len(), 1);

        // The filter reaches them too, on the one thing they have: a name.
        view.filter = String::from("barr");
        assert!(listed(&view).is_empty());
        assert_eq!(
            names_in(
                &build_sections(
                    &ClipLibrary::default(),
                    &AudioLibrary::default(),
                    &models,
                    &view,
                ),
                Section::Models,
            ),
            vec!["barrel"]
        );
    }

    /// Every fold is toggled by redrawing the list, so a toggle that does not
    /// bump the generation is a click that does nothing at all until something
    /// else happens to ask for a redraw.
    #[test]
    fn folding_a_heading_asks_for_the_list_to_be_redrawn() {
        let mut view = LibraryView::default();
        let before = view.generation;
        view.toggle_collapsed((Section::Audio, None));
        assert_ne!(view.generation, before);

        let before = view.generation;
        view.toggle_tag("loop");
        assert_ne!(view.generation, before);
    }

    /// The selection moves on its own — `--take` lands on the candidate it
    /// just made — and it can land inside a fold that is shut. The reveal that
    /// scrolls to a row gives up after a handful of frames, so the fold has to
    /// be open before it starts looking rather than after it has given up.
    #[test]
    fn selecting_into_a_shut_fold_opens_it() {
        let mut library = ClipLibrary::default();
        let roll = library.push(entry("roll"), Handle::default(), None, ClipOrigin::Shipped);
        let mut view = LibraryView::default();
        view.toggle_collapsed((Section::Clips, None));
        assert!(listed_by(&library, &view).is_empty());

        let mut app = App::new();
        app.insert_resource(library)
            .insert_resource(view)
            .init_resource::<AudioLibrary>()
            .init_resource::<Selection>()
            .add_systems(Update, expand_to_selection);
        app.update();
        assert!(
            listed_by(
                app.world().resource::<ClipLibrary>(),
                app.world().resource::<LibraryView>()
            )
            .is_empty(),
            "nothing is selected, so nothing should have been opened"
        );

        app.world_mut()
            .resource_mut::<Selection>()
            .select(roll.clone());
        app.update();

        assert_eq!(
            listed_by(
                app.world().resource::<ClipLibrary>(),
                app.world().resource::<LibraryView>()
            ),
            vec![roll],
            "the section should have opened"
        );
    }

    /// Two chips mean *both*, which is the question the text field cannot ask:
    /// typing two words looks for one string containing both, and clicking two
    /// tags looks for an asset carrying both.
    #[test]
    fn tag_chips_narrow_the_list_rather_than_widening_it() {
        let mut view = LibraryView::default();
        view.toggle_tag("loop");
        assert!(view.matches("roll", Some(&tagged(&["loop", "locomotion"]))));
        assert!(!view.matches("roll", Some(&tagged(&["oneshot"]))));
        // No record at all means no tags, not "matches whatever is asked".
        assert!(!view.matches("roll", None));

        view.toggle_tag("locomotion");
        assert!(!view.matches("roll", Some(&tagged(&["loop"]))));
        assert!(view.matches("roll", Some(&tagged(&["loop", "locomotion"]))));

        // What is typed still applies on top of what is clicked.
        view.filter = String::from("sword");
        assert!(!view.matches("roll", Some(&tagged(&["loop", "locomotion"]))));
        assert!(view.matches("sword_slash", Some(&tagged(&["loop", "locomotion"]))));

        // And clicking a chip again drops it.
        view.filter.clear();
        view.toggle_tag("locomotion");
        view.toggle_tag("loop");
        assert!(view.matches("roll", Some(&tagged(&[]))));
    }

    /// The counts on the chips are of the whole library, not of what is on
    /// screen: a count that fell to zero while you typed would make the one
    /// chip that would find what you are after look like the spent one.
    #[test]
    fn tag_counts_are_of_the_library_and_not_of_the_filtered_list() {
        let mut library = ClipLibrary::default();
        let mut looping = entry("walk");
        looping.sidecar = Some(tagged(&["loop"]));
        library.push(looping, Handle::default(), None, ClipOrigin::Shipped);

        let mut also_looping = entry("jog");
        also_looping.sidecar = Some(tagged(&["loop", "locomotion"]));
        library.push(also_looping, Handle::default(), None, ClipOrigin::Candidate);

        let mut models = ModelLibrary::default();
        models.push("sword", "models/sword.glb", Kind::Model);
        models.models[0].sidecar = Some(tagged(&["weapon"]));

        let counts = tag_counts(&library, &AudioLibrary::default(), &models);
        assert_eq!(counts.get("loop"), Some(&2));
        assert_eq!(counts.get("locomotion"), Some(&1));
        assert_eq!(counts.get("weapon"), Some(&1));
        assert_eq!(counts.len(), 3);
    }

    /// `--model` is typed by a person, so every spelling the catalog accepts
    /// has to land on the same row: the path, the file name, the stem, in any
    /// case. A body wins over a prop of the same stem, because a body is what
    /// the stage is for.
    #[test]
    fn a_mesh_resolves_by_path_file_name_or_stem() {
        let mut models = two_meshes();
        models.push("vex_runner", "models/vex_runner.glb", Kind::Model);
        for wanted in [
            "vex_runner",
            "VEX_RUNNER",
            "vex_runner.glb",
            "bodies/vex_runner.glb",
            "./bodies/vex_runner.glb",
        ] {
            let found = models.resolve(wanted).expect(wanted);
            assert_eq!(found.rel_path, "bodies/vex_runner.glb", "{wanted}");
        }
        assert_eq!(
            models.resolve("models/vex_runner.glb").map(|m| m.kind),
            Some(Kind::Model)
        );
        assert!(models.resolve("").is_none());
        assert!(models.resolve("nope").is_none());
    }

    /// A recipe file is either a recipe or a sidecar holding one, and the
    /// difference matters: a sidecar read as a bare recipe parses — every
    /// field defaults — and silently previews the identity edit.
    #[test]
    fn a_recipe_file_may_be_a_bare_recipe_or_a_whole_sidecar() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bare = dir.path().join("bare.json");
        std::fs::write(&bare, br#"{"trim_start_s": 0.5, "in_place": "strip"}"#).expect("write");
        let recipe = read_recipe(&bare).expect("a bare recipe");
        assert!((recipe.trim_start_s - 0.5).abs() < f32::EPSILON);
        assert_eq!(recipe.in_place, forge_library::schema::InPlaceMode::Strip);

        let mut sidecar = Sidecar::new(Kind::Clip, "roll");
        sidecar.recipe = Some(forge_library::ClipRecipe {
            lean_deg: 12.0,
            ..forge_library::ClipRecipe::default()
        });
        let whole = dir.path().join("roll.json");
        forge_library::sidecar::save(&whole, &sidecar).expect("sidecar");
        let recipe = read_recipe(&whole).expect("a sidecar's recipe");
        assert!((recipe.lean_deg - 12.0).abs() < f32::EPSILON);

        let broken = dir.path().join("broken.json");
        std::fs::write(&broken, b"not json").expect("write");
        assert!(read_recipe(&broken).is_err());
        assert!(read_recipe(&dir.path().join("missing.json")).is_err());
    }
}
