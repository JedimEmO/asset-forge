//! Serialize an edited take into GLB bytes.
//!
//! The JSON side is built with `gltf-json` — the same types the reader
//! validates against — and the binary chunk is laid out by the crate-private `glb` module:
//! one buffer view per accessor, four-byte aligned, in emission order, which
//! here means the skin, then the constant anchor mesh, then the animation.

use std::collections::BTreeMap;

use glam::Quat;
use gltf::json;
use json::Index;
use json::validation::Checked::Valid;

use crate::glb::{Builder, default_node, f32_bytes};
use crate::rig::RigDef;
use crate::skeleton::{JOINT_COUNT, JOINTS};
use crate::{BakeError, Edit, Result, Take};

/// Bake `take` through `edit` into a self-contained `.glb`, posed on `rig`.
///
/// The animation is named `clip_name` — the name an engine binds by, so it must
/// be the sidecar recipe's `clip`, not the file stem. The output carries
/// exactly 28 channels: one rotation curve per cskel27 joint and one `Hips`
/// translation curve (see the crate docs for why the Blender path's 53
/// constant channels are deliberately absent).
///
/// # Errors
///
/// Refuses a take that edits down to fewer than two frames or carries a
/// non-positive frame rate; both would key a curve that cannot be sampled.
pub fn bake(take: &Take, edit: &Edit, rig: &RigDef, clip_name: &str) -> Result<Vec<u8>> {
    let edited = edit.apply(take);
    let frames = edited.frames();
    if frames < 2 {
        return Err(BakeError::TooShort { frames });
    }
    if edited.fps <= 0.0 {
        return Err(BakeError::BadFps(edited.fps));
    }

    let times: Vec<f32> = (0..frames).map(|i| i as f32 / edited.fps).collect();
    let rotations: Vec<Vec<Quat>> = (0..JOINT_COUNT)
        .map(|j| {
            let track = edited
                .rotations
                .iter()
                .map(|frame| rig.to_node_rotation(j, frame[j]));
            sign_continuous(track)
        })
        .collect();

    let mut doc = Builder::default();
    write(&mut doc, &times, &rotations, &edited.root, rig, clip_name);
    doc.finish()
}

/// Flip signs along a quaternion track so consecutive keys stay on the same
/// hemisphere. `q` and `-q` are the same rotation, but a linear sampler
/// interpolating across the sign boundary passes through garbage — the glTF
/// spec leaves continuity to the writer, and Blender's exporter does the
/// same thing.
fn sign_continuous(track: impl Iterator<Item = Quat>) -> Vec<Quat> {
    let mut out: Vec<Quat> = Vec::new();
    for q in track {
        let q = match out.last() {
            Some(prev) if prev.dot(q) < 0.0 => -q,
            _ => q,
        };
        out.push(q);
    }
    out
}

/// The `SkinAnchor` triangle: three vertices a centimetre across, fully
/// weighted to skin joint 0 (`Hips`). It exists because importers — Bevy's
/// glTF loader included — only assemble a skeleton for joints a skin
/// references; without it the clip would load as loose, untargetable nodes.
const ANCHOR_POSITIONS: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [0.01, 0.0, 0.0], [0.0, 0.0, -0.01]];
const ANCHOR_NORMALS: [[f32; 3]; 3] = [[0.0, 1.0, 0.0]; 3];

fn write(
    doc: &mut Builder,
    times: &[f32],
    rotations: &[Vec<Quat>],
    root_track: &[glam::Vec3],
    rig: &RigDef,
    clip_name: &str,
) {
    doc.root.asset = json::Asset {
        generator: Some(format!("forge_motion {}", env!("CARGO_PKG_VERSION"))),
        ..json::Asset::default()
    };

    // Nodes: bones 0..27 in ARDY joint order (parents precede children),
    // then the anchor, then the armature the scene points at.
    let mut children: Vec<Vec<Index<json::Node>>> = vec![Vec::new(); JOINT_COUNT];
    for (j, bone) in rig.bones.iter().enumerate().skip(1) {
        children[bone.parent.expect("only Hips is rootless")].push(Index::new(j as u32));
    }
    for (j, bone) in rig.bones.iter().enumerate() {
        doc.root.nodes.push(json::Node {
            name: Some(bone.name.to_owned()),
            rotation: Some(json::scene::UnitQuaternion(bone.rotation.to_array())),
            translation: Some(bone.translation.to_array()),
            children: (!children[j].is_empty()).then(|| std::mem::take(&mut children[j])),
            ..default_node()
        });
    }
    let anchor = doc.root.push(json::Node {
        name: Some(String::from("SkinAnchor")),
        mesh: Some(Index::new(0)),
        skin: Some(Index::new(0)),
        ..default_node()
    });
    let armature = doc.root.push(json::Node {
        name: Some(String::from("Armature")),
        children: Some(vec![Index::new(0), anchor]),
        ..default_node()
    });
    doc.root.scene = Some(doc.root.push(json::Scene {
        extensions: None,
        extras: <_>::default(),
        name: Some(String::from("Scene")),
        nodes: vec![armature],
    }));

    skin(doc, rig);
    anchor_mesh(doc);
    animation(doc, times, rotations, root_track, clip_name);
}

fn skin(doc: &mut Builder, rig: &RigDef) {
    let mut ibms = Vec::with_capacity(JOINT_COUNT * 16);
    for bone in &rig.bones {
        ibms.extend_from_slice(&bone.inverse_bind);
    }
    let view = doc.view(&f32_bytes(&ibms), None);
    let matrices = doc.accessor(view, json::accessor::Type::Mat4, JOINT_COUNT, None);
    doc.root.skins.push(json::Skin {
        inverse_bind_matrices: Some(matrices),
        joints: (0..JOINT_COUNT as u32).map(Index::new).collect(),
        name: Some(String::from("Armature")),
        skeleton: None,
        extensions: None,
        extras: <_>::default(),
    });
}

fn anchor_mesh(doc: &mut Builder) {
    let positions = {
        let view = doc.view(
            &f32_bytes(ANCHOR_POSITIONS.as_flattened()),
            Some(json::buffer::Target::ArrayBuffer),
        );
        // POSITION accessors must carry min/max per the glTF spec.
        let bounds = (
            json::Value::from(vec![0.0, 0.0, -0.01]),
            json::Value::from(vec![0.01, 0.0, 0.0]),
        );
        doc.accessor(view, json::accessor::Type::Vec3, 3, Some(bounds))
    };
    let normals = {
        let view = doc.view(
            &f32_bytes(ANCHOR_NORMALS.as_flattened()),
            Some(json::buffer::Target::ArrayBuffer),
        );
        doc.accessor(view, json::accessor::Type::Vec3, 3, None)
    };
    let joints = {
        let view = doc.view(&[0u8; 12], Some(json::buffer::Target::ArrayBuffer));
        doc.accessor_of(
            view,
            json::accessor::ComponentType::U8,
            json::accessor::Type::Vec4,
            3,
            None,
        )
    };
    let weights = {
        let data: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
        let view = doc.view(&f32_bytes(&data), Some(json::buffer::Target::ArrayBuffer));
        doc.accessor(view, json::accessor::Type::Vec4, 3, None)
    };
    let indices = {
        let data: Vec<u8> = [0u16, 1, 2].iter().flat_map(|i| i.to_le_bytes()).collect();
        let view = doc.view(&data, Some(json::buffer::Target::ElementArrayBuffer));
        doc.accessor_of(
            view,
            json::accessor::ComponentType::U16,
            json::accessor::Type::Scalar,
            3,
            None,
        )
    };

    let mut attributes = BTreeMap::new();
    attributes.insert(Valid(json::mesh::Semantic::Positions), positions);
    attributes.insert(Valid(json::mesh::Semantic::Normals), normals);
    attributes.insert(Valid(json::mesh::Semantic::Joints(0)), joints);
    attributes.insert(Valid(json::mesh::Semantic::Weights(0)), weights);
    doc.root.meshes.push(json::Mesh {
        name: Some(String::from("SkinAnchor")),
        primitives: vec![json::mesh::Primitive {
            attributes,
            indices: Some(indices),
            material: None,
            mode: Valid(json::mesh::Mode::Triangles),
            targets: None,
            extensions: None,
            extras: <_>::default(),
        }],
        weights: None,
        extensions: None,
        extras: <_>::default(),
    });
}

fn animation(
    doc: &mut Builder,
    times: &[f32],
    rotations: &[Vec<Quat>],
    root_track: &[glam::Vec3],
    clip_name: &str,
) {
    let input = {
        let view = doc.view(&f32_bytes(times), None);
        // Animation input accessors must carry min/max per the spec.
        let bounds = (
            json::Value::from(vec![times[0]]),
            json::Value::from(vec![times[times.len() - 1]]),
        );
        doc.accessor(
            view,
            json::accessor::Type::Scalar,
            times.len(),
            Some(bounds),
        )
    };

    let mut channels = Vec::with_capacity(JOINT_COUNT + 1);
    let mut samplers = Vec::with_capacity(JOINT_COUNT + 1);
    let mut push = |doc: &mut Builder,
                    node: usize,
                    path: json::animation::Property,
                    data: Vec<u8>,
                    type_: json::accessor::Type,
                    count: usize| {
        let view = doc.view(&data, None);
        let output = doc.accessor(view, type_, count, None);
        samplers.push(json::animation::Sampler {
            input,
            interpolation: Valid(json::animation::Interpolation::Linear),
            output,
            extensions: None,
            extras: <_>::default(),
        });
        channels.push(json::animation::Channel {
            sampler: Index::new(samplers.len() as u32 - 1),
            target: json::animation::Target {
                node: Index::new(node as u32),
                path: Valid(path),
                extensions: None,
                extras: <_>::default(),
            },
            extensions: None,
            extras: <_>::default(),
        });
    };

    for (j, track) in rotations.iter().enumerate() {
        let data: Vec<f32> = track.iter().flat_map(|q| q.to_array()).collect();
        push(
            doc,
            j,
            json::animation::Property::Rotation,
            f32_bytes(&data),
            json::accessor::Type::Vec4,
            track.len(),
        );
    }
    let hips = JOINTS.iter().position(|j| *j == "Hips").expect("cskel27");
    let data: Vec<f32> = root_track.iter().flat_map(glam::Vec3::to_array).collect();
    push(
        doc,
        hips,
        json::animation::Property::Translation,
        f32_bytes(&data),
        json::accessor::Type::Vec3,
        root_track.len(),
    );

    doc.root.animations.push(json::Animation {
        channels,
        samplers,
        name: Some(clip_name.to_owned()),
        extensions: None,
        extras: <_>::default(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_flips_are_removed_from_a_track() {
        let q = Quat::from_rotation_y(0.3);
        let track = sign_continuous([q, -q, q, -q].into_iter());
        assert!(track.windows(2).all(|w| w[0].dot(w[1]) > 0.0));
    }
}
