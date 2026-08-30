//! Write a profile's `contract.json` from its `rig.glb`.
//!
//! ```sh
//! cargo run -p forge_rig --example export_contract -- rigs/humanoid
//! ```
//!
//! Reads `rig.glb` and `motion_skeleton.json` from the directory, hashes
//! `rig.blend` when it is there, and writes `contract.json` beside them with
//! the scalars below — the numbers the humanoid profile gates on, stated
//! once here and mirrored in `profile.toml`. An unchanged rig reproduces the
//! file byte for byte; a changed one shows up as a diff in the bones.
//!
//! The `forge rig export-contract` subcommand is this with the scalars on
//! flags.

use std::{path::PathBuf, process::ExitCode};

use forge_rig::{CONTRACT_FILE, MOTION_SKELETON_FILE, MotionSkeleton, Stature, export};

/// The humanoid profile's scalars: 1.80 m reference stature with a 1.4–2.2 m
/// band, feet within 5 cm of the ground, a 1e-3 rest-rotation tolerance, the
/// walk as the binding reference, and the four a fitted skeleton is held to —
/// a bone's rest translation may not turn more than a degree, its length may
/// land between 0.4 and 2.5 of the contract's, and the planted foot's own
/// lowest vertex stays within 5 cm of the floor on a contact frame.
fn humanoid(dir: &std::path::Path) -> export::Options {
    export::Options {
        name: String::from("humanoid"),
        version: 1,
        front: String::from("+Z"),
        stature_m: Stature {
            reference: 1.8,
            min: 1.4,
            max: 2.2,
        },
        foot_tolerance_m: 0.05,
        rest_rotation_tolerance: 1e-3,
        rest_direction_tolerance_deg: 1.0,
        length_ratio_min: 0.4,
        length_ratio_max: 2.5,
        contact_foot_tolerance_m: 0.05,
        reference_clip: String::from("walk"),
        glb: String::from("rig.glb"),
        blend: dir
            .join("rig.blend")
            .is_file()
            .then(|| String::from("rig.blend")),
    }
}

fn main() -> ExitCode {
    let Some(dir) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: export_contract <profile dir>");
        return ExitCode::from(2);
    };
    let motion = match MotionSkeleton::load(&dir.join(MOTION_SKELETON_FILE)) {
        Ok(motion) => motion,
        Err(e) => {
            eprintln!("export_contract: {e}");
            return ExitCode::from(1);
        }
    };
    let out = dir.join(CONTRACT_FILE);
    match export::write(&dir, &motion, &humanoid(&dir), &out) {
        Ok(contract) => {
            let driven = contract.driven().count();
            println!(
                "export_contract: {} bones ({driven} driven by {}), root {}, glb {} -> {}",
                contract.bones.len(),
                contract.driven_layout,
                contract.root,
                &contract.sources.glb_sha256[..12],
                out.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("export_contract: {e}");
            ExitCode::from(1)
        }
    }
}
