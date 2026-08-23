//! Measure a `.glb` without an engine: what the file contains, read as data.
//!
//! This is the read half a file needs before a library will take it. It
//! answers the questions a sidecar's `measured.mesh` block records (vertices,
//! triangles, bones, bounds) plus the two only a foreign file raises: which
//! mesh-bearing nodes it has and which exporter wrote it (the glTF
//! `asset.generator` string — measured from the file, never declared).
//!
//! Only self-contained GLBs pass, for the same reason [`open_glb`] refuses a
//! `uri` buffer: a file that references siblings is not the file that was
//! hashed, and everything a record claims rests on the hash.

use std::collections::BTreeSet;

use crate::{Result, RigError};

/// What a `.glb` measures as, read entirely from its bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct GlbMeasurement {
    /// Vertex count summed over every mesh, as the file will draw it.
    pub vertices: u32,
    /// Triangle count summed over every mesh.
    pub triangles: u32,
    /// Distinct bones referenced by the file's skins.
    pub bones_skinned: u32,
    /// The lowest Y of any vertex — feet-on-ground is `0.0` here.
    pub lowest_y: f32,
    /// Axis-aligned bounds over every mesh, `[min, max]`.
    pub bounds: [[f32; 3]; 2],
    /// Names of the mesh-bearing nodes, in document order. These are the
    /// exporter's object names — `Body`, `Prop` — so a convention about them
    /// can be checked.
    pub mesh_nodes: Vec<String>,
    /// Distinct joint node names across every skin, sorted. A body promote
    /// checks every contract name is here ([`crate::missing_joints`]).
    pub joint_names: Vec<String>,
    /// The glTF `asset.generator` string, e.g. `Khronos glTF Blender I/O v…`.
    pub generator: Option<String>,
}

/// Open a GLB and hand back its document and its binary chunk.
///
/// # Errors
///
/// The bytes do not parse as glTF, or the container has no binary chunk — a
/// JSON-only file references its buffers by `uri`, which is not
/// self-contained.
pub fn open_glb(bytes: &[u8]) -> Result<(gltf::Document, Vec<u8>)> {
    let gltf::Gltf { document, blob } =
        gltf::Gltf::from_slice(bytes).map_err(|e| RigError::Glb(e.to_string()))?;
    let blob = blob.ok_or_else(|| RigError::Glb(String::from("no binary chunk")))?;
    Ok((document, blob))
}

/// Measure a self-contained, skinned `.glb` — a body.
///
/// # Errors
///
/// Refuses bytes that do not parse as GLB, a file whose buffers or images
/// reference external `uri`s, a file with no mesh, a mesh with no vertices or
/// triangles, a non-finite bound, and a file with no skin — an unskinned
/// character would load, bind nothing, and stand in its rest pose forever,
/// which is exactly the silent failure this pipeline exists to refuse early.
pub fn measure_glb(bytes: &[u8]) -> Result<GlbMeasurement> {
    let measured = measure_glb_geometry(bytes)?;
    if measured.bones_skinned == 0 {
        return Err(RigError::Glb(String::from(
            "no skin; an unskinned character binds nothing and holds its rest pose",
        )));
    }
    Ok(measured)
}

/// Measure a self-contained `.glb` that need not be skinned — a prop.
///
/// Everything [`measure_glb`] checks except the skin: a prop has none, and
/// what a prop's consumers want to hold it to is its bounds (a decor kind's
/// height, a weapon's reach), which this reads the same way.
///
/// # Errors
///
/// As [`measure_glb`], minus the skin requirement.
pub fn measure_glb_geometry(bytes: &[u8]) -> Result<GlbMeasurement> {
    let (document, blob) = open_glb(bytes)?;

    for buffer in document.buffers() {
        if matches!(buffer.source(), gltf::buffer::Source::Uri(_)) {
            return Err(RigError::Glb(String::from(
                "a buffer references an external uri; files must be self-contained",
            )));
        }
    }
    for image in document.images() {
        if matches!(image.source(), gltf::image::Source::Uri { .. }) {
            return Err(RigError::Glb(String::from(
                "an image references an external uri; files must be self-contained",
            )));
        }
    }

    let mut vertices: u64 = 0;
    let mut triangles: u64 = 0;
    let mut bounds: Option<[[f32; 3]; 2]> = None;
    for mesh in document.meshes() {
        let label = || mesh.name().unwrap_or("<unnamed>").to_owned();
        let mut mesh_vertices: u64 = 0;
        let mut mesh_triangles: u64 = 0;
        for primitive in mesh.primitives() {
            let Some(positions) = primitive.get(&gltf::Semantic::Positions) else {
                continue;
            };
            mesh_vertices += positions.count() as u64;
            mesh_triangles += primitive
                .indices()
                .map_or(positions.count() as u64, |i| i.count() as u64)
                / 3;

            // The spec requires min/max on POSITION accessors, but a bound is
            // only as trustworthy as the exporter that wrote it — so read the
            // vertices and take the measured envelope.
            let reader = primitive.reader(|buffer| match buffer.source() {
                gltf::buffer::Source::Bin => Some(blob.as_slice()),
                gltf::buffer::Source::Uri(_) => None,
            });
            let Some(read) = reader.read_positions() else {
                return Err(RigError::Glb(format!(
                    "mesh {} has unreadable POSITION data",
                    label()
                )));
            };
            for position in read {
                if !position.iter().all(|c| c.is_finite()) {
                    return Err(RigError::NonFiniteVertex(label()));
                }
                let entry = bounds.get_or_insert([position, position]);
                for axis in 0..3 {
                    entry[0][axis] = entry[0][axis].min(position[axis]);
                    entry[1][axis] = entry[1][axis].max(position[axis]);
                }
            }
        }
        if mesh_vertices == 0 || mesh_triangles == 0 {
            return Err(RigError::EmptyMesh(label()));
        }
        vertices += mesh_vertices;
        triangles += mesh_triangles;
    }
    let Some(bounds) = bounds else {
        return Err(RigError::NoMeshes);
    };

    let mut joints: BTreeSet<usize> = BTreeSet::new();
    let mut joint_names: BTreeSet<String> = BTreeSet::new();
    for skin in document.skins() {
        for joint in skin.joints() {
            joints.insert(joint.index());
            if let Some(name) = joint.name() {
                joint_names.insert(name.to_owned());
            }
        }
    }
    let mesh_nodes: Vec<String> = document
        .nodes()
        .filter(|node| node.mesh().is_some())
        .filter_map(|node| node.name().map(str::to_owned))
        .collect();

    let generator = document.clone().into_json().asset.generator;

    Ok(GlbMeasurement {
        vertices: u32::try_from(vertices).unwrap_or(u32::MAX),
        triangles: u32::try_from(triangles).unwrap_or(u32::MAX),
        bones_skinned: u32::try_from(joints.len()).unwrap_or(u32::MAX),
        lowest_y: bounds[0][1],
        bounds,
        mesh_nodes,
        joint_names: joint_names.into_iter().collect(),
        generator,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_file(relative: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../rigs/humanoid")
            .join(relative);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn measures_the_fixture_clip() {
        // The fixture is a baked clip: one anchor triangle skinned to the 27
        // driven joints, so it is the smallest skinned file the profile ships.
        let measured = measure_glb(&profile_file("fixture/cskel27_idle.glb")).expect("readable");
        assert_eq!(measured.mesh_nodes, ["SkinAnchor"]);
        assert_eq!(measured.bones_skinned, 27);
        assert_eq!(measured.joint_names.len(), 27);
        assert!(measured.joint_names.contains(&String::from("Hips")));
        assert!(measured.joint_names.contains(&String::from("LeftToeBase")));
        assert!(measured.vertices > 0 && measured.triangles > 0);
        assert!(measured.generator.is_some());
    }

    #[test]
    fn the_bare_rig_has_no_mesh_to_measure() {
        let error = measure_glb(&profile_file("rig.glb")).expect_err("no mesh");
        assert!(matches!(error, RigError::NoMeshes), "{error}");
        let error = measure_glb_geometry(&profile_file("rig.glb")).expect_err("no mesh");
        assert!(matches!(error, RigError::NoMeshes), "{error}");
    }

    #[test]
    fn refuses_bytes_that_are_not_a_glb() {
        assert!(measure_glb(b"not a glb").is_err());
    }

    #[test]
    fn refuses_an_external_buffer_uri() {
        // Hand-build the smallest container claiming a uri buffer: the point
        // is that the refusal happens before any accessor is read.
        let json =
            br#"{"asset":{"version":"2.0"},"buffers":[{"uri":"external.bin","byteLength":4}]}"#;
        let mut padded = json.to_vec();
        while !padded.len().is_multiple_of(4) {
            padded.push(b' ');
        }
        let mut glb = Vec::new();
        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&2u32.to_le_bytes());
        let total = 12 + 8 + padded.len() + 8 + 4;
        glb.extend_from_slice(&u32::try_from(total).expect("small").to_le_bytes());
        glb.extend_from_slice(&u32::try_from(padded.len()).expect("small").to_le_bytes());
        glb.extend_from_slice(b"JSON");
        glb.extend_from_slice(&padded);
        glb.extend_from_slice(&4u32.to_le_bytes());
        glb.extend_from_slice(b"BIN\0");
        glb.extend_from_slice(&[0u8; 4]);
        let error = measure_glb(&glb).expect_err("must refuse");
        assert!(error.to_string().contains("uri"), "{error}");
    }
}
