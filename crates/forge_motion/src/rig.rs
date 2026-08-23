//! The rest pose and inverse bind matrices a bake needs, read from a rig
//! `.glb` rather than hardcoded.
//!
//! The rig profile's `rig.glb` (`rigs/humanoid/rig.glb` for the humanoid
//! profile) is the single source of truth for the skeleton — 55 bones, of
//! which clips drive the 27 cskel27 joints. Reading it at bake time instead
//! of freezing its numbers into this crate means a rig revision changes every
//! future bake without a code change, and a rig that quietly diverged from
//! cskel27's topology is refused instead of conjugated into plausible-looking
//! nonsense.
//!
//! This reader deliberately knows nothing of the profile's `contract.json`:
//! it finds the 27 joints by the names in [`crate::skeleton`], so the crate
//! depends on no profile loader and a clip can be baked against any `.glb`
//! that carries the driven layout.

use glam::{Quat, Vec3};

use crate::skeleton::{self, JOINT_COUNT, JOINTS, PARENTS};
use crate::{BakeError, Result, open_glb};

/// How far a rest scale may sit from 1.0 before it stops being exporter
/// float noise. The shipped rigs carry values like 1.0000002.
const SCALE_TOLERANCE: f32 = 1e-3;

/// One cskel27 bone as the rig file defines it.
#[derive(Debug, Clone)]
pub struct RigBone {
    /// The joint name, from [`JOINTS`].
    pub name: &'static str,
    /// Parent's index into [`RigDef::bones`]; `None` for `Hips`.
    pub parent: Option<usize>,
    /// Rest rotation, local to the parent bone.
    pub rotation: Quat,
    /// Rest translation, local to the parent bone, metres.
    pub translation: Vec3,
    /// Inverse bind matrix, column-major as glTF stores it.
    pub inverse_bind: [f32; 16],
}

/// The 27 cskel27 bones in ARDY joint order, plus their cumulative rest
/// rotations.
#[derive(Debug, Clone)]
pub struct RigDef {
    /// Bones indexed exactly like [`JOINTS`].
    pub bones: Vec<RigBone>,
    /// `crest[j]` — the cumulative world rest rotation of joint `j`, the
    /// `Crest` of the channel convention (see the crate docs).
    crest: Vec<Quat>,
}

impl RigDef {
    /// Read the rest pose and inverse bind matrices out of a rig `.glb`.
    ///
    /// The file may carry more bones than cskel27 — the humanoid `rig.glb` adds
    /// finger leaves — but the 27 must be present, uniquely named, parented
    /// as [`PARENTS`] says, resting at scale 1, under an identity armature,
    /// and covered by one skin.
    ///
    /// # Errors
    ///
    /// Refuses, naming the finding, when any of those constraints fails; see
    /// [`BakeError`].
    pub fn from_glb(bytes: &[u8]) -> Result<Self> {
        let (document, blob) = open_glb(bytes)?;

        let mut node_of = vec![None; JOINT_COUNT];
        for node in document.nodes() {
            let Some(j) = node.name().and_then(skeleton::index_of) else {
                continue;
            };
            if node_of[j].replace(node.index()).is_some() {
                return Err(BakeError::DuplicateJoint(JOINTS[j]));
            }
        }

        let nodes: Vec<gltf::Node<'_>> = document.nodes().collect();
        let mut parent_of = vec![None; nodes.len()];
        for node in &nodes {
            for child in node.children() {
                parent_of[child.index()] = Some(node.index());
            }
        }

        let mut bones = Vec::with_capacity(JOINT_COUNT);
        let mut crest = Vec::with_capacity(JOINT_COUNT);
        for (j, joint) in JOINTS.into_iter().enumerate() {
            let index = node_of[j].ok_or(BakeError::MissingJoint(joint))?;
            let parent_index = parent_of[index];
            let found = || {
                parent_index.map_or_else(
                    || String::from("<none>"),
                    |p| nodes[p].name().unwrap_or("<unnamed>").to_owned(),
                )
            };
            if let Some(expected) = PARENTS[j] {
                if parent_index != node_of[expected] {
                    return Err(BakeError::WrongParent {
                        joint,
                        expected: JOINTS[expected],
                        found: found(),
                    });
                }
            } else {
                // Hips must hang under an identity armature, or the root
                // track would need re-expressing before it became the
                // translation channel.
                let armature =
                    parent_index.ok_or_else(|| BakeError::ArmatureNotIdentity(found()))?;
                let (t, r, s) = nodes[armature].transform().decomposed();
                if !is_identity(t, r, s) {
                    return Err(BakeError::ArmatureNotIdentity(format!(
                        "{}: T {t:?} R {r:?} S {s:?}",
                        nodes[armature].name().unwrap_or("<unnamed>")
                    )));
                }
            }

            let (t, r, s) = nodes[index].transform().decomposed();
            if s.iter().any(|c| (c - 1.0).abs() > SCALE_TOLERANCE) {
                return Err(BakeError::ScaledRest { joint, scale: s });
            }
            let rotation = Quat::from_xyzw(r[0], r[1], r[2], r[3]).normalize();
            crest.push(match PARENTS[j] {
                Some(p) => crest[p] * rotation,
                None => rotation,
            });
            bones.push(RigBone {
                name: joint,
                parent: PARENTS[j],
                rotation,
                translation: Vec3::from_array(t),
                inverse_bind: [0.0; 16],
            });
        }

        read_inverse_binds(&document, &blob, &node_of, &mut bones)?;
        Ok(Self { bones, crest })
    }

    /// Re-express an ARDY joint-local rotation as this rig's node rotation —
    /// the channel convention from the crate docs, in one place.
    #[must_use]
    pub fn to_node_rotation(&self, joint: usize, ardy_local: Quat) -> Quat {
        let parent_crest = self.bones[joint]
            .parent
            .map_or(Quat::IDENTITY, |p| self.crest[p]);
        (parent_crest.inverse() * ardy_local * self.crest[joint]).normalize()
    }
}

/// Whether a decomposed transform is identity, allowing exporter float noise.
fn is_identity(t: [f32; 3], r: [f32; 4], s: [f32; 3]) -> bool {
    t.iter().all(|c| c.abs() < 1e-5)
        && r[..3].iter().all(|c| c.abs() < 1e-5)
        && (r[3].abs() - 1.0).abs() < 1e-5
        && s.iter().all(|c| (c - 1.0).abs() < 1e-5)
}

/// Copy each joint's inverse bind matrix out of the first skin covering all
/// 27, keyed by node so the rig file's own joint ordering does not matter.
fn read_inverse_binds(
    document: &gltf::Document,
    blob: &[u8],
    node_of: &[Option<usize>],
    bones: &mut [RigBone],
) -> Result<()> {
    for skin in document.skins() {
        let joints: Vec<usize> = skin.joints().map(|n| n.index()).collect();
        if !node_of
            .iter()
            .all(|n| n.is_some_and(|n| joints.contains(&n)))
        {
            continue;
        }
        let reader = skin.reader(|buffer| match buffer.source() {
            gltf::buffer::Source::Bin => Some(blob),
            gltf::buffer::Source::Uri(_) => None,
        });
        let Some(matrices) = reader.read_inverse_bind_matrices() else {
            continue;
        };
        let matrices: Vec<[[f32; 4]; 4]> = matrices.collect();
        if matrices.len() < joints.len() {
            return Err(BakeError::Glb(format!(
                "skin has {} joints but {} inverse bind matrices",
                joints.len(),
                matrices.len()
            )));
        }
        for (bone, node) in bones.iter_mut().zip(node_of) {
            let position = joints
                .iter()
                .position(|j| Some(*j) == *node)
                .expect("checked above");
            let m = matrices[position];
            let mut flat = [0.0; 16];
            for (column, cells) in m.into_iter().enumerate() {
                flat[column * 4..column * 4 + 4].copy_from_slice(&cells);
            }
            bone.inverse_bind = flat;
        }
        return Ok(());
    }
    Err(BakeError::NoSkin)
}
