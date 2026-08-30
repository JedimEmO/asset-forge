//! Sidecar schema 2: what an asset records about itself.
//!
//! # Why the shape is what it is
//!
//! The first sidecars this library descends from were whatever a generator
//! happened to dump: a flat object of twenty-two keys, no version marker, and
//! no way to tell a recorded value from an argparse default. That last part
//! was not theoretical — thirty shipped clips claimed `seed 0, duration_s
//! 4.0` because those are the defaults, and every one of them was actually
//! promoted out of a sweep with different values. The metadata was not wrong
//! by accident; it was wrong by construction, because the writer had no way
//! to say "I do not know".
//!
//! So the whole shape follows from one rule: **`null` means unknown, and a
//! default is never written as if it were a measurement.** Every generator
//! field is an [`Option`]. [`Provenance`] says out loud how much of the record
//! to believe. The one place where identity values *are* written explicitly is
//! [`ClipRecipe`], and that is the opposite case — there, an absent field is
//! what causes the damage (see [`ClipRecipe::to_edit`], which turns every one
//! of those fields into something the baker applies).
//!
//! # Why 1
//!
//! asset-forge starts its sidecar schema over rather than continuing the
//! numbering of the library it was distilled from. That schema carried five
//! generations of one game's history — a review block, a parametric body
//! generator, its garments — and the reader carried a legacy mapping for
//! records that no longer exist. A fresh 1 says the honest thing: nothing
//! older than this document exists for this reader, so there is no
//! compatibility story to carry, and no legacy reader ships. What the reset
//! keeps is the mechanism: the schema field is the honesty device, and
//! **refuse-newer** is the promise — a build meeting a record it does not
//! know refuses on the number, out loud, before it can rewrite that record
//! without the fields it did not understand.
//!
//! # Why 2
//!
//! One addition, [`Sidecar::body`]: a body's own bone lengths and the motion
//! scale a consumer applies to a root track. It is a bump rather than an
//! optional extra because it changes what a consumer must do with a clip —
//! a game that scaled no root track would put a short body's feet through
//! the floor of its own stride — and because the manifest that projects it
//! bumped with it. Nothing else in the record changed: no kind, no
//! generator, no claim, and not one clip was rebaked. A schema-1 record is
//! not read here; `forge migrate` brings it forward, re-deriving the bone
//! table from each shipped `.glb` rather than copying the profile's, because
//! a default written where a measurement belongs is the one thing this
//! schema exists to prevent.
//!
//! # Two empties that are not the same
//!
//! [`Sidecar::events`] is `None` when nothing has ever examined the clip for
//! events and `Some(vec![])` when something looked and found none. Collapsing
//! them would make "checked, silent clip" indistinguishable from "never
//! checked" — the same lie `null`-means-unknown exists to prevent everywhere
//! else. The distinction is load-bearing and the writer keeps it.

mod body;
mod clip;
mod events;

pub use body::{BONE_TOLERANCE_M, Body, BodyBone, MOTION_SCALE_TOLERANCE};
pub use clip::{
    AutoTrim, ClipRecipe, DEFAULT_FPS, InPlaceMode, PartialRecipe, RootYMode, format_retime,
    overlay_recipe, parse_retime,
};
pub use events::{AnimEvent, AudioRef, EventOrigin, RootMotion, valid_event_name};

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The schema version this build writes, and the only one it reads.
pub const SCHEMA: u64 = 2;

/// The schema `forge migrate` reads to bring forward. One version back and
/// no further: there is no legacy reader here, and 1 is migrated rather than
/// read because a body's [`Sidecar::body`] block cannot be invented from the
/// record — it is re-derived from the `.glb`.
pub const MIGRATABLE_SCHEMA: u64 = 1;

/// What kind of asset a sidecar describes.
///
/// The kinds are deliberately not "mesh, clip and audio": music, sfx and
/// voice have different generators, different parameters and different review
/// questions, and collapsing them would mean a sidecar that cannot say which
/// voice model spoke a line. A body and a model are split on the same
/// reasoning — a body records a mesh measurement *and* a rig-contract claim,
/// and a prop must never be able to claim the latter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A baked `.glb` animation clip under `clips/`.
    Clip,
    /// A rigged humanoid body under `bodies/`, on the rig contract.
    Body,
    /// A static mesh under `models/`: a prop, a fixture, a held weapon.
    Model,
    /// A sound effect under `audio/sfx/`.
    Sfx,
    /// A music track under `audio/music/`.
    Music,
    /// A spoken line under `audio/voice/`.
    Voice,
}

impl Kind {
    /// Every kind, in the order the library browser lists them.
    pub const ALL: [Self; 6] = [
        Self::Clip,
        Self::Body,
        Self::Model,
        Self::Sfx,
        Self::Music,
        Self::Voice,
    ];

    /// The three audio kinds, in table order.
    pub const AUDIO: [Self; 3] = [Self::Sfx, Self::Music, Self::Voice];

    /// Where assets of this kind live, relative to the asset root.
    #[must_use]
    pub const fn dir(self) -> &'static str {
        match self {
            Self::Clip => "clips",
            Self::Body => "bodies",
            Self::Model => "models",
            Self::Sfx => "audio/sfx",
            Self::Music => "audio/music",
            Self::Voice => "audio/voice",
        }
    }

    /// The lower-case name used in records, CLI flags and tool arguments.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clip => "clip",
            Self::Body => "body",
            Self::Model => "model",
            Self::Sfx => "sfx",
            Self::Music => "music",
            Self::Voice => "voice",
        }
    }

    /// Parse the name back, case-insensitively, so an agent passing `SFX` is
    /// not refused over capitalisation.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|k| k.as_str() == text)
    }

    /// Whether files of this kind are audio, which decides whether the studio
    /// offers a waveform or a viewport.
    ///
    /// Stated as what audio *is* rather than as "not a clip": the negative
    /// form silently reclassified every kind added after it.
    #[must_use]
    pub const fn is_audio(self) -> bool {
        matches!(self, Self::Music | Self::Sfx | Self::Voice)
    }

    /// Whether files of this kind are meshes — a body or a model. Only a
    /// body additionally stands on the rig contract.
    #[must_use]
    pub const fn is_mesh(self) -> bool {
        matches!(self, Self::Body | Self::Model)
    }

    /// The manifest's audio kind, for the three kinds that have one.
    #[must_use]
    pub const fn audio_kind(self) -> Option<forge_manifest::AudioKind> {
        match self {
            Self::Sfx => Some(forge_manifest::AudioKind::Sfx),
            Self::Music => Some(forge_manifest::AudioKind::Music),
            Self::Voice => Some(forge_manifest::AudioKind::Voice),
            Self::Clip | Self::Body | Self::Model => None,
        }
    }

    /// The kind implied by a path relative to the asset root.
    ///
    /// Directory-derived rather than extension-derived: a `.wav` says nothing
    /// about whether it is a bark or a line of dialogue, and the library's
    /// layout already carries that.
    ///
    /// **The full directory always wins over the flat-layout fallback**, and
    /// the two passes below are what make that true. Testing both predicates
    /// per kind in one pass meant the first kind whose *either* rule matched
    /// won — and since the first audio kind's `top` is `audio`, every sound in
    /// the library resolved as that kind, `audio/voice/line.wav` included.
    /// Nothing failed loudly: the studio's browser simply mislabelled every
    /// line of dialogue.
    #[must_use]
    pub fn of_rel_path(rel_path: &str) -> Option<Self> {
        let rel = rel_path.replace('\\', "/");
        let under = |prefix: &str| rel.starts_with(&format!("{prefix}/"));
        Self::ALL
            .into_iter()
            .find(|k| under(k.dir()))
            .or_else(|| Self::ALL.into_iter().find(|k| under(k.top())))
    }

    /// The first path segment of [`Self::dir`], so `audio/x.wav` still
    /// resolves when a project keeps a flatter layout.
    ///
    /// **Meshes and clips have no such fallback**: their directories are
    /// already one segment deep, and a fallback there would be the directory
    /// itself.
    const fn top(self) -> &'static str {
        match self {
            Self::Clip | Self::Body | Self::Model => self.dir(),
            Self::Music | Self::Sfx | Self::Voice => "audio",
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who made an asset.
///
/// Serialised as one string — `"human"`, `"agent:<name>"`, `"unknown"` —
/// because that is what fits in a table cell and in a `git diff`, and because
/// the agent's name is free-form: there will be more agents than this enum
/// could ever list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Actor {
    /// Somebody sat at the keyboard and said so.
    Human,
    /// An agent, named however it named itself.
    Agent(String),
    /// The record predates anyone recording this.
    #[default]
    Unknown,
}

impl Actor {
    /// Read an actor from the string form, treating anything unrecognised as
    /// an agent name rather than as unknown — an unfamiliar name is still a
    /// fact, and discarding it would be exactly the laundering this schema
    /// exists to stop.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let text = text.trim();
        match text {
            "" | "unknown" => Self::Unknown,
            "human" => Self::Human,
            other => Self::Agent(other.trim_start_matches("agent:").to_owned()),
        }
    }
}

impl std::fmt::Display for Actor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Human => f.write_str("human"),
            Self::Agent(name) => write!(f, "agent:{name}"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

impl From<Actor> for String {
    fn from(actor: Actor) -> Self {
        actor.to_string()
    }
}

impl From<String> for Actor {
    fn from(text: String) -> Self {
        Self::parse(&text)
    }
}

impl Serialize for Actor {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Actor {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Ok(Self::parse(&text))
    }
}

/// How much of the record to believe.
///
/// This is the field that makes the rest honest. A migrated clip keeps its
/// real recipe but has had its fabricated generator parameters nulled, and
/// [`Self::Reconstructed`] is how it says so — without it, a reader has no way
/// to distinguish "nobody recorded the seed" from "the seed was 0". It only
/// ever moves down: a rebuild nulls what it cannot know; it never launders a
/// guess up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provenance {
    /// Written by the generator at the moment it generated, values as used.
    Recorded,
    /// Rebuilt after the fact from what survived. Trust the recipe; do not
    /// trust anything the writer could have defaulted.
    Reconstructed,
    /// No provenance at all — the file exists and nothing is known about how.
    #[default]
    Unknown,
}

impl Provenance {
    /// The lower-case name used in records.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Recorded => "recorded",
            Self::Reconstructed => "reconstructed",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for Provenance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The model and parameters that produced an asset.
///
/// Tagged by `tool`, because the parameters have nothing in common: a seed
/// means an integer to ARDY and a comma-separated pair to ACE-Step, and a
/// voice reference has no analogue in either. Flattening them into one struct
/// of optional fields would let a music sidecar claim an `arm_bend_deg`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum Generator {
    /// NVIDIA ARDY, for clips.
    Ardy(ArdyParams),
    /// TRELLIS.2, for a body or a model lifted from a reference image —
    /// followed by a headless-Blender step (auto-rig or prop normalize) the
    /// record names in [`LiftParams::post`].
    Trellis2(LiftParams),
    /// MOSS `SoundEffect`, for sfx.
    MossSoundEffect(SoundEffectParams),
    /// ACE-Step, for music.
    AceStep(AceStepParams),
    /// MOSS TTS, for voice.
    MossTts(SpeechParams),
}

impl Generator {
    /// The tool name as recorded, for display.
    #[must_use]
    pub const fn tool(&self) -> &'static str {
        match self {
            Self::Ardy(_) => "ardy",
            Self::Trellis2(_) => "trellis2",
            Self::MossSoundEffect(_) => "moss_sound_effect",
            Self::AceStep(_) => "ace_step",
            Self::MossTts(_) => "moss_tts",
        }
    }
}

/// What ARDY was asked for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ArdyParams {
    /// Upstream repository the checkpoint came from.
    pub repo: Option<String>,
    /// Short commit of that repository at generation time.
    pub commit: Option<String>,
    /// Model nickname, e.g. `core`.
    pub model: Option<String>,
    /// Random seed actually used, not the CLI default.
    pub seed: Option<i64>,
    /// Seconds of motion asked for.
    pub duration_s: Option<f32>,
    /// Classifier-free guidance scale, when the take came from a sweep.
    pub cfg: Option<f32>,
    /// Which sample of the sweep batch this was.
    pub sample: Option<u32>,
    /// The audition take this clip was promoted from, as a bare file name.
    ///
    /// A bare name and not a path: sweep directories are scratch and are
    /// swept, so a path into one is a lie with a plausible shape; the file
    /// name is the part that is still true and still useful for matching a
    /// clip back to a sweep manifest.
    pub sweep_take: Option<String>,
}

/// What TRELLIS.2 was asked for, and what it was handed.
///
/// There is no recipe here and never will be: a lifted mesh's shipped `.glb`
/// is pinned by `content_hash` and the committed `.blend` it was exported
/// from by [`Source::sha256`]. The claim is integrity — *this is the file that
/// was checked and approved* — not regeneration, because neither the lift
/// nor Blender's glTF export is byte-stable. `rebake` skips these on exactly
/// that ground.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LiftParams {
    /// Model identifier, e.g. `microsoft/TRELLIS.2-4B`.
    pub model: Option<String>,
    /// Commit of the TRELLIS.2 checkout that ran.
    pub trellis_commit: Option<String>,
    /// Voxel resolution of the lift (`512`, `1024`).
    pub resolution: Option<u32>,
    /// Pipeline preset, e.g. `1024_cascade`.
    pub pipeline_type: Option<String>,
    /// Random seed actually used. A real knob: one front view
    /// underdetermines the back of a shape, and a seed can leave the rear of
    /// a skull absent.
    pub seed: Option<i64>,
    /// Decimation target, vertices.
    pub decimation_target_vertices: Option<u32>,
    /// Texture atlas size, pixels on a side.
    pub texture_size: Option<u32>,
    /// Whether the remesh pass ran.
    pub remesh: Option<bool>,
    /// The texture baker the lift used. A licence fact, not a detail: today
    /// it is `nvdiffrast (non-commercial)`, and a record that dropped it
    /// would hide the one thing a publisher has to know.
    pub texture_baker: Option<String>,
    /// The reference image, relative to the project root.
    pub image: Option<String>,
    /// `sha256:…` of that image as lifted. The image claims integrity and a
    /// ledger row, never regeneration.
    pub image_sha256: Option<String>,
    /// `sha256:…` of the raw lift as it left TRELLIS.2, before any Blender
    /// step touched it.
    pub lift_sha256: Option<String>,
    /// The Blender step that turned the raw lift into what shipped.
    pub post: Option<PostStep>,
}

/// A headless-Blender step recorded on a lifted asset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PostStep {
    /// The tool, always `blender`.
    pub tool: String,
    /// The script that ran, e.g. `rig`, `export`, `prop`.
    pub script: Option<String>,
    /// The glTF `asset.generator` string of the exported file, e.g.
    /// `Khronos glTF Blender I/O v…` — measured from the `.glb`, not declared.
    pub version: Option<String>,
}

/// What ACE-Step was asked for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AceStepParams {
    /// Language-model stage checkpoint.
    pub lm_model: Option<String>,
    /// Diffusion stage checkpoint.
    pub dit_model: Option<String>,
    /// Seeds as ACE-Step reports them: two comma-separated integers, one per
    /// stage. Kept verbatim as a string rather than split, because feeding it
    /// back is the only thing it is for.
    pub seed: Option<String>,
    /// Beats per minute requested.
    pub bpm: Option<u32>,
    /// Key and scale, e.g. `F minor`.
    pub keyscale: Option<String>,
    /// Time signature, as written — `4`, `6/8`.
    pub timesignature: Option<String>,
    /// Genre hints, or `N/A`.
    pub genres: Option<String>,
    /// Lyrics, or `[instrumental]`.
    pub lyrics: Option<String>,
    /// Seconds requested.
    pub duration_s: Option<f32>,
}

/// What MOSS `SoundEffect` was asked for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SoundEffectParams {
    /// Checkpoint identifier.
    pub model: Option<String>,
    /// Random seed. A generator that took none produced a sound that cannot
    /// be regenerated at all, and the null says so.
    pub seed: Option<i64>,
    /// Seconds requested.
    pub duration_s: Option<f32>,
    /// Diffusion steps.
    pub steps: Option<u32>,
    /// Classifier-free guidance scale.
    pub cfg: Option<f32>,
}

/// What MOSS TTS was asked for. The spoken line itself is the sidecar's
/// `prompt`, since that is what a reader searches for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpeechParams {
    /// Checkpoint identifier.
    pub model: Option<String>,
    /// Random seed, when the generator took one.
    pub seed: Option<i64>,
    /// Character voice name.
    pub voice: Option<String>,
    /// Explicit reference audio, when it overrode the named voice.
    pub reference: Option<String>,
    /// The designed voice's record (`assets-src/voices/<name>/voice.json`)
    /// when the reference was made by `forge gen voice`, so the line's
    /// provenance chains back to the description and the seed. `null` for a
    /// brought clip. Added while schema 1 was current, before unknown keys
    /// were refused; from here on, saying more is a schema bump — a reader
    /// that refuses an unknown key must be able to say "you are behind"
    /// rather than dropping a field it would unauthor on rewrite.
    pub voice_record: Option<String>,
    /// Language, when it was not inferred.
    pub language: Option<String>,
}

/// Where the durable inputs of an asset live.
///
/// Separate from [`Generator`] because a source survives a re-bake and a
/// generation parameter does not: the `.npz` under `assets-src/takes/` is
/// what makes a shipped clip editable at all, and it is still there long
/// after the sweep that produced it was swept away.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Source {
    /// Project-relative path to the durable input, e.g.
    /// `assets-src/takes/roll.npz` or `assets-src/blender/vex_runner.blend`.
    pub path: Option<String>,
    /// `sha256:…` of that input as it was when the asset shipped. Drift
    /// against the file afterwards is ordinary iteration — a draft that moved
    /// on from what shipped — and `verify` reports it as a warning, not a
    /// failure; a source that is *gone* is a failure.
    pub sha256: Option<String>,
    /// Skeleton the rotations are expressed in, for clips (`cskel27`).
    pub skeleton: Option<String>,
}

/// Facts measured from the built asset, not asked of the generator.
///
/// Kept out of [`ClipRecipe`] on purpose: a recipe is an input, and these are
/// outputs. Mixing them is how you end up with a "recipe" that cannot be
/// replayed because two of its fields are results.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Measured {
    /// Frames in the built clip, after trimming.
    pub frames: Option<u32>,
    /// Frames per second. Also the unit conversion the recipe needs: its trims
    /// are in seconds and [`forge_motion::Edit`]'s are in frames.
    pub fps: Option<f32>,
    /// Seconds of built asset — a clip's length, a sound's length.
    pub duration_s: Option<f32>,
    /// Mean root speed over the clip, metres per second. Small values on a
    /// locomotion clip mean the character is running on the spot.
    pub avg_speed_mps: Option<f32>,
    /// What the root's travel actually was before `in_place` treated it.
    /// `None` means never measured — not "the clip does not move".
    pub root_motion: Option<RootMotion>,
    /// What the built mesh measures, for kinds that are one. `None` on every
    /// clip and every sound, per the null-means-unknown rule — a `.wav` has no
    /// triangles, and saying `0` would be a measurement nobody made.
    pub mesh: Option<MeshMeasured>,
}

/// What a built mesh measures.
///
/// Every field is always computable from the mesh that was just written, so
/// none of them is an [`Option`]: the honest "unknown" here is the absence of
/// the whole block ([`Measured::mesh`]), the same way [`RootMotion`] states
/// what it always knows and options only what it may not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MeshMeasured {
    /// Vertices across every primitive, counted on the mesh as written to the
    /// file, so the number agrees with what any glTF inspector reports.
    pub vertices: u32,
    /// Triangles across every primitive.
    pub triangles: u32,
    /// How many joints the skin binds — the contract's bone count for a body,
    /// `0` for a model.
    pub bones_skinned: u32,
    /// The lowest vertex in metres. The contract wants a body's feet on
    /// `y = 0`, and this is the number that says whether they are.
    pub lowest_y: f32,
    /// Axis-aligned bounds as `[min, max]`, metres, glTF axes.
    pub bounds: [[f32; 3]; 2],
}

impl MeshMeasured {
    /// Height in metres: the stature the rig contract bounds.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.bounds[1][1] - self.bounds[0][1]
    }
}

impl From<&forge_rig::measure::GlbMeasurement> for MeshMeasured {
    fn from(measured: &forge_rig::measure::GlbMeasurement) -> Self {
        Self {
            vertices: measured.vertices,
            triangles: measured.triangles,
            bones_skinned: measured.bones_skinned,
            lowest_y: measured.lowest_y,
            bounds: measured.bounds,
        }
    }
}

/// Everything a shipped asset records about itself.
///
/// One envelope for every kind. The kind-specific parts are the `generator`
/// and the `recipe`; everything else — who, when, from what, hashed to what —
/// is the same question regardless of whether the answer is a clip or a bark,
/// and a library browser that had to special-case those per kind would grow a
/// branch per kind forever.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sidecar {
    /// Schema version. Always [`SCHEMA`] when written by this build.
    pub schema: u64,
    /// What kind of asset this is.
    pub kind: Kind,
    /// File stem of the asset this sits beside.
    pub name: String,
    /// The text that produced it — the motion description, the music
    /// description, the spoken line, the prop described to the image model.
    /// `None` when genuinely unknown; never invented, and never an empty
    /// string standing in for one.
    pub prompt: Option<String>,
    /// Free-form tags, for the library filter. Lower-case by convention.
    #[serde(default)]
    pub tags: Vec<String>,
    /// The day the record was written, `YYYY-MM-DD`. Always known: a record
    /// is written by a promote, and the promote knows what day it is.
    pub created: String,
    /// Who created it.
    #[serde(default)]
    pub created_by: Actor,
    /// How much of this record to believe.
    #[serde(default)]
    pub provenance: Provenance,
    /// The rig profile this asset stands on — a body is skinned to it, a clip
    /// is baked against it. `None` on models and sounds, which stand on no
    /// rig at all.
    pub rig: Option<String>,
    /// Model and parameters, when they were recorded.
    pub generator: Option<Generator>,
    /// Durable inputs.
    #[serde(default)]
    pub source: Source,
    /// The edit recipe, for kinds that have one. Only clips do; the field is
    /// typed rather than an enum with one variant because a one-armed match
    /// costs every reader something and buys nothing until there is a second
    /// arm.
    pub recipe: Option<ClipRecipe>,
    /// Facts measured from the built file.
    pub measured: Option<Measured>,
    /// The per-body skeleton: every bone's own rest translation and the
    /// motion scale a consumer applies to a root track. `Some` on a body and
    /// on nothing else — [`Sidecar::validate`] refuses it anywhere else,
    /// because a prop that could claim a skeleton is a prop a game would try
    /// to animate.
    ///
    /// `None` on a body means nobody has re-derived it yet, which is what
    /// `forge migrate` is for; it never means the body has no bones.
    /// The default is what lets a schema-1 record, which has no such key at
    /// all, be read by the migration rather than refused for a missing
    /// field.
    #[serde(default)]
    pub body: Option<Body>,
    /// Gameplay events on the clip's timeline.
    ///
    /// The distinction between the two empties is load-bearing: `None` means
    /// nothing has ever examined this clip for events, `Some(vec![])` means
    /// something looked and found none.
    pub events: Option<Vec<AnimEvent>>,
    /// `sha256:…` of the asset file as shipped. Required: integrity is the
    /// one claim every kind can make, so a record without it says nothing.
    pub content_hash: String,
    /// Anything a human wanted the next reader to know.
    pub note: Option<String>,
}

impl Sidecar {
    /// An empty record for a new asset, dated today: honest about knowing
    /// nothing yet. The content hash is empty until the promote that writes
    /// the file fills it — an empty hash fails `verify`, which is the right
    /// outcome for a record that never went through a door.
    #[must_use]
    pub fn new(kind: Kind, name: impl Into<String>) -> Self {
        Self {
            schema: SCHEMA,
            kind,
            name: name.into(),
            prompt: None,
            tags: Vec::new(),
            created: crate::clock::today_iso(),
            created_by: Actor::Unknown,
            provenance: Provenance::Unknown,
            rig: None,
            generator: None,
            source: Source::default(),
            recipe: None,
            measured: None,
            body: None,
            events: None,
            content_hash: String::new(),
            note: None,
        }
    }

    /// Read a schema 1 sidecar out of parsed JSON.
    ///
    /// The schema check runs before the typed parse on purpose: a newer record
    /// may well fail to match these types, and the error the caller shows
    /// must say "you are behind", not "missing field". A document with no
    /// `schema` at all is refused the same way — there is no legacy shape
    /// this build reads.
    ///
    /// # Errors
    ///
    /// Fails when the document declares a schema other than [`SCHEMA`], does
    /// not match these types, or breaks [`Sidecar::validate`].
    pub fn from_value(value: serde_json::Value, path: &Path) -> crate::Result<Self> {
        match value.get("schema").and_then(serde_json::Value::as_u64) {
            Some(SCHEMA) => {
                let record: Self = serde_json::from_value(value)
                    .map_err(|e| crate::LibraryError::json(path, e))?;
                record.validate(path)?;
                Ok(record)
            }
            Some(other) => Err(crate::LibraryError::UnsupportedSchema {
                path: path.to_path_buf(),
                schema: other,
            }),
            None => Err(crate::LibraryError::UnsupportedSchema {
                path: path.to_path_buf(),
                schema: 0,
            }),
        }
    }

    /// What the types cannot say: a [`Sidecar::body`] block belongs to a
    /// body and to nothing else.
    ///
    /// Checked at every door a record comes through, read and written, and
    /// not only at the promote — a hand-typed sidecar giving a barrel a
    /// skeleton would otherwise be a manifest entry a game tries to
    /// animate, and the failure would surface in the consumer with a message
    /// that names nothing.
    ///
    /// # Errors
    ///
    /// The record carries a body block under a kind that is not
    /// [`Kind::Body`].
    pub fn validate(&self, path: &Path) -> crate::Result<()> {
        if self.body.is_some() && self.kind != Kind::Body {
            return Err(crate::LibraryError::rejected(format!(
                "{}: a {} carries a body block, and only a body has a skeleton — bone lengths \
                 and a motion scale mean nothing on it",
                path.display(),
                self.kind
            )));
        }
        Ok(())
    }

    /// Read a sidecar from JSON bytes. See [`Self::from_value`].
    ///
    /// # Errors
    ///
    /// As [`Self::from_value`], plus the bytes not being JSON.
    pub fn from_slice(bytes: &[u8], path: &Path) -> crate::Result<Self> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| crate::LibraryError::json(path, e))?;
        Self::from_value(value, path)
    }

    /// The record as bytes: pretty JSON at two-space indent, one trailing
    /// newline. Byte-deterministic for an unchanged record, which is what
    /// lets `migrate` say "unchanged" by comparing bytes.
    ///
    /// # Errors
    ///
    /// Fails only when `serde_json` refuses to serialise, which these types
    /// give it no reason to do.
    pub fn to_bytes(&self) -> crate::Result<Vec<u8>> {
        let mut json = serde_json::to_vec_pretty(self)
            .map_err(|e| crate::LibraryError::json(format!("{}.json", self.name), e))?;
        json.push(b'\n');
        Ok(json)
    }

    /// Whether the prompt or the name contains `needle`, case-insensitively.
    ///
    /// Both, because an agent searching for "roll" means either the clip called
    /// `roll` or the one whose prompt says "dives forward into a roll", and
    /// making it guess which costs a turn.
    #[must_use]
    pub fn matches_text(&self, needle: &str) -> bool {
        let needle = needle.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return true;
        }
        self.name.to_ascii_lowercase().contains(&needle)
            || self
                .prompt
                .as_deref()
                .is_some_and(|p| p.to_ascii_lowercase().contains(&needle))
    }

    /// Whether the record carries `tag`, case-insensitively.
    #[must_use]
    pub fn has_tag(&self, tag: &str) -> bool {
        let tag = tag.trim();
        self.tags.iter().any(|t| t.eq_ignore_ascii_case(tag))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(document: &str) -> crate::Result<Sidecar> {
        Sidecar::from_value(
            serde_json::from_str(document).expect("test json"),
            Path::new("test.json"),
        )
    }

    #[test]
    fn a_newer_schema_is_refused_naming_the_number() {
        let error = parse(r#"{"schema": 6}"#).expect_err("must refuse");
        assert!(error.to_string().contains('6'), "{error}");
    }

    #[test]
    fn a_typoed_key_is_refused_not_read_as_a_default() {
        // A serializer round-trip is the honest source of a valid document.
        let valid = serde_json::to_value(Sidecar::new(Kind::Clip, "walk")).expect("json");

        // An unknown key at the top level.
        let mut with_stray = valid.clone();
        with_stray
            .as_object_mut()
            .expect("object")
            .insert(String::from("totally_made_up"), serde_json::json!(1));
        let error = Sidecar::from_value(with_stray, Path::new("test.json"))
            .expect_err("an unknown key is a refusal, not silence");
        assert!(error.to_string().contains("totally_made_up"), "{error}");

        // A renamed recipe knob would otherwise become the default — a
        // default standing in for a measurement, the exact failure mode the
        // schema exists to refuse.
        let mut with_typo = valid;
        with_typo.as_object_mut().expect("object").insert(
            String::from("recipe"),
            serde_json::json!({"trimstart_s": 0.4}),
        );
        let error = Sidecar::from_value(with_typo, Path::new("test.json"))
            .expect_err("a typo'd recipe key is a refusal");
        assert!(error.to_string().contains("trimstart_s"), "{error}");
    }

    #[test]
    fn a_record_with_no_schema_is_refused_not_guessed() {
        let error = parse(r#"{"prompt": "A person walks.", "loop": true}"#).expect_err("refuse");
        assert!(
            matches!(
                error,
                crate::LibraryError::UnsupportedSchema { schema: 0, .. }
            ),
            "{error}"
        );
    }

    /// A body record's claim is integrity plus provenance: the bytes it
    /// shipped and the `.blend` they came from, hashed — and the mesh it
    /// measured, which has to survive the round trip exactly.
    #[test]
    fn a_body_record_round_trips_with_its_provenance_and_its_mesh() {
        let mut sidecar = Sidecar::new(Kind::Body, "vex_runner");
        sidecar.rig = Some(String::from("humanoid"));
        sidecar.generator = Some(Generator::Trellis2(LiftParams {
            model: Some(String::from("microsoft/TRELLIS.2-4B")),
            seed: Some(42),
            texture_baker: Some(String::from("nvdiffrast (non-commercial)")),
            post: Some(PostStep {
                tool: String::from("blender"),
                script: Some(String::from("rig")),
                version: Some(String::from("Khronos glTF Blender I/O v5.2")),
            }),
            ..LiftParams::default()
        }));
        sidecar.source = Source {
            path: Some(String::from("assets-src/blender/vex_runner.blend")),
            sha256: Some(String::from("sha256:abcd")),
            skeleton: None,
        };
        sidecar.measured = Some(Measured {
            mesh: Some(MeshMeasured {
                vertices: 4321,
                triangles: 8500,
                bones_skinned: 55,
                lowest_y: 0.0,
                bounds: [[-0.31, 0.0, -0.14], [0.31, 1.8, 0.16]],
            }),
            ..Measured::default()
        });
        sidecar.content_hash = String::from("sha256:ffff");
        let bytes = sidecar.to_bytes().expect("serialise");
        let text = String::from_utf8(bytes.clone()).expect("utf8");
        assert!(text.contains("\"tool\": \"trellis2\""), "{text}");
        assert!(text.contains("nvdiffrast"), "{text}");
        assert!(text.ends_with("}\n"));
        let back = Sidecar::from_slice(&bytes, Path::new("vex_runner.json")).expect("parse back");
        assert_eq!(back, sidecar);
        assert_eq!(back.to_bytes().expect("again"), bytes, "byte-stable");
        let mesh = back.measured.and_then(|m| m.mesh).expect("mesh");
        assert!((mesh.height() - 1.8).abs() < 1e-5);
        assert_eq!(mesh.bones_skinned, 55);
    }

    /// A record from a retired generator is unreadable by design: none
    /// exist, and a build that silently read one as a body with no provenance
    /// would be worse than one that refuses it by name.
    #[test]
    fn an_unknown_generator_tag_is_refused_not_laundered() {
        let text = r#"{"schema": 2, "kind": "body", "name": "neutral", "created": "2026-08-23",
            "content_hash": "sha256:00", "generator": {"tool": "lab_body", "version": "0.2.0"}}"#;
        let error = parse(text).expect_err("must refuse");
        assert!(error.to_string().contains("lab_body"), "{error}");
    }

    /// The full directory beats the flat-layout fallback. This is the check
    /// that was missing once: `audio` is every audio kind's first segment,
    /// and the first of them in table order claimed every sound — silently,
    /// in the studio's browser.
    #[test]
    fn a_path_resolves_to_its_own_directory_not_the_first_audio_kind() {
        assert_eq!(Kind::of_rel_path("audio/sfx/blip.wav"), Some(Kind::Sfx));
        assert_eq!(Kind::of_rel_path("audio/voice/line.wav"), Some(Kind::Voice));
        assert_eq!(
            Kind::of_rel_path("audio/music/theme.ogg"),
            Some(Kind::Music)
        );
        assert_eq!(Kind::of_rel_path("clips/walk.glb"), Some(Kind::Clip));
        assert_eq!(Kind::of_rel_path("bodies/vex_runner.glb"), Some(Kind::Body));
        assert_eq!(Kind::of_rel_path("models/sword.glb"), Some(Kind::Model));
        assert_eq!(Kind::of_rel_path("library.json"), None);
        // And the fallback still catches a flatter layout, which is its job.
        assert_eq!(Kind::of_rel_path("audio/loose.wav"), Some(Kind::Sfx));
    }

    /// The two questions the studio asks of a kind: waveform or viewport, and
    /// does the rig contract apply.
    #[test]
    fn kinds_know_what_they_are() {
        assert!(!Kind::Body.is_audio());
        assert!(Kind::Body.is_mesh());
        assert!(Kind::Model.is_mesh());
        assert!(!Kind::Clip.is_mesh());
        assert!(Kind::Voice.is_audio());
        for kind in Kind::ALL {
            assert_eq!(Kind::parse(kind.as_str()), Some(kind));
            assert_eq!(kind.is_audio(), kind.audio_kind().is_some());
        }
        assert_eq!(Kind::parse("SFX"), Some(Kind::Sfx));
        assert_eq!(Kind::parse("animation"), None);
    }

    #[test]
    fn a_record_round_trips_events_and_root_motion() {
        let mut sidecar = Sidecar::new(Kind::Clip, "test");
        sidecar.events = Some(vec![AnimEvent {
            t: 0.15,
            t_src: Some(4.1),
            name: String::from("footstep_l"),
            origin: EventOrigin::Contacts,
            audio: AudioRef::parse("sfx:pistol_shot"),
        }]);
        sidecar.measured = Some(Measured {
            root_motion: Some(RootMotion {
                net_m: [2.611, 0.043],
                direction_deg: Some(0.9),
                peak_speed_mps: Some(1.4),
                track_xz_m: vec![[0.0, 0.0], [0.052, 0.001]],
            }),
            ..Measured::default()
        });
        let text = serde_json::to_string(&sidecar).expect("serialise");
        let back = parse(&text).expect("parse back");
        assert_eq!(back, sidecar);
        // Field order is the byte contract: events sits between measured and
        // content_hash.
        let measured_at = text.find("\"measured\"").expect("measured");
        let events_at = text.find("\"events\"").expect("events");
        let hash_at = text.find("\"content_hash\"").expect("content_hash");
        assert!(measured_at < events_at && events_at < hash_at, "{text}");
    }

    #[test]
    fn examined_and_empty_is_not_the_same_as_never_examined() {
        let mut sidecar = Sidecar::new(Kind::Clip, "test");
        sidecar.events = Some(Vec::new());
        let text = serde_json::to_string(&sidecar).expect("serialise");
        assert!(text.contains("\"events\":[]"), "{text}");
        let back = parse(&text).expect("parse back");
        assert_eq!(back.events, Some(Vec::new()));

        let never = Sidecar::new(Kind::Clip, "test");
        let text = serde_json::to_string(&never).expect("serialise");
        assert!(text.contains("\"events\":null"), "{text}");
    }

    #[test]
    fn actors_round_trip_and_keep_unfamiliar_names() {
        for (text, actor) in [
            ("human", Actor::Human),
            ("agent:claude", Actor::Agent(String::from("claude"))),
            ("claude", Actor::Agent(String::from("claude"))),
            ("unknown", Actor::Unknown),
            ("", Actor::Unknown),
        ] {
            assert_eq!(Actor::parse(text), actor, "{text:?}");
        }
        assert_eq!(Actor::Agent(String::from("x")).to_string(), "agent:x");
    }
}
