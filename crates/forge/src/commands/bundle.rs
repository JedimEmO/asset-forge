//! `forge bundle`: one body and its clips as a single glb to hand out.
//!
//! The work is [`forge_library::bundle`]; what happens here is the flags, and
//! printing what the record says so the person at the terminal sees the
//! animation names an engine will bind by — which are the clips' names inside
//! their own files, not necessarily the library names that were asked for.

use forge_library::Project;
use forge_library::bundle::{BundleRequest, write};
use forge_library::schema::Actor;

use crate::cli::BundleArgs;
use crate::outcome::Outcome;

/// Merge a body and its clips, write the glb and its record.
pub(crate) fn run(project: &Project, args: &BundleArgs) -> Outcome {
    let bundled = write(
        project,
        &BundleRequest {
            body: args.body.clone(),
            clips: args.clips.clone(),
            out: args.out.clone(),
            motion_scale: args.motion_scale,
            created_by: Actor::parse(&args.created_by),
        },
    )?;
    let record = &bundled.record;
    println!(
        "wrote {} — {} on {}, {} animation(s): {}",
        record.output.path,
        pluralise(record.clips.len()),
        record.body.path,
        record.output.animations.len(),
        record.output.animations.join(", "),
    );
    // Printed even at 1.0, and with its source: "the root travel was not
    // scaled" is as much a decision as scaling it, and a reader who cannot
    // see which number was used cannot tell a measured stride from a
    // forgotten flag.
    println!(
        "root travel scaled by {} ({}) — rotations unchanged",
        record.motion_scale, record.motion_scale_source
    );
    println!("{} bytes, {}", record.output.bytes, record.output.sha256);
    println!(
        "record: {}",
        project
            .rel_to_root(&bundled.record_path)
            .unwrap_or_else(|| bundled.record_path.display().to_string())
    );
    Ok(())
}

/// "1 clip" / "3 clips", because a line that says "1 clips" reads as a bug.
fn pluralise(clips: usize) -> String {
    if clips == 1 {
        String::from("1 clip")
    } else {
        format!("{clips} clips")
    }
}
