//! ARDY's cskel27 skeleton — the driven layout of the humanoid rig profile.
//!
//! The order is `bone_order_names` from `ardy.skeleton.registry.build_skeleton(27)`
//! and it is load-bearing: `local_rot_mats[:, j]` is indexed by it. Getting it
//! wrong does not error — it silently animates the wrong limb.
//!
//! The same layout is published as data in `rigs/humanoid/motion_skeleton.json`
//! for the Python review to read; a test under `tests/` holds every constant
//! here equal to that file, so the two cannot drift apart unnoticed.

/// Joint count.
pub const JOINT_COUNT: usize = 27;

/// Joint names, in the order ARDY writes them.
pub const JOINTS: [&str; JOINT_COUNT] = [
    "Hips",
    "Spine",
    "Spine1",
    "Spine2",
    "Spine3",
    "Neck",
    "Head",
    "RightShoulder",
    "RightArm",
    "RightForeArm",
    "RightHand",
    "RightHandEnd",
    "RightHandThumb1",
    "LeftShoulder",
    "LeftArm",
    "LeftForeArm",
    "LeftHand",
    "LeftHandEnd",
    "LeftHandThumb1",
    "RightUpLeg",
    "RightLeg",
    "RightFoot",
    "RightToeBase",
    "LeftUpLeg",
    "LeftLeg",
    "LeftFoot",
    "LeftToeBase",
];

/// Parent index per joint; `None` for the root.
pub const PARENTS: [Option<usize>; JOINT_COUNT] = [
    None,     // Hips
    Some(0),  // Spine
    Some(1),  // Spine1
    Some(2),  // Spine2
    Some(3),  // Spine3
    Some(4),  // Neck
    Some(5),  // Head
    Some(4),  // RightShoulder
    Some(7),  // RightArm
    Some(8),  // RightForeArm
    Some(9),  // RightHand
    Some(10), // RightHandEnd
    Some(10), // RightHandThumb1
    Some(4),  // LeftShoulder
    Some(13), // LeftArm
    Some(14), // LeftForeArm
    Some(15), // LeftHand
    Some(16), // LeftHandEnd
    Some(16), // LeftHandThumb1
    Some(0),  // RightUpLeg
    Some(19), // RightLeg
    Some(20), // RightFoot
    Some(21), // RightToeBase
    Some(0),  // LeftUpLeg
    Some(23), // LeftLeg
    Some(24), // LeftFoot
    Some(25), // LeftToeBase
];

/// The root joint: `Hips`, the one whose translation a clip carries.
pub const ROOT: usize = 0;

/// The `Head` joint — where a stature is measured to and a close-up framed.
pub const HEAD: usize = 6;

/// The two hand joints, right then left, matching [`JOINTS`] order.
pub const HANDS: [usize; 2] = [10, 16];

/// The four foot joints — `RightFoot`, `RightToeBase`, `LeftFoot`,
/// `LeftToeBase` — in [`JOINTS`] order, right leg first.
pub const FEET: [usize; 4] = [21, 22, 25, 26];

/// Which joint each column of a take's `foot_contacts` array labels:
/// `[LeftFoot, LeftToeBase, RightFoot, RightToeBase]`.
///
/// ARDY writes the left side first, which is the *reverse* of [`FEET`] and
/// of the joint order (right leg before left). See
/// [`crate::Take::contacts`] for where this was verified.
pub const CONTACT_COLUMNS: [usize; 4] = [25, 26, 21, 22];

/// Every right-side joint, in [`JOINTS`] order.
pub const RIGHT: [usize; 10] = [7, 8, 9, 10, 11, 12, 19, 20, 21, 22];

/// Every left-side joint, in [`JOINTS`] order.
pub const LEFT: [usize; 10] = [13, 14, 15, 16, 17, 18, 23, 24, 25, 26];

/// Joints the `exaggerate` knob scales — the arm set the retired
/// `tools/ardy_npz_to_glb.py` scaled, kept identical so the frozen
/// Blender-era fixtures still reproduce.
///
/// Arms only, deliberately: legs are left alone so foot contacts survive, and
/// amplified torso pitch reads as lurching rather than energy.
pub const ARM_JOINTS: [&str; 8] = [
    "RightShoulder",
    "RightArm",
    "RightForeArm",
    "RightHand",
    "LeftShoulder",
    "LeftArm",
    "LeftForeArm",
    "LeftHand",
];

/// Index of a joint by name.
#[must_use]
pub fn index_of(name: &str) -> Option<usize> {
    JOINTS.iter().position(|j| *j == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parents_are_consistent_and_acyclic() {
        assert_eq!(JOINTS.len(), PARENTS.len());
        assert!(PARENTS[ROOT].is_none(), "Hips must be the root");
        for (i, parent) in PARENTS.iter().enumerate().skip(1) {
            let p = parent.expect("only Hips may be rootless");
            assert!(p < i, "{} parents a later joint", JOINTS[i]);
        }
    }

    #[test]
    fn arm_joints_all_exist() {
        for name in ARM_JOINTS {
            assert!(index_of(name).is_some(), "{name} missing from cskel27");
        }
    }

    #[test]
    fn named_indices_name_what_they_say() {
        assert_eq!(JOINTS[ROOT], "Hips");
        assert_eq!(JOINTS[HEAD], "Head");
        assert_eq!(HANDS.map(|j| JOINTS[j]), ["RightHand", "LeftHand"]);
        assert_eq!(
            FEET.map(|j| JOINTS[j]),
            ["RightFoot", "RightToeBase", "LeftFoot", "LeftToeBase"]
        );
        assert_eq!(
            CONTACT_COLUMNS.map(|j| JOINTS[j]),
            ["LeftFoot", "LeftToeBase", "RightFoot", "RightToeBase"]
        );
        for j in RIGHT {
            assert!(JOINTS[j].starts_with("Right"), "{} is not right", JOINTS[j]);
        }
        for j in LEFT {
            assert!(JOINTS[j].starts_with("Left"), "{} is not left", JOINTS[j]);
        }
        let mut sided: Vec<usize> = RIGHT.into_iter().chain(LEFT).collect();
        sided.sort_unstable();
        let expected: Vec<usize> = (0..JOINT_COUNT)
            .filter(|j| JOINTS[*j].starts_with("Right") || JOINTS[*j].starts_with("Left"))
            .collect();
        assert_eq!(sided, expected, "every sided joint is in exactly one set");
    }
}
