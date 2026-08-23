//! The fixture mannequin holds to the profile it is built from: zero drift
//! against the contract, measures as a body, the same bytes every time, and
//! nothing outside the file.

use std::path::{Path, PathBuf};

use forge_rig::{
    RigProfile, check_drift, derive_from_glb,
    fixture::{ARMATURE_NODE, BODY_NODE, GENERATOR, mannequin_glb, write_mannequin},
    measure::measure_glb,
};

fn profile_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/humanoid")
}

fn profile() -> RigProfile {
    RigProfile::load(&profile_dir()).expect("the shipped profile loads")
}

fn mannequin() -> Vec<u8> {
    mannequin_glb(&profile().contract).expect("the mannequin builds")
}

fn is_identity(matrix: [[f32; 4]; 4]) -> bool {
    glam::Mat4::from_cols_array_2d(&matrix).abs_diff_eq(glam::Mat4::IDENTITY, 0.0)
}

// ------------------------------------------------------------ (1) zero drift

#[test]
fn the_mannequin_derives_back_to_the_contract_without_drift() {
    let profile = profile();
    let derived = derive_from_glb(&mannequin()).expect("the mannequin is a rig");
    assert_eq!(derived.armature, ARMATURE_NODE);
    assert_eq!(derived.bones.len(), profile.contract.bones.len());
    let drift = check_drift(&profile.contract, &derived.bones);
    assert!(
        drift.is_empty(),
        "the mannequin drifted from contract.json:\n{}",
        drift.join("\n")
    );
    // Exact, not merely within tolerance: the node transforms are the
    // contract's own floats.
    for (found, spec) in derived.bones.iter().zip(&profile.contract.bones) {
        assert_eq!(
            found.rest_translation.map(f32::to_bits),
            spec.rest_translation.map(f32::to_bits),
            "{}: rest translation is not bit-identical",
            spec.name
        );
        assert_eq!(
            found.rest_rotation.map(f32::to_bits),
            spec.rest_rotation.map(f32::to_bits),
            "{}: rest rotation is not bit-identical",
            spec.name
        );
    }
}

// -------------------------------------------------------- (2) measures as a body

#[test]
fn the_mannequin_measures_as_a_body_standing_on_the_ground() {
    let profile = profile();
    let contract = &profile.contract;
    let bytes = mannequin();
    let measured = measure_glb(&bytes).expect("measurable");
    assert_eq!(measured.bones_skinned, 55);
    assert_eq!(measured.joint_names.len(), 55);
    assert!(
        forge_rig::missing_joints(contract, &measured.joint_names).is_empty(),
        "every contract bone is a joint of the skin"
    );
    assert_eq!(measured.mesh_nodes, [BODY_NODE]);
    assert_eq!(measured.generator.as_deref(), Some(GENERATOR));
    assert!(
        measured.lowest_y.abs() <= 0.05,
        "the feet stand on the ground: lowest y = {}",
        measured.lowest_y
    );
    assert!(
        measured.lowest_y >= 0.0,
        "nothing pokes through the floor: lowest y = {}",
        measured.lowest_y
    );
    assert!(
        measured.lowest_y.abs() <= contract.foot_tolerance_m,
        "within the profile's own foot tolerance"
    );
    let stature = measured.bounds[1][1] - measured.lowest_y;
    assert!(
        (stature - contract.stature_m.reference).abs() < 0.1,
        "stature {stature} m against the reference {} m",
        contract.stature_m.reference
    );
    assert!(
        stature > contract.stature_m.min && stature < contract.stature_m.max,
        "inside the band the profile gates on"
    );
    // One capsule per driven bone: 8 sides, 2 rings, 2 apexes.
    assert_eq!(measured.vertices, 27 * 18);
    assert_eq!(measured.triangles, 27 * 32);

    // Every vertex is weighted wholly to one bone, and that bone is driven.
    let (document, blob) = forge_rig::measure::open_glb(&bytes).expect("opens");
    let meshes: Vec<_> = document.meshes().collect();
    assert_eq!(meshes.len(), 1, "one mesh");
    let skin = document.skins().next().expect("one skin");
    let joint_names: Vec<&str> = skin.joints().map(|j| j.name().unwrap_or("")).collect();
    assert_eq!(
        joint_names,
        contract
            .bones
            .iter()
            .map(|b| b.name.as_str())
            .collect::<Vec<_>>(),
        "the skin lists the bones in contract order"
    );
    let mut weighted = 0usize;
    for primitive in meshes[0].primitives() {
        let reader = primitive.reader(|_| Some(blob.as_slice()));
        let joints: Vec<[u16; 4]> = reader
            .read_joints(0)
            .expect("JOINTS_0")
            .into_u16()
            .collect();
        let weights: Vec<[f32; 4]> = reader
            .read_weights(0)
            .expect("WEIGHTS_0")
            .into_f32()
            .collect();
        assert_eq!(joints.len(), weights.len());
        for (joint, weight) in joints.iter().zip(&weights) {
            let sum: f32 = weight.iter().sum();
            assert!((sum - 1.0).abs() < 1e-6, "weights sum to one: {weight:?}");
            assert_eq!(
                weight[0].to_bits(),
                1.0f32.to_bits(),
                "rigid: the whole weight on one bone, {weight:?}"
            );
            let bone = &contract.bones[joint[0] as usize];
            assert!(
                bone.driven,
                "{} is not driven but carries a vertex",
                bone.name
            );
            weighted += 1;
        }
    }
    assert_eq!(weighted as u32, measured.vertices, "every vertex weighted");

    // Every driven bone carries geometry.
    let mut carried = vec![false; contract.bones.len()];
    for primitive in meshes[0].primitives() {
        let reader = primitive.reader(|_| Some(blob.as_slice()));
        for joint in reader.read_joints(0).expect("JOINTS_0").into_u16() {
            carried[joint[0] as usize] = true;
        }
    }
    for (index, bone) in contract.driven() {
        assert!(carried[index], "{} has no capsule", bone.name);
    }
}

#[test]
fn the_mannequin_has_the_rig_artifacts_shape() {
    let bytes = mannequin();
    let (document, _) = forge_rig::measure::open_glb(&bytes).expect("opens");
    // One scene, one root, the armature at identity with the root bone and
    // the body beneath it; the body at identity.
    assert_eq!(document.scenes().count(), 1);
    let scene = document.scenes().next().expect("scene");
    let roots: Vec<_> = scene.nodes().collect();
    assert_eq!(roots.len(), 1);
    let armature = &roots[0];
    assert_eq!(armature.name(), Some(ARMATURE_NODE));
    assert!(
        is_identity(armature.transform().matrix()),
        "the armature is at identity"
    );
    let children: Vec<&str> = armature
        .children()
        .map(|c| c.name().unwrap_or(""))
        .collect();
    assert_eq!(children, ["Hips", BODY_NODE]);
    let body = armature.children().nth(1).expect("body");
    assert!(body.mesh().is_some());
    assert!(body.skin().is_some());
    assert!(
        is_identity(body.transform().matrix()),
        "the body is at identity"
    );
    // Node order is the contract order: bone i is node i.
    let contract = profile().contract;
    for (i, bone) in contract.bones.iter().enumerate() {
        let node = document.nodes().nth(i).expect("node");
        assert_eq!(node.name(), Some(bone.name.as_str()), "node {i}");
    }
    // No images, no extensions, no textures.
    assert_eq!(document.images().count(), 0);
    assert_eq!(document.textures().count(), 0);
    assert!(document.extensions_used().next().is_none());
    assert!(document.extensions_required().next().is_none());
    let material = document.materials().next().expect("one material");
    assert!(material.double_sided());
    let pbr = material.pbr_metallic_roughness();
    assert!(pbr.metallic_factor().abs() < 1e-6);
    assert!((pbr.roughness_factor() - 0.9).abs() < 1e-6);
}

// ------------------------------------------------------- (3) deterministic

#[test]
fn two_builds_are_the_same_bytes() {
    let first = mannequin();
    let second = mannequin();
    assert!(first == second, "the mannequin is not byte-deterministic");
    assert!(!first.is_empty());

    let out = std::env::temp_dir().join(format!(
        "forge_rig_mannequin_{}/mannequin.glb",
        std::process::id()
    ));
    write_mannequin(&profile(), &out).expect("writes, creating the directory");
    let written = std::fs::read(&out).expect("written");
    let _ = std::fs::remove_file(&out);
    let _ = out.parent().map(std::fs::remove_dir);
    assert!(written == first, "the written file is the built bytes");
}

// ------------------------------------------------------ (4) self-contained

#[test]
fn the_glb_is_one_json_chunk_and_one_bin_chunk_with_no_uri() {
    let bytes = mannequin();
    assert_eq!(&bytes[..4], b"glTF");
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().expect("4")), 2);
    let total = u32::from_le_bytes(bytes[8..12].try_into().expect("4")) as usize;
    assert_eq!(total, bytes.len());
    assert!(bytes.len().is_multiple_of(4));

    let mut chunks = Vec::new();
    let mut at = 12;
    while at < bytes.len() {
        let length = u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4")) as usize;
        let tag = &bytes[at + 4..at + 8];
        chunks.push((tag.to_vec(), &bytes[at + 8..at + 8 + length]));
        at += 8 + length;
    }
    assert_eq!(chunks.len(), 2, "one JSON chunk and one BIN chunk");
    assert_eq!(chunks[0].0, b"JSON");
    assert_eq!(chunks[1].0, b"BIN\0");

    let json: serde_json::Value = serde_json::from_slice(chunks[0].1).expect("valid JSON");
    assert!(
        !String::from_utf8_lossy(chunks[0].1).contains("\"uri\""),
        "nothing references a sibling file"
    );
    assert_eq!(json["asset"]["generator"], GENERATOR);
    assert_eq!(json["asset"]["version"], "2.0");
    assert!(json.get("extensionsUsed").is_none());
    assert!(json.get("images").is_none());
    let buffers = json["buffers"].as_array().expect("buffers");
    assert_eq!(buffers.len(), 1);
    assert_eq!(
        buffers[0]["byteLength"].as_u64().expect("byteLength") as usize,
        chunks[1].1.len(),
        "the BIN chunk is exactly the one buffer (already four-byte aligned)"
    );
    // And the reader agrees it is self-contained.
    let (document, _) = forge_rig::measure::open_glb(&bytes).expect("opens");
    for buffer in document.buffers() {
        assert!(matches!(buffer.source(), gltf::buffer::Source::Bin));
    }
}
