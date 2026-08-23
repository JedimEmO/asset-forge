//! Turn a raw ARDY take into an [`AnimationClip`] the rig can play.
//!
//! This is what makes generated candidates previewable the instant they exist,
//! with no Blender in the loop.
//!
//! # The transform
//!
//! A take's rotations are joint-local in ARDY's own bone frames. The rig's bones
//! rest in different frames, so each has to be re-expressed:
//!
//! ```text
//! q_local(j, t) = Crest(parent(j))⁻¹ · L_ardy(j, t) · Crest(j)
//! ```
//!
//! `Crest(j)` is the cumulative world **rest** rotation of bone `j`. Two things
//! about this are easy to get wrong and neither fails loudly:
//!
//! * It is **two-sided, and the left side is the *parent's* frame**. A
//!   single-bone conjugation is wrong by up to 169° and produces plausible
//!   poses, not obvious garbage.
//! * `Crest` must be read from the **rest** pose. Once a player is attached the
//!   bone transforms are animated, and sampling them then quietly bakes one
//!   frame of animation into the correction.
//!
//! Verified against a Blender-baked `.glb` of the same take: 0.0001° across all
//! 27 bones — and, since the bake went native, against [`forge_motion::bake`]
//! to under a millimetre by `tests/npz_fidelity.rs`, which is the test the
//! whole instant-preview design rests on.

use bevy::{
    animation::{AnimationClip, AnimationTargetId, animated_field, prelude::AnimatableCurve},
    math::curve::UnevenSampleAutoCurve,
    platform::collections::HashMap,
    prelude::*,
};

use forge_motion::{Take, skeleton};

/// One bone's animation target and rest frames.
#[derive(Debug, Clone)]
struct BoneFrame {
    target: AnimationTargetId,
    /// Cumulative world rest rotation of this bone.
    crest: Quat,
    /// Cumulative world rest rotation of its parent.
    parent_crest: Quat,
}

/// The rig's rest frames, sampled once so takes can be re-expressed into them.
#[derive(Debug, Clone, Default)]
pub struct RigFrames {
    bones: HashMap<String, BoneFrame>,
}

impl RigFrames {
    /// Walk a spawned rig's **rest** pose from its animation root.
    ///
    /// Call this before attaching an [`AnimationPlayer`]. Afterwards the bone
    /// transforms hold animated values, and the corrections would silently
    /// absorb a frame of motion.
    #[must_use]
    pub fn from_world(world: &World, anim_root: Entity) -> Self {
        let mut this = Self::default();
        this.walk(world, anim_root, &mut Vec::new(), Quat::IDENTITY);
        this
    }

    fn walk(&mut self, world: &World, entity: Entity, path: &mut Vec<Name>, parent_crest: Quat) {
        let Some(name) = world.get::<Name>(entity) else {
            return;
        };
        path.push(name.clone());
        let local = world
            .get::<Transform>(entity)
            .map_or(Quat::IDENTITY, |t| t.rotation);
        let crest = parent_crest * local;
        self.bones.insert(
            name.as_str().to_owned(),
            BoneFrame {
                target: AnimationTargetId::from_names(path.iter()),
                crest,
                parent_crest,
            },
        );
        if let Some(children) = world.get::<Children>(entity) {
            let children: Vec<Entity> = children.iter().collect();
            for child in children {
                self.walk(world, child, path, crest);
            }
        }
        path.pop();
    }

    /// Number of named bones recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bones.len()
    }

    /// Whether no bones were found — usually the wrong root entity.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bones.is_empty()
    }

    /// cskel27 joints this rig is missing, if any.
    ///
    /// A take cannot drive bones that are not there, and the omission is
    /// otherwise silent.
    #[must_use]
    pub fn missing_joints(&self) -> Vec<&'static str> {
        skeleton::JOINTS
            .into_iter()
            .filter(|j| !self.bones.contains_key(*j))
            .collect()
    }
}

/// Build a playable clip from `take`, expressed in `rig`'s bone frames.
///
/// The take is used as-is: trimming, retiming and root treatment all belong to
/// [`forge_motion::Edit`], which is applied first — **always**, even the
/// identity edit, because [`forge_motion::Edit::apply`] is also where a take
/// is turned into rig space (facing −Z). A raw take handed straight to this
/// function plays facing the wrong way. Keeping the edit out of here is what
/// lets the viewer tune a take and the CLI promote it through the same path.
///
/// Bones the rig lacks are skipped; use [`RigFrames::missing_joints`] to find
/// out rather than wondering why a limb is still.
#[must_use]
pub fn build(take: &Take, rig: &RigFrames) -> AnimationClip {
    let mut clip = AnimationClip::default();
    let frames = take.frames();
    if frames < 2 || take.fps <= 0.0 {
        return clip;
    }

    let times: Vec<f32> = (0..frames).map(|i| i as f32 / take.fps).collect();

    for (j, joint) in skeleton::JOINTS.into_iter().enumerate() {
        let Some(bone) = rig.bones.get(joint) else {
            continue;
        };
        let inv_parent = bone.parent_crest.inverse();
        let values = take
            .rotations
            .iter()
            .map(|frame| inv_parent * frame[j] * bone.crest);
        if let Ok(curve) = UnevenSampleAutoCurve::new(times.iter().copied().zip(values)) {
            clip.add_curve_to_target(
                bone.target,
                AnimatableCurve::new(animated_field!(Transform::rotation), curve),
            );
        }
    }

    if let Some(hips) = rig.bones.get(skeleton::JOINTS[skeleton::ROOT]) {
        // The rig expects hips translation in its parent's frame.
        let inv_parent = hips.parent_crest.inverse();
        let values = take.root.iter().map(|p| inv_parent * *p);
        if let Ok(curve) = UnevenSampleAutoCurve::new(times.iter().copied().zip(values)) {
            clip.add_curve_to_target(
                hips.target,
                AnimatableCurve::new(animated_field!(Transform::translation), curve),
            );
        }
    }

    clip
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rig with only the root bone, so the frames are checkable by hand.
    fn hips_only() -> (World, Entity) {
        let mut world = World::new();
        let hips = world
            .spawn((
                Name::new("Hips"),
                Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
            ))
            .id();
        let root = world
            .spawn((Name::new("Armature"), Transform::IDENTITY))
            .add_child(hips)
            .id();
        (world, root)
    }

    #[test]
    fn rest_frames_accumulate_down_the_hierarchy_and_name_what_is_missing() {
        let (world, root) = hips_only();
        let rig = RigFrames::from_world(&world, root);
        assert_eq!(rig.len(), 2);
        let hips = rig.bones.get("Hips").expect("Hips");
        assert!(hips.parent_crest.abs_diff_eq(Quat::IDENTITY, 1e-6));
        assert!(
            hips.crest
                .abs_diff_eq(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2), 1e-6)
        );
        let missing = rig.missing_joints();
        assert_eq!(missing.len(), skeleton::JOINT_COUNT - 1);
        assert!(!missing.contains(&"Hips"));
    }

    #[test]
    fn a_take_too_short_to_sample_builds_nothing() {
        let (world, root) = hips_only();
        let rig = RigFrames::from_world(&world, root);
        let take = Take {
            rotations: vec![[Quat::IDENTITY; skeleton::JOINT_COUNT]],
            root: vec![Vec3::ZERO],
            contacts: None,
            fps: 20.0,
            prompt: String::new(),
        };
        assert!(build(&take, &rig).curves().is_empty());
    }

    #[test]
    fn only_bones_the_rig_has_get_curves() {
        let (world, root) = hips_only();
        let rig = RigFrames::from_world(&world, root);
        let take = Take {
            rotations: vec![[Quat::IDENTITY; skeleton::JOINT_COUNT]; 3],
            root: vec![Vec3::ZERO, Vec3::Y, Vec3::Y * 2.0],
            contacts: None,
            fps: 20.0,
            prompt: String::new(),
        };
        let clip = build(&take, &rig);
        // One target (Hips) carrying a rotation and a translation curve.
        assert_eq!(clip.curves().len(), 1);
        assert!((clip.duration() - 0.1).abs() < 1e-6);
    }
}
