//! The fixture mannequin: a rigid-weighted capsule figure skinned to a
//! contract, written as a self-contained glb in pure Rust.
//!
//! Every test that needs "a body on the profile" — ingest, clip binding, a
//! contact sheet — used to borrow the sample body from the library, which
//! made the sample a test dependency: a re-lift of the hero changed the
//! goldens, and a fresh clone without the sample could not run the suite.
//! This module is the replacement. It takes a [`Contract`] and hands back a
//! figure built from nothing but that contract: one node per bone with the
//! rest transform copied exactly, one skin, and one mesh of low-poly capsules,
//! one per driven bone, each vertex weighted wholly to its bone. The file is
//! not a character anyone would ship; it is the smallest thing that binds
//! every clip, stands with its feet on the ground, and measures as a body.
//!
//! Two properties are load-bearing:
//!
//! - **Zero drift.** [`crate::derive_from_glb`] on the mannequin reproduces
//!   the contract's bone table position for position, because the node order
//!   is the contract order, the armature root sits above the root bone the way
//!   the rig artifact has it, and the rest transforms are the contract's own
//!   `f32` values printed at shortest round-trip precision.
//! - **Byte determinism.** No timestamps, a fixed generator string, one buffer
//!   view per accessor appended in call order (so the emission order is the
//!   layout), and no hash-ordered collection anywhere on the path. Two calls
//!   are two identical byte strings, which is what lets a golden pin the file.
//!
//! The glb container is written by hand — 12-byte header, a JSON chunk padded
//! with spaces, a BIN chunk padded with zeros, the total length in the
//! header — rather than through a glTF writer, so this crate reads glTF and
//! writes only this one file, and the `gltf` dependency stays a reader.

use std::path::Path;

use glam::{Mat4, Quat, Vec3};
use serde::Serialize;

use crate::{Contract, Result, RigError, RigProfile};

/// The `asset.generator` string the mannequin declares. Fixed: it is part of
/// the bytes a golden pins, and a measured file reports it back.
pub const GENERATOR: &str = "forge_rig fixture 1";

/// The scene node above the root bone, named as the rig artifact names it.
/// It is the first segment of every bone's name path, so a clip bound on the
/// rig binds on the mannequin.
pub const ARMATURE_NODE: &str = "Armature";

/// The mesh-bearing node's name — the convention a body export follows.
pub const BODY_NODE: &str = "Body";

/// How far a capsule runs past a leaf bone's head along the bone's local +Y,
/// metres: a head, a toe, a hand with nothing driven beneath it.
pub const STUB_LENGTH_M: f32 = 0.08;

/// Sides per capsule ring. Eight is enough to read as a limb at sheet size
/// and keeps the whole figure under a thousand vertices.
const SIDES: usize = 8;

/// Radius of a capsule from its length: thin enough that a foot does not sink
/// through the floor, thick enough that a torso segment reads as a torso.
fn radius_for(length: f32) -> f32 {
    0.15f32.mul_add(length, 0.025).min(0.075)
}

/// Write the mannequin for `profile`'s contract to `out`, creating the parent
/// directory if it is missing.
///
/// # Errors
///
/// As [`mannequin_glb`], plus the file or its directory being unwritable.
pub fn write_mannequin(profile: &RigProfile, out: &Path) -> Result<()> {
    let bytes = mannequin_glb(&profile.contract)?;
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|source| RigError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(out, bytes).map_err(|source| RigError::Io {
        path: out.to_path_buf(),
        source,
    })
}

/// Build the mannequin for `contract` as glb bytes.
///
/// The node array is the contract's bones in contract order, then the
/// armature root at identity with the root bone and the mesh node as its
/// children, then the mesh node at identity. The skin lists every bone in the
/// same order with inverse bind matrices taken from the rest world transforms;
/// the mesh is one capsule per driven bone, spanning the bone's head to the
/// head of the child that best continues the bone's local +Y, or a
/// [`STUB_LENGTH_M`] stub along that axis when nothing hangs beneath it.
///
/// # Errors
///
/// The contract fails [`Contract::validate`], or the JSON could not be
/// serialised (only on a non-finite float, which validation refuses first).
pub fn mannequin_glb(contract: &Contract) -> Result<Vec<u8>> {
    contract.validate()?;

    let world: Vec<(Vec3, Quat)> = (0..contract.bones.len())
        .map(|i| contract.rest_world(i))
        .collect();
    let geometry = build_geometry(contract, &world);

    let mut builder = Builder::default();

    // Vertex attributes, in the order the primitive names them.
    let (position_min, position_max) = geometry.position_bounds();
    let position_view = builder.view(&f32_bytes(&geometry.positions), Some(ARRAY_BUFFER));
    let positions = builder.accessor(
        position_view,
        FLOAT,
        "VEC3",
        geometry.positions.len() / 3,
        Some((position_min.to_vec(), position_max.to_vec())),
    );
    let normal_view = builder.view(&f32_bytes(&geometry.normals), Some(ARRAY_BUFFER));
    let normals = builder.accessor(normal_view, FLOAT, "VEC3", geometry.normals.len() / 3, None);
    let joint_view = builder.view(&u16_bytes(&geometry.joints), Some(ARRAY_BUFFER));
    let joints = builder.accessor(
        joint_view,
        UNSIGNED_SHORT,
        "VEC4",
        geometry.joints.len() / 4,
        None,
    );
    let weight_view = builder.view(&f32_bytes(&geometry.weights), Some(ARRAY_BUFFER));
    let weights = builder.accessor(weight_view, FLOAT, "VEC4", geometry.weights.len() / 4, None);
    let index_view = builder.view(&u32_bytes(&geometry.indices), Some(ELEMENT_ARRAY_BUFFER));
    let indices = builder.accessor(
        index_view,
        UNSIGNED_INT,
        "SCALAR",
        geometry.indices.len(),
        None,
    );

    // Inverse bind matrices: the inverse of each bone's rest world transform,
    // column-major. (T·R)⁻¹ = R⁻¹·T⁻¹ is formed directly rather than by a
    // general matrix inverse, so the numbers are the same on every target.
    let mut ibm = Vec::with_capacity(world.len() * 16);
    for (translation, rotation) in &world {
        let inverse_rotation = rotation.inverse();
        let matrix =
            Mat4::from_rotation_translation(inverse_rotation, -(inverse_rotation * *translation));
        ibm.extend_from_slice(&matrix.to_cols_array());
    }
    let ibm_view = builder.view(&f32_bytes(&ibm), None);
    let inverse_bind_matrices = builder.accessor(ibm_view, FLOAT, "MAT4", world.len(), None);

    // Nodes: bones in contract order, then the armature, then the body.
    let bone_count = contract.bones.len();
    let armature_index = bone_count;
    let body_index = bone_count + 1;
    let mut nodes: Vec<Node> = Vec::with_capacity(bone_count + 2);
    for (i, bone) in contract.bones.iter().enumerate() {
        let children: Vec<usize> = contract
            .bones
            .iter()
            .enumerate()
            .filter(|(_, child)| child.parent == Some(i))
            .map(|(j, _)| j)
            .collect();
        nodes.push(Node {
            name: bone.name.clone(),
            children: (!children.is_empty()).then_some(children),
            translation: Some(bone.rest_translation),
            rotation: Some(bone.rest_rotation),
            mesh: None,
            skin: None,
        });
    }
    nodes.push(Node {
        name: String::from(ARMATURE_NODE),
        children: Some(vec![contract.root_index(), body_index]),
        translation: None,
        rotation: None,
        mesh: None,
        skin: None,
    });
    nodes.push(Node {
        name: String::from(BODY_NODE),
        children: None,
        translation: None,
        rotation: None,
        mesh: Some(0),
        skin: Some(0),
    });

    let root = Root {
        asset: Asset {
            generator: GENERATOR,
            version: "2.0",
        },
        scene: 0,
        scenes: vec![Scene {
            name: "Scene",
            nodes: vec![armature_index],
        }],
        nodes,
        meshes: vec![Mesh {
            name: BODY_NODE,
            primitives: vec![Primitive {
                attributes: Attributes {
                    position: positions,
                    normal: normals,
                    joints_0: joints,
                    weights_0: weights,
                },
                indices,
                material: 0,
                mode: TRIANGLES,
            }],
        }],
        materials: vec![Material {
            name: "Mannequin",
            pbr_metallic_roughness: PbrMetallicRoughness {
                base_color: [0.6, 0.6, 0.6, 1.0],
                metallic: 0.0,
                roughness: 0.9,
            },
            double_sided: true,
        }],
        skins: vec![Skin {
            name: ARMATURE_NODE,
            inverse_bind_matrices,
            joints: (0..bone_count).collect(),
        }],
        accessors: builder.accessors,
        buffer_views: builder.buffer_views,
        buffers: vec![Buffer {
            byte_length: builder.bin.len(),
        }],
    };
    let json = serde_json::to_vec(&root)
        .map_err(|e| RigError::Glb(format!("serializing glTF JSON: {e}")))?;
    Ok(container(&json, &builder.bin))
}

// ------------------------------------------------------------------ geometry

/// The mesh as flat arrays, every capsule appended in driven-bone order.
struct Geometry {
    positions: Vec<f32>,
    normals: Vec<f32>,
    joints: Vec<u16>,
    weights: Vec<f32>,
    indices: Vec<u32>,
}

impl Geometry {
    fn vertex_count(&self) -> u32 {
        u32::try_from(self.positions.len() / 3).unwrap_or(u32::MAX)
    }

    fn position_bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for vertex in self.positions.chunks_exact(3) {
            for axis in 0..3 {
                min[axis] = min[axis].min(vertex[axis]);
                max[axis] = max[axis].max(vertex[axis]);
            }
        }
        (min, max)
    }

    fn push_vertex(&mut self, position: Vec3, normal: Vec3, joint: u16) {
        self.positions.extend_from_slice(&position.to_array());
        self.normals.extend_from_slice(&normal.to_array());
        self.joints.extend_from_slice(&[joint, 0, 0, 0]);
        self.weights.extend_from_slice(&[1.0, 0.0, 0.0, 0.0]);
    }

    /// One capsule from `head` to `tail`, every vertex on `joint`: an apex, a
    /// ring at each end, an apex. Winding is counter-clockwise seen from
    /// outside, normals point out, so the figure reads right with culling on
    /// and lights as a smooth tube.
    fn push_capsule(&mut self, head: Vec3, tail: Vec3, radius: f32, joint: u16) {
        let axis = (tail - head).normalize();
        let u = axis.any_orthonormal_vector();
        let v = axis.cross(u); // (u, v, axis) is right-handed: u × v = axis
        let base = self.vertex_count();

        self.push_vertex(head - axis * radius, -axis, joint);
        for end in [head, tail] {
            for side in 0..SIDES {
                let theta = (side as f32) / (SIDES as f32) * std::f32::consts::TAU;
                let outward = u * theta.cos() + v * theta.sin();
                self.push_vertex(end + outward * radius, outward, joint);
            }
        }
        self.push_vertex(tail + axis * radius, axis, joint);

        let sides = SIDES as u32;
        let bottom = base;
        let ring_a = base + 1;
        let ring_b = base + 1 + sides;
        let top = base + 1 + 2 * sides;
        for i in 0..sides {
            let j = (i + 1) % sides;
            self.indices
                .extend_from_slice(&[bottom, ring_a + j, ring_a + i]);
            self.indices
                .extend_from_slice(&[ring_a + i, ring_a + j, ring_b + j]);
            self.indices
                .extend_from_slice(&[ring_a + i, ring_b + j, ring_b + i]);
            self.indices
                .extend_from_slice(&[top, ring_b + i, ring_b + j]);
        }
    }
}

/// Where each driven bone's capsule ends: the head of the child whose
/// direction best continues the bone's own +Y (a hip has a spine above it and
/// two legs beside it; the spine is the one the hip bone points at), or a
/// stub along that axis when no child sits a measurable distance away.
fn capsule_tail(contract: &Contract, world: &[(Vec3, Quat)], index: usize) -> Vec3 {
    let (head, rotation) = world[index];
    let along = rotation * Vec3::Y;
    let mut best: Option<(f32, Vec3)> = None;
    for (child, bone) in contract.bones.iter().enumerate() {
        if bone.parent != Some(index) {
            continue;
        }
        let offset = world[child].0 - head;
        if offset.length() < 1e-4 {
            continue;
        }
        let alignment = offset.normalize().dot(along);
        if best.is_none_or(|(score, _)| alignment > score) {
            best = Some((alignment, world[child].0));
        }
    }
    best.map_or_else(|| head + along * STUB_LENGTH_M, |(_, tail)| tail)
}

fn build_geometry(contract: &Contract, world: &[(Vec3, Quat)]) -> Geometry {
    let mut geometry = Geometry {
        positions: Vec::new(),
        normals: Vec::new(),
        joints: Vec::new(),
        weights: Vec::new(),
        indices: Vec::new(),
    };
    for (index, _) in contract.driven() {
        let head = world[index].0;
        let tail = capsule_tail(contract, world, index);
        let radius = radius_for((tail - head).length());
        let joint = u16::try_from(index).unwrap_or(u16::MAX);
        geometry.push_capsule(head, tail, radius, joint);
    }
    geometry.stand_on_floor();
    geometry
}

impl Geometry {
    /// Flatten every vertex below y = 0 onto the floor.
    ///
    /// The profile's foot gate says the lowest skinned vertex sits within a
    /// tolerance of y = 0, and a rig's foot joints sit on the floor itself —
    /// the toe head of the shipped humanoid is at y = 0 exactly — so a capsule
    /// of any radius around them reaches below it. The mannequin exists to
    /// pass that gate by construction, not by tuning radii against one rig:
    /// its soles are pressed flat to the ground instead of poking through it.
    fn stand_on_floor(&mut self) {
        for vertex in self.positions.chunks_exact_mut(3) {
            vertex[1] = vertex[1].max(0.0);
        }
    }
}

// ----------------------------------------------------------------- the file

/// glTF component types and targets, by their schema numbers.
const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const TRIANGLES: u32 = 4;

/// A glTF document under construction: the accessors and views, plus the
/// binary chunk they point into.
///
/// The layout rule is one buffer view per accessor, appended in call order and
/// four-byte aligned. That is more views than a packed exporter would write and
/// deliberately so: with `byteOffset` zero on every accessor, the emission
/// order of the calls IS the layout, which is what makes the output
/// byte-reproducible from the same inputs.
#[derive(Default)]
struct Builder {
    accessors: Vec<Accessor>,
    buffer_views: Vec<BufferView>,
    bin: Vec<u8>,
}

impl Builder {
    /// Append `data` to the binary chunk as its own buffer view, four-byte
    /// aligned so every component type this file uses is legally offset.
    fn view(&mut self, data: &[u8], target: Option<u32>) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let byte_offset = self.bin.len();
        self.bin.extend_from_slice(data);
        self.buffer_views.push(BufferView {
            buffer: 0,
            byte_length: data.len(),
            byte_offset,
            target,
        });
        self.buffer_views.len() - 1
    }

    /// An accessor over the whole of `view`.
    fn accessor(
        &mut self,
        view: usize,
        component_type: u32,
        type_: &'static str,
        count: usize,
        bounds: Option<(Vec<f32>, Vec<f32>)>,
    ) -> usize {
        let (min, max) = bounds.map_or((None, None), |(min, max)| (Some(min), Some(max)));
        self.accessors.push(Accessor {
            buffer_view: view,
            byte_offset: 0,
            component_type,
            count,
            type_,
            min,
            max,
        });
        self.accessors.len() - 1
    }
}

fn f32_bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn u16_bytes(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn u32_bytes(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Frame JSON and binary chunks as a GLB: 12-byte header, then each chunk as
/// length + tag + payload, JSON padded to four bytes with spaces and BIN with
/// zeros, total length in the header.
fn container(json: &[u8], bin: &[u8]) -> Vec<u8> {
    let json_padded = json.len().next_multiple_of(4);
    let bin_padded = bin.len().next_multiple_of(4);
    let total = 12 + 8 + json_padded + 8 + bin_padded;
    let length = |n: usize| u32::try_from(n).unwrap_or(u32::MAX).to_le_bytes();

    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&length(total));

    out.extend_from_slice(&length(json_padded));
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(json);
    out.resize(out.len() + (json_padded - json.len()), b' ');

    out.extend_from_slice(&length(bin_padded));
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(bin);
    out.resize(out.len() + (bin_padded - bin.len()), 0);
    out
}

// ------------------------------------------------------------ the JSON tree
//
// The subset of the glTF 2.0 schema this file uses, as serde structs rather
// than a `Value` tree: `f32` fields print at shortest round-trip precision
// (a `Value` would widen them to f64 and print seventeen digits), and the
// field order is the declaration order, so the bytes are fixed by the code.

#[derive(Serialize)]
struct Root {
    asset: Asset,
    scene: usize,
    scenes: Vec<Scene>,
    nodes: Vec<Node>,
    meshes: Vec<Mesh>,
    materials: Vec<Material>,
    skins: Vec<Skin>,
    accessors: Vec<Accessor>,
    #[serde(rename = "bufferViews")]
    buffer_views: Vec<BufferView>,
    buffers: Vec<Buffer>,
}

#[derive(Serialize)]
struct Asset {
    generator: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
struct Scene {
    name: &'static str,
    nodes: Vec<usize>,
}

#[derive(Serialize)]
struct Node {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    children: Option<Vec<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    translation: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rotation: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skin: Option<usize>,
}

#[derive(Serialize)]
struct Mesh {
    name: &'static str,
    primitives: Vec<Primitive>,
}

#[derive(Serialize)]
struct Primitive {
    attributes: Attributes,
    indices: usize,
    material: usize,
    mode: u32,
}

#[derive(Serialize)]
struct Attributes {
    #[serde(rename = "POSITION")]
    position: usize,
    #[serde(rename = "NORMAL")]
    normal: usize,
    #[serde(rename = "JOINTS_0")]
    joints_0: usize,
    #[serde(rename = "WEIGHTS_0")]
    weights_0: usize,
}

#[derive(Serialize)]
struct Material {
    name: &'static str,
    #[serde(rename = "pbrMetallicRoughness")]
    pbr_metallic_roughness: PbrMetallicRoughness,
    #[serde(rename = "doubleSided")]
    double_sided: bool,
}

#[derive(Serialize)]
struct PbrMetallicRoughness {
    #[serde(rename = "baseColorFactor")]
    base_color: [f32; 4],
    #[serde(rename = "metallicFactor")]
    metallic: f32,
    #[serde(rename = "roughnessFactor")]
    roughness: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Skin {
    name: &'static str,
    inverse_bind_matrices: usize,
    joints: Vec<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Accessor {
    buffer_view: usize,
    byte_offset: usize,
    component_type: u32,
    count: usize,
    #[serde(rename = "type")]
    type_: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    min: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<Vec<f32>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BufferView {
    buffer: usize,
    byte_length: usize,
    byte_offset: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Buffer {
    byte_length: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_container_is_padded_and_sized() {
        let glb = container(b"{\"a\":1}", &[1, 2, 3, 4, 5]);
        assert_eq!(&glb[..4], b"glTF");
        let total = u32::from_le_bytes(glb[8..12].try_into().expect("4 bytes")) as usize;
        assert_eq!(total, glb.len());
        assert_eq!(glb.len() % 4, 0);
        // JSON chunk: 7 bytes padded to 8 with a trailing space.
        let json_len = u32::from_le_bytes(glb[12..16].try_into().expect("4 bytes")) as usize;
        assert_eq!(json_len, 8);
        assert_eq!(glb[20 + 7], b' ');
        // BIN chunk: 5 bytes padded to 8 with zeros.
        let bin_len = u32::from_le_bytes(glb[28..32].try_into().expect("4 bytes")) as usize;
        assert_eq!(bin_len, 8);
        assert_eq!(&glb[36..], &[1, 2, 3, 4, 5, 0, 0, 0]);
    }

    #[test]
    fn views_are_four_byte_aligned_and_appended_in_call_order() {
        let mut b = Builder::default();
        b.view(&[1, 2, 3], None);
        b.view(&[4, 5], Some(ARRAY_BUFFER));
        assert_eq!(b.buffer_views[0].byte_offset, 0);
        assert_eq!(b.buffer_views[1].byte_offset, 4);
        assert_eq!(b.buffer_views[1].target, Some(ARRAY_BUFFER));
        assert_eq!(b.bin, [1, 2, 3, 0, 4, 5]);
    }

    #[test]
    fn a_capsule_has_two_rings_two_apexes_and_outward_normals() {
        let mut geometry = Geometry {
            positions: Vec::new(),
            normals: Vec::new(),
            joints: Vec::new(),
            weights: Vec::new(),
            indices: Vec::new(),
        };
        geometry.push_capsule(Vec3::ZERO, Vec3::Y, 0.1, 7);
        assert_eq!(geometry.vertex_count() as usize, 2 + 2 * SIDES);
        assert_eq!(geometry.indices.len(), 4 * SIDES * 3);
        assert!(geometry.joints.iter().step_by(4).all(|&j| j == 7));
        assert!(
            geometry
                .weights
                .chunks_exact(4)
                .all(|w| w == [1.0, 0.0, 0.0, 0.0])
        );
        let (min, max) = geometry.position_bounds();
        assert!((min[1] + 0.1).abs() < 1e-6, "{min:?}");
        assert!((max[1] - 1.1).abs() < 1e-6, "{max:?}");
        // Every triangle's normal agrees with its vertices' normals: outward.
        for tri in geometry.indices.chunks_exact(3) {
            let p = |i: u32| {
                let i = i as usize * 3;
                Vec3::new(
                    geometry.positions[i],
                    geometry.positions[i + 1],
                    geometry.positions[i + 2],
                )
            };
            let n = |i: u32| {
                let i = i as usize * 3;
                Vec3::new(
                    geometry.normals[i],
                    geometry.normals[i + 1],
                    geometry.normals[i + 2],
                )
            };
            let face = (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]));
            let vertex = n(tri[0]) + n(tri[1]) + n(tri[2]);
            assert!(face.dot(vertex) > 0.0, "triangle {tri:?} winds inward");
        }
    }
}
