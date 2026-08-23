//! The shipped humanoid profile holds together: the contract matches its
//! artifact, the driven bones are the motion layout, every socket hangs off
//! a bone, and re-exporting the contract reproduces the committed bytes.

use std::path::{Path, PathBuf};

use forge_rig::{
    CONTRACT_FILE, Contract, MotionSkeleton, RigProfile, Socket, check_drift, derive_from_glb,
    export, sha256_file,
};
use glam::Vec3;

fn profile_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/humanoid")
}

fn profile() -> RigProfile {
    RigProfile::load(&profile_dir()).expect("the shipped profile loads")
}

fn read(relative: &str) -> Vec<u8> {
    let path = profile_dir().join(relative);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

// ---------------------------------------------------------------- (a) drift

#[test]
fn the_contract_matches_the_rig_artifact() {
    let profile = profile();
    let derived = derive_from_glb(&read("rig.glb")).expect("rig.glb is a rig");
    assert_eq!(
        derived.armature, "Armature",
        "the scene root is the first segment of every bone path"
    );
    let drift = check_drift(&profile.contract, &derived.bones);
    assert!(
        drift.is_empty(),
        "contract.json drifted from rig.glb:\n{}",
        drift.join("\n")
    );
}

#[test]
fn the_contract_hashes_name_the_shipped_artifacts() {
    let profile = profile();
    let check = profile.check_sources().expect("artifacts readable");
    assert!(
        check.glb_matches,
        "rig.glb is not the file the contract was derived from"
    );
    assert_eq!(
        check.blend_matches,
        Some(true),
        "rig.blend changed under the contract"
    );
    assert_eq!(
        sha256_file(&profile.glb_path()).expect("rig.glb"),
        profile.contract.sources.glb_sha256
    );
}

// ----------------------------------------------------- (b) shape and layout

#[test]
fn fifty_five_bones_and_hips_is_the_only_root() {
    let profile = profile();
    let contract = &profile.contract;
    assert_eq!(
        contract.bones.len(),
        55,
        "27 driven joints + 28 finger leaves"
    );
    assert_eq!(contract.driven().count(), 27);
    assert_eq!(contract.root, "Hips");
    let roots: Vec<&str> = contract
        .bones
        .iter()
        .filter(|bone| bone.parent.is_none())
        .map(|bone| bone.name.as_str())
        .collect();
    assert_eq!(roots, ["Hips"], "only Hips may be rootless");
    assert_eq!(contract.root_index(), contract.find("Hips").expect("Hips"));
    assert_eq!(contract.driven_layout, "cskel27");
}

#[test]
fn every_driven_bone_is_a_motion_joint_and_the_chains_collapse_to_its_parents() {
    let profile = profile();
    let contract = &profile.contract;
    let motion = &profile.motion;
    assert_eq!(motion.name, "cskel27");
    assert_eq!(motion.len(), 27);

    // Both directions: no driven bone the layout lacks, no layout joint the
    // contract does not drive.
    for (index, bone) in contract.driven() {
        let joint = motion
            .index_of(&bone.name)
            .unwrap_or_else(|| panic!("driven bone {} is not a cskel27 joint", bone.name));
        let layout_parent = motion.parents[joint].map(|p| motion.joints[p].as_str());
        let contract_parent = contract
            .driven_parent(index)
            .map(|p| contract.bones[p].name.as_str());
        assert_eq!(
            contract_parent, layout_parent,
            "{}: the driven chain collapses to {contract_parent:?}, cskel27 says {layout_parent:?}",
            bone.name
        );
    }
    for joint in &motion.joints {
        let index = contract
            .find(joint)
            .unwrap_or_else(|| panic!("cskel27 joint {joint} is not a contract bone"));
        assert!(
            contract.bones[index].driven,
            "{joint} is a joint but not driven"
        );
    }

    // The finger leaves are exactly the rest, and nothing driven hangs under
    // one of them — that would be the inserted-bone failure.
    for (index, bone) in contract.bones.iter().enumerate() {
        if bone.driven {
            continue;
        }
        assert!(
            bone.name.contains("Hand"),
            "{} is undriven but is not a finger leaf",
            bone.name
        );
        let hand = contract
            .driven_parent(index)
            .unwrap_or_else(|| panic!("finger leaf {} hangs off nothing driven", bone.name));
        assert!(
            contract.bones[hand].name.ends_with("Hand")
                || contract.bones[hand].name.ends_with("HandThumb1"),
            "finger leaf {} hangs off {}, not a hand",
            bone.name,
            contract.bones[hand].name
        );
    }
    for (index, bone) in contract.driven() {
        let mut current = contract.bones[index].parent;
        while let Some(p) = current {
            assert!(
                contract.bones[p].driven,
                "driven bone {} sits under undriven {} — a stranger between contract bones",
                bone.name, contract.bones[p].name
            );
            current = contract.bones[p].parent;
        }
    }
}

#[test]
fn the_motion_layout_reads_as_the_review_tools_expect() {
    let motion = profile().motion;
    let names = |indices: &[usize]| -> Vec<&str> {
        indices.iter().map(|&i| motion.joints[i].as_str()).collect()
    };
    assert_eq!(motion.joints[motion.root], "Hips");
    assert_eq!(motion.joints[motion.head], "Head");
    assert_eq!(names(&motion.hands), ["RightHand", "LeftHand"]);
    assert_eq!(
        names(&motion.feet),
        ["RightFoot", "RightToeBase", "LeftFoot", "LeftToeBase"]
    );
    // Contact columns are left side first — the reverse of joint order.
    assert_eq!(
        names(&motion.contact_columns),
        ["LeftFoot", "LeftToeBase", "RightFoot", "RightToeBase"]
    );
    assert_eq!(motion.right.len(), 10);
    assert_eq!(motion.left.len(), 10);
    for &i in &motion.right {
        assert!(
            motion.joints[i].starts_with("Right"),
            "{}",
            motion.joints[i]
        );
    }
    for &i in &motion.left {
        assert!(motion.joints[i].starts_with("Left"), "{}", motion.joints[i]);
    }
    assert_eq!(motion.arm_joints.len(), 8);
    for (i, parent) in motion.parents.iter().enumerate().skip(1) {
        let p = parent.expect("only Hips is rootless");
        assert!(p < i, "{} parents a later joint", motion.joints[i]);
    }
}

// ------------------------------------------------- (c) find / expected_depth

#[test]
fn names_are_unique_and_find_returns_the_first_index() {
    let contract = profile().contract;
    for (i, bone) in contract.bones.iter().enumerate() {
        assert_eq!(
            contract.find(&bone.name),
            Some(i),
            "{} not found first at {i}",
            bone.name
        );
    }
    assert_eq!(contract.find("hips"), None, "names are exact");
    assert_eq!(contract.find("Root"), None);
}

#[test]
fn depths_walk_back_to_hips() {
    let contract = profile().contract;
    let hips = contract.find("Hips").expect("Hips");
    assert_eq!(contract.expected_depth(hips), 2);
    let toe = contract.find("LeftToeBase").expect("LeftToeBase");
    // Hips > LeftUpLeg > LeftLeg > LeftFoot > LeftToeBase, under Armature.
    assert_eq!(contract.expected_depth(toe), 6);
    let thumb3 = contract.find("RightHandThumb3").expect("RightHandThumb3");
    // Hips > Spine > Spine1 > Spine2 > Spine3 > RightShoulder > RightArm >
    // RightForeArm > RightHand > Thumb1 > Thumb2 > Thumb3.
    assert_eq!(contract.expected_depth(thumb3), 13);
}

#[test]
fn rest_rotations_are_unit_quaternions() {
    for bone in &profile().contract.bones {
        let [x, y, z, w] = bone.rest_rotation;
        let norm = z.mul_add(z, x.mul_add(x, y * y)) + w * w;
        assert!((norm - 1.0).abs() < 1e-3, "{} norm {norm}", bone.name);
    }
}

#[test]
fn the_rest_pose_faces_plus_z_and_right_is_minus_x() {
    let contract = profile().contract;
    assert_eq!(contract.front, "+Z");
    let world = |name: &str| contract.rest_world(contract.find(name).expect(name)).0;
    let toe = world("RightToeBase");
    let ankle = world("RightFoot");
    assert!(
        toe.z > ankle.z + 0.1,
        "the toes point +Z: toe z = {}, ankle z = {}",
        toe.z,
        ankle.z
    );
    assert!(world("RightUpLeg").x < -0.05, "right is -X");
    assert!(world("LeftUpLeg").x > 0.05, "left is +X");
    // The stature the profile was measured for: the head joint sits at 1.68 m
    // and the crown 0.12 m above it.
    let head = world("Head");
    assert!((head.y - 1.68).abs() < 0.02, "head joint at y = {}", head.y);
    assert!((contract.stature_m.reference - 1.8).abs() < 1e-6);
}

// --------------------------------------------------------------- (d) sockets

#[test]
fn every_socket_hangs_off_a_contract_bone() {
    let profile = profile();
    for socket in &profile.sockets.sockets {
        assert!(
            profile.contract.find(&socket.bone).is_some(),
            "socket '{}' names '{}', which the contract does not have",
            socket.name,
            socket.bone
        );
    }
    profile
        .sockets
        .validate(&profile.contract)
        .expect("validates");
    assert_eq!(profile.sockets.authoring_frame.long_axis, "+Y");
    assert_eq!(profile.sockets.authoring_frame.front, "-Z");
    assert_eq!(profile.sockets.authoring_frame.origin, "grip");
}

#[test]
fn socket_names_are_the_vocabulary_and_a_bone_name_is_not_one() {
    let sockets = profile().sockets;
    assert_eq!(
        sockets.names(),
        ["hand_r", "hand_l", "back", "hip_l", "head"]
    );
    assert_eq!(
        sockets.find("hand_r").map(|s| s.bone.as_str()),
        Some("RightHand")
    );
    assert_eq!(
        sockets.find("hand_l").map(|s| s.bone.as_str()),
        Some("LeftHand")
    );
    assert!(sockets.find("hand_x").is_none(), "a typo must not resolve");
    assert!(
        sockets.find("RightHand").is_none(),
        "the bone's own name is not a socket name"
    );
}

fn socket_world(profile: &RigProfile, socket: &Socket) -> Vec3 {
    profile.sockets.rest_world(socket, &profile.contract).0
}

fn socket_axis(profile: &RigProfile, socket: &Socket, prop_axis: Vec3) -> Vec3 {
    profile.sockets.rest_world(socket, &profile.contract).1 * prop_axis
}

#[test]
fn sockets_land_on_the_body_in_the_rest_pose() {
    // Bands, not points: the last centimetres are a visual tune. Each band is
    // a claim about anatomy — a hand out to the side at chest height, a hat at
    // the crown — so a sign flip or a swapped bone cannot pass, while a 5 cm
    // fit adjustment can.
    let profile = profile();
    let expected = [
        ("hand_r", [-0.85, 1.38], [-0.65, 1.56]),
        ("hand_l", [0.65, 1.38], [0.85, 1.56]),
        ("back", [-0.15, 1.30], [0.05, 1.50]),
        ("hip_l", [0.05, 0.85], [0.25, 1.00]),
        ("head", [-0.05, 1.74], [0.05, 1.88]),
    ];
    for (name, min, max) in expected {
        let socket = profile
            .sockets
            .find(name)
            .expect("the table ships this socket");
        let world = socket_world(&profile, socket);
        assert!(
            world.x > min[0] && world.x < max[0],
            "socket '{name}' x = {} outside [{}, {}]",
            world.x,
            min[0],
            max[0]
        );
        assert!(
            world.y > min[1] && world.y < max[1],
            "socket '{name}' y = {} outside [{}, {}]",
            world.y,
            min[1],
            max[1]
        );
    }

    let right = socket_world(&profile, profile.sockets.find("hand_r").expect("hand_r"));
    let left = socket_world(&profile, profile.sockets.find("hand_l").expect("hand_l"));
    assert!(right.x < -0.4, "hand_r is on the -X side: x = {}", right.x);
    assert!(left.x > 0.4, "hand_l is on the +X side: x = {}", left.x);
    assert!((right.x + left.x).abs() < 0.02, "the grips mirror in x");
    assert!(
        (right.y - left.y).abs() < 0.005,
        "the grips sit at one height"
    );
    assert!(
        (right.z - left.z).abs() < 0.005,
        "the grips sit at one depth"
    );
    assert!(
        (right.x - left.x).abs() > 1.0,
        "a body's width apart, not the same point"
    );

    // The back stow sits behind the spine: the toes are the front, and the
    // stow has to be on the other side of the spine from them.
    let contract = &profile.contract;
    let spine = contract
        .rest_world(contract.find("Spine3").expect("Spine3"))
        .0;
    let back = socket_world(&profile, profile.sockets.find("back").expect("back"));
    assert!(
        back.z < spine.z - 0.05,
        "the back stow must sit behind Spine3 (z = {}), not on the sternum: z = {}",
        spine.z,
        back.z
    );
    assert!(
        (spine.z - back.z) < 0.15,
        "the stow rides the back, it does not trail behind it"
    );
}

#[test]
fn the_grips_send_the_blade_out_along_the_arms() {
    let profile = profile();
    let blade_r = socket_axis(
        &profile,
        profile.sockets.find("hand_r").expect("hand_r"),
        Vec3::Y,
    );
    let blade_l = socket_axis(
        &profile,
        profile.sockets.find("hand_l").expect("hand_l"),
        Vec3::Y,
    );
    assert!(
        blade_r.x < -0.8,
        "hand_r sends the blade out the right arm (-X): {blade_r:?}"
    );
    assert!(
        blade_l.x > 0.8,
        "hand_l sends the blade out the left arm (+X): {blade_l:?}"
    );
    assert!(
        (blade_r.x + blade_l.x).abs() < 0.01
            && (blade_r.y - blade_l.y).abs() < 0.01
            && (blade_r.z - blade_l.z).abs() < 0.01,
        "the two blades mirror across the midline: {blade_r:?} vs {blade_l:?}"
    );
}

#[test]
fn the_back_stow_lies_flat_against_the_spine_with_its_tip_down() {
    let profile = profile();
    let contract = &profile.contract;
    let back = profile.sockets.find("back").expect("back");
    let (_, spine_rotation) = contract.rest_world(contract.find(&back.bone).expect("Spine3"));
    // Out of the back: the bone's own -Z, since the rest pose faces +Z.
    let outward = spine_rotation * Vec3::NEG_Z;

    let blade = socket_axis(&profile, back, Vec3::Y);
    assert!(
        blade.y < -0.8,
        "the blade hangs down the back, tip below the hilt: {blade:?}"
    );
    assert!(
        blade.x > 0.2,
        "and across toward the character's left (+X): {blade:?}"
    );
    assert!(
        blade.dot(outward).abs() < 0.01,
        "the blade lies in the plane of the back: {blade:?} against {outward:?}"
    );
    let flat = socket_axis(&profile, back, Vec3::X);
    assert!(
        flat.dot(outward) > 0.99,
        "the flat's normal points out of the back, not into it: {flat:?} against {outward:?}"
    );
}

// ------------------------------------------------------ (e) byte-for-byte

#[test]
fn re_exporting_the_contract_reproduces_the_committed_bytes() {
    let dir = profile_dir();
    let committed = read(CONTRACT_FILE);
    let contract = Contract::from_json(&committed, &dir.join(CONTRACT_FILE)).expect("parses");
    let motion = MotionSkeleton::load(&dir.join("motion_skeleton.json")).expect("loads");

    // The scalars the example writes, pinned here too: a change in either
    // place without the other is a diff in contract.json.
    assert_eq!(contract.name, "humanoid");
    assert_eq!(contract.version, 1);
    assert_eq!(contract.reference_clip, "walk");
    assert!((contract.stature_m.min - 1.4).abs() < 1e-6);
    assert!((contract.stature_m.max - 2.2).abs() < 1e-6);
    assert!((contract.foot_tolerance_m - 0.05).abs() < 1e-6);
    assert!((contract.rest_rotation_tolerance - 1e-3).abs() < 1e-9);
    assert_eq!(contract.sources.blend.as_deref(), Some("rig.blend"));

    let out = std::env::temp_dir().join(format!(
        "forge_rig_contract_{}_{}.json",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let options = export::Options::from_contract(&contract);
    let rebuilt = export::write(&dir, &motion, &options, &out).expect("rebuilds");
    let written = std::fs::read(&out).expect("written");
    let _ = std::fs::remove_file(&out);

    assert_eq!(rebuilt, contract, "the rebuilt contract differs as data");
    assert!(
        written == committed,
        "re-exporting contract.json changed its bytes — the rig or the writer's formatting moved"
    );
    assert!(written.ends_with(b"}\n"), "one trailing newline");
}
