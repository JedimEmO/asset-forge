//! Shipping an asset into the library: the one place that writes `assets/`.
//!
//! Four doors, all direct — there is no review queue; a promote writes the
//! library now and refuses an existing name unless told otherwise — and every
//! door keeps the same order: **payload first, record last, both atomic**, so
//! a scan never sees a sidecar describing bytes that are not there yet.
//!
//! The clip bake is native: [`forge_motion`] applies the recipe's
//! [`forge_motion::Edit`] and serializes the `.glb` directly, so what ships
//! is a pure function of `(take, recipe, rig)` with no external generator in
//! the path. Every knob comes from the caller's [`ClipRecipe`], stated in
//! full — an earlier Python baker resolved unstated arguments by inheriting
//! them from the sidecar of the clip being overwritten, and a promote that
//! silently adopted the old clip's trims looked plausible right up until
//! somebody watched it. When a promote *does* replace a clip, what it
//! replaced comes back in [`Promoted::replaced`] so the caller can echo the
//! old recipe beside the new one.
//!
//! Bodies, models and sounds are not baked here and never will be: none of
//! TRELLIS.2, Blender's glTF export, MOSS or ACE-Step is byte-stable, so what
//! ships is the exact bytes that were checked, and the record claims
//! integrity plus provenance rather than regeneration. The collision gate
//! lives here too: a promote that would replace a shipped asset refuses
//! unless the caller said that was the intent.

use std::path::{Path, PathBuf};

use forge_motion::events::{Foot, footsteps};
use forge_motion::{Edit, InPlace, RigDef, Take};
use forge_rig::measure::{GlbMeasurement, measure_glb, measure_glb_geometry};

use crate::generator_record::{GeneratorRecord, RecordKind};
use crate::schema::{
    Actor, AnimEvent, ClipRecipe, EventOrigin, Generator, Kind, LiftParams, Measured, MeshMeasured,
    PostStep, Provenance, RootMotion, SCHEMA, Sidecar, Source, valid_event_name,
};
use crate::{
    LibraryError, Project, Result, clock, hash, manifest, read_bytes, sidecar, write_atomic,
};

/// What a successful promote produced.
#[derive(Debug, Clone)]
pub struct Promoted {
    /// The shipped asset.
    pub asset: PathBuf,
    /// Its sidecar.
    pub sidecar: PathBuf,
    /// Path relative to the asset root, for reporting back.
    pub rel_path: String,
    /// What shipped, in one or two lines a human wants to read.
    pub report: String,
    /// The record as written beside the asset — read back from the file, so
    /// it is exactly what a later scan will see.
    pub record: Sidecar,
    /// The record this promote replaced, when it replaced one — so a caller
    /// that overwrote a clip can echo the old recipe beside the new.
    pub replaced: Option<Sidecar>,
}

impl Promoted {
    /// The recipe that shipped, for a clip.
    #[must_use]
    pub fn recipe(&self) -> Option<&ClipRecipe> {
        self.record.recipe.as_ref()
    }

    /// The recipe of the clip this one replaced, when it replaced one that
    /// recorded a recipe.
    #[must_use]
    pub fn previous_recipe(&self) -> Option<&ClipRecipe> {
        self.replaced.as_ref()?.recipe.as_ref()
    }
}

// ------------------------------------------------------------------ clips ---

/// One clip to bake into the library.
#[derive(Debug, Clone)]
pub struct PromoteClip {
    /// Output stem: `<assets>/clips/<name>.glb`.
    pub name: String,
    /// The raw `.npz` take to bake. Copied to `<sources>/takes/<name>.npz`
    /// untrimmed, which is what keeps the clip re-bakeable.
    pub take_path: PathBuf,
    /// The recipe, stated in full.
    pub recipe: ClipRecipe,
    /// What the motion is. `None` lets the take's own prompt stand; it is
    /// never invented.
    pub prompt: Option<String>,
    /// Curation tags. Empty means "say nothing about them", and the tags of
    /// a clip being replaced survive — see [`promote_clip`].
    pub tags: Vec<String>,
    /// Anything the next reader should know.
    pub note: Option<String>,
    /// Authored events, timed on the **raw take** (`t_src`). Footsteps are
    /// derived from the take's contact labels and must not be stated here.
    pub events: Vec<AnimEvent>,
    /// Who is promoting.
    pub created_by: Actor,
    /// The ARDY run's record, when the take came with one. With it the
    /// generator block is recorded; without it the block is nulls and the
    /// provenance says `reconstructed`.
    pub take_record: Option<GeneratorRecord>,
    /// Whether replacing an existing clip of this name is intended.
    pub overwrite: bool,
}

/// Bake a take into the shipped library, then make its sidecar the truth.
///
/// Tags are curation rather than provenance, so they survive a re-promote: a
/// caller that states none is saying nothing about them, not asking for them
/// to be cleared, and without this every re-bake of a locomotion clip would
/// quietly drop its `loop` tag. The prompt is deliberately *not* inherited
/// this way, because a different take under the same name may be a different
/// motion.
///
/// # Errors
///
/// Fails when the clip name is unusable, the take is missing or unreadable,
/// the profile's rig cannot be read, the recipe does not apply, an authored
/// event is malformed, the target exists and overwriting was not asked for,
/// or a write fails.
pub fn promote_clip(project: &Project, request: &PromoteClip) -> Result<Promoted> {
    promote_clip_carrying(project, request, None)
}

/// [`promote_clip`], carrying the generator block, provenance, authorship
/// and authored events of an existing record — what `rebake` needs, since a
/// re-bake claims nothing new about who made the take or how.
pub(crate) fn promote_clip_carrying(
    project: &Project,
    request: &PromoteClip,
    carry: Option<&Sidecar>,
) -> Result<Promoted> {
    let name = validate_name(&request.name)?;
    if !request.take_path.is_file() {
        return Err(LibraryError::rejected(format!(
            "no take at {} — pass the path exactly as it was reported",
            request.take_path.display()
        )));
    }
    let target = project.kind_dir(Kind::Clip).join(format!("{name}.glb"));
    guard_collision(&name, &target, request.overwrite)?;
    let authored = checked_events(&request.events, &request.created_by)?;

    let take = Take::read(&request.take_path)
        .map_err(|e| LibraryError::bake(format!("{}: {e}", request.take_path.display())))?;
    let edit = request.recipe.to_edit(take.fps)?;
    let clip_name = clip_name_for(&name, &request.recipe);
    let profile = project.profile()?;
    let rig = load_rig(&profile)?;
    let glb = forge_motion::bake(&take, &edit, &rig, &clip_name)
        .map_err(|e| LibraryError::bake(e.to_string()))?;

    let carried_events = carry
        .and_then(|s| s.events.as_deref())
        .map(|events| {
            events
                .iter()
                .filter(|e| !e.origin.is_contacts())
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let authored = if authored.is_empty() {
        carried_events
    } else {
        authored
    };
    let facts = measure(&take, &edit, &rig, &authored);

    // The record this replaces, still on disk because the sidecar is written
    // last. Only curation is read from it.
    let previous = sidecar::load_beside(&target).unwrap_or(None);
    write_atomic(&target, &glb)?;
    let kept_take = keep_take(project, &name, &request.take_path)?;

    let mut record = carry
        .cloned()
        .unwrap_or_else(|| Sidecar::new(Kind::Clip, &name));
    record.schema = SCHEMA;
    record.kind = Kind::Clip;
    record.name.clone_from(&name);
    record.rig = Some(profile.contract.name.clone());

    // The recipe as applied, including the clip name the bake resolved — an
    // engine binds by it, so recording anything else would describe a
    // different asset.
    let mut resolved = request.recipe.clone();
    resolved.clip = Some(clip_name.clone());
    record.recipe = Some(resolved);
    record.measured = Some(facts.measured);
    record.events = facts.events;
    record.source = Source {
        path: project.rel_to_root(&kept_take),
        sha256: Some(hash::sha256_file(&kept_take)?),
        skeleton: Some(profile.motion.name.clone()),
    };

    if let Some(prompt) = &request.prompt {
        record.prompt = Some(prompt.clone());
    }
    // The take records the prompt it was generated from; a caller that
    // states none is silent, not contradicting it.
    if record.prompt.is_none() && !take.prompt.is_empty() {
        record.prompt = Some(take.prompt.clone());
    }
    if let Some(note) = &request.note {
        record.note = Some(note.clone());
    }
    if carry.is_none() {
        if let Some(take_record) = &request.take_record {
            record.generator = Some(Generator::Ardy(take_record.ardy_params()));
            record.provenance = Provenance::Recorded;
            if record.prompt.is_none() {
                record.prompt = take_record.prompt().map(str::to_owned);
            }
        } else {
            // This is an ARDY take — `Take::read` enforces the skeleton —
            // and a recipe was applied to motion this module did not
            // generate, which is exactly what *reconstructed* means. The
            // file's own name is the one honest fact about the sweep.
            let sweep_take = request
                .take_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned());
            record.generator = Some(Generator::Ardy(crate::schema::ArdyParams {
                sweep_take,
                ..Default::default()
            }));
            record.provenance = Provenance::Reconstructed;
        }
        record.created_by = request.created_by.clone();
        record.created = clock::today_iso();
    }
    if !request.tags.is_empty() {
        record.tags.clone_from(&request.tags);
    } else if record.tags.is_empty()
        && let Some(previous) = &previous
    {
        record.tags.clone_from(&previous.tags);
    }
    record.tags.retain(|t| t != "loop");
    if request.recipe.looping {
        record.tags.push(String::from("loop"));
    }
    record.content_hash = hash::sha256_file(&target)?;

    let sidecar_path = sidecar::path_for(&target);
    let record = save_record(&sidecar_path, &record)?;

    let mut report = describe_clip(&record, &name);
    refresh_manifest(project, &mut report);
    Ok(Promoted {
        rel_path: project
            .rel_to_assets(&target)
            .unwrap_or_else(|| format!("{}/{name}.glb", Kind::Clip.dir())),
        asset: target,
        sidecar: sidecar_path,
        report,
        record,
        replaced: previous,
    })
}

/// The name the animation carries inside the `.glb`, which is what an engine
/// binds by.
///
/// A stated `clip` wins; otherwise the asset name stands in. Either way a loop
/// gets the `-loop` suffix — a naming convention only, but one every shipped
/// loop follows, and consistency here is what lets a game tell loops apart in
/// a raw glTF viewer.
fn clip_name_for(name: &str, recipe: &ClipRecipe) -> String {
    let mut clip = recipe.clip.clone().unwrap_or_else(|| name.to_owned());
    if recipe.looping && !clip.ends_with("-loop") {
        clip.push_str("-loop");
    }
    clip
}

/// The profile's rig, read fresh from its artifact so the bake and the
/// library can never disagree about rest pose or bind matrices.
pub(crate) fn load_rig(profile: &forge_rig::RigProfile) -> Result<RigDef> {
    let path = profile.glb_path();
    let bytes = read_bytes(&path)?;
    RigDef::from_glb(&bytes).map_err(|e| LibraryError::bake(format!("{}: {e}", path.display())))
}

/// Authored events, validated: a usable name, no contacts origin (footsteps
/// are derived, never stated), and an origin naming who authored them when
/// the event did not say.
fn checked_events(events: &[AnimEvent], author: &Actor) -> Result<Vec<AnimEvent>> {
    let mut out = Vec::with_capacity(events.len());
    for event in events {
        if !valid_event_name(&event.name) {
            return Err(LibraryError::rejected(format!(
                "event name {:?} is not [a-z0-9_]+ — it cannot key game code",
                event.name
            )));
        }
        if event.origin.is_contacts() {
            return Err(LibraryError::rejected(format!(
                "event {:?} claims origin \"contacts\" — footsteps are derived from the \
                 take's own labels at bake time; do not state them",
                event.name
            )));
        }
        let mut event = event.clone();
        if event.origin == EventOrigin::Unknown {
            event.origin = EventOrigin::from_actor(author);
        }
        out.push(event);
    }
    Ok(out)
}

/// What the bake measured and derived, before it is merged into the record.
struct BakeFacts {
    measured: Measured,
    events: Option<Vec<AnimEvent>>,
}

/// Measure the built clip and derive its events.
///
/// Speeds and the root-motion track are taken from the edit with `in_place`
/// forced off: that is the travel `strip`/`detrend` are about to delete, and
/// the whole point of recording it is giving the game back what the bake
/// removed. `avg_speed_mps` additionally excludes the loop blend, matching how
/// every shipped record measured it.
fn measure(take: &Take, edit: &Edit, rig: &RigDef, authored: &[AnimEvent]) -> BakeFacts {
    let built = edit.apply(take);
    let frames = built.rotations.len();
    let fps = built.fps;
    let duration = if frames > 1 {
        (frames - 1) as f32 / fps
    } else {
        0.0
    };

    let speed_take = Edit {
        in_place: InPlace::Off,
        loop_blend_s: 0.0,
        ..edit.clone()
    }
    .apply(take);
    let steps: Vec<f32> = speed_take
        .root
        .windows(2)
        .map(|w| {
            let d = w[1] - w[0];
            (d.x * d.x + d.z * d.z).sqrt()
        })
        .collect();
    let avg_speed = if duration > 0.0 {
        steps.iter().sum::<f32>() / duration
    } else {
        0.0
    };
    let peak_speed = steps.iter().fold(0.0_f32, |top, step| top.max(*step)) * fps;

    let track_take = Edit {
        in_place: InPlace::Off,
        ..edit.clone()
    }
    .apply(take);
    let track: Vec<[f32; 2]> = track_take.root.iter().map(|p| [p.x, p.z]).collect();
    let net = track.last().copied().unwrap_or([0.0, 0.0]);
    let direction = travel_direction(&track_take, rig, net);

    let measured = Measured {
        frames: Some(frames as u32),
        fps: Some(fps),
        duration_s: Some(duration),
        avg_speed_mps: Some(avg_speed),
        root_motion: Some(RootMotion {
            net_m: net,
            direction_deg: direction,
            peak_speed_mps: Some(peak_speed),
            track_xz_m: track,
        }),
        // A clip has no mesh of its own; the rig it poses is a separate
        // asset with its own record.
        mesh: None,
    };

    BakeFacts {
        measured,
        events: derive_events(take, edit, duration, authored),
    }
}

/// Where the clip travels relative to where the character faces, in the
/// review sheets' convention: 0 forward, +90 right, 180 back, -90 left.
///
/// The lateral axis is `LeftUpLeg - RightUpLeg`. Both hang directly off
/// `Hips`, so their world separation is the hips rotation applied to the
/// difference of their rest offsets — no full pose reconstruction needed.
/// `None` when net travel is under 0.3 m, where a heading is noise.
fn travel_direction(track_take: &Take, rig: &RigDef, net: [f32; 2]) -> Option<f32> {
    let travel = (net[0] * net[0] + net[1] * net[1]).sqrt();
    if travel <= 0.3 {
        return None;
    }
    let offset = |joint: &str| {
        rig.bones
            .iter()
            .find(|b| b.name == joint)
            .map(|b| b.translation)
    };
    let across = offset("LeftUpLeg")? - offset("RightUpLeg")?;
    let mut lx = 0.0_f64;
    let mut lz = 0.0_f64;
    for frame in &track_take.rotations {
        let lat = frame[0] * across;
        lx += f64::from(lat.x);
        lz += f64::from(lat.z);
    }
    let norm = (lx * lx + lz * lz).sqrt();
    if norm == 0.0 {
        return None;
    }
    let (lm_x, lm_z) = (lx / norm, lz / norm);
    let (tv_x, tv_z) = (
        f64::from(net[0]) / f64::from(travel),
        f64::from(net[1]) / f64::from(travel),
    );
    let rel = (tv_z * lm_x - tv_x * lm_z)
        .atan2(tv_x * lm_x + tv_z * lm_z)
        .to_degrees();
    let dir = (rel - 90.0 + 180.0).rem_euclid(360.0) - 180.0;
    Some(dir as f32)
}

/// Derived footsteps plus the authored events, on the built clip's clock.
///
/// Derived events are recomputed every bake, never carried forward — they are
/// a measurement, and re-measuring is what keeps them honest. Authored events
/// ride on their take-time `t_src`: the caller may have re-tuned the trim
/// under them, so the built-clip time is recomputed here and an event whose
/// moment was trimmed away is dropped rather than pinned to a wrong second.
/// `None` comes back only when there is nothing to say at all — no contact
/// data in the take and no authored events — which is "never examined", not
/// "examined and empty".
pub(crate) fn derive_events(
    take: &Take,
    edit: &Edit,
    duration: f32,
    authored: &[AnimEvent],
) -> Option<Vec<AnimEvent>> {
    let mut events: Vec<AnimEvent> = Vec::new();
    let mut examined = false;

    if let Some(contacts) = &take.contacts {
        examined = true;
        for (foot, t_raw) in footsteps(contacts, take.fps) {
            let Some(t) = edit.map_time(t_raw, take.fps) else {
                continue;
            };
            if t > duration + 1e-3 {
                continue;
            }
            events.push(AnimEvent {
                t,
                t_src: Some(t_raw),
                name: String::from(match foot {
                    Foot::Left => "footstep_l",
                    Foot::Right => "footstep_r",
                }),
                origin: EventOrigin::Contacts,
                audio: None,
            });
        }
    }

    for authored in authored.iter().filter(|e| !e.origin.is_contacts()) {
        examined = true;
        let mut event = authored.clone();
        if let Some(t_src) = event.t_src {
            let Some(t) = edit.map_time(t_src, take.fps) else {
                continue;
            };
            if t > duration + 1e-3 {
                continue;
            }
            event.t = t;
        }
        events.push(event);
    }

    if !examined {
        return None;
    }
    events.sort_by(|a, b| a.t.total_cmp(&b.t));
    Some(events)
}

/// Copy the take beside the other durable masters, so the shipped clip stays
/// re-bakeable after the sweep directory it came from is swept. Returns
/// where it now lives.
fn keep_take(project: &Project, name: &str, take: &Path) -> Result<PathBuf> {
    let dir = project.takes_dir();
    let target = dir.join(format!("{name}.npz"));
    if take == target {
        return Ok(target);
    }
    std::fs::create_dir_all(&dir).map_err(|e| LibraryError::io(&dir, e))?;
    std::fs::copy(take, &target).map_err(|e| LibraryError::io(&target, e))?;
    Ok(target)
}

/// The one-line account of what shipped, for the status line.
fn describe_clip(record: &Sidecar, name: &str) -> String {
    let measured = record.measured.as_ref();
    let frames = measured.and_then(|m| m.frames).unwrap_or(0);
    let duration = measured.and_then(|m| m.duration_s).unwrap_or(0.0);
    let events = record.events.as_ref().map_or(0, Vec::len);
    format!("baked {name}.glb: {frames} frames, {duration:.2}s, {events} event(s)")
}

// ----------------------------------------------------------------- bodies ---

/// One rigged body, exported from a `.blend`, to ingest into the library.
#[derive(Debug, Clone)]
pub struct PromoteBody {
    /// Output stem: `<assets>/bodies/<name>.glb`.
    pub name: String,
    /// The exported `.glb` to ingest, copied verbatim — no generator runs.
    pub glb_path: PathBuf,
    /// The committed `.blend` it was exported from, under the project. It
    /// is the provenance claim: hashed into `source`, and `verify` expects
    /// it to still exist.
    pub blend_path: Option<PathBuf>,
    /// The TRELLIS.2 run's record, when the mesh was lifted. With it the
    /// generator block is recorded; without it there is no generator block
    /// and the provenance says so.
    pub lift_record: Option<GeneratorRecord>,
    /// The auto-rig's record.
    pub rig_record: Option<GeneratorRecord>,
    /// The export's record.
    pub export_record: Option<GeneratorRecord>,
    /// What the body is, for the catalog.
    pub prompt: Option<String>,
    /// Curation tags. Empty keeps a replaced body's tags.
    pub tags: Vec<String>,
    /// Anything the next reader should know.
    pub note: Option<String>,
    /// Who is promoting.
    pub created_by: Actor,
    /// Whether replacing an existing body of this name is intended.
    pub overwrite: bool,
}

/// Ingest a body `.glb`: measure, validate, copy verbatim, record, project.
///
/// Validation here is the engine-free half of the rig contract: the file
/// parses, is self-contained, is skinned, carries every contract bone, stands
/// inside the stature band with its feet on the ground. The engine half —
/// rest rotations, clip binding, weight sums — is the studio's rig check,
/// which the `promote-mesh` recipe runs before this.
///
/// # Errors
///
/// Fails on an unusable name, an unintended collision, a `.glb` that
/// [`measure_glb`] refuses, a body missing contract joints or outside the
/// stature band, a named `.blend` that does not exist or lies outside the
/// project, or a failed write.
pub fn promote_body(project: &Project, request: &PromoteBody) -> Result<Promoted> {
    let name = validate_name(&request.name)?;
    let target = project.kind_dir(Kind::Body).join(format!("{name}.glb"));
    guard_collision(&name, &target, request.overwrite)?;

    let bytes = read_bytes(&request.glb_path)?;
    let measured = measure_glb(&bytes).map_err(|e| LibraryError::bake(e.to_string()))?;
    let profile = project.profile()?;
    check_contract(&profile.contract, &measured)?;
    let source = blend_source(project, request.blend_path.as_deref())?;

    write_atomic(&target, &bytes)?;

    let previous = sidecar::load_beside(&target).unwrap_or(None);
    let post_records: Vec<&GeneratorRecord> = request
        .rig_record
        .iter()
        .chain(request.export_record.iter())
        .collect();
    let mut record = mesh_record(
        Kind::Body,
        &name,
        &measured,
        source,
        request.lift_record.as_ref(),
        &post_records,
        request.prompt.as_deref(),
        &request.tags,
        request.note.as_deref(),
        &request.created_by,
        previous.as_ref(),
    );
    record.rig = Some(profile.contract.name.clone());
    record.content_hash = hash::sha256_file(&target)?;
    let sidecar_path = sidecar::path_for(&target);
    let record = save_record(&sidecar_path, &record)?;

    let mut report = format!(
        "ingested {name}.glb: {} verts, {} tris, {} bones, {:.2} m tall{}",
        measured.vertices,
        measured.triangles,
        measured.bones_skinned,
        measured.bounds[1][1] - measured.lowest_y,
        record
            .source
            .path
            .as_deref()
            .map_or_else(String::new, |p| format!("; source {p}")),
    );
    refresh_manifest(project, &mut report);
    Ok(Promoted {
        rel_path: project
            .rel_to_assets(&target)
            .unwrap_or_else(|| format!("{}/{name}.glb", Kind::Body.dir())),
        asset: target,
        sidecar: sidecar_path,
        report,
        record,
        replaced: previous,
    })
}

/// A body must carry the whole contract skeleton: a missing bone would not
/// fail at load, it would bind nothing and hold its rest pose — the silent
/// failure this pipeline refuses early wherever it can. And it must be a
/// body-sized thing standing on the ground, or every clip plays on a giant
/// hovering above the floor.
fn check_contract(contract: &forge_rig::Contract, measured: &GlbMeasurement) -> Result<()> {
    let missing = forge_rig::missing_joints(contract, &measured.joint_names);
    if !missing.is_empty() {
        return Err(LibraryError::rejected(format!(
            "the skin is missing {} contract bone(s): {}",
            missing.len(),
            missing.join(", ")
        )));
    }
    let stature = measured.bounds[1][1] - measured.lowest_y;
    let band = contract.stature_m;
    if stature < band.min || stature > band.max {
        return Err(LibraryError::rejected(format!(
            "stature {stature:.2} m is outside the contract's {:.2}–{:.2} m — a prop mistaken \
             for a body, or a mesh still in raw units",
            band.min, band.max
        )));
    }
    if measured.lowest_y.abs() > contract.foot_tolerance_m {
        return Err(LibraryError::rejected(format!(
            "the lowest vertex sits at y = {:.3} m; the contract wants feet within {:.3} m \
             of the ground",
            measured.lowest_y, contract.foot_tolerance_m
        )));
    }
    Ok(())
}

/// The `.blend` a mesh names as its source, as the record states it: a path
/// under the project, hashed. A `.blend` outside the project cannot be a
/// durable source — nothing would commit it — so it is refused.
fn blend_source(project: &Project, blend: Option<&Path>) -> Result<Source> {
    let Some(blend) = blend else {
        return Ok(Source::default());
    };
    if !blend.is_file() {
        return Err(LibraryError::rejected(format!(
            "source {} does not exist — commit the .blend before promoting what was \
             exported from it",
            blend.display()
        )));
    }
    let absolute = blend
        .canonicalize()
        .map_err(|e| LibraryError::io(blend, e))?;
    let root = project
        .root
        .canonicalize()
        .map_err(|e| LibraryError::io(&project.root, e))?;
    let Some(rel) = crate::rel_string(&absolute, &root) else {
        return Err(LibraryError::rejected(format!(
            "source {} is outside the project at {} — a .blend nobody commits is not a \
             provenance claim; keep it under {}",
            blend.display(),
            project.root.display(),
            project.blender_dir().display()
        )));
    };
    Ok(Source {
        path: Some(rel),
        sha256: Some(hash::sha256_file(blend)?),
        skeleton: None,
    })
}

/// The record a mesh promote writes: the ingest owns the generator block,
/// the measurements, the source and the hash; the caller — or, on a
/// re-promote, the record being replaced — owns the curation.
///
/// Provenance is `recorded` when the lift record is in hand: every knob in
/// the generator block then comes from the run that produced the mesh.
/// Without it the block is absent and the record says `reconstructed`: the
/// measurements are true of the bytes, and nothing is claimed about how
/// they came to be.
#[expect(
    clippy::too_many_arguments,
    reason = "one record from every input a mesh door has; a builder would only rename them"
)]
fn mesh_record(
    kind: Kind,
    name: &str,
    measured: &GlbMeasurement,
    source: Source,
    lift_record: Option<&GeneratorRecord>,
    post_records: &[&GeneratorRecord],
    prompt: Option<&str>,
    tags: &[String],
    note: Option<&str>,
    created_by: &Actor,
    previous: Option<&Sidecar>,
) -> Sidecar {
    let mut record = Sidecar::new(kind, name);
    record.source = source;
    record.measured = Some(Measured {
        mesh: Some(MeshMeasured::from(measured)),
        ..Measured::default()
    });
    if let Some(lift) = lift_record {
        let mut params: LiftParams = lift.lift_params();
        if !post_records.is_empty() {
            params.post = Some(PostStep {
                tool: String::from("blender"),
                script: Some(
                    post_records
                        .iter()
                        .map(|r| r.kind.as_str())
                        .collect::<Vec<_>>()
                        .join("+"),
                ),
                version: measured.generator.clone(),
            });
        }
        record.generator = Some(Generator::Trellis2(params));
        record.provenance = Provenance::Recorded;
        if lift.fake || post_records.iter().any(|r| r.fake) {
            record.note = Some(String::from(
                "generated with --fake: a placeholder that passes the validators and \
                 nothing else",
            ));
        }
    } else {
        record.generator = None;
        record.provenance = Provenance::Reconstructed;
    }
    record.prompt = prompt
        .map(str::to_owned)
        .or_else(|| lift_record.and_then(|r| r.prompt().map(str::to_owned)));
    if let Some(note) = note {
        record.note = Some(note.to_owned());
    }
    record.created_by = created_by.clone();
    if record.created_by == Actor::Unknown
        && let Some(lift) = lift_record
    {
        record.created_by = Actor::parse(&lift.created_by);
    }
    if !tags.is_empty() {
        record.tags = tags.to_vec();
    } else if let Some(previous) = previous {
        record.tags.clone_from(&previous.tags);
    }
    record
}

// ----------------------------------------------------------------- models ---

/// One static mesh — a prop, a fixture, a held weapon — to ingest.
#[derive(Debug, Clone)]
pub struct PromoteModel {
    /// Output stem: `<assets>/models/<name>.glb`.
    pub name: String,
    /// The normalized `.glb` to ingest, copied verbatim.
    pub glb_path: PathBuf,
    /// A committed `.blend`, when the prop has one.
    pub blend_path: Option<PathBuf>,
    /// The TRELLIS.2 run's record, when the mesh was lifted.
    pub lift_record: Option<GeneratorRecord>,
    /// The prop normalizer's record.
    pub prop_record: Option<GeneratorRecord>,
    /// What the model is, for the catalog.
    pub prompt: Option<String>,
    /// Curation tags. Empty keeps a replaced model's tags.
    pub tags: Vec<String>,
    /// Anything the next reader should know.
    pub note: Option<String>,
    /// Who is promoting.
    pub created_by: Actor,
    /// Whether replacing an existing model of this name is intended.
    pub overwrite: bool,
}

/// Ingest a model `.glb`: measure its bounds, copy verbatim, record, project.
///
/// A model makes no rig claim and is checked against none; what its
/// consumers hold it to is its bounds — a decor kind's height, a weapon's
/// reach — which the manifest publishes as `bounds_m`.
///
/// # Errors
///
/// Fails on an unusable name, an unintended collision, a `.glb` that
/// [`measure_glb_geometry`] refuses, a named `.blend` that does not exist or
/// lies outside the project, or a failed write.
pub fn promote_model(project: &Project, request: &PromoteModel) -> Result<Promoted> {
    let name = validate_name(&request.name)?;
    let target = project.kind_dir(Kind::Model).join(format!("{name}.glb"));
    guard_collision(&name, &target, request.overwrite)?;

    let bytes = read_bytes(&request.glb_path)?;
    let measured = measure_glb_geometry(&bytes).map_err(|e| LibraryError::bake(e.to_string()))?;
    let source = blend_source(project, request.blend_path.as_deref())?;

    write_atomic(&target, &bytes)?;

    let previous = sidecar::load_beside(&target).unwrap_or(None);
    let post_records: Vec<&GeneratorRecord> = request.prop_record.iter().collect();
    let mut record = mesh_record(
        Kind::Model,
        &name,
        &measured,
        source,
        request.lift_record.as_ref(),
        &post_records,
        request.prompt.as_deref(),
        &request.tags,
        request.note.as_deref(),
        &request.created_by,
        previous.as_ref(),
    );
    record.content_hash = hash::sha256_file(&target)?;
    let sidecar_path = sidecar::path_for(&target);
    let record = save_record(&sidecar_path, &record)?;

    let [min, max] = measured.bounds;
    let mut report = format!(
        "ingested {name}.glb: {} verts, {} tris, {:.2} × {:.2} × {:.2} m",
        measured.vertices,
        measured.triangles,
        max[0] - min[0],
        max[1] - min[1],
        max[2] - min[2],
    );
    refresh_manifest(project, &mut report);
    Ok(Promoted {
        rel_path: project
            .rel_to_assets(&target)
            .unwrap_or_else(|| format!("{}/{name}.glb", Kind::Model.dir())),
        asset: target,
        sidecar: sidecar_path,
        report,
        record,
        replaced: previous,
    })
}

// ------------------------------------------------------------------ audio ---

/// One audio file to copy into the library.
#[derive(Debug, Clone)]
pub struct PromoteAudio {
    /// Which audio directory it belongs in: sfx, music or voice.
    pub kind: Kind,
    /// Output stem.
    pub name: String,
    /// The file to copy in.
    pub file: PathBuf,
    /// The generator run's record, when the sound came with one. With it
    /// the generator block is recorded; without it nothing is known about
    /// how the sound was made and the provenance says `unknown`.
    pub record: Option<GeneratorRecord>,
    /// What it is: the description it was generated from, or for voice, the
    /// spoken line. `None` lets the record's prompt stand.
    pub prompt: Option<String>,
    /// Curation tags.
    pub tags: Vec<String>,
    /// Anything the next reader should know.
    pub note: Option<String>,
    /// Who is promoting.
    pub created_by: Actor,
    /// Whether replacing an existing sound of this name *in this kind* is
    /// intended. A stem already taken by another audio kind is refused
    /// regardless: a game's audio map is by stem.
    pub overwrite: bool,
}

/// Copy an audio file into the library and write its sidecar.
///
/// No generator runs: neither MOSS nor ACE-Step is bit-reproducible, so the
/// approved asset has to be the exact bytes that were auditioned. That is also
/// why the sidecar gets a fresh content hash — reproduction is off the table,
/// so integrity is what remains provable. The one measurement taken is the
/// duration, decoded from the bytes, because a manifest that guessed it would
/// be the same bug as a sidecar recording a default.
///
/// # Errors
///
/// Fails on an unusable name, a kind that is not audio, a missing or
/// extension-less payload, a file that does not decode, a record of the
/// wrong kind, a stem another audio kind already uses, an unintended
/// collision, or a failed copy.
pub fn promote_audio(project: &Project, request: &PromoteAudio) -> Result<Promoted> {
    let name = validate_name(&request.name)?;
    if !request.kind.is_audio() {
        return Err(LibraryError::rejected(format!(
            "{} is not an audio kind — use sfx, music or voice",
            request.kind
        )));
    }
    let extension = request
        .file
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| {
            LibraryError::rejected(format!(
                "{} has no extension, so nothing can tell what it is",
                request.file.display()
            ))
        })?
        .to_ascii_lowercase();
    let directory = project.kind_dir(request.kind);
    let target = directory.join(format!("{name}.{extension}"));

    // A game's audio map is by stem, so `hit` is one sound across every
    // audio directory: a voice line called `hit` beside an sfx called `hit`
    // would make which of them plays a matter of load order.
    for other in Kind::AUDIO.into_iter().filter(|k| *k != request.kind) {
        if let Some(existing) = same_stem(&project.kind_dir(other), &name) {
            return Err(LibraryError::rejected(format!(
                "{name} is already a {other}: {} — a game's audio map is by stem, so pick \
                 another name",
                project
                    .rel_to_assets(&existing)
                    .unwrap_or_else(|| existing.display().to_string())
            )));
        }
    }
    let shipped = same_stem(&directory, &name);
    if let Some(existing) = &shipped
        && !request.overwrite
    {
        return Err(LibraryError::WouldOverwrite {
            name: name.clone(),
            path: existing.clone(),
        });
    }
    if let Some(record) = &request.record {
        let expected = match request.kind {
            Kind::Sfx => RecordKind::Sfx,
            Kind::Music => RecordKind::Music,
            _ => RecordKind::Speech,
        };
        if record.kind != expected {
            return Err(LibraryError::rejected(format!(
                "the record describes a {} run, not a {} — it is not this sound's record",
                record.kind, request.kind
            )));
        }
    }

    let audio = forge_audio::decode(&request.file).map_err(|e| {
        LibraryError::rejected(format!(
            "{} does not decode as audio: {e}",
            request.file.display()
        ))
    })?;
    let duration = audio.duration();

    let bytes = read_bytes(&request.file)?;
    write_atomic(&target, &bytes)?;

    // A replacement in a different container has to take the old file with it.
    // `bark.wav` landing beside a shipped `bark.ogg` would leave two assets
    // sharing one `bark.json`, and which of them a game loads is a matter of
    // directory order — the sort of thing that is discovered as "the new bark
    // did not take effect" months later.
    let mut report = String::new();
    let previous = sidecar::load_beside(&target).unwrap_or(None);
    if let Some(existing) = shipped.filter(|existing| *existing != target) {
        std::fs::remove_file(&existing).map_err(|e| LibraryError::io(&existing, e))?;
        report = format!(
            "replaced {}\n",
            project
                .rel_to_assets(&existing)
                .unwrap_or_else(|| existing.display().to_string())
        );
    }

    let mut record = Sidecar::new(request.kind, &name);
    record.prompt = request.prompt.clone().or_else(|| {
        request
            .record
            .as_ref()
            .and_then(|r| r.prompt().map(str::to_owned))
    });
    record.tags.clone_from(&request.tags);
    if record.tags.is_empty()
        && let Some(previous) = &previous
    {
        record.tags.clone_from(&previous.tags);
    }
    record.note.clone_from(&request.note);
    record.created_by = request.created_by.clone();
    if let Some(run) = &request.record {
        record.generator = Some(match request.kind {
            Kind::Sfx => Generator::MossSoundEffect(run.sound_effect_params()),
            Kind::Music => Generator::AceStep(run.ace_step_params()),
            _ => Generator::MossTts(run.speech_params()),
        });
        record.provenance = Provenance::Recorded;
        if record.created_by == Actor::Unknown {
            record.created_by = Actor::parse(&run.created_by);
        }
        if run.fake {
            record.note = Some(String::from(
                "generated with --fake: a placeholder that passes the validators and \
                 nothing else",
            ));
        }
    } else {
        record.generator = None;
        record.provenance = Provenance::Unknown;
    }
    record.measured = Some(Measured {
        duration_s: Some(duration),
        ..Measured::default()
    });
    record.content_hash = hash::sha256_file(&target)?;
    let sidecar_path = sidecar::path_for(&target);
    let record = save_record(&sidecar_path, &record)?;

    {
        use std::fmt::Write as _;
        let _ = write!(
            report,
            "shipped {name}.{extension}: {duration:.2}s, {} Hz, {} channel(s)",
            audio.sample_rate, audio.channels
        );
    }
    refresh_manifest(project, &mut report);
    Ok(Promoted {
        rel_path: project
            .rel_to_assets(&target)
            .unwrap_or_else(|| format!("{}/{name}.{extension}", request.kind.dir())),
        asset: target,
        sidecar: sidecar_path,
        report,
        record,
        replaced: previous,
    })
}

/// A shipped asset of this name in this directory, whatever its extension.
///
/// The audio library holds `.wav`, `.ogg`, `.mp3` and `.flac`, and `bark` is
/// one asset in it regardless of which. The gate used to compare the whole file
/// name, so promoting `bark.wav` over a shipped `bark.ogg` sailed straight
/// through a check whose entire job is to stop exactly that.
///
/// The sidecar is skipped: `bark.json` shares the stem by design, and treating
/// the record as a colliding asset would make every re-promote impossible.
fn same_stem(directory: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(directory)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .find(|path| {
            path.file_stem().is_some_and(|stem| stem == name)
                && path.extension().is_none_or(|ext| ext != "json")
        })
}

// ----------------------------------------------------------------- shared ---

/// Write the record and hand back what was written.
///
/// Read back rather than returned as built: the writer rounds every event
/// time and root-track sample to three decimals, and a caller comparing the
/// record it was handed against the file beside the asset must find them
/// equal — the file is the truth, and the in-memory value is what the file
/// says.
fn save_record(path: &Path, record: &Sidecar) -> Result<Sidecar> {
    sidecar::save(path, record)?;
    sidecar::load(path)
}

/// Rewrite the manifest so the library's committed projection includes what
/// just shipped.
///
/// Best-effort by design: at this point the asset and its sidecar are already
/// on disk and consistent, and failing the promote now would leave a shipped
/// asset looking failed. A stale manifest is the lesser harm — `manifest
/// --check` fails CI until `just manifest` repairs it, and the warning below
/// says exactly that.
fn refresh_manifest(project: &Project, report: &mut String) {
    match manifest::write(project) {
        Ok(_) => report.push_str("\nmanifest refreshed"),
        Err(err) => {
            use std::fmt::Write as _;
            let _ = write!(
                report,
                "\nWARNING: manifest not refreshed ({err}) — run `just manifest`"
            );
        }
    }
}

/// Refuse to replace a shipped asset unless that was the point.
///
/// The default has to be refusal. An agent that picks a name already in use
/// has almost certainly not looked, and the cost of guessing wrong is somebody
/// else's asset — which, unlike a rejected call, is not recoverable from the
/// conversation.
fn guard_collision(name: &str, target: &Path, overwrite: bool) -> Result<()> {
    if target.exists() && !overwrite {
        return Err(LibraryError::WouldOverwrite {
            name: name.to_owned(),
            path: target.to_path_buf(),
        });
    }
    Ok(())
}

/// Check an asset name is usable as a file stem and an asset key.
///
/// Lower-case letters, digits, underscores — kept because these names end up
/// in file paths, in glTF clip names and in a game's asset keys, and a name
/// that survives all three is a small set.
///
/// # Errors
///
/// Fails with the rule spelled out, since the caller may well be an agent that
/// can fix its own call next turn.
pub fn validate_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(LibraryError::rejected(format!(
            "{name:?} is not a usable asset name — use lower-case letters, digits and underscores"
        )));
    }
    Ok(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clip_name_follows_the_loop_convention() {
        let plain = ClipRecipe::default();
        assert_eq!(clip_name_for("roll", &plain), "roll");

        let stated = ClipRecipe {
            clip: Some(String::from("Roll")),
            ..ClipRecipe::default()
        };
        assert_eq!(clip_name_for("roll", &stated), "Roll");

        let looping = ClipRecipe {
            clip: Some(String::from("Walk")),
            looping: true,
            ..ClipRecipe::default()
        };
        assert_eq!(clip_name_for("walk", &looping), "Walk-loop");

        let already = ClipRecipe {
            clip: Some(String::from("Walk-loop")),
            looping: true,
            ..ClipRecipe::default()
        };
        assert_eq!(clip_name_for("walk", &already), "Walk-loop");
    }

    #[test]
    fn events_carry_authored_times_and_recompute_derived_ones() {
        let take = Take {
            rotations: vec![[glam::Quat::IDENTITY; forge_motion::skeleton::JOINT_COUNT]; 40],
            root: vec![glam::Vec3::ZERO; 40],
            contacts: Some(
                (0..40)
                    .map(|i| {
                        let down = !(10..20).contains(&i);
                        [down, down, true, true]
                    })
                    .collect(),
            ),
            fps: 20.0,
            prompt: String::new(),
        };
        // Trim the first half second away: the left strike at frame 20 (1.0s)
        // must land at 0.5s on the built clock, and an authored event at the
        // same source moment must move with it.
        let edit = Edit {
            trim_start: 10,
            ..Edit::default()
        };
        let authored = vec![AnimEvent {
            t: 999.0,
            t_src: Some(1.0),
            name: String::from("fire"),
            origin: EventOrigin::Agent(String::from("tester")),
            audio: None,
        }];

        let events =
            derive_events(&take, &edit, 29.0 / 20.0, &authored).expect("contacts were examined");
        let fire = events.iter().find(|e| e.name == "fire").expect("authored");
        assert!((fire.t - 0.5).abs() < 1e-4, "{}", fire.t);
        let step = events
            .iter()
            .find(|e| e.name == "footstep_l")
            .expect("derived");
        assert_eq!(step.origin, EventOrigin::Contacts);
        assert!((step.t - 0.5).abs() < 1e-4, "{}", step.t);
    }

    #[test]
    fn a_take_without_contacts_and_no_authored_events_says_nothing() {
        let take = Take {
            rotations: vec![[glam::Quat::IDENTITY; forge_motion::skeleton::JOINT_COUNT]; 4],
            root: vec![glam::Vec3::ZERO; 4],
            contacts: None,
            fps: 20.0,
            prompt: String::new(),
        };
        assert_eq!(derive_events(&take, &Edit::default(), 0.15, &[]), None);
    }

    #[test]
    fn authored_events_are_checked_and_signed() {
        let fire = AnimEvent {
            t: 0.0,
            t_src: Some(1.0),
            name: String::from("fire"),
            origin: EventOrigin::Unknown,
            audio: None,
        };
        let checked = checked_events(
            std::slice::from_ref(&fire),
            &Actor::Agent(String::from("claude")),
        )
        .expect("valid");
        assert_eq!(
            checked[0].origin,
            EventOrigin::Agent(String::from("claude"))
        );

        let bad_name = AnimEvent {
            name: String::from("Fire!"),
            ..fire.clone()
        };
        assert!(checked_events(&[bad_name], &Actor::Human).is_err());
        let stated_step = AnimEvent {
            name: String::from("footstep_l"),
            origin: EventOrigin::Contacts,
            ..fire
        };
        let error = checked_events(&[stated_step], &Actor::Human).expect_err("refuse");
        assert!(error.to_string().contains("derived"), "{error}");
    }

    #[test]
    fn a_name_that_cannot_be_a_path_is_refused() {
        assert!(validate_name("roll").is_ok());
        for bad in ["", "Gen Roll", "../escape", "gen-roll"] {
            let error = validate_name(bad).expect_err("should refuse");
            assert!(format!("{error}").contains("lower-case"), "{error}");
        }
    }

    #[test]
    fn an_existing_asset_is_not_replaced_by_accident() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("roll.glb");
        std::fs::write(&target, b"existing").expect("write");
        let error = guard_collision("roll", &target, false).expect_err("refuse");
        assert!(
            error.to_string().starts_with("roll already exists"),
            "{error}"
        );
        assert!(guard_collision("roll", &target, true).is_ok());
    }
}
