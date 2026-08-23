//! `forge promote`: the four doors into the library, all direct.
//!
//! There is no review queue; a promote writes the library now and refuses
//! an existing name unless told `--overwrite`. Every door does the same
//! three things here — read the records named on the command line, build the
//! library's request, print what shipped — and the clip door does one more:
//! it resolves the recipe.
//!
//! # The recipe a clip bakes with
//!
//! When the name already exists the shipped clip's recorded recipe is the
//! starting point and the flags stated here land on top of it, so an
//! `--overwrite` that only says `--lean 6` keeps the trims the clip already
//! had. The **whole** effective recipe is echoed before the bake, every knob
//! stated, and on an overwrite the recipe it replaced is printed beside it —
//! because the failure this guards against was a promote that looked
//! plausible right up until somebody watched it: an argv missing `--no-loop`
//! once produced a clip built to somebody else's recipe. A new name starts
//! from the identity recipe, and the echo says so.

use std::path::Path;

use forge_library::generator_record::RecordKind;
use forge_library::promote::{
    PromoteAudio, PromoteBody, PromoteClip, PromoteModel, Promoted, promote_audio, promote_body,
    promote_clip, promote_model,
};
use forge_library::schema::{Actor, AnimEvent, AudioRef, ClipRecipe, EventOrigin, PartialRecipe};
use forge_library::{Catalog, GeneratorRecord, Kind, Project};

use crate::cli::{
    PromoteAudioArgs, PromoteBodyArgs, PromoteClipArgs, PromoteDoor, PromoteModelArgs,
};
use crate::commands::catalog::parse_kind;
use crate::outcome::{Failure, Outcome};

/// Dispatch one door.
pub(crate) fn run(project: &Project, door: &PromoteDoor) -> Outcome {
    match door {
        PromoteDoor::Clip(args) => clip(project, args),
        PromoteDoor::Body(args) => body(project, args),
        PromoteDoor::Model(args) => model(project, args),
        PromoteDoor::Audio(args) => audio(project, args),
    }
}

// ------------------------------------------------------------------ clips ---

/// Bake a take into a clip and file it.
fn clip(project: &Project, args: &PromoteClipArgs) -> Outcome {
    let name = forge_library::promote::validate_name(&args.name)?;
    let catalog = Catalog::scan(project);
    let shipped = catalog.resolve(&name, Some(Kind::Clip));
    let base = shipped.and_then(|r| r.sidecar.as_ref()?.recipe.clone());

    if let Some(spec) = args
        .retime
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        forge_library::schema::parse_retime(spec)?;
    }
    let recipe = effective_recipe(base.as_ref(), args);

    if let Some(shipped) = shipped {
        println!(
            "{name} already exists as {}; {}",
            shipped.rel_path,
            if base.is_some() {
                "the knobs not stated here were read from its recipe, not defaulted"
            } else {
                "it records no recipe, so the knobs not stated here are identity"
            }
        );
        if !args.curation.overwrite {
            return Err(Failure::refused(format!(
                "{name} already exists as {} — pass --overwrite if replacing it is the intent.\n\
                 the recipe it would have baked with:\n{}",
                shipped.rel_path,
                recipe.lines()
            )));
        }
    }

    let events = args
        .events
        .iter()
        .map(|spec| parse_event(spec))
        .collect::<Result<Vec<_>, _>>()?;
    let take_record = load_record(args.record.as_deref(), RecordKind::Take, "the take")?;

    println!("recipe (every knob stated, nothing inherited from the bake):");
    print!("{}", recipe.lines());

    let request = PromoteClip {
        name,
        take_path: args.take.clone(),
        recipe,
        prompt: stated(args.curation.prompt.as_deref()),
        tags: args.curation.tags.clone(),
        note: stated(args.curation.note.as_deref()),
        events,
        created_by: Actor::parse(&args.curation.created_by),
        take_record,
        overwrite: args.curation.overwrite,
    };
    let promoted = promote_clip(project, &request)?;
    report(&promoted);
    if let (Some(was), Some(now)) = (promoted.previous_recipe(), promoted.recipe()) {
        println!("\nthe recipe it replaced, beside the one that shipped:");
        print!("{}", side_by_side(&was.lines(), &now.lines(), "was", "now"));
    }
    // Authored events are not curation: a different take under the same
    // name is a different motion, and an event restated blindly would sit at
    // a moment that no longer exists. So they are not carried — and the one
    // honest thing to do about what was dropped is to say so.
    if args.events.is_empty()
        && let Some(replaced) = &promoted.replaced
        && let Some(events) = &replaced.events
    {
        let authored: Vec<&str> = events
            .iter()
            .filter(|e| !e.origin.is_contacts())
            .map(|e| e.name.as_str())
            .collect();
        if !authored.is_empty() {
            println!(
                "note: the replaced clip carried {} authored event(s) ({}) and none were stated \
                 here, so they did not ship — restate them with --event to keep them",
                authored.len(),
                authored.join(", ")
            );
        }
    }
    Ok(())
}

/// The shipped recipe (or identity) with the stated knobs on top, then the
/// two loop flags, which say more than a blend can: `--loop` with no blend
/// is a loop by name, `--no-loop` turns an inherited one off.
fn effective_recipe(base: Option<&ClipRecipe>, args: &PromoteClipArgs) -> ClipRecipe {
    let requested = PartialRecipe {
        trim_start_s: args.trim_start,
        trim_end_s: args.trim_end,
        in_place: args.in_place,
        y_mode: args.y_mode,
        exaggerate: args.exaggerate,
        arm_bend_deg: args.arm_bend,
        lean_deg: args.lean,
        shoulder_back_deg: args.shoulder_back,
        loop_blend_s: args.loop_blend,
        retime: args.retime.clone(),
        clip: args.clip.clone(),
    };
    let identity = ClipRecipe::default();
    let mut recipe = forge_library::schema::overlay_recipe(base.unwrap_or(&identity), &requested);
    if args.r#loop {
        recipe.looping = true;
    }
    if args.no_loop {
        recipe.looping = false;
        recipe.loop_blend_s = 0.0;
    }
    recipe
}

/// Read one `--event T:NAME[:KIND:SOUND]`.
///
/// The time is on the raw take; the built-clip time is recomputed by the
/// bake through the recipe's time map, and an event whose moment is trimmed
/// away is dropped rather than pinned to a wrong second. The origin is left
/// unknown here and filled from `--created-by` by the bake.
fn parse_event(spec: &str) -> Result<AnimEvent, Failure> {
    let refuse = |why: &str| {
        Failure::refused(format!(
            "--event {spec:?}: {why} — the form is T:NAME, seconds on the raw take then \
             [a-z0-9_]+, with an optional :sfx:NAME link"
        ))
    };
    let mut parts = spec.trim().splitn(3, ':');
    let t_src: f32 = parts
        .next()
        .and_then(|t| t.trim().parse().ok())
        .ok_or_else(|| refuse("the time does not parse"))?;
    if !t_src.is_finite() || t_src < 0.0 {
        return Err(refuse("the time must be a non-negative number"));
    }
    let name = parts
        .next()
        .map(str::trim)
        .filter(|n| forge_library::schema::valid_event_name(n))
        .ok_or_else(|| refuse("the name is missing or not [a-z0-9_]+"))?;
    let audio = match parts.next() {
        None => None,
        Some(link) => Some(
            AudioRef::parse(link.trim())
                .ok_or_else(|| refuse("the link is not sfx:NAME, music:NAME or voice:NAME"))?,
        ),
    };
    Ok(AnimEvent {
        t: 0.0,
        t_src: Some(t_src),
        name: name.to_owned(),
        origin: EventOrigin::Unknown,
        audio,
    })
}

/// Two line blocks as two columns, headed.
fn side_by_side(left: &str, right: &str, left_head: &str, right_head: &str) -> String {
    let left: Vec<&str> = left.lines().collect();
    let right: Vec<&str> = right.lines().collect();
    let width = left
        .iter()
        .map(|l| l.chars().count())
        .chain(std::iter::once(left_head.chars().count()))
        .max()
        .unwrap_or(0);
    let mut out = format!("{left_head:<width$} | {right_head}\n");
    for i in 0..left.len().max(right.len()) {
        let l = left.get(i).copied().unwrap_or("");
        let r = right.get(i).copied().unwrap_or("");
        let pad = width.saturating_sub(l.chars().count());
        out.push_str(l);
        out.extend(std::iter::repeat_n(' ', pad));
        out.push_str(" | ");
        out.push_str(r);
        out.push('\n');
    }
    out
}

// ----------------------------------------------------------------- bodies ---

/// File a rigged body.
fn body(project: &Project, args: &PromoteBodyArgs) -> Outcome {
    let request = PromoteBody {
        name: args.name.clone(),
        glb_path: args.glb.clone(),
        blend_path: args.blend.clone(),
        lift_record: load_record(args.lift_record.as_deref(), RecordKind::Lift, "the lift")?,
        rig_record: load_record(args.rig_record.as_deref(), RecordKind::Rig, "the rig")?,
        export_record: load_record(
            args.export_record.as_deref(),
            RecordKind::Export,
            "the export",
        )?,
        prompt: stated(args.curation.prompt.as_deref()),
        tags: args.curation.tags.clone(),
        note: stated(args.curation.note.as_deref()),
        created_by: Actor::parse(&args.curation.created_by),
        overwrite: args.curation.overwrite,
    };
    let promoted = promote_body(project, &request)?;
    report(&promoted);
    Ok(())
}

// ----------------------------------------------------------------- models ---

/// File a static mesh.
fn model(project: &Project, args: &PromoteModelArgs) -> Outcome {
    let request = PromoteModel {
        name: args.name.clone(),
        glb_path: args.glb.clone(),
        blend_path: args.blend.clone(),
        lift_record: load_record(args.lift_record.as_deref(), RecordKind::Lift, "the lift")?,
        prop_record: load_record(args.prop_record.as_deref(), RecordKind::Prop, "the prop")?,
        prompt: stated(args.curation.prompt.as_deref()),
        tags: args.curation.tags.clone(),
        note: stated(args.curation.note.as_deref()),
        created_by: Actor::parse(&args.curation.created_by),
        overwrite: args.curation.overwrite,
    };
    let promoted = promote_model(project, &request)?;
    report(&promoted);
    Ok(())
}

// ------------------------------------------------------------------ audio ---

/// File a sound.
fn audio(project: &Project, args: &PromoteAudioArgs) -> Outcome {
    let kind = parse_kind(&args.kind)?;
    if !kind.is_audio() {
        return Err(Failure::refused(format!(
            "{kind} is not an audio kind — use sfx, music or voice{}",
            match kind {
                Kind::Clip => " (a clip goes through `forge promote clip`)",
                Kind::Body => " (a body goes through `forge promote body`)",
                Kind::Model => " (a model goes through `forge promote model`)",
                _ => "",
            }
        )));
    }
    let expected = match kind {
        Kind::Sfx => RecordKind::Sfx,
        Kind::Music => RecordKind::Music,
        _ => RecordKind::Speech,
    };
    let request = PromoteAudio {
        kind,
        name: args.name.clone(),
        file: args.file.clone(),
        record: load_record(args.record.as_deref(), expected, "the sound")?,
        prompt: stated(args.curation.prompt.as_deref()),
        tags: args.curation.tags.clone(),
        note: stated(args.curation.note.as_deref()),
        created_by: Actor::parse(&args.curation.created_by),
        overwrite: args.curation.overwrite,
    };
    let promoted = promote_audio(project, &request)?;
    report(&promoted);
    Ok(())
}

// ----------------------------------------------------------------- shared ---

/// Read a generator record named on the command line and hold it to the
/// kind this door expects: a lift record handed to `--rig-record` is a
/// record, but not this step's, and it would be filed as if it were.
fn load_record(
    path: Option<&Path>,
    expected: RecordKind,
    what: &str,
) -> Result<Option<GeneratorRecord>, Failure> {
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.is_file() {
        return Err(Failure::refused(format!(
            "no record at {} — pass the path exactly as the generator reported it",
            path.display()
        )));
    }
    let record = GeneratorRecord::load(path)?;
    if record.kind != expected {
        return Err(Failure::refused(format!(
            "{} describes a {} run, not {expected} — it is not {what}'s record",
            path.display(),
            record.kind
        )));
    }
    Ok(Some(record))
}

/// An optional flag, with an empty or blank value read as "not stated".
fn stated(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

/// What shipped, for the terminal.
fn report(promoted: &Promoted) {
    println!("{}", promoted.report);
    println!(
        "-> {} ({}, {}, created {} by {})",
        promoted.rel_path,
        promoted.record.provenance,
        promoted
            .record
            .generator
            .as_ref()
            .map_or("no generator block", |g| g.tool()),
        promoted.record.created,
        promoted.record.created_by,
    );
    if let Some(replaced) = &promoted.replaced {
        println!(
            "replaced the {} {} created {} by {}",
            replaced.kind, replaced.name, replaced.created, replaced.created_by
        );
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::*;

    #[test]
    fn an_event_is_time_name_and_an_optional_link() {
        let plain = parse_event("0.4:swing").expect("plain");
        assert_eq!(plain.t_src, Some(0.4));
        assert_eq!(plain.name, "swing");
        assert!(plain.audio.is_none());
        assert_eq!(plain.origin, EventOrigin::Unknown);

        let linked = parse_event("1.25:hit:sfx:thud").expect("linked");
        assert_eq!(
            linked.audio.map(|a| a.to_string()).as_deref(),
            Some("sfx:thud")
        );

        for bad in [
            "swing",
            "x:swing",
            "0.4:Swing",
            "0.4:swing:clip:x",
            "-1:swing",
        ] {
            assert!(parse_event(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_loop_flags_outrank_the_overlay() {
        let base = ClipRecipe {
            looping: true,
            loop_blend_s: 0.2,
            ..ClipRecipe::default()
        };
        let off =
            crate::cli::Cli::try_parse_from(["forge", "promote", "clip", "t", "w", "--no-loop"])
                .expect("parse");
        let crate::cli::Command::Promote(PromoteDoor::Clip(args)) = off.command else {
            panic!("not a clip promote");
        };
        let recipe = effective_recipe(Some(&base), &args);
        assert!(!recipe.looping);
        assert!(recipe.loop_blend_s.abs() < f32::EPSILON);

        let on = crate::cli::Cli::try_parse_from(["forge", "promote", "clip", "t", "w", "--loop"])
            .expect("parse");
        let crate::cli::Command::Promote(PromoteDoor::Clip(args)) = on.command else {
            panic!("not a clip promote");
        };
        let recipe = effective_recipe(None, &args);
        assert!(recipe.looping);
        assert!(
            recipe.loop_blend_s.abs() < f32::EPSILON,
            "a loop by name, no blend stated"
        );
    }

    #[test]
    fn side_by_side_pads_the_left_column() {
        let text = side_by_side("a\nlonger line\n", "x\ny\nz\n", "was", "now");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].ends_with("| now"), "{text}");
        assert_eq!(lines[1], "a           | x");
        assert_eq!(lines[2], "longer line | y");
        assert_eq!(lines[3], "            | z");
    }
}
