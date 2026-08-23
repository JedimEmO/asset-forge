//! Write a contract from a rig artifact.
//!
//! `forge rig export-contract` is this module with a command line on it: read
//! the profile's `rig.glb`, derive the bones, mark as driven the ones the
//! profile's `motion_skeleton.json` names, hash the artifacts, attach the
//! scalars the profile gates on, and print the result with
//! [`Contract::to_json`]. Run twice on an unchanged profile it writes the same
//! bytes, which is what lets a test hold the committed file to the artifact.

use std::path::Path;

use crate::{
    Bone, Contract, MotionSkeleton, Result, RigError, SCHEMA, Sources, Stature, derive_from_glb,
    sha256_file, sha256_hex,
};

/// The scalars a contract carries that cannot be read out of the artifact:
/// the profile's name and version, the stature band, the tolerances, and
/// what the bones were derived from.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// [`Contract::name`].
    pub name: String,
    /// [`Contract::version`].
    pub version: u32,
    /// [`Contract::front`].
    pub front: String,
    /// [`Contract::stature_m`].
    pub stature_m: Stature,
    /// [`Contract::foot_tolerance_m`].
    pub foot_tolerance_m: f32,
    /// [`Contract::rest_rotation_tolerance`].
    pub rest_rotation_tolerance: f32,
    /// [`Contract::reference_clip`].
    pub reference_clip: String,
    /// The rig glb, relative to the profile directory.
    pub glb: String,
    /// The rig blend, relative to the profile directory, if the profile ships
    /// one. Hashed into the contract when present on disk.
    pub blend: Option<String>,
}

impl Options {
    /// The scalars an existing contract was written with, so re-exporting a
    /// profile keeps every field the artifact does not decide.
    #[must_use]
    pub fn from_contract(contract: &Contract) -> Self {
        Self {
            name: contract.name.clone(),
            version: contract.version,
            front: contract.front.clone(),
            stature_m: contract.stature_m,
            foot_tolerance_m: contract.foot_tolerance_m,
            rest_rotation_tolerance: contract.rest_rotation_tolerance,
            reference_clip: contract.reference_clip.clone(),
            glb: contract.sources.glb.clone(),
            blend: contract.sources.blend.clone(),
        }
    }
}

/// Build a contract for the profile at `dir`: bones from the glb the options
/// name, driven flags from `motion`, hashes from the files as they are.
///
/// The root is whichever derived bone has no parent; the driven layout's
/// root must be that bone, and every layout joint must be a bone of the
/// artifact — a layout that names a bone the rig lacks is not this rig's
/// layout.
///
/// # Errors
///
/// The glb is unreadable or not a rig ([`derive_from_glb`]); the artifact
/// has no rootless bone or more than one; a layout joint is missing from it;
/// the resulting contract fails [`Contract::validate`].
pub fn build(dir: &Path, motion: &MotionSkeleton, options: &Options) -> Result<Contract> {
    let glb_path = dir.join(&options.glb);
    let glb_bytes = std::fs::read(&glb_path).map_err(|source| RigError::Io {
        path: glb_path.clone(),
        source,
    })?;
    let derived = derive_from_glb(&glb_bytes)?;

    let bones: Vec<Bone> = derived
        .bones
        .into_iter()
        .map(|bone| Bone {
            driven: motion.index_of(&bone.name).is_some(),
            name: bone.name,
            parent: bone.parent,
            rest_translation: bone.rest_translation,
            rest_rotation: bone.rest_rotation,
        })
        .collect();

    let roots: Vec<&Bone> = bones.iter().filter(|bone| bone.parent.is_none()).collect();
    let [root] = roots.as_slice() else {
        return Err(RigError::Hierarchy(format!(
            "{} rootless bones in {}; a rig has exactly one",
            roots.len(),
            options.glb
        )));
    };
    if let Some(joint) = motion
        .joints
        .iter()
        .find(|joint| !bones.iter().any(|bone| &bone.name == *joint))
    {
        return Err(RigError::Invalid(format!(
            "{} joint {joint:?} is not a bone of {}",
            motion.name, options.glb
        )));
    }
    if motion.joints[motion.root] != root.name {
        return Err(RigError::Invalid(format!(
            "{}'s root is {} but the rig's is {}",
            motion.name, motion.joints[motion.root], root.name
        )));
    }

    let blend_sha256 = match &options.blend {
        Some(blend) => Some(sha256_file(&dir.join(blend))?),
        None => None,
    };

    let contract = Contract {
        schema: SCHEMA,
        name: options.name.clone(),
        version: options.version,
        root: root.name.clone(),
        front: options.front.clone(),
        stature_m: options.stature_m,
        foot_tolerance_m: options.foot_tolerance_m,
        rest_rotation_tolerance: options.rest_rotation_tolerance,
        driven_layout: motion.name.clone(),
        reference_clip: options.reference_clip.clone(),
        sources: Sources {
            glb: options.glb.clone(),
            glb_sha256: sha256_hex(&glb_bytes),
            blend: options.blend.clone(),
            blend_sha256,
        },
        bones,
    };
    contract.validate()?;
    Ok(contract)
}

/// [`build`] the contract and write it to `out` as [`Contract::to_json`]
/// bytes.
///
/// # Errors
///
/// As [`build`], plus `out` being unwritable.
pub fn write(
    dir: &Path,
    motion: &MotionSkeleton,
    options: &Options,
    out: &Path,
) -> Result<Contract> {
    let contract = build(dir, motion, options)?;
    std::fs::write(out, contract.to_json()?).map_err(|source| RigError::Io {
        path: out.to_path_buf(),
        source,
    })?;
    Ok(contract)
}
