//! What the rig contract makes of a spawned model, as named findings.
//!
//! The failure mode this exists for is silent: Bevy binds animation curves by
//! hashing each bone's full name path from the animation root, so a mesh whose
//! rig merely *resembles* the profile's — one inserted root bone, one renamed
//! joint — spawns fine, renders fine, and then holds its rest pose forever with
//! no warning at any log level. Every check here turns one way that can happen
//! into a [`Finding`].
//!
//! # The contract is data
//!
//! Every check takes a [`Contract`] — the profile's `contract.json`, loaded
//! by [`forge_rig`] — rather than a table pasted into source. The bone
//! names, the depth each must sit at, the rest rotations, the stature band,
//! the foot tolerance, the rest-rotation tolerance and the name of the clip
//! the skeleton is bound to all come from the file; nothing here knows what
//! a humanoid is. A project on another profile runs the same checks against
//! its own contract.
//!
//! # Two callers, one set of checks
//!
//! [`crate::rig_check`] spawns a subject into a world of its own and prints
//! the findings; the studio runs them on the model standing on its stage and
//! draws them in the metadata panel. They differ only in how the world got
//! there, so the orchestration is per-caller and everything below [`diagnose`]
//! takes plain data: a slice of [`Bone`], a [`SkinMeasurement`], a
//! [`ClipDiff`]. A check that took an [`App`] could only be tested by spawning
//! one.
//!
//! # The rest pose is read from live transforms
//!
//! [`check_contract_bones`] compares each bone's *rest* rotation against the
//! contract, and what it reads is `Transform` as it stands. That is only the
//! rest pose while no [`AnimationPlayer`] has posed the rig — the same
//! load-bearing order [`RigFrames`](crate::npz_clip::RigFrames) documents, for
//! the same reason. `rig_check` never attaches a player at all; the studio
//! must run [`diagnose`] in `Update` in the very frame the model stands up,
//! before Bevy's animation systems evaluate anything in `PostUpdate`. Run
//! these checks a frame later and every bone the reference clip drives reads
//! as re-posed.

use bevy::{
    animation::AnimationClip,
    mesh::{Mesh, Mesh3d, VertexAttributeValues, skinning::SkinnedMesh},
    prelude::*,
};
use forge_rig::Contract;

use crate::binding::{ClipDiff, SkeletonPaths};

/// How much one finding weighs.
///
/// A note is neither a pass nor a failure: it records something worth knowing
/// that cannot break a clip — an extra leaf bone, say — so it never decides an
/// exit code and never reads as an error. A warning is a check that could
/// not run — no reference clip in the library, say — which is worth saying
/// out loud and is not the mesh's fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// A check that passed.
    Ok,
    /// Advisory: breaks nothing, decides nothing.
    Note,
    /// A check that could not be made.
    Warn,
    /// The contract is not satisfied.
    Fail,
}

impl Severity {
    /// The mark a report line starts with: `ok:`, `note:`, `WARN:`, `FAIL:`.
    #[must_use]
    pub const fn mark(self) -> &'static str {
        match self {
            Self::Ok => "ok:",
            Self::Note => "note:",
            Self::Warn => "WARN:",
            Self::Fail => "FAIL:",
        }
    }
}

/// One thing the contract has to say about a model, said once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// How much it weighs.
    pub severity: Severity,
    /// The whole finding, in one line, naming what is wrong and why it matters.
    pub text: String,
}

impl Finding {
    /// A check that passed.
    #[must_use]
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            severity: Severity::Ok,
            text: text.into(),
        }
    }

    /// A check that failed.
    #[must_use]
    pub fn fail(text: impl Into<String>) -> Self {
        Self {
            severity: Severity::Fail,
            text: text.into(),
        }
    }

    /// Something worth saying that breaks nothing.
    #[must_use]
    pub fn note(text: impl Into<String>) -> Self {
        Self {
            severity: Severity::Note,
            text: text.into(),
        }
    }

    /// A check that could not be made.
    #[must_use]
    pub fn warn(text: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warn,
            text: text.into(),
        }
    }

    /// Whether the contract is satisfied. `true` for a note and a warning:
    /// neither is the mesh failing a check.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.severity != Severity::Fail
    }

    /// Whether this is advisory rather than a check at all.
    #[must_use]
    pub fn is_note(&self) -> bool {
        self.severity == Severity::Note
    }
}

/// One named entity of a spawned hierarchy, as the checks need it.
#[derive(Debug, Clone)]
pub struct Bone {
    /// The name Bevy hashes into this bone's `AnimationTargetId`.
    pub name: String,
    /// Name-path depth: the animation root is 1, so the root bone belongs at 2.
    pub depth: usize,
    /// The name of the entity above it, if that entity is named too.
    pub parent_name: Option<String>,
    /// How many of its children are named — a bone with none is a leaf.
    pub named_children: usize,
    /// Local rotation, which is the rest rotation only while nothing has posed
    /// the rig; see the module docs.
    pub rotation: Quat,
}

/// Every named entity under `anim_root`, depth-first, the root included.
///
/// Unnamed entities end their branch, mirroring how `bevy_gltf` stops building
/// target ids: a bone below one is unaddressable, so it is not a bone.
#[must_use]
pub fn collect_bones(world: &World, anim_root: Entity) -> Vec<Bone> {
    let mut bones = Vec::new();
    walk(world, anim_root, 1, None, &mut bones);
    bones
}

fn walk(world: &World, entity: Entity, depth: usize, parent: Option<&str>, out: &mut Vec<Bone>) {
    let Some(name) = world.get::<Name>(entity) else {
        return;
    };
    let children: Vec<Entity> = world
        .get::<Children>(entity)
        .map(|c| c.iter().collect())
        .unwrap_or_default();
    let named_children = children
        .iter()
        .filter(|&&c| world.get::<Name>(c).is_some())
        .count();
    out.push(Bone {
        name: name.as_str().to_owned(),
        depth,
        parent_name: parent.map(str::to_owned),
        named_children,
        rotation: world
            .get::<Transform>(entity)
            .map_or(Quat::IDENTITY, |t| t.rotation),
    });
    for child in children {
        walk(world, child, depth + 1, Some(name.as_str()), out);
    }
}

/// The name every animation root must carry. It is the first segment of
/// every bone's name path, which is why the rig artifact, the fixture
/// mannequin and every export agree on it.
pub const ARMATURE: &str = forge_rig::fixture::ARMATURE_NODE;

/// The animation root itself: it must be the identity-transformed `Armature`,
/// because its name is the first segment of every hash and its transform
/// would silently scale or shove every clip.
pub fn check_root(name: &str, transform: Option<&Transform>, out: &mut Vec<Finding>) {
    if name == ARMATURE {
        out.push(Finding::ok(format!("animation root is named {ARMATURE}")));
    } else {
        out.push(Finding::fail(format!(
            "animation root is named '{name}', expected '{ARMATURE}' — the root name is the \
             first segment of every AnimationTargetId, so every clip binds to nothing"
        )));
    }
    let identity = transform.is_none_or(|t| {
        t.translation.length() < 1e-4
            && t.rotation.angle_between(Quat::IDENTITY) < 1e-4
            && (t.scale - Vec3::ONE).length() < 1e-4
    });
    if identity {
        out.push(Finding::ok("armature transform is identity"));
    } else {
        out.push(Finding::fail(
            "armature transform is not identity — it would bake a hidden offset or scale \
             into every pose; apply transforms before export",
        ));
    }
}

/// Every contract bone, present exactly once at its contract depth, under its
/// contract parent, resting at its contract rotation. Returns the depth the
/// root bone was actually found at so later checks can anchor to it.
#[must_use]
pub fn check_contract_bones(
    contract: &Contract,
    bones: &[Bone],
    out: &mut Vec<Finding>,
) -> Option<usize> {
    let root = contract.root.as_str();
    let mut missing = 0;
    let mut misplaced = 0;
    let mut reposed = 0;

    let roots: Vec<&Bone> = bones.iter().filter(|n| n.name == root).collect();
    let root_depth = match roots.as_slice() {
        [] => {
            out.push(Finding::fail(format!(
                "{root} missing — nothing can bind without the root bone"
            )));
            missing += 1;
            None
        }
        [one] => {
            if one.depth == 2 {
                out.push(Finding::ok(format!(
                    "{root} found at depth 2, directly under the animation root"
                )));
            } else {
                out.push(Finding::fail(format!(
                    "{root} found at depth {}, expected 2 — a parent above {root} rewrites every \
                     AnimationTargetId and every clip binds to nothing",
                    one.depth
                )));
                misplaced += 1;
            }
            Some(one.depth)
        }
        many => {
            out.push(Finding::fail(format!(
                "{} entities named {root} — bones must be unique",
                many.len()
            )));
            misplaced += 1;
            None
        }
    };

    for (index, spec) in contract.bones.iter().enumerate() {
        let found: Vec<&Bone> = bones.iter().filter(|n| n.name == spec.name).collect();
        let [bone] = found.as_slice() else {
            // The root's absence or duplication was already reported, with
            // the counters bumped, by the match above.
            if spec.name != root {
                if found.is_empty() {
                    out.push(Finding::fail(format!(
                        "contract bone {} missing",
                        spec.name
                    )));
                    missing += 1;
                } else {
                    out.push(Finding::fail(format!(
                        "{} entities named {} — bones must be unique",
                        found.len(),
                        spec.name
                    )));
                    misplaced += 1;
                }
            }
            continue;
        };

        // Depth is measured relative to where the root bone actually sits, so
        // one inserted root reads as one finding, not fifty-five.
        let expected = contract.expected_depth(index);
        let anchored = root_depth.map(|h| expected - 2 + h);
        if spec.name != root && anchored.is_some_and(|want| bone.depth != want) {
            out.push(Finding::fail(format!(
                "{} at name-path depth {}, contract says {expected} — its \
                 AnimationTargetId no longer matches any clip",
                spec.name, bone.depth
            )));
            misplaced += 1;
        }
        if let Some(parent_index) = spec.parent {
            let want = contract.bones[parent_index].name.as_str();
            if bone.parent_name.as_deref() != Some(want) {
                out.push(Finding::fail(format!(
                    "{} is parented to {:?}, contract says {want}",
                    spec.name,
                    bone.parent_name.as_deref().unwrap_or("<none>")
                )));
                misplaced += 1;
            }
        }

        let rest = Quat::from_array(spec.rest_rotation);
        let gap = quat_gap(bone.rotation, rest);
        if gap > contract.rest_rotation_tolerance {
            out.push(Finding::fail(format!(
                "{} rest rotation differs from the contract by {gap:.5} — the rig was re-posed; \
                 skin onto the profile's rig instead of adjusting it",
                spec.name
            )));
            reposed += 1;
        }
    }
    if missing == 0 && misplaced == 0 {
        out.push(Finding::ok(format!(
            "all {} contract bones present at contract depth",
            contract.bones.len()
        )));
    }
    if reposed == 0 {
        out.push(Finding::ok("rest rotations match the contract"));
    }
    root_depth
}

/// Largest per-component gap between two quaternions naming the same
/// rotation, sign-aligned first because `q` and `-q` are the same rotation.
#[must_use]
pub fn quat_gap(a: Quat, b: Quat) -> f32 {
    let b = if a.dot(b) < 0.0 { -b } else { b };
    (Vec4::from(a) - Vec4::from(b)).abs().max_element()
}

/// Names inside the root bone's subtree that the contract does not know.
///
/// What decides a stranger's fate is not whether it has children but *whose*
/// children: target ids hash the name path from the root *down*, so a stranger
/// only rewrites an existing id when a contract bone hangs somewhere beneath
/// it. That is the failure — a renamed or inserted joint, with the rest of the
/// contract dangling off it. A stranger with nothing but strangers below it
/// mints new ids and touches none, so it breaks nothing no matter how deep it
/// goes, and gets a note.
///
/// A lone strange leaf is a marker — a holster, a muzzle point, somewhere to
/// hang a prop — and keeps its own note. A run of strangers hanging off a
/// contract bone reads as one thing rather than four, so it gets one note
/// naming the whole run. The contract's only claim about either is that
/// these bones cannot break a clip; nothing in the library animates them.
pub fn check_unknown_bones(
    contract: &Contract,
    bones: &[Bone],
    root_depth: Option<usize>,
    out: &mut Vec<Finding>,
) {
    let Some(root_depth) = root_depth else {
        return; // no (single) root bone: already failed above
    };
    let root = contract.root.as_str();
    // The root bone's subtree, recovered from the depth-first bone list:
    // everything after it until the walk climbs back to its depth. Mesh nodes
    // hang beside the root bone under the armature, so they never enter this
    // window.
    let mut inside: Vec<&Bone> = Vec::new();
    let mut root_seen = false;
    for bone in bones {
        if bone.name == root {
            root_seen = true;
            continue;
        }
        if root_seen && bone.depth > root_depth {
            inside.push(bone);
        } else if root_seen && bone.depth <= root_depth {
            root_seen = false; // left the subtree
        }
    }

    let mut strangers = 0;
    let mut index = 0;
    while index < inside.len() {
        let bone = inside[index];
        if contract.find(&bone.name).is_some() {
            index += 1;
            continue;
        }
        // This stranger's subtree, read off the same depth-first list: itself
        // and everything after it until the walk climbs back to its own depth.
        let mut end = index + 1;
        while end < inside.len() && inside[end].depth > bone.depth {
            end += 1;
        }
        let subtree = &inside[index..end];

        if subtree
            .iter()
            .any(|below| contract.find(&below.name).is_some())
        {
            strangers += 1;
            out.push(Finding::fail(format!(
                "unknown bone {} inside the hierarchy — a renamed or inserted bone rewrites \
                 the AnimationTargetId of every bone beneath it, and those curves bind to \
                 nothing",
                bone.name
            )));
            // Step into it rather than over it: the contract bones it displaced
            // may carry strangers of their own, and each is its own finding.
            index += 1;
            continue;
        }

        match subtree {
            [leaf] => out.push(Finding::note(format!(
                "extra leaf bone {} — harmless: target ids hash the name path from the root \
                 down, so a leaf adds a new id without changing any existing bone's",
                leaf.name
            ))),
            [first, .., last] => out.push(Finding::note(format!(
                "extra bones {}..{} under {} ({} bones) — harmless: no clip animates them and \
                 no contract bone hangs beneath them",
                first.name,
                last.name,
                first.parent_name.as_deref().unwrap_or("<none>"),
                subtree.len()
            ))),
            // `end` starts past `index`, so the subtree always holds the
            // stranger itself.
            [] => {}
        }
        index = end;
    }
    if strangers == 0 {
        out.push(Finding::ok(format!(
            "no unknown bones between {root} and the leaves"
        )));
    }
}

/// How far a vertex's joint weights may sum from 1.0 before the skin is
/// deforming that vertex by something other than the pose.
///
/// Generous next to float noise (which lands near 1e-7 after the u8/u16
/// quantisation a glTF may apply) and far tighter than any authoring mistake:
/// a vertex that lost an influence sums to 0.7, not to 0.999.
const WEIGHT_SUM_TOLERANCE: f32 = 1e-3;

/// What the skinned meshes measure: their weights and their vertical extent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkinMeasurement {
    /// Vertices with a non-zero weight on any joint.
    pub weighted: usize,
    /// Vertices whose weights do not sum to 1.0.
    ///
    /// Measured but not reported: the body checks' finding counts are
    /// pinned, and a new line in that list would be a schema change to what
    /// `rig check` says about every shipped mesh.
    pub unnormalized: usize,
    /// Lowest vertex, metres; infinite when no positions were readable.
    pub lowest: f32,
    /// Highest vertex, metres; infinite when no positions were readable.
    pub highest: f32,
}

/// Measure every skinned mesh in `world`, or `None` if there is not one.
///
/// Positions are bind-pose values in armature space, which the identity-armature
/// check in [`check_root`] makes world space. Every skinned mesh counts, without
/// asking which model it belongs to: both callers put exactly one on the stage,
/// and a second would be a bug worth measuring rather than hiding.
#[must_use]
pub fn measure_skins(world: &World) -> Option<SkinMeasurement> {
    let handles: Vec<Handle<Mesh>> = world
        .iter_entities()
        .filter(EntityRef::contains::<SkinnedMesh>)
        .filter_map(|entity| entity.get::<Mesh3d>().map(|mesh| mesh.0.clone()))
        .collect();
    if handles.is_empty() {
        return None;
    }

    let meshes = world.resource::<Assets<Mesh>>();
    let mut measured = SkinMeasurement {
        weighted: 0,
        unnormalized: 0,
        lowest: f32::INFINITY,
        highest: f32::NEG_INFINITY,
    };
    for handle in &handles {
        let Some(mesh) = meshes.get(handle) else {
            continue;
        };
        if let Some(VertexAttributeValues::Float32x4(weights)) =
            mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT)
        {
            measured.weighted += weights
                .iter()
                .filter(|w| w.iter().any(|&value| value > 0.0))
                .count();
            // Counted over the whole vertex, not per influence: glTF stores
            // exactly four slots, so "at most four influences" is true by the
            // format and the only thing left to be wrong is the sum.
            measured.unnormalized += weights
                .iter()
                .filter(|w| {
                    let sum: f32 = w.iter().sum();
                    (sum - 1.0).abs() > WEIGHT_SUM_TOLERANCE
                })
                .count();
        }
        if let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        {
            for p in positions {
                measured.lowest = measured.lowest.min(p[1]);
                measured.highest = measured.highest.max(p[1]);
            }
        }
    }
    Some(measured)
}

/// The skin, the stature and the feet — the band and the tolerance from the
/// contract.
///
/// `None` means the file holds no skinned mesh at all, which is one finding and
/// not four: with no skin there is nothing to weigh or measure.
pub fn check_skin(contract: &Contract, measured: Option<SkinMeasurement>, out: &mut Vec<Finding>) {
    let Some(measured) = measured else {
        out.push(Finding::fail(
            "no skinned mesh — nothing in the file binds vertices to the rig",
        ));
        return;
    };

    if measured.weighted == 0 {
        out.push(Finding::fail(
            "the skin weights every vertex to nothing — was Armature Deform applied?",
        ));
    } else {
        out.push(Finding::ok(format!(
            "skin present, {} weighted vertices",
            measured.weighted
        )));
    }

    let band = contract.stature_m;
    if measured.lowest.is_finite() && measured.highest.is_finite() {
        let height = measured.highest - measured.lowest;
        if (band.min..=band.max).contains(&height) {
            out.push(Finding::ok(format!(
                "height {height:.2} m, within {}-{} m",
                band.min, band.max
            )));
        } else {
            out.push(Finding::fail(format!(
                "height {height:.2} m, expected {}-{} m (contract reference is a {} m human)",
                band.min, band.max, band.reference
            )));
        }
        if measured.lowest.abs() <= contract.foot_tolerance_m {
            out.push(Finding::ok(format!("feet at y={:.3} m", measured.lowest)));
        } else {
            out.push(Finding::fail(format!(
                "lowest vertex at y={:.3} m — feet must rest on y=0, or every clip \
                 hovers or sinks the character",
                measured.lowest
            )));
        }
    } else {
        out.push(Finding::fail(
            "the skinned mesh has no vertex positions to measure",
        ));
    }
}

/// The one check that asks the engine instead of the contract: what a reference
/// clip's curve targets actually found on this skeleton.
///
/// The contract's reference clip drives every driven bone, so the bar is the
/// contract's driven count; anything short of it is a bone that will hold its
/// rest pose through every clip in the library. Zero is the total, silent
/// mismatch the module exists for and is named as such.
pub fn check_clip_binding(contract: &Contract, diff: &ClipDiff, out: &mut Vec<Finding>) {
    let driven = contract.driven().count();
    let line = format!(
        "reference clip: {} bone(s) driven, {} at rest, {} orphaned curve(s)",
        diff.bound.len(),
        diff.unbound.len(),
        diff.orphaned
    );
    if diff.bound.is_empty() {
        out.push(Finding::fail(format!(
            "{line} — nothing binds: the clip's bone names miss this skeleton entirely, and \
             the engine reports nothing; the character would hold its rest pose"
        )));
    } else if diff.bound.len() < driven {
        out.push(Finding::fail(format!(
            "{line} — every clip in the library drives all {driven} {} joints",
            contract.driven_layout
        )));
    } else {
        out.push(Finding::ok(line));
    }
}

/// Every finding about the model standing at `anim_root`, in report order.
///
/// `anim_root` is the entity [`find_animation_root`](crate::stage::find_animation_root)
/// picked, with animation targets already installed. `reference` is a clip to
/// diff the skeleton against — one that is *already loaded*, because a
/// diagnosis has no business starting an asset load — and `None` simply leaves
/// that finding out: a caller for whom a missing reference is itself a defect
/// says so in its own words.
///
/// Read the module docs before calling this on a rig that has been posed: the
/// rest-rotation check reads live transforms.
#[must_use]
pub fn diagnose(
    world: &World,
    anim_root: Entity,
    contract: &Contract,
    reference: Option<&AnimationClip>,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let name = world
        .get::<Name>(anim_root)
        .map_or_else(String::new, |n| n.as_str().to_owned());
    check_root(&name, world.get::<Transform>(anim_root), &mut out);

    let bones = collect_bones(world, anim_root);
    let root_depth = check_contract_bones(contract, &bones, &mut out);
    check_unknown_bones(contract, &bones, root_depth, &mut out);
    check_skin(contract, measure_skins(world), &mut out);
    if let Some(clip) = reference {
        let paths = SkeletonPaths::from_world(world, anim_root);
        check_clip_binding(contract, &ClipDiff::new(clip, &paths), &mut out);
    }
    out
}

// ----------------------------------------------------------- in the studio ---

/// What the contract makes of the model on the studio's stage.
///
/// Rebuilt from scratch every time a rig stands up, so it always describes what
/// is standing rather than accumulating across swaps. Empty until the first one
/// does — see [`RigFindings::generation`].
///
/// **Order is load-bearing** for whoever fills this: [`diagnose`] must run in
/// `Update`, in the same frame the model finished loading and before anything
/// poses it, because the rest-rotation check reads live transforms. See the
/// module docs.
#[derive(Resource, Debug, Default)]
pub struct RigFindings {
    /// Every finding, in report order.
    pub findings: Vec<Finding>,
    /// The stage generation these were read at; 0 means no model has been
    /// diagnosed yet. Watch it to redraw when the stage changes.
    pub generation: u64,
}

impl RigFindings {
    /// Replace the findings with what a freshly stood-up model says.
    pub fn record(&mut self, findings: Vec<Finding>, generation: u64) {
        self.findings = findings;
        self.generation = generation;
    }

    /// Every finding as the panel draws it: its weight and its line.
    pub fn lines(&self) -> impl Iterator<Item = (Severity, &str)> {
        self.findings
            .iter()
            .map(|finding| (finding.severity, finding.text.as_str()))
    }

    /// Whether the model on the stage satisfies the contract outright — every
    /// check passed, notes and warnings aside.
    ///
    /// At least one real check must have run: a stage that produced only
    /// notes — a static model, say — made no claim, and "conforms to the rig
    /// contract" over a barrel would be an invented pass.
    #[must_use]
    pub fn conforms(&self) -> bool {
        self.generation > 0
            && self.findings.iter().any(|finding| !finding.is_note())
            && self.findings.iter().all(Finding::passed)
    }

    /// The checks that failed, in report order.
    pub fn failures(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|finding| !finding.passed())
    }
}

/// Say once, in the log, everything the contract holds against this model.
///
/// One block rather than one line per finding: a headless run is read after the
/// fact, and findings scattered through a frame's worth of other logging cannot
/// be reassembled into "what is wrong with this mesh". Silence when everything
/// passed — including for a note, which is not a fault.
pub fn log_failures(model: &str, contract: &Contract, findings: &[Finding]) {
    let failed: Vec<&str> = findings
        .iter()
        .filter(|finding| !finding.passed())
        .map(|finding| finding.text.as_str())
        .collect();
    if failed.is_empty() {
        return;
    }
    warn!(
        "{model} does not conform to the {} v{} contract.\nBevy binds animation curves to bones \
         by hashed name path and reports nothing when they miss,\nso a mesh that deviates from \
         the contract simply holds its rest pose. The findings:\n  {}",
        contract.name,
        contract.version,
        failed.join("\n  ")
    );
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// The toolkit's own humanoid contract: the data every check reads.
    fn contract() -> Contract {
        Contract::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/humanoid/contract.json"),
        )
        .expect("the humanoid contract")
    }

    /// A hierarchy that conforms: every contract bone at its contract depth,
    /// under its contract parent, at its contract rest rotation.
    ///
    /// Built from the contract itself rather than from a spawned `.glb`, which
    /// is the point — these checks take plain data, so the fixture is data.
    fn conforming(contract: &Contract) -> Vec<Bone> {
        contract
            .bones
            .iter()
            .enumerate()
            .map(|(index, spec)| Bone {
                name: spec.name.clone(),
                depth: contract.expected_depth(index),
                parent_name: spec.parent.map(|p| contract.bones[p].name.clone()),
                named_children: contract
                    .bones
                    .iter()
                    .filter(|other| other.parent == Some(index))
                    .count(),
                rotation: Quat::from_array(spec.rest_rotation),
            })
            .collect()
    }

    fn messages(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|f| f.text.as_str()).collect()
    }

    fn failures(findings: &[Finding]) -> Vec<&str> {
        findings
            .iter()
            .filter(|f| !f.passed())
            .map(|f| f.text.as_str())
            .collect()
    }

    #[test]
    fn a_conforming_rig_produces_two_findings_and_no_failures() {
        let contract = contract();
        let mut out = Vec::new();
        let hips = check_contract_bones(&contract, &conforming(&contract), &mut out);

        assert_eq!(hips, Some(2));
        assert_eq!(
            messages(&out),
            vec![
                "Hips found at depth 2, directly under the animation root",
                "all 55 contract bones present at contract depth",
                "rest rotations match the contract",
            ]
        );
    }

    /// The single most valuable property of the depth check: a rig with one
    /// bone inserted above `Hips` is *one* finding. Measuring every bone
    /// against an absolute depth instead would bury the cause under
    /// fifty-four consequences, and the artist would go looking in the wrong
    /// place.
    #[test]
    fn one_inserted_root_reads_as_one_finding() {
        let contract = contract();
        let mut bones = conforming(&contract);
        for bone in &mut bones {
            bone.depth += 1;
        }
        let mut out = Vec::new();
        let hips = check_contract_bones(&contract, &bones, &mut out);

        assert_eq!(hips, Some(3));
        assert_eq!(
            failures(&out),
            vec![
                "Hips found at depth 3, expected 2 — a parent above Hips rewrites every \
                 AnimationTargetId and every clip binds to nothing"
            ]
        );
    }

    /// A missing bone and a renamed one are different problems: the first is
    /// gone, the second is gone *and* has taken a stranger's place, so a rename
    /// must not read as a single absence.
    #[test]
    fn a_missing_bone_is_named_and_a_duplicate_is_not_mistaken_for_one() {
        let contract = contract();
        let mut bones = conforming(&contract);
        bones.retain(|bone| bone.name != "LeftFoot");
        let mut out = Vec::new();
        let _ = check_contract_bones(&contract, &bones, &mut out);
        assert_eq!(failures(&out), vec!["contract bone LeftFoot missing"]);

        let mut bones = conforming(&contract);
        let spine = bones
            .iter()
            .find(|bone| bone.name == "Spine")
            .expect("Spine")
            .clone();
        bones.push(spine);
        let mut out = Vec::new();
        let _ = check_contract_bones(&contract, &bones, &mut out);
        assert_eq!(
            failures(&out),
            vec!["2 entities named Spine — bones must be unique"]
        );
    }

    /// A re-posed rig binds perfectly and animates wrongly, which is the worst
    /// kind of defect: nothing in the engine complains. Half a degree is the
    /// scale of an accidental nudge in Blender.
    #[test]
    fn a_bone_nudged_off_the_rest_pose_is_caught() {
        let contract = contract();
        let mut bones = conforming(&contract);
        let neck = bones
            .iter_mut()
            .find(|bone| bone.name == "Neck")
            .expect("Neck");
        neck.rotation *= Quat::from_rotation_x(0.5_f32.to_radians());

        let mut out = Vec::new();
        let _ = check_contract_bones(&contract, &bones, &mut out);
        let reposed = failures(&out);
        assert_eq!(reposed.len(), 1, "{reposed:?}");
        assert!(
            reposed[0].starts_with("Neck rest rotation differs from the contract by"),
            "{reposed:?}"
        );
        // Exporter float noise must not read as a re-pose.
        let mut bones = conforming(&contract);
        bones[0].rotation.x += 1e-6;
        let mut out = Vec::new();
        let _ = check_contract_bones(&contract, &bones, &mut out);
        assert!(failures(&out).is_empty());
    }

    /// The tolerance is the contract's, not a constant: tighten it and the
    /// same float noise becomes a re-pose.
    #[test]
    fn the_rest_rotation_tolerance_comes_from_the_contract() {
        let mut contract = contract();
        let mut bones = conforming(&contract);
        bones[0].rotation.x += 1e-5;
        let mut out = Vec::new();
        let _ = check_contract_bones(&contract, &bones, &mut out);
        assert!(failures(&out).is_empty());

        contract.rest_rotation_tolerance = 1e-6;
        let mut out = Vec::new();
        let _ = check_contract_bones(&contract, &bones, &mut out);
        assert_eq!(failures(&out).len(), 1, "{:?}", messages(&out));
    }

    /// `q` and `-q` are the same rotation, and a `.glb` may store either.
    #[test]
    fn the_rest_rotation_gap_ignores_quaternion_sign() {
        let q = Quat::from_rotation_y(1.2);
        assert!(quat_gap(q, -q) < 1e-6);
        assert!(quat_gap(q, Quat::from_rotation_y(1.2 + 0.05)) > 1e-3);
    }

    /// A stranger with children rewrites every path beneath it; a strange leaf
    /// cannot break anything. Both have to be said, and only one may fail.
    #[test]
    fn an_inserted_bone_fails_while_an_extra_leaf_only_notes() {
        let contract = contract();
        let bone = |name: &str, depth: usize, parent: &str, children: usize| Bone {
            name: name.to_owned(),
            depth,
            parent_name: Some(parent.to_owned()),
            named_children: children,
            rotation: Quat::IDENTITY,
        };
        let bones = vec![
            bone("Hips", 2, "Armature", 1),
            bone("Spine", 3, "Hips", 1),
            // A bone the contract does not know, with the rest of the spine
            // hanging off it.
            bone("Spine1_extra", 4, "Spine", 1),
            bone("Spine2", 5, "Spine1_extra", 1),
            bone("Neck", 6, "Spine2", 0),
            bone("Ponytail", 6, "Spine2", 0),
        ];

        let mut out = Vec::new();
        check_unknown_bones(&contract, &bones, Some(2), &mut out);

        assert_eq!(
            failures(&out),
            vec![
                "unknown bone Spine1_extra inside the hierarchy — a renamed or inserted bone \
                 rewrites the AnimationTargetId of every bone beneath it, and those curves bind \
                 to nothing"
            ]
        );
        let notes: Vec<&str> = out
            .iter()
            .filter(|f| f.is_note())
            .map(|f| f.text.as_str())
            .collect();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(
            notes[0].starts_with("extra leaf bone Ponytail"),
            "{notes:?}"
        );
    }

    /// A run of strangers hanging off a contract bone must read as one note
    /// rather than four oddities — and it must not fail, because nothing
    /// above it moved.
    ///
    /// The negative half is the whole reason the check is subtree-aware: put a
    /// contract bone under the same run and the identical bones become the
    /// failure they always were.
    #[test]
    fn a_stranger_chain_is_one_note_and_a_contract_bone_beneath_it_is_a_failure() {
        let contract = contract();
        let bone = |name: &str, depth: usize, parent: &str, children: usize| Bone {
            name: name.to_owned(),
            depth,
            parent_name: Some(parent.to_owned()),
            named_children: children,
            rotation: Quat::IDENTITY,
        };
        let chain = |leaf_children: usize| {
            vec![
                bone("Hips", 2, "Armature", 1),
                bone("Spine", 3, "Hips", 1),
                bone("Head", 4, "Spine", 2),
                bone("Ponytail1", 5, "Head", 1),
                bone("Ponytail2", 6, "Ponytail1", 1),
                bone("Ponytail3", 7, "Ponytail2", 1),
                bone("Ponytail4", 8, "Ponytail3", leaf_children),
                // A marker bone beside the chain: one bone is not a chain.
                bone("Holster", 5, "Head", 0),
            ]
        };

        let mut out = Vec::new();
        check_unknown_bones(&contract, &chain(0), Some(2), &mut out);
        assert!(failures(&out).is_empty(), "{:?}", messages(&out));
        let notes: Vec<&str> = out
            .iter()
            .filter(|f| f.is_note())
            .map(|f| f.text.as_str())
            .collect();
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(
            notes[0].starts_with("extra bones Ponytail1..Ponytail4 under Head (4 bones)"),
            "{notes:?}"
        );
        assert!(notes[1].starts_with("extra leaf bone Holster"), "{notes:?}");

        // The same four bones, with a contract bone hanging off the end. The
        // run's note is gone and every stranger standing over `Neck` is
        // named, because each of them rewrote its path.
        let mut bones = chain(1);
        bones.insert(7, bone("Neck", 9, "Ponytail4", 0));
        let mut out = Vec::new();
        check_unknown_bones(&contract, &bones, Some(2), &mut out);
        let failed = failures(&out);
        assert_eq!(
            failed.first().copied(),
            Some(
                "unknown bone Ponytail1 inside the hierarchy — a renamed or inserted bone \
                 rewrites the AnimationTargetId of every bone beneath it, and those curves \
                 bind to nothing"
            ),
            "{failed:?}"
        );
        assert_eq!(failed.len(), 4, "{failed:?}");
        assert!(
            !out.iter().any(|f| f.text.starts_with("extra bones")),
            "{:?}",
            messages(&out)
        );
    }

    /// A mesh node sits beside `Hips` under the armature, not inside it, and
    /// must not be reported as an inserted bone.
    #[test]
    fn a_sibling_of_hips_is_not_inside_the_hierarchy() {
        let contract = contract();
        let bones = vec![
            Bone {
                name: String::from("Hips"),
                depth: 2,
                parent_name: Some(String::from("Armature")),
                named_children: 0,
                rotation: Quat::IDENTITY,
            },
            Bone {
                name: String::from("Body"),
                depth: 2,
                parent_name: Some(String::from("Armature")),
                named_children: 0,
                rotation: Quat::IDENTITY,
            },
        ];
        let mut out = Vec::new();
        check_unknown_bones(&contract, &bones, Some(2), &mut out);
        assert_eq!(
            messages(&out),
            vec!["no unknown bones between Hips and the leaves"]
        );
    }

    /// The bare rig artifact is a legitimate thing to put on a stage: it
    /// has no skin, and that is one finding rather than a cascade of four.
    #[test]
    fn a_rig_with_no_skin_is_one_finding() {
        let mut out = Vec::new();
        check_skin(&contract(), None, &mut out);
        assert_eq!(
            messages(&out),
            vec!["no skinned mesh — nothing in the file binds vertices to the rig"]
        );
    }

    #[test]
    fn the_skin_is_judged_on_weights_stature_and_where_the_feet_are() {
        let contract = contract();
        let mut out = Vec::new();
        check_skin(
            &contract,
            Some(SkinMeasurement {
                weighted: 552,
                unnormalized: 0,
                lowest: 0.0,
                highest: 1.8,
            }),
            &mut out,
        );
        assert_eq!(
            messages(&out),
            vec![
                "skin present, 552 weighted vertices",
                "height 1.80 m, within 1.4-2.2 m",
                "feet at y=0.000 m",
            ]
        );

        // A prop-sized mesh, unweighted, hovering.
        let mut out = Vec::new();
        check_skin(
            &contract,
            Some(SkinMeasurement {
                weighted: 0,
                unnormalized: 0,
                lowest: 0.4,
                highest: 1.0,
            }),
            &mut out,
        );
        assert_eq!(failures(&out).len(), 3, "{:?}", messages(&out));

        // A skin whose positions could not be read at all.
        let mut out = Vec::new();
        check_skin(
            &contract,
            Some(SkinMeasurement {
                weighted: 10,
                unnormalized: 0,
                lowest: f32::INFINITY,
                highest: f32::NEG_INFINITY,
            }),
            &mut out,
        );
        assert_eq!(
            failures(&out),
            vec!["the skinned mesh has no vertex positions to measure"]
        );
    }

    /// The band and the foot tolerance are the contract's: narrow them and a
    /// body that passed stops passing, with the new numbers in the line.
    #[test]
    fn the_stature_band_and_foot_tolerance_come_from_the_contract() {
        let mut contract = contract();
        contract.stature_m.min = 1.9;
        contract.foot_tolerance_m = 0.001;
        let mut out = Vec::new();
        check_skin(
            &contract,
            Some(SkinMeasurement {
                weighted: 552,
                unnormalized: 0,
                lowest: 0.01,
                highest: 1.81,
            }),
            &mut out,
        );
        assert_eq!(
            failures(&out),
            vec![
                "height 1.80 m, expected 1.9-2.2 m (contract reference is a 1.8 m human)",
                "lowest vertex at y=0.010 m — feet must rest on y=0, or every clip hovers or \
                 sinks the character",
            ]
        );
    }

    /// The root's name is the first segment of every `AnimationTargetId`, so a
    /// renamed armature is total, silent breakage — and a scaled one bakes an
    /// offset into every pose.
    #[test]
    fn the_armature_must_be_named_and_untransformed() {
        let mut out = Vec::new();
        check_root("Armature", Some(&Transform::IDENTITY), &mut out);
        assert!(failures(&out).is_empty(), "{:?}", messages(&out));

        let mut out = Vec::new();
        check_root(
            "rig",
            Some(&Transform::from_scale(Vec3::splat(0.01))),
            &mut out,
        );
        assert_eq!(failures(&out).len(), 2, "{:?}", messages(&out));
        assert!(failures(&out)[0].contains("expected 'Armature'"));
    }

    /// Binding is judged against the contract's driven count, and nothing
    /// bound is its own, louder finding.
    #[test]
    fn binding_is_held_to_the_driven_count_and_zero_is_named() {
        let contract = contract();
        let diff = |bound: usize| ClipDiff {
            bound: (0..bound).map(|i| format!("Armature/B{i}")).collect(),
            unbound: Vec::new(),
            orphaned: 27 - bound,
        };
        let mut out = Vec::new();
        check_clip_binding(&contract, &diff(27), &mut out);
        assert_eq!(
            messages(&out),
            vec!["reference clip: 27 bone(s) driven, 0 at rest, 0 orphaned curve(s)"]
        );

        let mut out = Vec::new();
        check_clip_binding(&contract, &diff(20), &mut out);
        assert_eq!(failures(&out).len(), 1, "{:?}", messages(&out));
        assert!(failures(&out)[0].contains("all 27 cskel27 joints"));

        let mut out = Vec::new();
        check_clip_binding(&contract, &diff(0), &mut out);
        assert!(
            failures(&out)[0].starts_with("reference clip: 0 bone(s) driven"),
            "{:?}",
            messages(&out)
        );
        assert!(failures(&out)[0].contains("nothing binds"));
    }

    /// The resource is what the panel reads, so "nothing has stood up yet" and
    /// "what stood up conforms" must not be the same answer.
    #[test]
    fn an_undiagnosed_stage_does_not_claim_to_conform() {
        let mut findings = RigFindings::default();
        assert!(!findings.conforms());
        assert_eq!(findings.generation, 0);
        assert_eq!(findings.lines().count(), 0);

        findings.record(
            vec![Finding::ok(
                "all 55 contract bones present at contract depth",
            )],
            1,
        );
        assert!(findings.conforms());

        findings
            .findings
            .push(Finding::note("extra leaf bone Tail"));
        assert!(findings.conforms(), "a note is not a failure");
        findings
            .findings
            .push(Finding::warn("no reference clip in the library"));
        assert!(findings.conforms(), "a warning is not a failure");

        findings.findings.push(Finding::fail("Hips missing"));
        assert!(!findings.conforms());
        assert_eq!(findings.failures().count(), 1);
        let lines: Vec<(Severity, &str)> = findings.lines().collect();
        assert_eq!(lines[3], (Severity::Fail, "Hips missing"));
        assert_eq!(Severity::Fail.mark(), "FAIL:");
        assert_eq!(Severity::Warn.mark(), "WARN:");
    }
}
