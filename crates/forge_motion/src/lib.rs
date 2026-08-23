//! Motion takes as data: read an ARDY `.npz`, apply the edit knobs, derive
//! footsteps, and bake the result into a `.glb` clip on a rig — no engine, no
//! Blender.
//!
//! The crate has two halves that used to be two crates. The reading half
//! ([`take`], [`npy`], [`skeleton`], [`edit`], [`events`]) turns a raw take
//! into an edited one in memory, which is what makes an interactive
//! generate / reroll / edit loop feel immediate rather than batch. The
//! writing half ([`rig`], [`mod@bake`], [`channels`]) is the native bake that
//! replaced the npz → BVH → Blender pipeline, comparable frame-by-frame
//! against what that pipeline shipped.
//!
//! # Turning a take into skeleton rotations
//!
//! A take's rotations are joint-local in ARDY's own bone frames. A rig whose
//! bones rest in different frames — which is every rig, including one built
//! from this very skeleton via Blender — needs them re-expressed:
//!
//! ```text
//! q_node(j, t) = Crest(parent(j))⁻¹ · L_ardy(j, t) · Crest(j)
//! ```
//!
//! where `L_ardy` is the edited take's joint-local rotation and `Crest(j)` is
//! the cumulative world **rest** rotation of bone `j` in the target rig,
//! rooted at an `Armature` node whose own transform is identity. The
//! correction is two-sided and reaches into the *parent's* rest frame; a
//! single-bone conjugation is wrong by up to 169°, and wrong in a way that
//! looks like a plausible pose rather than an error. Verified against a
//! Blender-baked `.glb` of the same take to 0.0001°.
//!
//! # The channel convention, measured, not assumed
//!
//! The clips the Blender pipeline shipped were produced by writing a BVH and
//! round-tripping it through Blender's importer and glTF exporter, so what
//! ended up in the node channels was an empirical question. The answer,
//! verified frame-by-frame against `gen_walk` (strip + loop blend),
//! `gen_roll` (detrend, travelling), `gen_pistol_shoot` (retimed) and
//! `gen_rifle_idle`, is recorded by `tests/convention.rs` and is exactly the
//! conjugation above — the same maths the studio's take preview uses. This
//! crate adds **no basis change of its own** — but the edited take it
//! receives is already in **rig space**: [`Edit::apply`] ends every recipe
//! by turning the whole pose 180° about Y (root joint orientation and root
//! travel together), so baked clips *play* facing −Z, the rig profile's
//! forward. The Blender-era clips did not have that turn — they played
//! facing ARDY's +Z, and every consumer had to carry its own compensation;
//! the frozen fixtures under `tests/fixtures/blender` still hold that old
//! output, and the proof tests factor the turn out explicitly before
//! comparing.
//!
//! The `Hips` translation channel is the edited root track **verbatim**: same
//! metres, same Y-up axes, no sign flips beyond the rig-space turn already in
//! the take. Measured against the four fixture clips, the conjugation
//! reproduces the frozen rotation channels (modulo that turn on the root) to
//! a worst quaternion-component difference in the 1e-6 range and the frozen
//! translation to under a thousandth of a millimetre — the residue is float32
//! noise in the Blender chain, not a missing term.
//!
//! # What [`fn@bake`] writes, and where it deliberately differs
//!
//! One `Armature` root, the 27 cskel27 bones with rest transforms read from
//! the rig file, a skin with that file's inverse bind matrices, and a
//! three-vertex `SkinAnchor` mesh skinned to `Hips` — importers only build a
//! skeleton for nodes a skin references, so a clip with no mesh at all would
//! load as loose transforms. The animation carries **28 channels**: 27
//! rotation curves plus one `Hips` translation curve, linear, keyed at the
//! take's frame rate.
//!
//! The Blender path wrote 81 channels — the extra 53 were 2-key constant
//! curves restating every bone's rest offset and a 1.0 scale. Those were not
//! harmless padding: a constant translation channel *imposes cskel27's bone
//! lengths* on whatever rig the clip is bound to. Dropping them is a planned
//! fix, not an omission, and it is why the proof tests compare rotations and
//! the `Hips` translation rather than byte equality.

pub mod bake;
pub mod channels;
pub mod edit;
pub mod events;
mod glb;
pub mod npy;
pub mod rig;
pub mod skeleton;
pub mod take;

pub use bake::bake;
pub use channels::ClipChannels;
pub use edit::{Edit, InPlace, YMode};
pub use rig::RigDef;
pub use take::{Take, TakeError};

use std::fmt;

/// Why a bake or a `.glb` inspection failed.
#[derive(Debug, Clone)]
pub enum BakeError {
    /// The bytes did not parse as glTF, or a buffer they promise is absent.
    Glb(String),
    /// A cskel27 joint the rig file must define is missing.
    MissingJoint(&'static str),
    /// A joint name appears on more than one node, so "the" bone is ambiguous.
    DuplicateJoint(&'static str),
    /// A joint is parented differently than cskel27 says, which would make
    /// the rest conjugation silently wrong rather than loudly.
    WrongParent {
        /// The joint whose parent is off.
        joint: &'static str,
        /// The parent cskel27 requires.
        expected: &'static str,
        /// The parent the file has, or `<none>`.
        found: String,
    },
    /// A bone rests at a scale far enough from 1 that dropping the scale
    /// channels would visibly change the rig.
    ScaledRest {
        /// The offending joint.
        joint: &'static str,
        /// Its rest scale.
        scale: [f32; 3],
    },
    /// The armature node above `Hips` is not identity, which would break the
    /// "translation channel is the root track verbatim" convention.
    ArmatureNotIdentity(String),
    /// No skin in the rig file covers all 27 joints, so there are no inverse
    /// bind matrices to copy.
    NoSkin,
    /// The edited take has too few frames to key a curve.
    TooShort {
        /// Frames left after the edit.
        frames: usize,
    },
    /// The take's frame rate is not usable as a key spacing.
    BadFps(f32),
    /// The animation to inspect is missing entirely.
    NoAnimation,
    /// A character was handed to the writer with no mesh at all. Importers
    /// only assemble a skeleton for joints some skin uses, so the bones would
    /// arrive as loose nodes.
    NoMeshes,
    /// A named mesh has no vertices or no triangles, so it has no bounds to
    /// declare and nothing to draw.
    EmptyMesh(String),
    /// A vertex coordinate is NaN or infinite. Left alone it would reach the
    /// POSITION bounds and serialize as JSON `null`.
    NonFiniteVertex {
        /// The mesh holding it.
        mesh: String,
        /// Its index in that mesh.
        vertex: usize,
    },
    /// A vertex is weighted to a joint beyond the end of the skin's joint
    /// list — a mesh generated against a different set of extra bones.
    JointOutOfRange {
        /// The mesh holding the vertex.
        mesh: String,
        /// The joint index it names.
        joint: u16,
        /// How many joints the skin has.
        joints: usize,
    },
    /// An extra bone hangs under a bone the file will not contain.
    UnknownBoneParent {
        /// The extra bone.
        bone: String,
        /// The parent it named.
        parent: String,
    },
    /// Two nodes in one file would carry the same name. Bones and mesh parts
    /// are both addressed by name at runtime, so the second one would be
    /// unreachable — and if it shadowed a bone, so would the bone.
    DuplicateNodeName(String),
    /// A rest rotation is not a unit quaternion, which would bake a scale or
    /// a skew into that bone's inverse bind matrix.
    NonUnitBoneRotation {
        /// The bone.
        bone: String,
        /// Its rest rotation, `[x, y, z, w]`.
        rotation: [f32; 4],
    },
}

impl fmt::Display for BakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Glb(msg) => write!(f, "not a readable .glb: {msg}"),
            Self::MissingJoint(joint) => write!(f, "the rig has no {joint} bone"),
            Self::DuplicateJoint(joint) => {
                write!(f, "the rig names {joint} on more than one node")
            }
            Self::WrongParent {
                joint,
                expected,
                found,
            } => write!(
                f,
                "{joint} is parented to {found}, but cskel27 parents it to {expected}"
            ),
            Self::ScaledRest { joint, scale } => write!(
                f,
                "{joint} rests at scale {scale:?}; this baker writes no scale channels, \
                 so a scaled rest would ship a different skeleton than the rig file"
            ),
            Self::ArmatureNotIdentity(detail) => write!(
                f,
                "the armature above Hips is not an identity transform ({detail}), \
                 so the root track would not be the Hips translation channel"
            ),
            Self::NoSkin => write!(f, "no skin in the rig file covers all 27 cskel27 joints"),
            Self::TooShort { frames } => write!(
                f,
                "the edited take has {frames} frame(s); a curve needs at least 2"
            ),
            Self::BadFps(fps) => write!(f, "cannot key frames at {fps} fps"),
            Self::NoAnimation => write!(f, "the .glb has no animation"),
            Self::NoMeshes => write!(
                f,
                "a character .glb needs at least one mesh; a skin nothing \
                 references does not import as a skeleton"
            ),
            Self::EmptyMesh(mesh) => write!(f, "mesh {mesh} has no vertices or no triangles"),
            Self::NonFiniteVertex { mesh, vertex } => {
                write!(f, "mesh {mesh} vertex {vertex} is not a finite position")
            }
            Self::JointOutOfRange {
                mesh,
                joint,
                joints,
            } => write!(
                f,
                "mesh {mesh} weights a vertex to joint {joint}, but the skin \
                 has {joints} joints"
            ),
            Self::UnknownBoneParent { bone, parent } => {
                write!(
                    f,
                    "extra bone {bone} hangs under {parent}, which this file \
                     will not contain"
                )
            }
            Self::DuplicateNodeName(name) => write!(
                f,
                "two nodes would be named {name}; bones and mesh parts are \
                 both resolved by name"
            ),
            Self::NonUnitBoneRotation { bone, rotation } => write!(
                f,
                "{bone} rests at rotation {rotation:?}, which is not a unit \
                 quaternion"
            ),
        }
    }
}

impl std::error::Error for BakeError {}

/// Convenience alias used throughout the bake half of the crate.
pub type Result<T> = std::result::Result<T, BakeError>;

/// Parse GLB bytes into a document plus its binary chunk.
///
/// Only self-contained GLBs are accepted: every shipped clip and the rig file
/// embed their one buffer, and a `uri` reference would mean reading files
/// this crate was never handed.
///
/// # Errors
///
/// Fails when the bytes do not parse as glTF, or when the container carries
/// no binary chunk — a `uri`-buffered file, which is exactly the
/// non-self-contained case this refuses.
pub fn open_glb(bytes: &[u8]) -> Result<(gltf::Document, Vec<u8>)> {
    let gltf::Gltf { document, blob } =
        gltf::Gltf::from_slice(bytes).map_err(|e| BakeError::Glb(e.to_string()))?;
    let blob = blob.ok_or_else(|| BakeError::Glb(String::from("no binary chunk")))?;
    Ok((document, blob))
}
