//! The manifest a game reads: `assets/library.json`, as types.
//!
//! Games never parse sidecars. A sidecar answers the authoring questions —
//! who made this, from which take, through which recipe — and it changes
//! shape whenever authoring learns something new. What a game needs is
//! smaller and has to be stabler: which clips exist, how long they run,
//! whether they loop, which rig they bind to. This crate is that contract
//! and nothing else, which is why its whole dependency list is serde and
//! `serde_json` — it is the one authoring-side type a game's build pulls in
//! transitively, and it must never drag an engine or a pipeline with it.
//!
//! Two conventions carry over from the sidecars, because a consumer needs
//! them for the same reasons the library does:
//!
//! - **Null means unknown.** Every [`Option`] serialises as an explicit
//!   `null` rather than being skipped, so a clip whose duration was never
//!   measured says so, and a reader can tell that apart from a field this
//!   schema does not have.
//! - **Refuse newer.** [`Manifest::from_slice`] rejects a document whose
//!   `schema` is above [`SCHEMA`], naming both numbers, because reading a
//!   newer manifest with an older build would silently drop whatever the
//!   newer schema added.
//!
//! Serialisation is deterministic — entries sorted by name, keys in struct
//! order, trailing newline — because the manifest is a committed file kept
//! honest by rebuild-and-byte-compare (`forge manifest --check`), and that
//! check only means something if the same library always serialises to the
//! same bytes.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The manifest schema version this build writes, and the newest it reads.
///
/// # Why 1
///
/// asset-forge starts its manifest over rather than continuing the numbering
/// of the library it was distilled from. The old schema carried three
/// generations of a game's history — a parametric body generator, its
/// garments, a set of state-machine hints — and every one of those fields
/// had become a default that meant "none" rather than a fact a consumer
/// could use. A fresh 1 says the honest thing: nothing older than this
/// document exists for this reader, so there is no compatibility story to
/// carry, and every field present is written on purpose.
///
/// What the reset keeps is the mechanism, not the history: the schema field
/// is the honesty device, and refuse-newer is the promise this crate makes.
/// When 2 arrives, a consumer on 1 will be told "you are behind" rather than
/// handed a document missing what 2 added.
pub const SCHEMA: u64 = 1;

/// Everything a game needs to know about a shipped asset library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Schema version. Always [`SCHEMA`] when written by this build.
    pub schema: u64,
    /// Semver of the `forge` toolkit that wrote this manifest — a statement
    /// about the writer, kept so a consumer's bug report can name it.
    pub forge_version: String,
    /// Semver of the library's content, authored beside the sources — a
    /// statement about the assets, not about any crate that reads them.
    pub library_version: String,
    /// The day this manifest was written, `YYYY-MM-DD`.
    pub generated: String,
    /// The rig every body in `bodies` is skinned to and every clip in `clips`
    /// is expressed on.
    pub rig: RigInfo,
    /// Every shipped humanoid body, sorted by name.
    ///
    /// Separate from [`Self::models`] rather than a `kind` on a model entry:
    /// the two answer different questions. A model is a file a game spawns; a
    /// body is a file that additionally stands on the rig contract — every
    /// clip in [`Self::clips`] plays on it — and a prop must not be able to
    /// claim that.
    pub bodies: Vec<BodyEntry>,
    /// Every shipped model, sorted by name.
    pub models: Vec<ModelEntry>,
    /// Every shipped animation clip, sorted by name.
    pub clips: Vec<ClipEntry>,
    /// Every shipped sound, sorted by name.
    pub audio: Vec<AudioEntry>,
}

/// The rig contract as a consumer needs it: which profile, which revision,
/// the bone table in the order the `.glb` defines it, the attachment points,
/// and the exact bytes of the `.glb` so a bundled copy can be proven to be
/// the one the clips were baked against.
///
/// Inline rather than a pointer at `rigs/<profile>/contract.json` because
/// the manifest is the only file a consumer sees: a game that wants to
/// attach a sword to `hand_r`, or walk the skeleton to find the head, must
/// not have to learn the profile's on-disk layout to do it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigInfo {
    /// The rig profile's name, e.g. `humanoid`.
    pub profile: String,
    /// Contract revision. Bumped when the bone table changes.
    pub version: u64,
    /// How many bones the contract names — always `bones.len()`, restated as
    /// a number so a reader scanning by eye or by `jq` has it without
    /// counting.
    pub bone_count: u32,
    /// `sha256:…` of the rig's `.glb` as shipped, so a consumer can prove the
    /// rig it bundled is the rig the clips were baked against.
    pub glb_sha256: String,
    /// The bone table, in the rig `.glb`'s node order. **Not** sorted by
    /// name, unlike every other array here: `parent` is an index into this
    /// list, and the order is part of the contract.
    pub bones: Vec<RigBone>,
    /// Every attachment point, sorted by name.
    pub sockets: Vec<RigSocket>,
}

/// One bone of the rig contract, reduced to what a consumer binds by.
///
/// Rest transforms stay in the profile's `contract.json`: a game reads them
/// from the `.glb` it already loads, and restating them here would make the
/// manifest a second source of truth for numbers the file carries exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigBone {
    /// The bone's name — the exact string clips target, so it is the identity.
    pub name: String,
    /// Parent's index into [`RigInfo::bones`]; `None` for the root, whose
    /// parent is the armature scene node rather than a bone.
    pub parent: Option<usize>,
    /// Whether clips animate this bone. A bone that is not driven is still
    /// skinnable — the finger leaves — but no clip ever moves it.
    pub driven: bool,
}

/// One attachment point: a named transform offset from a contract bone.
///
/// Metres in the bone's node space (+Y runs along the bone toward its
/// children), rotation `[x, y, z, w]` carrying a prop's authoring frame —
/// long axis +Y, front −Z, grip at the origin — into bone space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigSocket {
    /// The socket's name — the string a game passes at the attach call site.
    pub name: String,
    /// Exact contract bone name; always one of [`RigInfo::bones`].
    pub bone: String,
    /// Offset local to the bone, metres.
    pub translation: [f32; 3],
    /// Offset rotation local to the bone, `[x, y, z, w]`.
    pub rotation: [f32; 4],
}

/// One shipped animation clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipEntry {
    /// File stem — the name everything refers to the clip by.
    pub name: String,
    /// Path relative to the asset root, forward-slashed.
    pub path: String,
    /// `sha256:…` of the file as shipped.
    pub sha256: String,
    /// Seconds of clip, when it was measured.
    pub duration_s: Option<f32>,
    /// Frames per second, when it was measured.
    pub fps: Option<f32>,
    /// Frame count, when it was measured.
    pub frames: Option<u32>,
    /// Whether the clip was baked as a loop.
    pub looped: bool,
    /// Free-form tags, lower-case by convention.
    pub tags: Vec<String>,
    /// What happened to the root's travel at bake time.
    pub root_motion: RootMotionInfo,
    /// Gameplay events on the clip's timeline. Empty when the clip was never
    /// examined for events *and* when it was examined and found silent — the
    /// distinction matters to authoring, not to a game.
    pub events: Vec<ManifestEvent>,
}

/// How a clip's root travel was treated, and what is known about it.
///
/// An unmeasured track is an explicit `null` fps and an empty list, per the
/// null-means-unknown rule; neither field is ever skipped on write.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootMotionInfo {
    /// How net root travel was removed at bake time.
    pub mode: RootMotionMode,
    /// Mean root speed over the clip in metres per second, when it was
    /// measured. This is the pre-strip speed: what a game should move the
    /// character by while an in-place clip plays.
    pub avg_speed_mps: Option<f32>,
    /// Frame rate of `track_xz_m`, when the track was measured. Restates the
    /// clip's fps rather than pointing at [`ClipEntry::fps`] so the track and
    /// its clock travel together — a consumer sampling the curve never has to
    /// reach back up the struct for the one number that times the samples.
    pub fps: Option<f32>,
    /// The pre-strip XZ root position per built frame, metres, in the rig's
    /// ground plane — the travel the bake removed, one `[x, z]` sample per
    /// frame of `fps`, so sample `i` sits at `i / fps` seconds. The plane is
    /// character-local rig space: the character faces −Z, so a forward walk
    /// samples increasingly negative Z and a sample's derivative rotates
    /// into world by the character's rotation alone — no compensation.
    /// Empty when the track was never measured — empty is this field's
    /// `null`.
    pub track_xz_m: Vec<[f32; 2]>,
}

/// How net root travel is removed. Mirrors the recipe's `in_place` modes,
/// restated here so a consumer never has to know the recipe schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RootMotionMode {
    /// The clip's travel is exactly as generated.
    Off,
    /// The hips' X/Z were pinned; the clip plays on the spot.
    Strip,
    /// Linear drift was removed, within-clip surges kept.
    Detrend,
}

impl RootMotionMode {
    /// The lower-case name used in the manifest.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Strip => "strip",
            Self::Detrend => "detrend",
        }
    }
}

/// One named instant on a clip's timeline — a footstep, a fire moment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestEvent {
    /// Event name, e.g. `footstep_left`.
    pub name: String,
    /// When it happens, seconds from the clip's start.
    pub time_s: f32,
    /// The library sound this instant plays, in the `kind:name` form
    /// (e.g. `sfx:pistol_shot`) — parse it with [`AudioLink::parse`] and
    /// resolve it with [`Manifest::sound`], which matches both parts. `null`
    /// when the event links no sound, which a game treats as "choose your
    /// own or stay silent", never as an error.
    ///
    /// Kept as the raw string rather than an [`AudioLink`] so a manifest
    /// carrying a link this reader cannot parse still reads: the link is a
    /// hint on one event, and one malformed hint must not refuse the whole
    /// library. [`ManifestEvent::audio_link`] is the typed view.
    pub audio: Option<String>,
}

impl ManifestEvent {
    /// The linked sound as a typed reference, when there is one and it is
    /// well-formed. `None` both for an event with no link and for a link
    /// this reader cannot parse — a game treats the two the same way.
    #[must_use]
    pub fn audio_link(&self) -> Option<AudioLink> {
        self.audio.as_deref().and_then(AudioLink::parse)
    }
}

/// A link from an event to a shipped sound, as written in
/// [`ManifestEvent::audio`]: `"sfx:pistol_shot"` — the kind's lower-case
/// name, a colon, the sound's stem.
///
/// One string rather than an object because that is the form agents already
/// pass names around in, and because a link is a reference, not a record:
/// everything true about the sound lives in its own [`AudioEntry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioLink {
    /// Which audio kind the target lives under.
    pub kind: AudioKind,
    /// The target's file stem.
    pub name: String,
}

impl AudioLink {
    /// Read a link from the `kind:name` form. `None` for anything else —
    /// a missing colon, a kind that is not one of [`AudioKind`]'s names, an
    /// empty stem. A clip "linked" to another clip is not a sound cue, and
    /// accepting it would defer the confusion to a game.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (kind, name) = text.split_once(':')?;
        let kind = AudioKind::parse(kind.trim())?;
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        Some(Self {
            kind,
            name: name.to_owned(),
        })
    }

    /// Whether `entry` is the sound this link names: both the kind and the
    /// stem must match, so `sfx:hit` never resolves to a voice line called
    /// `hit`.
    #[must_use]
    pub fn matches(&self, entry: &AudioEntry) -> bool {
        entry.kind == self.kind && entry.name == self.name
    }
}

impl fmt::Display for AudioLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind.as_str(), self.name)
    }
}

/// One shipped sound.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioEntry {
    /// File stem — the name everything refers to the sound by.
    pub name: String,
    /// Path relative to the asset root, forward-slashed.
    pub path: String,
    /// `sha256:…` of the file as shipped.
    pub sha256: String,
    /// What kind of sound it is.
    pub kind: AudioKind,
    /// Seconds of audio, when it was measured.
    pub duration_s: Option<f32>,
    /// Free-form tags, lower-case by convention.
    pub tags: Vec<String>,
}

/// What kind of sound an [`AudioEntry`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioKind {
    /// A music track.
    Music,
    /// A sound effect.
    Sfx,
    /// A spoken line.
    Voice,
}

impl AudioKind {
    /// The lower-case name used in the manifest.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Music => "music",
            Self::Sfx => "sfx",
            Self::Voice => "voice",
        }
    }

    /// The kind with exactly this lower-case name, if any.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "music" => Some(Self::Music),
            "sfx" => Some(Self::Sfx),
            "voice" => Some(Self::Voice),
            _ => None,
        }
    }
}

/// One shipped model — a prop, a fixture, anything a game spawns that does
/// not stand on the rig contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEntry {
    /// File stem — the name everything refers to the model by.
    pub name: String,
    /// Path relative to the asset root, forward-slashed.
    pub path: String,
    /// `sha256:…` of the file as shipped.
    pub sha256: String,
    /// Free-form tags, lower-case by convention.
    pub tags: Vec<String>,
    /// Axis-aligned bounds of the mesh as shipped, metres, glTF axes (Y up),
    /// as `[min, max]` — what a level placer needs to stand a prop on a
    /// floor or fit it through a door without loading the file. `null` when
    /// the mesh was never measured.
    pub bounds_m: Option<[[f32; 3]; 2]>,
}

/// One shipped humanoid body: a rigged `.glb` on the rig contract.
///
/// Structurally close to a [`ModelEntry`]; kept as its own type because the
/// two make different promises, and a consumer that wants "a thing every
/// clip plays on" should not have to guess which models qualify.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyEntry {
    /// File stem — the name everything refers to the body by.
    pub name: String,
    /// Path relative to the asset root, forward-slashed.
    pub path: String,
    /// `sha256:…` of the file as shipped.
    pub sha256: String,
    /// Free-form tags, lower-case by convention.
    pub tags: Vec<String>,
}

impl Manifest {
    /// Read a manifest from bytes, refusing a newer schema.
    ///
    /// The schema check runs before the typed parse on purpose: a newer
    /// manifest may well fail to match these types, and the error the caller
    /// shows must say "you are behind", not "missing field".
    ///
    /// # Errors
    ///
    /// Fails when the bytes are not JSON, when the document has no numeric
    /// `schema` field, when the schema is newer than [`SCHEMA`], or when the
    /// document does not match these types.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, ManifestError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(ManifestError::Json)?;
        match value.get("schema").and_then(serde_json::Value::as_u64) {
            None => Err(ManifestError::MissingSchema),
            Some(found) if found > SCHEMA => Err(ManifestError::NewerSchema {
                found,
                supported: SCHEMA,
            }),
            Some(_) => serde_json::from_value(value).map_err(ManifestError::Json),
        }
    }

    /// The manifest as pretty-printed JSON with a trailing newline, entries
    /// sorted — the same library always becomes the same bytes, which is what
    /// lets a committed manifest be verified by byte comparison.
    ///
    /// # Errors
    ///
    /// Fails only when `serde_json` refuses to serialise, which these types
    /// do not give it a reason to do.
    pub fn to_vec_pretty(&self) -> Result<Vec<u8>, ManifestError> {
        let mut sorted = self.clone();
        sorted.sort_entries();
        let mut bytes = serde_json::to_vec_pretty(&sorted).map_err(ManifestError::Json)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// The clip with exactly this name, when the library ships one.
    #[must_use]
    pub fn clip(&self, name: &str) -> Option<&ClipEntry> {
        self.clips.iter().find(|clip| clip.name == name)
    }

    /// The body with exactly this name, when the library ships one.
    #[must_use]
    pub fn body(&self, name: &str) -> Option<&BodyEntry> {
        self.bodies.iter().find(|body| body.name == name)
    }

    /// The model with exactly this name, when the library ships one.
    #[must_use]
    pub fn model(&self, name: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|model| model.name == name)
    }

    /// The sound an event link names, when the library ships one — both the
    /// kind and the stem must match.
    #[must_use]
    pub fn sound(&self, link: &AudioLink) -> Option<&AudioEntry> {
        self.audio.iter().find(|entry| link.matches(entry))
    }

    /// The socket with exactly this name, when the rig defines one.
    #[must_use]
    pub fn socket(&self, name: &str) -> Option<&RigSocket> {
        self.rig.sockets.iter().find(|socket| socket.name == name)
    }

    /// Sort every entry vector by name — with the path as tie-break, so two
    /// same-named assets in different directories still order stably. The
    /// bone table is left alone: its order is the contract's, and `parent`
    /// indexes into it.
    fn sort_entries(&mut self) {
        self.bodies
            .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
        self.models
            .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
        self.clips
            .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
        self.audio
            .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
        self.rig
            .sockets
            .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.bone.cmp(&b.bone)));
    }
}

/// Everything that can go wrong reading or writing a manifest.
#[derive(Debug)]
#[non_exhaustive]
pub enum ManifestError {
    /// The bytes are not JSON, or the JSON does not match these types.
    Json(serde_json::Error),
    /// The document has no numeric `schema` field, so nothing about it can be
    /// trusted to mean what these types say.
    MissingSchema,
    /// The document declares a schema newer than this build reads. Refused
    /// rather than partially read, because the fields the newer schema added
    /// would be silently dropped.
    NewerSchema {
        /// The version the document declared.
        found: u64,
        /// The newest version this build reads: [`SCHEMA`].
        supported: u64,
    },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(source) => write!(f, "the manifest does not parse: {source}"),
            Self::MissingSchema => {
                write!(f, "the manifest declares no numeric schema field")
            }
            Self::NewerSchema { found, supported } => write!(
                f,
                "the manifest declares schema {found}, but this build reads at most \
                 schema {supported} — update the reader before trusting it"
            ),
        }
    }
}

impl std::error::Error for ManifestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(source) => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small manifest with its entries deliberately out of order, so the
    /// tests below prove the sorting rather than inherit it.
    fn sample() -> Manifest {
        Manifest {
            schema: SCHEMA,
            forge_version: String::from("0.1.0"),
            library_version: String::from("0.1.0"),
            generated: String::from("2026-08-23"),
            rig: RigInfo {
                profile: String::from("humanoid"),
                version: 1,
                bone_count: 3,
                glb_sha256: String::from("sha256:0000"),
                // Child before parent on purpose: glb node order is the
                // contract's, and a sort by name would break `parent`.
                bones: vec![
                    RigBone {
                        name: String::from("Spine"),
                        parent: Some(1),
                        driven: true,
                    },
                    RigBone {
                        name: String::from("Hips"),
                        parent: None,
                        driven: true,
                    },
                    RigBone {
                        name: String::from("RightHand"),
                        parent: Some(0),
                        driven: false,
                    },
                ],
                sockets: vec![
                    RigSocket {
                        name: String::from("hand_r"),
                        bone: String::from("RightHand"),
                        translation: [0.0, 0.055, 0.005],
                        rotation: [0.0, 0.0, 0.0, 1.0],
                    },
                    RigSocket {
                        name: String::from("back"),
                        bone: String::from("Spine"),
                        translation: [-0.055, 0.12, -0.115],
                        rotation: [0.690_345_5, 0.153_045_9, -0.690_345_5, 0.153_045_9],
                    },
                ],
            },
            bodies: vec![
                BodyEntry {
                    name: String::from("vex_runner"),
                    path: String::from("bodies/vex_runner.glb"),
                    sha256: String::from("sha256:2222"),
                    tags: vec![String::from("hero")],
                },
                BodyEntry {
                    name: String::from("torv_warden"),
                    path: String::from("bodies/torv_warden.glb"),
                    sha256: String::from("sha256:3333"),
                    tags: Vec::new(),
                },
            ],
            models: vec![
                ModelEntry {
                    name: String::from("sword"),
                    path: String::from("models/sword.glb"),
                    sha256: String::from("sha256:1111"),
                    tags: vec![String::from("weapon")],
                    bounds_m: Some([[-0.04, -0.2, -0.01], [0.04, 0.9, 0.01]]),
                },
                ModelEntry {
                    name: String::from("barrel"),
                    path: String::from("models/barrel.glb"),
                    sha256: String::from("sha256:4444"),
                    tags: Vec::new(),
                    bounds_m: None,
                },
            ],
            clips: vec![
                ClipEntry {
                    name: String::from("walk"),
                    path: String::from("clips/walk.glb"),
                    sha256: String::from("sha256:5555"),
                    duration_s: Some(2.6),
                    fps: Some(20.0),
                    frames: Some(53),
                    looped: true,
                    tags: vec![String::from("loop")],
                    root_motion: RootMotionInfo {
                        mode: RootMotionMode::Strip,
                        avg_speed_mps: Some(1.045),
                        fps: Some(20.0),
                        track_xz_m: vec![[0.0, 0.0], [0.007, 0.053], [0.011, 0.103]],
                    },
                    events: vec![
                        ManifestEvent {
                            name: String::from("footstep_r"),
                            time_s: 0.6,
                            audio: Some(String::from("sfx:pistol_shot")),
                        },
                        ManifestEvent {
                            name: String::from("footstep_l"),
                            time_s: 1.25,
                            audio: None,
                        },
                    ],
                },
                ClipEntry {
                    name: String::from("death"),
                    path: String::from("clips/death.glb"),
                    sha256: String::from("sha256:6666"),
                    duration_s: None,
                    fps: None,
                    frames: None,
                    looped: false,
                    tags: Vec::new(),
                    root_motion: RootMotionInfo {
                        mode: RootMotionMode::Off,
                        avg_speed_mps: None,
                        fps: None,
                        track_xz_m: Vec::new(),
                    },
                    events: Vec::new(),
                },
            ],
            audio: vec![
                AudioEntry {
                    name: String::from("pistol_shot"),
                    path: String::from("audio/sfx/pistol_shot.wav"),
                    sha256: String::from("sha256:7777"),
                    kind: AudioKind::Sfx,
                    duration_s: Some(0.8),
                    tags: Vec::new(),
                },
                AudioEntry {
                    name: String::from("arena_combat"),
                    path: String::from("audio/music/arena_combat.ogg"),
                    sha256: String::from("sha256:8888"),
                    kind: AudioKind::Music,
                    duration_s: Some(90.0),
                    tags: vec![String::from("combat")],
                },
            ],
        }
    }

    #[test]
    fn a_newer_schema_is_refused_naming_both_numbers() {
        let bytes = sample().to_vec_pretty().expect("serialize");
        let text = String::from_utf8(bytes).expect("utf8");
        let newer = text.replace("\"schema\": 1", "\"schema\": 2");
        assert_ne!(
            newer, text,
            "the replacement must have found the schema line"
        );
        let error = Manifest::from_slice(newer.as_bytes()).expect_err("must refuse");
        assert!(
            matches!(
                error,
                ManifestError::NewerSchema {
                    found: 2,
                    supported: SCHEMA
                }
            ),
            "{error:?}"
        );
        let message = error.to_string();
        assert!(message.contains("schema 2"), "{message}");
        assert!(message.contains("schema 1"), "{message}");
    }

    /// The refusal is decided on the schema number alone: a newer document
    /// that happens to be unreadable by these types must still be reported
    /// as "you are behind", never as a missing field.
    #[test]
    fn a_newer_schema_is_refused_before_the_typed_parse() {
        let error =
            Manifest::from_slice(b"{\"schema\": 99, \"unheard_of\": true}").expect_err("refuse");
        assert!(
            matches!(error, ManifestError::NewerSchema { found: 99, .. }),
            "{error:?}"
        );
    }

    /// A typo'd or renamed key at the current schema is a parse refusal,
    /// not a default: an entry field that silently read as its default
    /// would be the manifest inventing a fact about an asset.
    #[test]
    fn a_typoed_key_at_the_current_schema_is_refused() {
        let bytes = sample().to_vec_pretty().expect("serialize");
        let text = String::from_utf8(bytes).expect("utf8");
        let with_stray = text.replacen("\"clips\":", "\"totally_made_up\": 1, \"clips\":", 1);
        assert_ne!(with_stray, text, "the insertion must have landed");
        let error = Manifest::from_slice(with_stray.as_bytes()).expect_err("must refuse");
        let message = error.to_string();
        assert!(message.contains("totally_made_up"), "{message}");
    }

    #[test]
    fn a_document_with_no_schema_is_refused() {
        let error = Manifest::from_slice(b"{\"clips\": []}").expect_err("must refuse");
        assert!(matches!(error, ManifestError::MissingSchema), "{error:?}");
        let error = Manifest::from_slice(b"not json").expect_err("must refuse");
        assert!(matches!(error, ManifestError::Json(_)), "{error:?}");
    }

    #[test]
    fn serialisation_is_deterministic_and_sorted() {
        let manifest = sample();
        let first = manifest.to_vec_pretty().expect("serialize");
        let second = manifest.to_vec_pretty().expect("serialize again");
        assert_eq!(first, second, "two serialisations must be byte-equal");
        assert_eq!(first.last(), Some(&b'\n'), "trailing newline");

        // The sample holds walk before death and pistol_shot before
        // arena_combat; the bytes must not.
        let parsed = Manifest::from_slice(&first).expect("parse back");
        let clip_names: Vec<&str> = parsed.clips.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(clip_names, ["death", "walk"]);
        let audio_names: Vec<&str> = parsed.audio.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(audio_names, ["arena_combat", "pistol_shot"]);
        let model_names: Vec<&str> = parsed.models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(model_names, ["barrel", "sword"]);
        // Bodies sort too, or the committed manifest would change bytes with
        // the order a scan happened to return.
        let body_names: Vec<&str> = parsed.bodies.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(body_names, ["torv_warden", "vex_runner"]);
        let socket_names: Vec<&str> = parsed.rig.sockets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(socket_names, ["back", "hand_r"]);
        // The bone table is the one array that keeps its order: `parent`
        // indexes into it.
        let bone_names: Vec<&str> = parsed.rig.bones.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(bone_names, ["Spine", "Hips", "RightHand"]);
        assert_eq!(parsed.rig.bones[0].parent, Some(1));
    }

    /// The committed-file gate, stated as a test: bytes → types → bytes
    /// lands on the same bytes, so `forge manifest --check` can compare a
    /// fresh projection against the file on disk without a tolerance.
    #[test]
    fn the_round_trip_is_byte_deterministic() {
        let first = sample().to_vec_pretty().expect("serialize");
        let parsed = Manifest::from_slice(&first).expect("parse");
        let second = parsed.to_vec_pretty().expect("serialize the parse");
        assert_eq!(first, second, "bytes → types → bytes must be the identity");
        // And the text is the pretty form a reviewer diffs, keys in struct
        // order, starting with the schema.
        let text = String::from_utf8(first).expect("utf8");
        assert!(text.starts_with("{\n  \"schema\": 1,\n"), "{text}");
    }

    #[test]
    fn unknown_values_serialise_as_explicit_null() {
        let bytes = sample().to_vec_pretty().expect("serialize");
        let text = String::from_utf8(bytes).expect("utf8");
        // death was never measured; its record must say so out loud rather
        // than omit the fields.
        assert!(text.contains("\"duration_s\": null"), "{text}");
        assert!(text.contains("\"fps\": null"), "{text}");
        assert!(text.contains("\"frames\": null"), "{text}");
        assert!(text.contains("\"avg_speed_mps\": null"), "{text}");
        // walk's footstep_l links no sound, death has no track, the barrel
        // was never measured, and the root bone has no parent: all written
        // out, none skipped.
        assert!(text.contains("\"audio\": null"), "{text}");
        assert!(text.contains("\"track_xz_m\": []"), "{text}");
        assert!(text.contains("\"bounds_m\": null"), "{text}");
        assert!(text.contains("\"parent\": null"), "{text}");
    }

    #[test]
    fn the_round_trip_preserves_everything() {
        let manifest = sample();
        let bytes = manifest.to_vec_pretty().expect("serialize");
        let parsed = Manifest::from_slice(&bytes).expect("parse");
        // The round trip lands on the sorted form of the same content.
        let mut sorted = manifest;
        sorted.sort_entries();
        assert_eq!(parsed, sorted);
    }

    #[test]
    fn enums_use_their_lower_case_names() {
        let bytes = sample().to_vec_pretty().expect("serialize");
        let text = String::from_utf8(bytes).expect("utf8");
        assert!(text.contains("\"mode\": \"strip\""), "{text}");
        assert!(text.contains("\"mode\": \"off\""), "{text}");
        assert!(text.contains("\"kind\": \"sfx\""), "{text}");
        assert!(text.contains("\"kind\": \"music\""), "{text}");
        for kind in [AudioKind::Music, AudioKind::Sfx, AudioKind::Voice] {
            assert_eq!(AudioKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(AudioKind::parse("Sfx"), None, "names are exact");
        assert_eq!(RootMotionMode::Detrend.as_str(), "detrend");
    }

    #[test]
    fn lookups_find_by_exact_name() {
        let manifest = sample();
        assert!(manifest.clip("walk").is_some());
        assert!(manifest.clip("missing").is_none());
        assert!(manifest.body("vex_runner").is_some());
        assert!(manifest.body("sword").is_none(), "a prop is not a body");
        assert!(manifest.model("sword").is_some());
        assert!(
            manifest.model("vex_runner").is_none(),
            "a body is not a model"
        );
        assert_eq!(
            manifest.socket("hand_r").map(|s| s.bone.as_str()),
            Some("RightHand")
        );
        assert!(manifest.socket("hand_x").is_none());
    }

    #[test]
    fn audio_links_parse_the_kind_name_form_and_nothing_else() {
        let link = AudioLink::parse("sfx:pistol_shot").expect("well-formed");
        assert_eq!(link.kind, AudioKind::Sfx);
        assert_eq!(link.name, "pistol_shot");
        assert_eq!(
            link.to_string(),
            "sfx:pistol_shot",
            "Display is the wire form"
        );
        assert_eq!(
            AudioLink::parse(" voice : line_01 ").map(|l| l.name),
            Some(String::from("line_01")),
            "whitespace around either part is forgiven"
        );
        // Not a link: no colon, an unknown kind, a non-audio kind, no stem.
        assert_eq!(AudioLink::parse("pistol_shot"), None);
        assert_eq!(AudioLink::parse("sound:pistol_shot"), None);
        assert_eq!(AudioLink::parse("clip:walk"), None);
        assert_eq!(AudioLink::parse("sfx:"), None);
        assert_eq!(AudioLink::parse("sfx:   "), None);
    }

    #[test]
    fn an_event_link_resolves_against_the_audio_list_by_both_parts() {
        let manifest = sample();
        let walk = manifest.clip("walk").expect("walk is listed");
        let link = walk.events[0]
            .audio_link()
            .expect("footstep_r links a sound");
        let sound = manifest.sound(&link).expect("the sound is shipped");
        assert_eq!(sound.path, "audio/sfx/pistol_shot.wav");
        assert!(
            walk.events[1].audio_link().is_none(),
            "footstep_l is silent"
        );

        // Same stem, different kind: not the same sound.
        let wrong_kind = AudioLink {
            kind: AudioKind::Voice,
            name: String::from("pistol_shot"),
        };
        assert!(manifest.sound(&wrong_kind).is_none());
    }

    /// A malformed link on one event is that event's problem, not the
    /// library's: the document still reads and the typed view says "no
    /// sound".
    #[test]
    fn a_malformed_event_link_reads_as_no_link() {
        let bytes = sample().to_vec_pretty().expect("serialize");
        let text = String::from_utf8(bytes)
            .expect("utf8")
            .replace("\"sfx:pistol_shot\"", "\"pistol_shot\"");
        let parsed = Manifest::from_slice(text.as_bytes()).expect("still parses");
        let walk = parsed.clip("walk").expect("walk is listed");
        assert_eq!(walk.events[0].audio.as_deref(), Some("pistol_shot"));
        assert!(walk.events[0].audio_link().is_none());
    }
}
