//! A rig profile as data.
//!
//! A profile is a directory — `rigs/humanoid/` ships one — holding the bone
//! contract every body must carry (`contract.json`), the attachment points a
//! prop rides on (`sockets.json`), and the driven layout a motion take writes
//! (`motion_skeleton.json`), beside the rig artifacts they are derived from
//! (`rig.glb`, `rig.blend`). This crate reads that directory, derives the bone
//! table back out of a `.glb` so the two can be held to each other, and
//! measures a skinned or unskinned file without an engine.
//!
//! Nothing here is a constant. The old shape of this data was a Rust table
//! pasted into source; the table is now a file a game, a Python step and a
//! validator all read, and the one thing source still knows is how to check
//! it: [`derive_from_glb`] re-derives the bones from the artifact with the
//! same rules the exporter used, and [`check_drift`] says whether the file
//! and the artifact still agree.
//!
//! Two facts about the shape of a contract:
//!
//! - **Order is glb node order.** Bones appear exactly as the nodes do in the
//!   rig's node array, with the non-bone armature root omitted. Nothing may
//!   reorder them: `parent` indices point into the array, and drift is
//!   checked position for position.
//! - **The root has no parent.** The armature object above it is a scene
//!   node, not a bone, and the contract forbids inserting one: an engine that
//!   binds clips by hashing each bone's full name path from the animation root
//!   sees every hash change when a parent appears above the root, and every
//!   clip silently binds to nothing.
//!
//! Float values are the glb's rest pose as `f32`, printed at shortest
//! round-trip precision, so regenerating a contract from an unchanged rig
//! reproduces the file byte for byte. The contract is generated, never typed.

pub mod export;
pub mod measure;

use std::{
    collections::HashSet,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

/// The profile file schema this crate reads and writes.
pub const SCHEMA: u32 = 1;

/// How far a re-derived rest transform may sit from the contract before it
/// counts as drift: above exporter float noise, below any deliberate rest-pose
/// edit.
pub const DRIFT_TOLERANCE: f32 = 1e-5;

/// The contract file inside a profile directory.
pub const CONTRACT_FILE: &str = "contract.json";
/// The sockets file inside a profile directory.
pub const SOCKETS_FILE: &str = "sockets.json";
/// The driven-layout file inside a profile directory.
pub const MOTION_SKELETON_FILE: &str = "motion_skeleton.json";

/// Everything that can go wrong reading a profile or a rig file.
#[derive(Debug)]
pub enum RigError {
    /// A file could not be read.
    Io {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// A profile file did not parse.
    Json {
        /// The file.
        path: PathBuf,
        /// The underlying error.
        source: serde_json::Error,
    },
    /// A profile file carries a schema this crate does not read.
    Schema {
        /// The file.
        path: PathBuf,
        /// The schema it declared.
        found: u32,
    },
    /// A profile file contradicts itself or another file of the profile.
    Invalid(String),
    /// Bytes that are not a readable, self-contained glb.
    Glb(String),
    /// A glb whose node tree is not a rig: no lone armature root, a bone
    /// hanging outside it, an unnamed node.
    Hierarchy(String),
    /// A glb with no mesh at all.
    NoMeshes,
    /// A mesh with no vertices or no triangles.
    EmptyMesh(String),
    /// A vertex position that is not a finite number.
    NonFiniteVertex(String),
}

impl fmt::Display for RigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Json { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Schema { path, found } => write!(
                f,
                "{}: schema {found}, this build reads schema {SCHEMA}",
                path.display()
            ),
            Self::Invalid(why) => write!(f, "invalid profile: {why}"),
            Self::Glb(why) => write!(f, "glb: {why}"),
            Self::Hierarchy(why) => write!(f, "rig hierarchy: {why}"),
            Self::NoMeshes => f.write_str("the file holds no mesh"),
            Self::EmptyMesh(mesh) => write!(f, "mesh {mesh} has no vertices or no triangles"),
            Self::NonFiniteVertex(mesh) => {
                write!(f, "mesh {mesh} has a non-finite vertex position")
            }
        }
    }
}

impl std::error::Error for RigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// The crate's result type.
pub type Result<T> = std::result::Result<T, RigError>;

/// One bone of a contract, as the rig `.glb` defines it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bone {
    /// The bone's name — the exact string clips hash, so it is the identity.
    pub name: String,
    /// Parent's index into [`Contract::bones`]; `None` for the root, whose
    /// parent is the armature scene node rather than a bone.
    pub parent: Option<usize>,
    /// Whether clips animate this bone. A bone that is not driven is still
    /// skinnable — the finger leaves — but no clip ever moves it.
    pub driven: bool,
    /// Rest translation local to the parent, metres, glTF axes (Y up).
    pub rest_translation: [f32; 3],
    /// Rest rotation local to the parent, quaternion `[x, y, z, w]`.
    pub rest_rotation: [f32; 4],
}

/// The stature band a body must fall in, metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Stature {
    /// The stature the rest pose was measured for.
    pub reference: f32,
    /// Shorter than this is refused: a prop mistaken for a body.
    pub min: f32,
    /// Taller than this is refused: a giant, or a mesh still in raw units.
    pub max: f32,
}

/// The artifacts a contract was derived from, with the hashes that prove it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sources {
    /// The rig `.glb`, relative to the profile directory.
    pub glb: String,
    /// Its sha256, lowercase hex. A mismatch is a failure: the bones came
    /// from these bytes.
    pub glb_sha256: String,
    /// The rig `.blend`, relative to the profile directory, when the profile
    /// ships one.
    pub blend: Option<String>,
    /// Its sha256. A mismatch is a warning: the blend is the authoring
    /// source, not what the contract was read from.
    pub blend_sha256: Option<String>,
}

/// A rig contract: what a skinned mesh must carry for every clip of the
/// library to play on it unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contract {
    /// Always [`SCHEMA`].
    pub schema: u32,
    /// The profile's name, the directory it lives in.
    pub name: String,
    /// Bumps only when a bone name, parent or rest transform changes.
    pub version: u32,
    /// The one rootless bone.
    pub root: String,
    /// Which way the rest pose faces, as a signed glTF axis (`"+Z"`).
    pub front: String,
    /// The stature band a body must fall in.
    pub stature_m: Stature,
    /// How far the lowest skinned vertex may sit from y = 0.
    pub foot_tolerance_m: f32,
    /// Per-component quaternion gap above which a rest rotation is a re-pose.
    pub rest_rotation_tolerance: f32,
    /// The driven layout's name; must equal [`MotionSkeleton::name`].
    pub driven_layout: String,
    /// The library clip a rig check binds to a subject: it drives every
    /// driven bone, so binding is fully exercised.
    pub reference_clip: String,
    /// Where the bones came from.
    pub sources: Sources,
    /// Every bone, in glb node order.
    pub bones: Vec<Bone>,
}

impl Contract {
    /// Parse a contract from JSON bytes and validate it.
    ///
    /// # Errors
    ///
    /// The bytes do not parse, declare another schema, or describe a table
    /// that is not a tree with one root: see [`Contract::validate`].
    pub fn from_json(bytes: &[u8], path: &Path) -> Result<Self> {
        let contract: Self = serde_json::from_slice(bytes).map_err(|source| RigError::Json {
            path: path.to_path_buf(),
            source,
        })?;
        if contract.schema != SCHEMA {
            return Err(RigError::Schema {
                path: path.to_path_buf(),
                found: contract.schema,
            });
        }
        contract.validate()?;
        Ok(contract)
    }

    /// Read and validate a contract file.
    ///
    /// # Errors
    ///
    /// As [`Contract::from_json`], plus the file being unreadable.
    pub fn load(path: &Path) -> Result<Self> {
        Self::from_json(&read(path)?, path)
    }

    /// The contract as bytes: pretty JSON, shortest round-trip floats, one
    /// trailing newline. Byte-stable for an unchanged contract.
    ///
    /// # Errors
    ///
    /// Serialisation fails only on a non-finite float, which
    /// [`Contract::validate`] refuses first.
    pub fn to_json(&self) -> Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec_pretty(self).map_err(|source| RigError::Json {
            path: PathBuf::from(CONTRACT_FILE),
            source,
        })?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// The invariants every reader relies on: one rootless bone and it is
    /// [`Contract::root`]; every parent inside the table, never a bone
    /// itself, never a cycle; unique names; unit rest rotations.
    ///
    /// # Errors
    ///
    /// The first invariant broken, named.
    pub fn validate(&self) -> Result<()> {
        if self.bones.is_empty() {
            return Err(RigError::Invalid(String::from("the contract has no bones")));
        }
        let mut roots = 0;
        let mut seen: HashSet<&str> = HashSet::new();
        for (i, bone) in self.bones.iter().enumerate() {
            if !seen.insert(&bone.name) {
                return Err(RigError::Invalid(format!(
                    "bone name {:?} appears twice; identical name paths hash to identical targets",
                    bone.name
                )));
            }
            match bone.parent {
                None => {
                    roots += 1;
                    if bone.name != self.root {
                        return Err(RigError::Invalid(format!(
                            "{} is rootless but the root is {}",
                            bone.name, self.root
                        )));
                    }
                }
                Some(p) if p >= self.bones.len() => {
                    return Err(RigError::Invalid(format!(
                        "{} parent {p} is outside the table of {} bones",
                        bone.name,
                        self.bones.len()
                    )));
                }
                Some(p) if p == i => {
                    return Err(RigError::Invalid(format!("{} parents itself", bone.name)));
                }
                Some(_) => {}
            }
            let [x, y, z, w] = bone.rest_rotation;
            let norm = z.mul_add(z, x.mul_add(x, y * y)) + w * w;
            if !norm.is_finite() || (norm - 1.0).abs() > 1e-3 {
                return Err(RigError::Invalid(format!(
                    "{} rest rotation has norm {norm}, not a unit quaternion",
                    bone.name
                )));
            }
            if !bone.rest_translation.iter().all(|c| c.is_finite()) {
                return Err(RigError::Invalid(format!(
                    "{} rest translation is not finite",
                    bone.name
                )));
            }
        }
        if roots != 1 {
            return Err(RigError::Invalid(format!(
                "{roots} rootless bones; exactly one ({}) is allowed",
                self.root
            )));
        }
        // Acyclic: every chain must reach the root within the table's length.
        for (i, bone) in self.bones.iter().enumerate() {
            let mut current = i;
            let mut steps = 0;
            while let Some(p) = self.bones[current].parent {
                steps += 1;
                if steps > self.bones.len() {
                    return Err(RigError::Invalid(format!(
                        "{} sits on a parent cycle",
                        bone.name
                    )));
                }
                current = p;
            }
        }
        Ok(())
    }

    /// Index of a contract bone by exact name, or `None` if the contract has
    /// no bone of that name.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|bone| bone.name == name)
    }

    /// The root bone's index.
    ///
    /// # Panics
    ///
    /// Panics on a contract that was not validated: a validated contract
    /// always has exactly one root.
    #[must_use]
    pub fn root_index(&self) -> usize {
        self.bones
            .iter()
            .position(|bone| bone.parent.is_none())
            .expect("a validated contract has a root")
    }

    /// The depth a bone must sit at in a spawned hierarchy, counting the
    /// armature root as depth 1 — so the root bone is 2, its children 3, and
    /// so on.
    ///
    /// This is the number of segments an engine's name-path target id has,
    /// which is why a validator checks it: a bone at the right name but the
    /// wrong depth hashes differently and binds to nothing.
    ///
    /// # Panics
    ///
    /// Panics if `index` is not a valid index into [`Contract::bones`], or
    /// the contract was never validated and holds a cycle.
    #[must_use]
    pub fn expected_depth(&self, index: usize) -> usize {
        let mut depth = 2; // the armature root is 1, the root bone is 2
        let mut current = index;
        while let Some(parent) = self.bones[current].parent {
            depth += 1;
            assert!(
                depth <= self.bones.len() + 2,
                "the contract holds a parent cycle; validate it first"
            );
            current = parent;
        }
        depth
    }

    /// The driven bones, with their indices, in table order.
    pub fn driven(&self) -> impl Iterator<Item = (usize, &Bone)> {
        self.bones
            .iter()
            .enumerate()
            .filter(|(_, bone)| bone.driven)
    }

    /// The names of the driven bones, in table order.
    #[must_use]
    pub fn driven_names(&self) -> Vec<&str> {
        self.driven().map(|(_, bone)| bone.name.as_str()).collect()
    }

    /// The nearest driven ancestor of a bone, or `None` when nothing driven
    /// sits above it. For a driven bone this is its parent in the driven
    /// layout, which is how the layout's own parent table is checked against
    /// the contract.
    #[must_use]
    pub fn driven_parent(&self, index: usize) -> Option<usize> {
        let mut current = self.bones[index].parent;
        while let Some(p) = current {
            if self.bones[p].driven {
                return Some(p);
            }
            current = self.bones[p].parent;
        }
        None
    }

    /// A bone's rest transform in character space, by chaining every parent
    /// up to the root. Pure arithmetic — no engine, so this pins the table
    /// itself rather than a renderer's opinion of it.
    ///
    /// # Panics
    ///
    /// Panics if `index` is not a valid index into [`Contract::bones`].
    #[must_use]
    pub fn rest_world(&self, index: usize) -> (Vec3, Quat) {
        let bone = &self.bones[index];
        let local_t = Vec3::from_array(bone.rest_translation);
        let local_r = Quat::from_array(bone.rest_rotation);
        match bone.parent {
            None => (local_t, local_r),
            Some(parent) => {
                let (pt, pr) = self.rest_world(parent);
                (pt + pr * local_t, pr * local_r)
            }
        }
    }
}

/// A bone as re-derived from a rig artifact, before it is compared to, or
/// written into, a contract.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedBone {
    /// The node's name.
    pub name: String,
    /// Parent's index into the derived list; `None` under the armature.
    pub parent: Option<usize>,
    /// Rest translation local to the parent, metres.
    pub rest_translation: [f32; 3],
    /// Rest rotation local to the parent, `[x, y, z, w]`.
    pub rest_rotation: [f32; 4],
}

/// What [`derive_from_glb`] reads out of a rig artifact.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedRig {
    /// The name of the lone scene root above the bones — the first segment of
    /// every bone's name path, so it is part of what a clip binds to.
    pub armature: String,
    /// Every bone, in glb node order.
    pub bones: Vec<DerivedBone>,
}

/// Re-derive the bone table from a rig `.glb`, with the rules the exporter
/// uses: the file's single scene has a single root node, the armature; bones
/// are every remaining non-mesh node, kept in glb node order; parents are
/// re-indexed into that list, with the armature parent becoming `None`.
///
/// # Errors
///
/// The bytes are not a glb; the scene or root is not lone; a bone hangs
/// outside the armature; a node is unnamed.
pub fn derive_from_glb(bytes: &[u8]) -> Result<DerivedRig> {
    let gltf::Gltf { document, .. } =
        gltf::Gltf::from_slice(bytes).map_err(|e| RigError::Glb(e.to_string()))?;

    let scenes: Vec<gltf::Scene<'_>> = document.scenes().collect();
    let [scene] = scenes.as_slice() else {
        return Err(RigError::Hierarchy(format!(
            "{} scenes; a rig holds exactly one",
            scenes.len()
        )));
    };
    let roots: Vec<gltf::Node<'_>> = scene.nodes().collect();
    let [root] = roots.as_slice() else {
        return Err(RigError::Hierarchy(format!(
            "{} scene roots; a rig's scene holds exactly one, the armature",
            roots.len()
        )));
    };
    let armature = root.index();
    let armature_name = root
        .name()
        .ok_or_else(|| {
            RigError::Hierarchy(String::from(
                "the scene root is unnamed; its name is the first segment of every bone path",
            ))
        })?
        .to_owned();

    let mut parent_of = vec![None; document.nodes().count()];
    for node in document.nodes() {
        for child in node.children() {
            parent_of[child.index()] = Some(node.index());
        }
    }

    let bone_nodes: Vec<gltf::Node<'_>> = document
        .nodes()
        .filter(|n| n.index() != armature && n.mesh().is_none())
        .collect();
    let table_index = |node: usize| bone_nodes.iter().position(|b| b.index() == node);

    let mut bones = Vec::with_capacity(bone_nodes.len());
    for node in &bone_nodes {
        let name = node
            .name()
            .ok_or_else(|| RigError::Hierarchy(format!("node {} is unnamed", node.index())))?
            .to_owned();
        let parent = match parent_of[node.index()] {
            Some(p) if p == armature => None,
            Some(p) => Some(table_index(p).ok_or_else(|| {
                RigError::Hierarchy(format!("{name}'s parent node {p} is not a bone"))
            })?),
            None => {
                return Err(RigError::Hierarchy(format!(
                    "bone {name} hangs outside the armature"
                )));
            }
        };
        let (rest_translation, rest_rotation, _scale) = node.transform().decomposed();
        bones.push(DerivedBone {
            name,
            parent,
            rest_translation,
            rest_rotation,
        });
    }
    Ok(DerivedRig {
        armature: armature_name,
        bones,
    })
}

/// Compare a contract against bones re-derived from its artifact.
///
/// Names and parent indices must match exactly, position for position; rest
/// translations and rotations within [`DRIFT_TOLERANCE`]. Returns one line
/// per disagreement, so an empty result means the file and the artifact
/// still say the same thing.
#[must_use]
pub fn check_drift(contract: &Contract, derived: &[DerivedBone]) -> Vec<String> {
    let mut drift = Vec::new();
    if derived.len() != contract.bones.len() {
        drift.push(format!(
            "the artifact has {} bones, the contract {} — regenerate the contract and bump its version",
            derived.len(),
            contract.bones.len()
        ));
    }
    for (i, (found, spec)) in derived.iter().zip(&contract.bones).enumerate() {
        if found.name != spec.name {
            drift.push(format!(
                "bone {i}: the artifact says {:?}, the contract says {:?}",
                found.name, spec.name
            ));
            continue;
        }
        if found.parent != spec.parent {
            drift.push(format!(
                "{}: parent is {:?} in the artifact, {:?} in the contract — the hierarchy drifted",
                spec.name, found.parent, spec.parent
            ));
        }
        for (axis, (a, b)) in found
            .rest_translation
            .iter()
            .zip(spec.rest_translation)
            .enumerate()
        {
            if (a - b).abs() >= DRIFT_TOLERANCE {
                drift.push(format!(
                    "{}: rest translation [{axis}] is {a} in the artifact, {b} in the contract",
                    spec.name
                ));
            }
        }
        for (axis, (a, b)) in found
            .rest_rotation
            .iter()
            .zip(spec.rest_rotation)
            .enumerate()
        {
            if (a - b).abs() >= DRIFT_TOLERANCE {
                drift.push(format!(
                    "{}: rest rotation [{axis}] is {a} in the artifact, {b} in the contract",
                    spec.name
                ));
            }
        }
    }
    drift
}

/// The contract bones a file does not carry, in contract order. `present` is
/// the file's joint names as [`measure::GlbMeasurement::joint_names`] reports
/// them; a body promote requires this to come back empty.
#[must_use]
pub fn missing_joints<'c, S: AsRef<str>>(contract: &'c Contract, present: &[S]) -> Vec<&'c str> {
    let present: HashSet<&str> = present.iter().map(AsRef::as_ref).collect();
    contract
        .bones
        .iter()
        .map(|bone| bone.name.as_str())
        .filter(|name| !present.contains(name))
        .collect()
}

/// The frame a prop is authored in, so a socket's rotation can carry it into
/// bone space without a per-prop correction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoringFrame {
    /// The axis convention the other three fields are stated in (`"gltf"`).
    pub axes: String,
    /// The prop's long axis — hilt to tip — as a signed axis (`"+Y"`).
    pub long_axis: String,
    /// The prop's front — a blade's edge, a pistol's sights (`"-Z"`).
    pub front: String,
    /// What sits at the prop's origin (`"grip"`).
    pub origin: String,
}

/// One attachment point: a named transform offset from a contract bone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Socket {
    /// The socket's name — the string a game passes at the attach call site.
    pub name: String,
    /// Exact contract bone name; [`Sockets::validate`] resolves it.
    pub bone: String,
    /// Offset local to the bone, metres, in the bone's node space (+Y runs
    /// along the bone toward its children).
    pub translation: [f32; 3],
    /// Offset rotation local to the bone, `[x, y, z, w]`, taking the prop's
    /// authoring frame into bone space.
    pub rotation: [f32; 4],
    /// Where the numbers came from, for the reader of the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Every attachment point of a profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sockets {
    /// Always [`SCHEMA`].
    pub schema: u32,
    /// The profile these belong to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// The frame a prop is authored in.
    pub authoring_frame: AuthoringFrame,
    /// How the table as a whole was derived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The sockets, in table order — the vocabulary an attach call accepts.
    pub sockets: Vec<Socket>,
}

impl Sockets {
    /// Read a sockets file. Cross-checks against the contract happen in
    /// [`Sockets::validate`], which [`RigProfile::load`] runs.
    ///
    /// # Errors
    ///
    /// The file is unreadable, does not parse, or declares another schema.
    pub fn load(path: &Path) -> Result<Self> {
        let sockets: Self =
            serde_json::from_slice(&read(path)?).map_err(|source| RigError::Json {
                path: path.to_path_buf(),
                source,
            })?;
        if sockets.schema != SCHEMA {
            return Err(RigError::Schema {
                path: path.to_path_buf(),
                found: sockets.schema,
            });
        }
        Ok(sockets)
    }

    /// Every socket names a contract bone, names are unique, no name is a
    /// bone name, rotations are unit quaternions.
    ///
    /// # Errors
    ///
    /// The first rule broken, named.
    pub fn validate(&self, contract: &Contract) -> Result<()> {
        let mut seen: HashSet<&str> = HashSet::new();
        for socket in &self.sockets {
            if !seen.insert(&socket.name) {
                return Err(RigError::Invalid(format!(
                    "socket {:?} appears twice",
                    socket.name
                )));
            }
            if contract.find(&socket.bone).is_none() {
                return Err(RigError::Invalid(format!(
                    "socket {:?} names bone {:?}, which the contract does not have",
                    socket.name, socket.bone
                )));
            }
            if contract.find(&socket.name).is_some() {
                return Err(RigError::Invalid(format!(
                    "socket {:?} is named like a bone; a bone name is not a socket name",
                    socket.name
                )));
            }
            let [x, y, z, w] = socket.rotation;
            let norm = z.mul_add(z, x.mul_add(x, y * y)) + w * w;
            if !norm.is_finite() || (norm - 1.0).abs() > 1e-4 {
                return Err(RigError::Invalid(format!(
                    "socket {:?} rotation has norm {norm}, not a unit quaternion",
                    socket.name
                )));
            }
        }
        Ok(())
    }

    /// The socket with exactly this name, or `None` when the profile has no
    /// such attachment point. Bone names are not socket names.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&Socket> {
        self.sockets.iter().find(|socket| socket.name == name)
    }

    /// Every socket name, in table order — the list an attach error names
    /// when a call misses.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.sockets
            .iter()
            .map(|socket| socket.name.as_str())
            .collect()
    }

    /// Where a socket sits in the rest pose, in character space, and which
    /// way it turns a prop's authoring frame.
    ///
    /// # Panics
    ///
    /// Panics if the socket's bone is not in the contract — validate first.
    #[must_use]
    pub fn rest_world(&self, socket: &Socket, contract: &Contract) -> (Vec3, Quat) {
        let index = contract
            .find(&socket.bone)
            .expect("a validated socket names a contract bone");
        let (bt, br) = contract.rest_world(index);
        (
            bt + br * Vec3::from_array(socket.translation),
            br * Quat::from_array(socket.rotation),
        )
    }
}

/// The driven layout: the joints a motion take writes, in the order it
/// writes them.
///
/// The order is load-bearing — a take's rotation array is indexed by it, and
/// getting it wrong does not error, it silently animates the wrong limb.
/// Every joint is a driven bone of the contract, and the contract's driven
/// bones are exactly these joints; [`RigProfile::load`] checks both ways and
/// that the parent chains agree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MotionSkeleton {
    /// Always [`SCHEMA`].
    pub schema: u32,
    /// The layout's name (`"cskel27"`); must equal [`Contract::driven_layout`].
    pub name: String,
    /// What the file is, for its reader.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Joint names, in take order.
    pub joints: Vec<String>,
    /// Parent index per joint; `None` for the root.
    pub parents: Vec<Option<usize>>,
    /// The root joint's index.
    pub root: usize,
    /// The head joint's index.
    pub head: usize,
    /// The hand joints, right then left.
    pub hands: Vec<usize>,
    /// The foot joints: heel and toe per side.
    pub feet: Vec<usize>,
    /// The joint each column of a take's per-frame foot-contact flags
    /// belongs to, in column order.
    pub contact_columns: Vec<usize>,
    /// Every joint on the right side.
    pub right: Vec<usize>,
    /// Every joint on the left side.
    pub left: Vec<usize>,
    /// The joints an exaggerate knob scales.
    pub arm_joints: Vec<String>,
}

impl MotionSkeleton {
    /// Read and validate a driven-layout file.
    ///
    /// # Errors
    ///
    /// The file is unreadable, does not parse, declares another schema, or
    /// fails [`MotionSkeleton::validate`].
    pub fn load(path: &Path) -> Result<Self> {
        let skeleton: Self =
            serde_json::from_slice(&read(path)?).map_err(|source| RigError::Json {
                path: path.to_path_buf(),
                source,
            })?;
        if skeleton.schema != SCHEMA {
            return Err(RigError::Schema {
                path: path.to_path_buf(),
                found: skeleton.schema,
            });
        }
        skeleton.validate()?;
        Ok(skeleton)
    }

    /// Parents are one per joint, always earlier than the joint (so the
    /// table is a tree written root-first), and only the root is rootless;
    /// every index field points inside the table; sides do not overlap;
    /// every named arm joint exists.
    ///
    /// # Errors
    ///
    /// The first rule broken, named.
    pub fn validate(&self) -> Result<()> {
        let count = self.joints.len();
        if count == 0 {
            return Err(RigError::Invalid(String::from(
                "the driven layout has no joints",
            )));
        }
        if self.parents.len() != count {
            return Err(RigError::Invalid(format!(
                "{} joints but {} parents",
                count,
                self.parents.len()
            )));
        }
        let mut seen: HashSet<&str> = HashSet::new();
        for (i, (joint, parent)) in self.joints.iter().zip(&self.parents).enumerate() {
            if !seen.insert(joint) {
                return Err(RigError::Invalid(format!("joint {joint:?} appears twice")));
            }
            match parent {
                None if i != self.root => {
                    return Err(RigError::Invalid(format!(
                        "{joint} is rootless but the root is joint {}",
                        self.root
                    )));
                }
                Some(p) if i == self.root => {
                    return Err(RigError::Invalid(format!(
                        "the root {joint} has a parent ({p})"
                    )));
                }
                Some(p) if *p >= i => {
                    return Err(RigError::Invalid(format!(
                        "{joint} parents a later joint ({p}); the table is written root-first"
                    )));
                }
                _ => {}
            }
        }
        let in_range = |label: &str, indices: &[usize]| -> Result<()> {
            match indices.iter().find(|&&i| i >= count) {
                Some(i) => Err(RigError::Invalid(format!(
                    "{label} index {i} is outside the {count} joints"
                ))),
                None => Ok(()),
            }
        };
        in_range("root", &[self.root])?;
        in_range("head", &[self.head])?;
        in_range("hands", &self.hands)?;
        in_range("feet", &self.feet)?;
        in_range("contact_columns", &self.contact_columns)?;
        in_range("right", &self.right)?;
        in_range("left", &self.left)?;
        if let Some(shared) = self.right.iter().find(|i| self.left.contains(i)) {
            return Err(RigError::Invalid(format!(
                "joint {} is on both sides",
                self.joints[*shared]
            )));
        }
        if let Some(missing) = self
            .arm_joints
            .iter()
            .find(|name| self.index_of(name).is_none())
        {
            return Err(RigError::Invalid(format!(
                "arm joint {missing:?} is not a joint of the layout"
            )));
        }
        Ok(())
    }

    /// Index of a joint by name.
    #[must_use]
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.joints.iter().position(|j| j == name)
    }

    /// Joint count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.joints.len()
    }

    /// Whether the layout has no joints (a validated one never does).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.joints.is_empty()
    }
}

/// A rig profile: the three data files of a profile directory, read and
/// cross-checked.
#[derive(Debug, Clone, PartialEq)]
pub struct RigProfile {
    /// The directory the profile was read from.
    pub dir: PathBuf,
    /// The bone contract.
    pub contract: Contract,
    /// The attachment points.
    pub sockets: Sockets,
    /// The driven layout.
    pub motion: MotionSkeleton,
}

impl RigProfile {
    /// Read a profile directory and check its files against each other:
    /// every socket hangs off a contract bone; the contract's driven bones
    /// are exactly the layout's joints; the driven parent chain of the
    /// contract collapses to the layout's parent table; the layout is the
    /// one the contract names.
    ///
    /// # Errors
    ///
    /// Any file unreadable or invalid on its own, or the first cross-check
    /// that fails, named.
    pub fn load(dir: &Path) -> Result<Self> {
        let contract = Contract::load(&dir.join(CONTRACT_FILE))?;
        let sockets = Sockets::load(&dir.join(SOCKETS_FILE))?;
        let motion = MotionSkeleton::load(&dir.join(MOTION_SKELETON_FILE))?;
        sockets.validate(&contract)?;
        check_driven_layout(&contract, &motion)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            contract,
            sockets,
            motion,
        })
    }

    /// The rig artifact the contract was derived from.
    #[must_use]
    pub fn glb_path(&self) -> PathBuf {
        self.dir.join(&self.contract.sources.glb)
    }

    /// The rig's authoring source, when the profile ships one.
    #[must_use]
    pub fn blend_path(&self) -> Option<PathBuf> {
        self.contract
            .sources
            .blend
            .as_ref()
            .map(|blend| self.dir.join(blend))
    }

    /// Re-hash the artifacts the contract names and report which still match.
    ///
    /// # Errors
    ///
    /// The glb is unreadable. A missing blend is reported, not an error: the
    /// blend is the authoring source, and a profile may ship without one.
    pub fn check_sources(&self) -> Result<SourceCheck> {
        let glb_matches = sha256_file(&self.glb_path())? == self.contract.sources.glb_sha256;
        let blend_matches = match (self.blend_path(), &self.contract.sources.blend_sha256) {
            (Some(path), Some(expected)) => match sha256_file(&path) {
                Ok(found) => Some(&found == expected),
                Err(RigError::Io { .. }) => Some(false),
                Err(other) => return Err(other),
            },
            _ => None,
        };
        Ok(SourceCheck {
            glb_matches,
            blend_matches,
        })
    }
}

/// What [`RigProfile::check_sources`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceCheck {
    /// The rig glb still hashes to what the contract was derived from. A
    /// `false` here is a failure: the bones came from other bytes.
    pub glb_matches: bool,
    /// The blend still hashes to what the contract recorded; `None` when the
    /// profile names no blend. A `false` is a warning: the blend is the
    /// authoring source, not what the contract was read from.
    pub blend_matches: Option<bool>,
}

/// The contract's driven bones and the motion layout describe the same
/// skeleton.
///
/// # Errors
///
/// A driven bone the layout lacks, a layout joint the contract does not
/// drive, a driven parent chain that collapses to a different parent than the
/// layout states, or a layout name the contract does not name.
pub fn check_driven_layout(contract: &Contract, motion: &MotionSkeleton) -> Result<()> {
    if contract.driven_layout != motion.name {
        return Err(RigError::Invalid(format!(
            "the contract drives layout {:?} but the layout file is {:?}",
            contract.driven_layout, motion.name
        )));
    }
    for (index, bone) in contract.driven() {
        let Some(joint) = motion.index_of(&bone.name) else {
            return Err(RigError::Invalid(format!(
                "{} is driven but is not a joint of {}",
                bone.name, motion.name
            )));
        };
        let contract_parent = contract
            .driven_parent(index)
            .map(|p| contract.bones[p].name.as_str());
        let layout_parent = motion.parents[joint].map(|p| motion.joints[p].as_str());
        if contract_parent != layout_parent {
            return Err(RigError::Invalid(format!(
                "{}'s nearest driven ancestor is {contract_parent:?} in the contract but its parent is {layout_parent:?} in {}",
                bone.name, motion.name
            )));
        }
    }
    if let Some(joint) = motion.joints.iter().find(|joint| {
        !contract
            .find(joint)
            .is_some_and(|i| contract.bones[i].driven)
    }) {
        return Err(RigError::Invalid(format!(
            "{} joint {joint:?} is not a driven bone of the contract",
            motion.name
        )));
    }
    Ok(())
}

/// The sha256 of a file, lowercase hex.
///
/// # Errors
///
/// The file is unreadable.
pub fn sha256_file(path: &Path) -> Result<String> {
    Ok(sha256_hex(&read(path)?))
}

/// The sha256 of bytes, lowercase hex.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use fmt::Write;
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|source| RigError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_bones() -> Contract {
        Contract {
            schema: SCHEMA,
            name: String::from("test"),
            version: 1,
            root: String::from("Hips"),
            front: String::from("+Z"),
            stature_m: Stature {
                reference: 1.8,
                min: 1.4,
                max: 2.2,
            },
            foot_tolerance_m: 0.05,
            rest_rotation_tolerance: 1e-3,
            driven_layout: String::from("two"),
            reference_clip: String::from("walk"),
            sources: Sources {
                glb: String::from("rig.glb"),
                glb_sha256: String::new(),
                blend: None,
                blend_sha256: None,
            },
            bones: vec![
                Bone {
                    name: String::from("Hips"),
                    parent: None,
                    driven: true,
                    rest_translation: [0.0, 1.0, 0.0],
                    rest_rotation: [0.0, 0.0, 0.0, 1.0],
                },
                Bone {
                    name: String::from("Spine"),
                    parent: Some(0),
                    driven: true,
                    rest_translation: [0.0, 0.1, 0.0],
                    rest_rotation: [0.0, 0.0, 0.0, 1.0],
                },
            ],
        }
    }

    #[test]
    fn a_small_contract_validates_and_walks() {
        let contract = two_bones();
        contract.validate().expect("valid");
        assert_eq!(contract.find("Spine"), Some(1));
        assert_eq!(contract.find("spine"), None);
        assert_eq!(contract.expected_depth(0), 2);
        assert_eq!(contract.expected_depth(1), 3);
        assert_eq!(contract.root_index(), 0);
        assert_eq!(contract.driven_names(), ["Hips", "Spine"]);
        let (world, _) = contract.rest_world(1);
        assert!((world.y - 1.1).abs() < 1e-6, "{world:?}");
    }

    #[test]
    fn a_second_root_is_refused() {
        let mut contract = two_bones();
        contract.bones[1].parent = None;
        let error = contract.validate().expect_err("two roots");
        assert!(error.to_string().contains("rootless"), "{error}");
    }

    #[test]
    fn a_cycle_is_refused() {
        let mut contract = two_bones();
        contract.bones[0].parent = Some(1);
        contract.root = String::from("nothing");
        let error = contract.validate().expect_err("a cycle");
        assert!(error.to_string().contains("rootless"), "{error}");
    }

    #[test]
    fn a_duplicate_name_is_refused() {
        let mut contract = two_bones();
        contract.bones[1].name = String::from("Hips");
        let error = contract.validate().expect_err("duplicate");
        assert!(error.to_string().contains("twice"), "{error}");
    }

    #[test]
    fn drift_reports_each_disagreement_once() {
        let contract = two_bones();
        let mut derived: Vec<DerivedBone> = contract
            .bones
            .iter()
            .map(|bone| DerivedBone {
                name: bone.name.clone(),
                parent: bone.parent,
                rest_translation: bone.rest_translation,
                rest_rotation: bone.rest_rotation,
            })
            .collect();
        assert!(check_drift(&contract, &derived).is_empty());
        derived[1].rest_translation[1] += 1e-3;
        derived[1].parent = None;
        let drift = check_drift(&contract, &derived);
        assert_eq!(drift.len(), 2, "{drift:?}");
        assert!(drift[0].contains("parent"), "{drift:?}");
        assert!(drift[1].contains("rest translation [1]"), "{drift:?}");
    }

    #[test]
    fn missing_joints_names_what_a_file_lacks() {
        let contract = two_bones();
        assert_eq!(missing_joints(&contract, &["Hips"]), ["Spine"]);
        assert!(
            missing_joints(&contract, &[String::from("Hips"), String::from("Spine")]).is_empty()
        );
    }

    #[test]
    fn contract_json_round_trips_byte_for_byte() {
        let contract = two_bones();
        let bytes = contract.to_json().expect("serialises");
        assert!(bytes.ends_with(b"}\n"));
        let back = Contract::from_json(&bytes, Path::new("test.json")).expect("parses");
        assert_eq!(back, contract);
        assert_eq!(back.to_json().expect("serialises"), bytes);
    }

    #[test]
    fn sha256_is_lowercase_hex_of_the_bytes() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
