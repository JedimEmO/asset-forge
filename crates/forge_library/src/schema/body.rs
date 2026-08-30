//! The per-body skeleton a body's record carries: schema 2's one addition.
//!
//! # Why a body says what its own bones are
//!
//! Until schema 2 a skeleton was one frozen table and every body was scaled
//! to fit it. That refuses a four-head witch and a giant on the same profile,
//! and the 2026-08-30 spike showed the trade is unnecessary: a baked clip
//! carries a rotation curve per bone plus one translation track on the root,
//! so a skeleton with the same **names**, **hierarchy** and rest
//! **rotations** but per-body bone **lengths** binds every clip in the
//! library with nothing rebaked. Lengths belong to the body; directions do
//! not.
//!
//! So the record grows the two things a consumer cannot re-derive from the
//! profile: every bone's local rest translation as this body actually
//! carries it, and [`Body::motion_scale`] — how far its root sits off the
//! ground against the profile's, which is the one number a consumer applies,
//! to the root translation track and to nothing else.
//!
//! # It is re-derived from the file, never copied from a rig record
//!
//! [`Body::derive`] reads the `.glb` being promoted. That is the whole point
//! of the claim: a sidecar that copied its bone table out of the fitter's
//! own output would say what the fitter *meant*, and `verify` could only
//! check it against the thing it came from. Read from the file, the claim is
//! re-derivable by anyone holding the file, which is what
//! [`Body::disagreements`] does on every run of `forge verify`.
//!
//! # Six decimals and four
//!
//! Translations are rounded to a micron and the scale to four decimals
//! before they are written. Not for looks: an `f32` printed at shortest
//! round-trip precision spells the same measurement differently on two
//! machines' last bit, and a record whose bytes move without its meaning
//! moving is a diff nobody can read. A micron is four orders below the
//! 0.1 mm the check runs at, so the rounding never decides anything.

use serde::{Deserialize, Serialize};

use forge_rig::{Contract, RestComparison, compare_rest_translation, derive_from_glb};

/// How far a shipped body's re-derived rest translation may sit from the one
/// its record states, per axis: 0.1 mm, the exporter's own old rule.
pub const BONE_TOLERANCE_M: f32 = 1e-4;

/// How far a recomputed [`Body::motion_scale`] may sit from the recorded
/// one. It multiplies a root track in metres, so five thousandths is
/// millimetres of stride on a body-sized skeleton.
pub const MOTION_SCALE_TOLERANCE: f32 = 0.005;

/// One bone of a body's own skeleton.
///
/// The rotation is not here on purpose: rest rotations are frozen by the
/// contract and checked against it, so restating them per body would be a
/// second copy of a fact that must never differ.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyBone {
    /// The contract bone's exact name — the string clips hash.
    pub name: String,
    /// Rest translation local to the parent, metres, glTF axes, rounded to
    /// six decimals.
    pub rest_translation: [f32; 3],
}

/// The skeleton one body carries, as its record states it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Body {
    /// This body's root rest height over the contract's, to four decimals.
    ///
    /// **A consumer multiplies the root translation track by this and
    /// nothing else** — never a rotation, never another bone's translation.
    /// A short body's stride shortens with its legs; everything else about a
    /// clip is already right, because a rotation is a rotation on any length
    /// of bone.
    pub motion_scale: f32,
    /// Every contract bone, in contract order.
    pub bones: Vec<BodyBone>,
}

impl Body {
    /// Read a body's own skeleton out of the `.glb` being promoted.
    ///
    /// Bones come back in **contract order**, not glb node order: the
    /// contract's order is what `parent` indices everywhere else point into,
    /// and an exporter is free to write its nodes in another one.
    ///
    /// # Errors
    ///
    /// The bytes are not a readable rig ([`derive_from_glb`]), or the file
    /// is missing a contract bone — which `promote_body`'s own contract
    /// check refuses first, in more words.
    pub fn derive(contract: &Contract, glb: &[u8]) -> Result<Self, String> {
        let derived = derive_from_glb(glb).map_err(|e| e.to_string())?;
        let mut bones = Vec::with_capacity(contract.bones.len());
        for spec in &contract.bones {
            let Some(index) = derived.find(&spec.name) else {
                return Err(format!(
                    "the file has no bone named {} — a body carries every contract bone",
                    spec.name
                ));
            };
            bones.push(BodyBone {
                name: spec.name.clone(),
                rest_translation: round_translation(derived.bones[index].rest_translation),
            });
        }
        let Some(root) = derived.find(&contract.root) else {
            return Err(format!(
                "the file has no root bone named {} — nothing can bind without it",
                contract.root
            ));
        };
        Ok(Self {
            motion_scale: round4(motion_scale(contract, derived.rest_world(root).0.y)),
            bones,
        })
    }

    /// The bone of this name, when the body carries one.
    #[must_use]
    pub fn bone(&self, name: &str) -> Option<&BodyBone> {
        self.bones.iter().find(|bone| bone.name == name)
    }

    /// Everything this record says that the file beside it does not, in one
    /// line each — empty when the two agree.
    ///
    /// Three questions, and they are deliberately different questions. The
    /// **translations** must match what the record states to
    /// [`BONE_TOLERANCE_M`], because the record is a claim about this file.
    /// The **directions** must match the *contract* to
    /// [`Contract::rest_direction_tolerance_deg`], because a fitted skeleton
    /// may lengthen a bone and may not turn one. And the **motion scale**
    /// must be the one the file's own root height gives, to
    /// [`MOTION_SCALE_TOLERANCE`], or a consumer scales a stride by a number
    /// nothing measured.
    #[must_use]
    pub fn disagreements(&self, contract: &Contract, glb: &[u8]) -> Vec<String> {
        let derived = match derive_from_glb(glb) {
            Ok(derived) => derived,
            Err(error) => {
                return vec![format!(
                    "the rig cannot be re-derived from the file: {error}"
                )];
            }
        };
        let mut out = Vec::new();
        if self.bones.len() != contract.bones.len() {
            out.push(format!(
                "the record states {} bone(s), the contract has {} — re-promote the body",
                self.bones.len(),
                contract.bones.len()
            ));
        }
        for spec in &contract.bones {
            let Some(stated) = self.bone(&spec.name) else {
                out.push(format!("the record says nothing about {}", spec.name));
                continue;
            };
            let Some(index) = derived.find(&spec.name) else {
                out.push(format!(
                    "{} is in the record and not in the file — the two are different bodies",
                    spec.name
                ));
                continue;
            };
            let found = derived.bones[index].rest_translation;
            for (axis, (a, b)) in found.iter().zip(stated.rest_translation).enumerate() {
                if (a - b).abs() > BONE_TOLERANCE_M {
                    out.push(format!(
                        "{}: rest translation [{axis}] is {a} in the file, {b} in the record \
                         ({:.4} mm apart)",
                        spec.name,
                        (a - b).abs() * 1000.0
                    ));
                }
            }
            if let RestComparison::Measured { direction_deg, .. } =
                compare_rest_translation(spec.rest_translation, found)
                && direction_deg > contract.rest_direction_tolerance_deg
            {
                out.push(format!(
                    "{}'s rest translation points {direction_deg:.2} deg off the contract — \
                     lengths are per body, directions are not",
                    spec.name
                ));
            }
        }
        match derived.find(&contract.root) {
            None => out.push(format!(
                "the file has no root bone named {} — nothing can bind without it",
                contract.root
            )),
            Some(root) => {
                let recomputed = motion_scale(contract, derived.rest_world(root).0.y);
                let gap = (recomputed - self.motion_scale).abs();
                if gap > MOTION_SCALE_TOLERANCE {
                    out.push(format!(
                        "motion_scale is {:.4} in the record and {recomputed:.4} measured off \
                         the file ({gap:.4} apart) — a consumer would scale the root track by \
                         a number nothing measured",
                        self.motion_scale
                    ));
                }
            }
        }
        out
    }
}

/// A body's root rest height over the contract's.
///
/// `1.0` when the contract's own root sits on the floor: a ratio against
/// zero is not a number, and a body whose profile puts its root at the
/// origin scales nothing.
fn motion_scale(contract: &Contract, found_y: f32) -> f32 {
    let reference = contract.rest_world(contract.root_index()).0.y;
    if reference.abs() < f32::EPSILON {
        1.0
    } else {
        found_y / reference
    }
}

/// To six decimals — a micron, four orders below the check's tolerance.
fn round_translation(t: [f32; 3]) -> [f32; 3] {
    [round6(t[0]), round6(t[1]), round6(t[2])]
}

/// One component, to six decimals.
///
/// Done in `f32` throughout rather than through `f64` and back: the record
/// holds `f32`, and a value that is already at metre scale multiplied by a
/// million stays inside `f32`'s seven digits, so the round trip a cast would
/// add buys nothing and could not be exact anyway.
fn round6(value: f32) -> f32 {
    (value * 1e6).round() / 1e6
}

/// The motion scale, to four decimals.
fn round4(value: f32) -> f32 {
    (value * 1e4).round() / 1e4
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> forge_rig::RigProfile {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/humanoid");
        forge_rig::RigProfile::load(&dir).expect("the humanoid profile")
    }

    /// The mannequin is the profile's own skeleton in a skin, so its
    /// derived block is the contract restated: fifty-five bones in contract
    /// order and a motion scale of exactly one.
    #[test]
    fn a_body_on_the_profile_derives_the_contract_and_a_scale_of_one() {
        let profile = profile();
        let glb = forge_rig::fixture::mannequin_glb(&profile.contract).expect("mannequin");
        let body = Body::derive(&profile.contract, &glb).expect("derives");
        assert_eq!(body.bones.len(), profile.contract.bones.len());
        for (stated, spec) in body.bones.iter().zip(&profile.contract.bones) {
            assert_eq!(stated.name, spec.name, "bones are in contract order");
        }
        assert!(
            (body.motion_scale - 1.0).abs() < 1e-6,
            "{}",
            body.motion_scale
        );
        assert!(body.disagreements(&profile.contract, &glb).is_empty());
    }

    /// Two millimetres of drift between the record and the file is the
    /// finding; so is a bone turned by a degree and a half, and so is a
    /// motion scale nobody measured. All three, because they fail for
    /// different reasons and a reader has to be told which.
    #[test]
    fn a_drifted_record_names_the_bone_the_angle_and_the_scale() {
        let profile = profile();
        let glb = forge_rig::fixture::mannequin_glb(&profile.contract).expect("mannequin");
        let good = Body::derive(&profile.contract, &glb).expect("derives");

        let mut drifted = good.clone();
        let bone = drifted
            .bones
            .iter_mut()
            .find(|b| b.name == "LeftFoot")
            .expect("LeftFoot");
        bone.rest_translation[1] += 0.002;
        let found = drifted.disagreements(&profile.contract, &glb);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].starts_with("LeftFoot: rest translation [1]"),
            "{found:?}"
        );
        assert!(found[0].contains("mm apart"), "{found:?}");
        assert!(found[0].contains("2.00"), "the gap is stated: {found:?}");

        let mut scaled = good.clone();
        scaled.motion_scale = 0.9;
        let found = scaled.disagreements(&profile.contract, &glb);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("motion_scale is 0.9000"), "{found:?}");

        // A degree and a half of turn on the bone the contract froze. The
        // record still matches the file to the micron — this is the file
        // disagreeing with the *contract*, which is the whole difference
        // between a length and a direction.
        let mut turned = profile.contract.clone();
        let index = turned.find("LeftFoot").expect("LeftFoot");
        let spec = &mut turned.bones[index];
        let axis = glam::Vec3::from_array(spec.rest_translation);
        let rotated = glam::Quat::from_rotation_z(1.5_f32.to_radians()) * axis;
        spec.rest_translation = rotated.to_array();
        let found = good.disagreements(&turned, &glb);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].starts_with("LeftFoot's rest translation points 1.50 deg off the contract"),
            "{found:?}"
        );
        assert!(
            found[0].ends_with("lengths are per body, directions are not"),
            "{found:?}"
        );
    }

    /// The rounding is the writer's, so a second derive of the same file is
    /// the same record, byte for byte.
    #[test]
    fn deriving_twice_writes_the_same_record() {
        let profile = profile();
        let glb = forge_rig::fixture::mannequin_glb(&profile.contract).expect("mannequin");
        let first = Body::derive(&profile.contract, &glb).expect("derives");
        let again = Body::derive(&profile.contract, &glb).expect("derives");
        assert_eq!(first, again);
        assert_eq!(
            serde_json::to_string(&first).expect("json"),
            serde_json::to_string(&again).expect("json")
        );
    }
}
