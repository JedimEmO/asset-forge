//! Does the library say true things about itself? The claims that can be
//! checked without an engine.
//!
//! Four claims, checked in order of how expensive they are to make:
//!
//! 1. **Integrity.** Every sidecar parses at a schema this build knows, and
//!    every asset hashes to what its record says — [`crate::verify`], run
//!    first because there is no point rebuilding clips whose records do not
//!    parse.
//! 2. **Derived footsteps re-derive.** Every event a sidecar credits to
//!    `contacts` must coincide — within one take frame — with a footstep
//!    re-derived from the take's own labels through the recipe's time map,
//!    none missing, none extra. A take with contacts whose sidecar says
//!    `events: null` is a failure, not backfill debt; a take *without*
//!    contacts is unable-to-check and reported as a note.
//! 3. **Root tracks rebuild.** Every recorded `measured.root_motion.track_xz_m`
//!    must match a rebuild of the recipe with `in_place` forced off — the
//!    same trick `promote` used to measure it — per built frame, within a
//!    millimetre plus the millimetre rounding the sidecar writer applies.
//!    `root_motion: null` on a clip is a failure by the same re-bake argument.
//! 4. **Every clip rebuilds.** The library claims each clip is reproducible
//!    from its own sidecar. This rebuilds every one from its raw take plus the
//!    recorded recipe against the profile's rig and compares the **bytes** to
//!    the shipped file. The bake is deterministic for a fixed input, so
//!    equality is the bar — with one reading: a file written by another
//!    version of the baker differs in its `asset.generator` string and
//!    nothing else it is *allowed* to differ in, so a generator mismatch is
//!    reported as a warning here rather than a failure.
//!
//!    TODO(P3, `forge_studio::audit`): when the generator string differs,
//!    bind both clips to the fixture mannequin and compare world-space bone
//!    positions to within 1 mm — the pose compare the byte compare stands
//!    in for. Until then a generator mismatch is a warning that names the
//!    two strings.
//!
//! **Bodies and models are skipped, loudly.** A lifted mesh claims integrity
//! and provenance, never regeneration — there is no recipe to replay and
//! Blender's export is not byte-stable — so the skip is the rule, reported
//! as a note per asset and counted in the summary rather than silently
//! improving the score.
//!
//! Parsing goes through this crate's own types — the same ones the studio,
//! the MCP server and the promote path use. Auditing a private copy of the
//! parser would prove the sidecars are reproducible by *something*, which is
//! not the claim.

use forge_motion::events::{Foot, footsteps};
use forge_motion::{Edit, InPlace, Take};

use crate::report::Report;
use crate::schema::{AnimEvent, Kind, RootMotion};
use crate::{Catalog, Project, verify};

/// Per-point agreement for the rebuilt root track: a millimetre of float
/// noise plus the 0.001 m quantisation the sidecar writer applies on write.
const TRACK_TOL_M: f32 = 0.002;

/// What the audit found, claim by claim.
#[derive(Debug, Clone, Default)]
pub struct Audit {
    /// Claim 1: `verify::all`.
    pub integrity: Report,
    /// Claim 2: footsteps re-derive.
    pub events: Report,
    /// Claim 3: root tracks rebuild.
    pub roots: Report,
    /// Claim 4: clips rebuild byte for byte.
    pub rebuilds: Report,
    /// Clips whose derived footsteps all coincided with a re-derivation.
    pub events_ok: usize,
    /// Clips whose footsteps were checkable at all. A take without contacts
    /// cannot be checked, and counting it as a pass would inflate the score.
    pub events_checked: usize,
    /// Clips whose recorded root track rebuilt. Every clip is in this
    /// denominator: a missing track is a failure, not a gap.
    pub roots_ok: usize,
    /// Clips whose bytes rebuilt exactly.
    pub rebuilt_ok: usize,
    /// How many clips there were.
    pub clips: usize,
    /// How many bodies and models were skipped.
    pub meshes_skipped: usize,
}

impl Audit {
    /// Whether every claim held.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.integrity.ok() && self.events.ok() && self.roots.ok() && self.rebuilds.ok()
    }

    /// Everything as one report, for a caller that wants one exit code.
    #[must_use]
    pub fn combined(&self) -> Report {
        let mut report = self.integrity.clone();
        report.absorb(self.events.clone());
        report.absorb(self.roots.clone());
        report.absorb(self.rebuilds.clone());
        report
    }

    /// The audit as text: each claim under a heading, then the scores.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        let section = |out: &mut String, title: &str, report: &Report, held: &str| {
            out.push_str(title);
            out.push('\n');
            let body = report.render();
            for line in body.lines() {
                out.push_str("  ");
                out.push_str(line);
                out.push('\n');
            }
            if report.ok() {
                out.push_str("  ");
                out.push_str(held);
                out.push('\n');
            }
            out.push('\n');
        };
        section(
            &mut out,
            "library integrity",
            &self.integrity,
            "sidecars parse and every recorded hash matches — integrity verified, generation not re-run",
        );
        section(
            &mut out,
            "derived footsteps",
            &self.events,
            &format!(
                "{}/{} clips with contacts re-derive their footsteps",
                self.events_ok, self.events_checked
            ),
        );
        section(
            &mut out,
            "root tracks",
            &self.roots,
            &format!("{}/{} root tracks rebuild", self.roots_ok, self.clips),
        );
        section(
            &mut out,
            "clip rebuilds",
            &self.rebuilds,
            &format!(
                "{}/{} clips rebuild byte for byte",
                self.rebuilt_ok, self.clips
            ),
        );
        if self.meshes_skipped > 0 {
            use std::fmt::Write as _;
            let _ = writeln!(
                out,
                "{} bodies/models skipped: integrity-only, no recipe",
                self.meshes_skipped
            );
        }
        out
    }
}

/// One clip under audit: where it lives, and what it says it was made of.
struct Clip {
    /// File stem, and the name in every report line.
    name: String,
    /// The shipped bytes.
    shipped: Vec<u8>,
    /// The take the recipe replays on top of, when the record names one.
    take: Option<std::path::PathBuf>,
    /// The recipe as recorded, or `None` when the record cannot supply one.
    recipe: Option<crate::ClipRecipe>,
    /// The recorded timeline events. `None` means the sidecar never examined
    /// the clip — a failure on a take with contacts.
    events: Option<Vec<AnimEvent>>,
    /// The recorded pre-strip root track and its headline numbers.
    root_motion: Option<RootMotion>,
}

/// Run every engine-free claim over the library.
#[must_use]
pub fn run(project: &Project) -> Audit {
    let mut audit = Audit {
        integrity: verify::all(project),
        ..Audit::default()
    };
    let catalog = Catalog::scan(project);
    let rig = project
        .profile()
        .ok()
        .and_then(|p| crate::promote::load_rig(&p).ok());

    for record in catalog.records() {
        match record.kind {
            Kind::Body | Kind::Model => {
                audit.meshes_skipped += 1;
                audit.rebuilds.note(
                    &record.name,
                    format!("{} skipped: integrity-only, no recipe", record.kind),
                );
                continue;
            }
            Kind::Clip => {}
            Kind::Sfx | Kind::Music | Kind::Voice => continue,
        }
        audit.clips += 1;
        audit.events.checked += 1;
        audit.roots.checked += 1;
        audit.rebuilds.checked += 1;
        let sidecar = record.sidecar.as_ref();
        let clip = Clip {
            name: record.name.clone(),
            shipped: std::fs::read(&record.path).unwrap_or_default(),
            take: sidecar
                .and_then(|s| s.source.path.as_deref())
                .map(|p| project.root.join(p)),
            recipe: sidecar.and_then(|s| s.recipe.clone()),
            events: sidecar.and_then(|s| s.events.clone()),
            root_motion: sidecar
                .and_then(|s| s.measured.as_ref())
                .and_then(|m| m.root_motion.clone()),
        };
        audit_clip(&mut audit, &clip, rig.as_ref());
    }
    audit
}

/// Re-derive footsteps, rebuild the pre-strip root track and re-bake the
/// bytes for one clip, mirroring `promote`'s `measure()`/`derive_events()` —
/// the production path that wrote these facts in the first place.
fn audit_clip(audit: &mut Audit, clip: &Clip, rig: Option<&forge_motion::RigDef>) {
    let name = clip.name.as_str();
    // Every check replays the recipe over the raw take, so a lost take or
    // recipe fails every claim rather than excusing them: the library
    // asserts these facts were measured, and unmeasurable is not measured.
    let fail_all = |audit: &mut Audit, detail: &str| {
        audit.events_checked += 1;
        audit.events.fail(name, detail);
        audit.roots.fail(name, detail);
        audit.rebuilds.fail(name, detail);
    };
    let Some(take_path) = &clip.take else {
        fail_all(audit, "no source take recorded in its sidecar");
        return;
    };
    let take = match Take::read(take_path) {
        Ok(take) => take,
        Err(error) => {
            fail_all(
                audit,
                &format!("no readable take at {}: {error}", take_path.display()),
            );
            return;
        }
    };
    let Some(recipe) = &clip.recipe else {
        fail_all(audit, "no recipe in its sidecar");
        return;
    };
    // The take's own clock, exactly as promote used it — not the sidecar's
    // measured fps, which is an output of the same bake.
    let edit = match recipe.to_edit(take.fps) {
        Ok(edit) => edit,
        Err(error) => {
            fail_all(audit, &format!("recipe will not apply: {error}"));
            return;
        }
    };
    // One rebuild serves two checks: `in_place` forced off is the same
    // trick promote's measure() uses to see the travel the bake is about
    // to delete, and it changes neither the frame count nor the clock the
    // events check needs for the built duration.
    let built = Edit {
        in_place: InPlace::Off,
        ..edit.clone()
    }
    .apply(&take);
    let duration = if built.frames() > 1 {
        (built.frames() - 1) as f32 / built.fps
    } else {
        0.0
    };

    match &take.contacts {
        None => audit.events.note(
            name,
            "take has no contact labels — footsteps cannot be re-derived",
        ),
        Some(contacts) => {
            audit.events_checked += 1;
            let before = audit.events.failures();
            footstep_findings(&mut audit.events, clip, contacts, &take, &edit, duration);
            if audit.events.failures() == before {
                audit.events_ok += 1;
            }
        }
    }

    let before = audit.roots.failures();
    root_findings(&mut audit.roots, clip, &built);
    if audit.roots.failures() == before {
        audit.roots_ok += 1;
    }

    let Some(rig) = rig else {
        audit.rebuilds.fail(
            name,
            "the profile's rig.glb does not load, so nothing can be re-baked",
        );
        return;
    };
    let clip_name = recipe.clip.clone().unwrap_or_else(|| clip.name.clone());
    match forge_motion::bake(&take, &edit, rig, &clip_name) {
        Err(error) => audit
            .rebuilds
            .fail(name, format!("re-bake failed: {error}")),
        Ok(rebuilt) if rebuilt == clip.shipped => audit.rebuilt_ok += 1,
        Ok(rebuilt) => {
            let shipped_gen = generator_of(&clip.shipped);
            let rebuilt_gen = generator_of(&rebuilt);
            if shipped_gen == rebuilt_gen {
                audit.rebuilds.fail(
                    name,
                    format!(
                        "re-bake from its own take and recipe produced different bytes \
                         ({} shipped, {} rebuilt) under the same baker — the sidecar does not \
                         describe the file beside it",
                        clip.shipped.len(),
                        rebuilt.len()
                    ),
                );
            } else {
                audit.rebuilds.warn(
                    name,
                    format!(
                        "bytes differ and so does the baker: shipped by {:?}, rebuilt by {:?} — \
                         the pose compare that settles this lands with the studio audit",
                        shipped_gen.unwrap_or_default(),
                        rebuilt_gen.unwrap_or_default()
                    ),
                );
            }
        }
    }
}

/// The glTF `asset.generator` string of a `.glb`, when it is readable.
fn generator_of(bytes: &[u8]) -> Option<String> {
    verify::verify_glb(bytes).ok().and_then(|s| s.generator)
}

/// Where the sidecar's derived footsteps disagree with a re-derivation.
///
/// Mirrors `derive_events` for `origin: "contacts"`: [`footsteps`] on the
/// take's labels, each strike mapped through the recipe's
/// [`Edit::map_time`] and dropped past the built clip's end. Coincidence is
/// within one take frame plus the millisecond rounding the sidecar writer
/// applies — none missing, none extra.
fn footstep_findings(
    report: &mut Report,
    clip: &Clip,
    contacts: &[[bool; 4]],
    take: &Take,
    edit: &Edit,
    duration: f32,
) {
    let name = clip.name.as_str();
    let Some(events) = clip.events.as_ref() else {
        report.fail(
            name,
            "events: null, but the take has contact labels — the bake should have derived footsteps",
        );
        return;
    };

    let mut rederived: Vec<(&str, f32)> = Vec::new();
    for (foot, t_raw) in footsteps(contacts, take.fps) {
        let Some(t) = edit.map_time(t_raw, take.fps) else {
            continue;
        };
        if t > duration + 1e-3 {
            continue;
        }
        let label = match foot {
            Foot::Left => "footstep_l",
            Foot::Right => "footstep_r",
        };
        rederived.push((label, t));
    }

    let tolerance = 1.0 / take.fps + 1e-3;
    let mut claimed = vec![false; rederived.len()];
    for event in events.iter().filter(|e| e.origin.is_contacts()) {
        // Nearest unclaimed re-derivation of the same foot; claiming stops
        // one real strike from vouching for two recorded ones.
        let nearest = rederived
            .iter()
            .enumerate()
            .filter(|(index, (label, _))| !claimed[*index] && *label == event.name)
            .min_by(|(_, a), (_, b)| (a.1 - event.t).abs().total_cmp(&(b.1 - event.t).abs()));
        match nearest {
            Some((index, (_, t))) if (t - event.t).abs() <= tolerance => claimed[index] = true,
            Some((_, (_, t))) => report.fail(
                name,
                format!(
                    "event {} @ {:.3}s: nearest re-derived footstep is at {t:.3}s, \
                     more than a frame ({tolerance:.3}s) away",
                    event.name, event.t
                ),
            ),
            None => report.fail(
                name,
                format!(
                    "event {} @ {:.3}s has no re-derived footstep to match",
                    event.name, event.t
                ),
            ),
        }
    }
    for (index, (label, t)) in rederived.iter().enumerate() {
        if !claimed[index] {
            report.fail(
                name,
                format!("re-derived {label} @ {t:.3}s is missing from the sidecar's events"),
            );
        }
    }
}

/// Where the recorded pre-strip root track disagrees with a rebuild.
///
/// `built` is the recipe applied with `in_place` forced off, so its root XZ
/// is the very track `measure()` recorded before the strip deleted it.
fn root_findings(report: &mut Report, clip: &Clip, built: &Take) {
    let name = clip.name.as_str();
    let Some(motion) = clip.root_motion.as_ref() else {
        report.fail(
            name,
            "measured.root_motion: null — the bake should have recorded the pre-strip track",
        );
        return;
    };
    if motion.track_xz_m.len() != built.frames() {
        report.fail(
            name,
            format!(
                "track_xz_m has {} point(s) but the rebuild has {} frame(s)",
                motion.track_xz_m.len(),
                built.frames()
            ),
        );
        return;
    }
    let mut worst: Option<(usize, f32, [f32; 2], [f32; 2])> = None;
    for (frame, (recorded, rebuilt)) in motion.track_xz_m.iter().zip(&built.root).enumerate() {
        let point = [rebuilt.x, rebuilt.z];
        let dx = recorded[0] - point[0];
        let dz = recorded[1] - point[1];
        let distance = (dx * dx + dz * dz).sqrt();
        if worst.is_none_or(|(_, top, ..)| distance > top) {
            worst = Some((frame, distance, *recorded, point));
        }
    }
    if let Some((frame, distance, recorded, rebuilt)) = worst
        && distance > TRACK_TOL_M
    {
        report.fail(
            name,
            format!(
                "root track diverges {:.1} mm at frame {frame}: recorded [{:.3}, {:.3}], \
                 rebuilt [{:.3}, {:.3}]",
                distance * 1000.0,
                recorded[0],
                recorded[1],
                rebuilt[0],
                rebuilt[1]
            ),
        );
    }
}
