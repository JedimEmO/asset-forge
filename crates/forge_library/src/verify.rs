//! The checks that need no engine: does the library say true things?
//!
//! The studio keeps the part that needs Bevy — binding every clip to a body
//! and reading the weights — because that is the claim worth the most and it
//! needs an animation player to make. Everything here is the other half, and
//! it is the half that also has to run in the MCP server and in CI on a
//! machine with no GPU.
//!
//! The distinction the reports draw is between **generation** and
//! **integrity**. A clip can be proved to rebuild (`audit`); a sound or a
//! lifted mesh cannot, because neither MOSS, ACE-Step, TRELLIS.2 nor Blender's
//! exporter is bit-reproducible. So for those the claim is narrower and stated
//! as such: the file is the one that was measured and approved, not the file
//! that the prompt would produce again.
//!
//! What is checked, in order of how much it would mean if it failed:
//!
//! 1. every sidecar parses at schema 1 and its `content_hash` matches the
//!    file beside it;
//! 2. every clip's take exists where the record says and still hashes to
//!    what was recorded; the clip stands on the project's rig;
//! 3. every body and model `.glb` is self-contained — one JSON and one BIN
//!    chunk, no `uri` — and a body stands in the contract's stature band
//!    with its feet on the ground; a named `.blend` still exists (drift is a
//!    warning, absence a failure);
//! 4. every event names a usable thing at a time on the clip, every
//!    contacts-derived event speaks the footstep vocabulary, every authored
//!    time agrees with its take time through the recipe, and every audio
//!    link resolves to a shipped sound;
//! 5. every reference PNG under `<sources>/refs` has a row in `SOURCES.md`;
//! 6. every voice under `<sources>/voices/<name>/ref.*` is accounted for:
//!    a `voice.json` beside it (a `voice` record whose output hash is the
//!    file) or a row in `SOURCES.md` (a brought clip), and a voice line's
//!    reference is still where its sidecar says;
//! 7. the rig profile has not drifted from its artifacts: the `.glb` hash
//!    mismatching is a failure, the `.blend` a warning, and the bones derived
//!    from the `.glb` still match the contract.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::generator_record::{GeneratorRecord, RecordKind};
use crate::report::{Report, Severity};
use crate::schema::{DEFAULT_FPS, Kind, Sidecar, valid_event_name};
use crate::{Catalog, Project, hash};

/// Slack past the measured duration before an event time counts as out of
/// range. Both numbers are rounded to three decimals on write, so each can
/// sit half a millisecond from what was measured; a full millisecond covers
/// the pair without excusing anything a human could notice.
const EVENT_ROUND_SLACK_S: f32 = 0.001;

/// How far an event's built-clip time may sit from its take time mapped
/// through the recipe: 25 ms, half a frame at ARDY's 20 fps. Tighter would
/// fail honest frame rounding; looser would pass a footstep on the wrong
/// frame.
const EVENT_MAP_TOLERANCE_S: f32 = 0.025;

/// The generator record beside a designed voice's clip.
const VOICE_RECORD: &str = "voice.json";

/// What a self-contained `.glb` container holds, read with nothing but the
/// container format — the questions an outside consumer would ask of the
/// file, answered without the exporter that wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlbSummary {
    /// Total bytes.
    pub bytes: usize,
    /// Node count.
    pub nodes: usize,
    /// Mesh count.
    pub meshes: usize,
    /// Embedded image count.
    pub images: usize,
    /// Skin count.
    pub skins: usize,
    /// The glTF `asset.generator` string, when the file declares one.
    pub generator: Option<String>,
}

const GLB_MAGIC: &[u8; 4] = b"glTF";
const GLB_VERSION: u32 = 2;
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;

/// Read a `.glb` container back and prove it is one self-contained file.
///
/// Plain chunk parsing and plain JSON on purpose: this asks what an outside
/// consumer would find in the file, and answering it through a glTF reader
/// would only prove the reader agrees with the exporter. Exactly one JSON
/// chunk and one BIN chunk, a declared length that matches, and no `uri` on
/// any buffer or image — a `.glb` that references files on the author's disk
/// renders pink everywhere else.
///
/// # Errors
///
/// The finding, phrased for the person who has to fix the export.
pub fn verify_glb(data: &[u8]) -> std::result::Result<GlbSummary, String> {
    if data.len() < 12 || &data[..4] != GLB_MAGIC {
        return Err(String::from("not a .glb container"));
    }
    let u32_at = |offset: usize| -> Option<u32> {
        data.get(offset..offset + 4)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
    };
    let version = u32_at(4).unwrap_or(0);
    if version != GLB_VERSION {
        return Err(format!("glB version {version}, want {GLB_VERSION}"));
    }
    let length = u32_at(8).unwrap_or(0) as usize;
    if length != data.len() {
        return Err(format!(
            "declares {length} bytes but is {} — the file is truncated",
            data.len()
        ));
    }

    let mut chunks: Vec<(u32, &[u8])> = Vec::new();
    let mut offset = 12;
    while offset + 8 <= data.len() {
        let chunk_length = u32_at(offset).unwrap_or(0) as usize;
        let chunk_type = u32_at(offset + 4).unwrap_or(0);
        offset += 8;
        let end = (offset + chunk_length).min(data.len());
        chunks.push((chunk_type, &data[offset..end]));
        offset = end + (chunk_length.wrapping_neg() % 4);
    }
    let json_chunks: Vec<&[u8]> = chunks
        .iter()
        .filter(|(kind, _)| *kind == CHUNK_JSON)
        .map(|(_, payload)| *payload)
        .collect();
    let bin_chunks = chunks.iter().filter(|(kind, _)| *kind == CHUNK_BIN).count();
    if json_chunks.len() != 1 || bin_chunks != 1 {
        let kinds: Vec<String> = chunks
            .iter()
            .map(|(kind, _)| format!("0x{kind:08X}"))
            .collect();
        return Err(format!(
            "holds {} JSON and {bin_chunks} BIN chunk(s) (types: {}), want exactly one of each",
            json_chunks.len(),
            if kinds.is_empty() {
                String::from("none")
            } else {
                kinds.join(", ")
            }
        ));
    }

    let document: serde_json::Value =
        serde_json::from_slice(json_chunks[0]).map_err(|e| format!("JSON chunk: {e}"))?;
    let mut external = Vec::new();
    for section in ["buffers", "images"] {
        if let Some(entries) = document.get(section).and_then(serde_json::Value::as_array) {
            for (index, entry) in entries.iter().enumerate() {
                if let Some(uri) = entry.get("uri").and_then(serde_json::Value::as_str) {
                    external.push(format!("{section}[{index}] -> {uri}"));
                }
            }
        }
    }
    if !external.is_empty() {
        return Err(format!(
            "points outside itself ({}) — a .glb that references files on the author's disk \
             renders pink everywhere else",
            external.join(", ")
        ));
    }
    let count = |section: &str| {
        document
            .get(section)
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len)
    };
    Ok(GlbSummary {
        bytes: data.len(),
        nodes: count("nodes"),
        meshes: count("meshes"),
        images: count("images"),
        skins: count("skins"),
        generator: document
            .get("asset")
            .and_then(|a| a.get("generator"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    })
}

/// Check every shipped asset: its sidecar parses, its bytes are the ones the
/// sidecar recorded, its sources are where it says, and every event it
/// carries says something true.
#[must_use]
pub fn library(project: &Project) -> Report {
    let catalog = Catalog::scan(project);
    let audio = audio_in(&catalog);
    let profile_name = project.profile().ok().map(|p| p.contract.name);
    let mut report = Report::default();
    for record in catalog.records() {
        report.checked += 1;
        if let Some(error) = &record.sidecar_error {
            report.fail(&record.name, format!("sidecar does not parse: {error}"));
            continue;
        }
        let Some(sidecar) = &record.sidecar else {
            report.warn(&record.name, "no sidecar — nothing is known about it");
            continue;
        };
        if sidecar.content_hash.is_empty() {
            report.fail(&record.name, "no content hash — integrity is unprovable");
        } else {
            match hash::sha256_file(&record.path) {
                Err(err) => report.fail(&record.name, format!("cannot be hashed: {err}")),
                Ok(actual) if actual != sidecar.content_hash => report.fail(
                    &record.name,
                    "content hash does not match the file beside it",
                ),
                Ok(_) => {}
            }
        }
        if sidecar.name != record.name {
            report.fail(
                &record.name,
                format!("sidecar names itself {:?}", sidecar.name),
            );
        }
        if sidecar.kind != record.kind {
            report.fail(
                &record.name,
                format!(
                    "sidecar says it is a {} but it lives under {}",
                    sidecar.kind,
                    record.kind.dir()
                ),
            );
        }
        match record.kind {
            Kind::Clip => clip_findings(
                &mut report,
                project,
                record,
                sidecar,
                profile_name.as_deref(),
            ),
            Kind::Body | Kind::Model => mesh_findings(&mut report, project, record, sidecar),
            Kind::Voice => voice_line_findings(&mut report, project, record, sidecar),
            Kind::Sfx | Kind::Music => {}
        }
        event_findings(&mut report, &record.name, sidecar, &audio);
    }
    report
}

/// What a clip's record has to say for itself: the take it claims to be
/// built from is there and unchanged, and the clip stands on this project's
/// rig.
fn clip_findings(
    report: &mut Report,
    project: &Project,
    record: &crate::AssetRecord,
    sidecar: &Sidecar,
    profile_name: Option<&str>,
) {
    let name = &record.name;
    match sidecar.source.path.as_deref() {
        None => report.fail(
            name,
            "no source take recorded — the clip cannot be re-baked or audited",
        ),
        Some(source) => {
            let take = project.root.join(source);
            if take.is_file() {
                source_hash_findings(
                    report,
                    name,
                    source,
                    &take,
                    sidecar.source.sha256.as_deref(),
                    Severity::Failure,
                );
            } else {
                report.fail(
                    name,
                    format!("take {source} is gone — the recipe has nothing to replay on"),
                );
            }
        }
    }
    if sidecar.recipe.is_none() {
        report.fail(name, "no recipe — the clip claims no reproduction");
    }
    if let Some(profile) = profile_name
        && sidecar.rig.as_deref() != Some(profile)
    {
        report.fail(
            name,
            format!(
                "baked against rig {:?}, but this project's profile is {profile:?}",
                sidecar.rig.as_deref().unwrap_or("<none>")
            ),
        );
    }
}

/// What a body's or a model's record has to say for itself.
///
/// A mesh's claim is integrity plus provenance, never regeneration — so what
/// is demanded of it is exactly that claim: the file is self-contained, the
/// committed `.blend` it points at is still there, and the hash says whether
/// the draft has moved on (a note, not a defect — iterating on the `.blend`
/// without re-promoting is the ordinary state). A body additionally has to
/// stand inside the contract's stature band with its feet on the ground.
fn mesh_findings(
    report: &mut Report,
    project: &Project,
    record: &crate::AssetRecord,
    sidecar: &Sidecar,
) {
    let name = &record.name;
    match std::fs::read(&record.path) {
        Err(err) => report.fail(name, format!("cannot be read: {err}")),
        Ok(bytes) => {
            if let Err(detail) = verify_glb(&bytes) {
                report.fail(name, detail);
            }
        }
    }
    if sidecar.generator.is_none() {
        report.warn(
            name,
            "no generator block — nothing is known about how it was made",
        );
    }
    match sidecar.source.path.as_deref() {
        // A body is exported from a .blend the rig step wrote, so a body with
        // no source has lost its provenance. A model normalized by `gen prop`
        // never had one: its provenance is the lift record and the prop record
        // in its generator block, and that is the whole story.
        None if sidecar.kind == Kind::Body => {
            report.warn(name, "no source .blend — its provenance points at nothing");
        }
        None => {}
        Some(source) => {
            let blend = project.root.join(source);
            if blend.is_file() {
                source_hash_findings(
                    report,
                    name,
                    source,
                    &blend,
                    sidecar.source.sha256.as_deref(),
                    Severity::Warning,
                );
            } else {
                report.fail(
                    name,
                    format!("source {source} is gone — the record's provenance points at nothing"),
                );
            }
        }
    }
    let mesh = sidecar.measured.as_ref().and_then(|m| m.mesh);
    match mesh {
        None => report.warn(name, "no mesh measurement — its size is unrecorded"),
        Some(mesh) if record.kind == Kind::Body => {
            if let Ok(profile) = project.profile() {
                let contract = &profile.contract;
                let stature = mesh.height();
                if stature < contract.stature_m.min || stature > contract.stature_m.max {
                    report.fail(
                        name,
                        format!(
                            "stature {stature:.2} m is outside the contract's {:.2}–{:.2} m",
                            contract.stature_m.min, contract.stature_m.max
                        ),
                    );
                }
                if mesh.lowest_y.abs() > contract.foot_tolerance_m {
                    report.fail(
                        name,
                        format!(
                            "lowest vertex at y = {:.3} m; the contract wants feet within \
                             {:.3} m of the ground",
                            mesh.lowest_y, contract.foot_tolerance_m
                        ),
                    );
                }
            }
        }
        Some(_) => {}
    }
}

/// What a voice line's record has to say for itself: the reference it was
/// cloned from is still there. Drift is a warning — the voice was
/// re-designed and this line still carries the old one, which is the
/// ordinary state between a re-design and the re-render of its lines — and
/// absence is a failure, because the line's provenance then points at
/// nothing. A line with no source (an uncloned voice, or one promoted
/// without its record) has nothing to check here.
fn voice_line_findings(
    report: &mut Report,
    project: &Project,
    record: &crate::AssetRecord,
    sidecar: &Sidecar,
) {
    let Some(source) = sidecar.source.path.as_deref() else {
        return;
    };
    let reference = project.root.join(source);
    if reference.is_file() {
        source_hash_findings(
            report,
            &record.name,
            source,
            &reference,
            sidecar.source.sha256.as_deref(),
            Severity::Warning,
        );
    } else {
        report.fail(
            &record.name,
            format!("voice reference {source} is gone — the line's provenance points at nothing"),
        );
    }
}

/// A record's source hash holds, or the drift is reported at `drift`
/// severity: a failure for a take (the clip claims to rebuild from it), a
/// warning for a `.blend` (iteration is the normal state).
fn source_hash_findings(
    report: &mut Report,
    name: &str,
    source: &str,
    file: &Path,
    recorded: Option<&str>,
    drift: Severity,
) {
    match recorded {
        None => report.warn(
            name,
            format!("no hash recorded for {source} — drift is unnoticeable"),
        ),
        Some(recorded) => match hash::sha256_file(file) {
            Err(err) => report.fail(name, format!("source {source} cannot be hashed: {err}")),
            Ok(actual) if actual != recorded => {
                let detail = format!(
                    "source {source} has changed since the promote — re-promote with \
                     --overwrite to ship what it is now"
                );
                match drift {
                    Severity::Failure => report.fail(name, detail),
                    Severity::Warning | Severity::Note => report.warn(name, detail),
                }
            }
            Ok(_) => {}
        },
    }
}

/// Every shipped sound, as the `kind:name` strings audio links use.
fn audio_in(catalog: &Catalog) -> HashSet<String> {
    catalog
        .records()
        .iter()
        .filter(|record| record.kind.is_audio())
        .map(|record| format!("{}:{}", record.kind, record.name))
        .collect()
}

/// Hold one record's events to what the rest of the record claims.
///
/// The checks, in order of how wrong they mean things went: the name is in
/// the event alphabet; the time is on the clip; a contact-derived event uses
/// the footstep vocabulary (anything else means something other than the
/// derivation wrote `origin: "contacts"`); an authored event's built-clip
/// time agrees with its take time replayed through the recipe within
/// [`EVENT_MAP_TOLERANCE_S`]; and a linked sound exists. In the shipped
/// library a dangling audio link is a failure: the manifest hands these to
/// games as facts.
fn event_findings(report: &mut Report, subject: &str, sidecar: &Sidecar, audio: &HashSet<String>) {
    let Some(events) = &sidecar.events else {
        return;
    };
    let measured = sidecar.measured.as_ref();
    let duration = measured.and_then(|m| m.duration_s);
    let fps = measured.and_then(|m| m.fps).unwrap_or(DEFAULT_FPS);
    let edit = sidecar.recipe.as_ref().map(|recipe| recipe.to_edit(fps));
    if events.iter().any(|e| e.t_src.is_some())
        && let Some(Err(err)) = &edit
    {
        report.fail(
            subject,
            format!("recipe will not apply, so event times cannot be verified: {err}"),
        );
    }
    for event in events {
        if !valid_event_name(&event.name) {
            report.fail(
                subject,
                format!(
                    "event name {:?} is not [a-z0-9_]+ — it cannot key game code",
                    event.name
                ),
            );
        }
        if event.t < 0.0 {
            report.fail(
                subject,
                format!(
                    "event {} at t {}s is before the clip starts",
                    event.name, event.t
                ),
            );
        } else if let Some(duration) = duration
            && event.t > duration + EVENT_ROUND_SLACK_S
        {
            report.fail(
                subject,
                format!(
                    "event {} at t {}s is past the clip's {duration}s",
                    event.name, event.t
                ),
            );
        }
        if event.origin.is_contacts() && !matches!(event.name.as_str(), "footstep_l" | "footstep_r")
        {
            report.fail(
                subject,
                format!(
                    "event {:?} claims origin \"contacts\", which only writes \
                     footstep_l and footstep_r",
                    event.name
                ),
            );
        }
        if let Some(t_src) = event.t_src
            && let Some(Ok(edit)) = &edit
        {
            match edit.map_time(t_src, fps) {
                None => report.fail(
                    subject,
                    format!(
                        "event {} claims t_src {t_src}s, which the recipe trims away",
                        event.name
                    ),
                ),
                Some(mapped) if (mapped - event.t).abs() > EVENT_MAP_TOLERANCE_S => report.fail(
                    subject,
                    format!(
                        "event {} sits at t {}s but its t_src {t_src}s maps to \
                         {mapped}s through the recipe",
                        event.name, event.t
                    ),
                ),
                Some(_) => {}
            }
        }
        if let Some(link) = &event.audio
            && !audio.contains(&link.to_string())
        {
            report.fail(
                subject,
                format!(
                    "event {} links {link}, which is not in the library",
                    event.name
                ),
            );
        }
    }
}

/// Every sidecar under `assets/` has its asset beside it.
///
/// A hand-deleted `.glb` or `.wav` leaves its record orphaned; the catalog
/// scans assets and loads the record *beside* each one, so an orphan is
/// invisible to every other check — and `just manifest` then carries the
/// residue forever. The record is the one part of an asset nobody can
/// re-derive, so a record describing bytes that are not there is a FAIL
/// naming the file: delete it if the removal was meant, or put the asset
/// back.
#[must_use]
pub fn orphans(project: &Project) -> Report {
    let mut report = Report::default();
    for kind in Kind::ALL {
        let directory = project.kind_dir(kind);
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        let mut records: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        records.sort();
        for record in records {
            report.checked += 1;
            let Some(stem) = record.file_stem().map(std::ffi::OsStr::to_owned) else {
                continue;
            };
            let has_asset = std::fs::read_dir(&directory)
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .any(|path| {
                    path.is_file()
                        && path.file_stem().is_some_and(|s| s == stem)
                        && path.extension().is_none_or(|ext| ext != "json")
                });
            if !has_asset {
                let rel = project
                    .rel_to_assets(&record)
                    .unwrap_or_else(|| record.display().to_string());
                report.fail(
                    stem.to_string_lossy(),
                    format!(
                        "{rel} is an orphan record — the asset beside it is gone. The \
                         record is the part nobody can re-derive: put the asset back, or \
                         delete the record and run `forge manifest` if the removal was \
                         meant"
                    ),
                );
            }
        }
    }
    report
}

/// Every reference PNG under `<sources>/refs` has a row in `SOURCES.md`.
///
/// A reference image claims integrity and a ledger row, never regeneration:
/// the row is where its origin and its licence live, and a PNG without one
/// is a file nobody can account for.
#[must_use]
pub fn refs(project: &Project) -> Report {
    let mut report = Report::default();
    let refs_dir = project.refs_dir();
    let mut pngs = Vec::new();
    collect_pngs(&refs_dir, &mut pngs);
    pngs.sort();
    if pngs.is_empty() {
        return report;
    }
    let ledger_path = project.sources_ledger();
    let ledger = std::fs::read_to_string(&ledger_path).unwrap_or_default();
    let rows = ledger_cells(&ledger);
    for png in pngs {
        report.checked += 1;
        let file_name = png
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let rel = project
            .rel_to_root(&png)
            .unwrap_or_else(|| png.display().to_string());
        if ledger.is_empty() {
            report.fail(
                &file_name,
                format!(
                    "{rel} has no row: {} does not exist",
                    project
                        .rel_to_root(&ledger_path)
                        .unwrap_or_else(|| ledger_path.display().to_string())
                ),
            );
            continue;
        }
        // The row's File cell is the path under refs/, exactly — matching on
        // the bare file name let a copy under another refs/ subdirectory
        // ride an existing row, and a substring match accounted for files a
        // row never named.
        let under_refs = png.strip_prefix(&refs_dir).map_or_else(
            |_| file_name.clone(),
            |p| p.to_string_lossy().replace('\\', "/"),
        );
        if !rows.iter().any(|cell| cell.trim_matches('`') == under_refs) {
            report.fail(
                &file_name,
                format!(
                    "{rel} has no row in {} — a reference image claims a ledger row (its \
                     File cell is `{under_refs}`), or it cannot be accounted for",
                    crate::project::SOURCES_LEDGER
                ),
            );
        }
    }
    report
}

/// Every voice under `<sources>/voices/<name>/ref.*` is accounted for.
///
/// A voice is the one source a project makes rather than brings, so it has
/// two ways to be accounted for and needs one of them: a `voice.json` beside
/// the clip — a `voice` generator record whose output hash is the clip, so
/// the description and the seed that made it are on record — or, for a clip
/// that was brought, a row in `SOURCES.md` where its origin and licence
/// live. A clip with neither is a voice nobody can account for, and every
/// line cloned from it inherits that.
#[must_use]
pub fn voices(project: &Project) -> Report {
    let mut report = Report::default();
    let voices_dir = project.voices_dir();
    let mut clips = Vec::new();
    collect_voice_clips(&voices_dir, &mut clips);
    clips.sort();
    if clips.is_empty() {
        return report;
    }
    let ledger = std::fs::read_to_string(project.sources_ledger()).unwrap_or_default();
    let rows = ledger_cells(&ledger);
    for clip in clips {
        report.checked += 1;
        let voice = clip
            .parent()
            .and_then(Path::file_name)
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let rel = project
            .rel_to_root(&clip)
            .unwrap_or_else(|| clip.display().to_string());
        let record_path = clip.with_file_name(VOICE_RECORD);
        if record_path.is_file() {
            voice_record_findings(&mut report, &voice, &rel, &clip, &record_path);
            continue;
        }
        let brought = rows
            .iter()
            .any(|cell| cell.contains(&format!("voices/{voice}")) || cell.contains(&rel));
        if !brought {
            report.fail(
                &voice,
                format!(
                    "{rel} has no {VOICE_RECORD} beside it and no row in {} — a designed voice \
                     keeps its record (`forge gen voice {voice} --describe \"…\"` writes it), \
                     a brought clip needs a ledger row with its origin and licence",
                    crate::project::SOURCES_LEDGER
                ),
            );
        }
    }
    report
}

/// The record beside a designed voice's clip says what it must: it parses,
/// it is a `voice` run, and its output is this clip.
fn voice_record_findings(
    report: &mut Report,
    voice: &str,
    rel: &str,
    clip: &Path,
    record_path: &Path,
) {
    let record = match GeneratorRecord::load(record_path) {
        Ok(record) => record,
        Err(err) => {
            report.fail(
                voice,
                format!("{VOICE_RECORD} beside {rel} does not read: {err}"),
            );
            return;
        }
    };
    if record.kind != RecordKind::Voice {
        report.fail(
            voice,
            format!(
                "{VOICE_RECORD} beside {rel} describes a {} run, not a voice design",
                record.kind
            ),
        );
        return;
    }
    let Some(recorded) = record.output().and_then(|o| o.sha256.clone()) else {
        report.fail(
            voice,
            format!("{VOICE_RECORD} beside {rel} records no output hash — the clip is unprovable"),
        );
        return;
    };
    match hash::sha256_file(clip) {
        Err(err) => report.fail(voice, format!("{rel} cannot be hashed: {err}")),
        Ok(actual) if actual != recorded => report.fail(
            voice,
            format!(
                "{rel} is not the clip its {VOICE_RECORD} describes — a designed voice is never \
                 edited; re-design it (`forge gen voice {voice} … --overwrite`) so the record \
                 and the clip agree"
            ),
        ),
        Ok(_) => {}
    }
}

/// The first cell of every table row in a ledger.
fn ledger_cells(ledger: &str) -> Vec<String> {
    ledger
        .lines()
        .filter(|line| line.trim_start().starts_with('|'))
        .filter_map(|line| {
            line.trim()
                .trim_start_matches('|')
                .split('|')
                .next()
                .map(|cell| cell.trim().to_owned())
        })
        .collect()
}

/// Every `<voices>/<name>/ref.<ext>` one level down.
fn collect_voice_clips(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.is_file()
                && path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case("ref"))
                && path.extension().is_some()
            {
                out.push(path);
            }
        }
    }
}

/// Every `.png` under `root`, recursively.
fn collect_pngs(root: &Path, out: &mut Vec<PathBuf>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("png"))
            {
                out.push(path);
            }
        }
    }
}

/// The rig profile has not drifted from its artifacts.
///
/// The `.glb` is what the contract was derived from, so a hash mismatch is a
/// failure and so is a bone table that no longer derives to the contract's.
/// The `.blend` is the authoring source, not what the contract was read
/// from, so its mismatch is a warning: the next `forge rig export-contract`
/// is where it becomes a failure or goes away.
#[must_use]
pub fn profile(project: &Project) -> Report {
    let mut report = Report {
        checked: 1,
        ..Report::default()
    };
    let subject = format!("rig {}", project.rig_name);
    let profile = match project.profile() {
        Ok(profile) => profile,
        Err(err) => {
            report.fail(subject, format!("does not load: {err}"));
            return report;
        }
    };
    match profile.check_sources() {
        Err(err) => report.fail(&subject, format!("rig.glb cannot be hashed: {err}")),
        Ok(check) => {
            if !check.glb_matches {
                report.fail(
                    &subject,
                    "rig.glb no longer hashes to what the contract was derived from — \
                     regenerate the contract and bump its version",
                );
            }
            if check.blend_matches == Some(false) {
                report.warn(
                    &subject,
                    "rig.blend has changed since the contract was exported — re-export it, or \
                     expect the next rig build to differ",
                );
            }
        }
    }
    match std::fs::read(profile.glb_path()) {
        Err(err) => report.fail(&subject, format!("rig.glb cannot be read: {err}")),
        Ok(bytes) => match forge_rig::derive_from_glb(&bytes) {
            Err(err) => report.fail(&subject, format!("rig.glb is not a rig: {err}")),
            Ok(derived) => {
                for line in forge_rig::check_drift(&profile.contract, &derived.bones) {
                    report.fail(&subject, line);
                }
            }
        },
    }
    report
}

/// Every engine-free check, as one report: the library, the reference
/// ledger, the voices, the profile. An empty library passes.
#[must_use]
pub fn all(project: &Project) -> Report {
    let mut report = library(project);
    report.absorb(orphans(project));
    report.absorb(refs(project));
    report.absorb(voices(project));
    report.absorb(profile(project));
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{AnimEvent, AudioRef, ClipRecipe, EventOrigin, Measured};
    use crate::testing::temp_project;

    /// A miniature library: one clip whose sidecar the test fills in (with
    /// its take where the record says), plus one shipped sound for links to
    /// land on.
    fn library_with(sidecar: &Sidecar) -> (tempfile::TempDir, Project) {
        let (dir, project) = temp_project();
        let clips = project.kind_dir(Kind::Clip);
        let clip = clips.join("test.glb");
        std::fs::write(&clip, b"glb").expect("clip");
        let take = project.takes_dir().join("test.npz");
        std::fs::write(&take, b"npz").expect("take");
        let mut sidecar = sidecar.clone();
        sidecar.content_hash = hash::sha256_bytes(b"glb");
        sidecar.source.path = Some(String::from("assets-src/takes/test.npz"));
        sidecar.source.sha256 = Some(hash::sha256_bytes(b"npz"));
        sidecar.rig = Some(String::from("humanoid"));
        crate::sidecar::save(&clips.join("test.json"), &sidecar).expect("sidecar");
        let sfx = project.kind_dir(Kind::Sfx);
        std::fs::write(sfx.join("blip.wav"), b"RIFF").expect("sound");
        (dir, project)
    }

    /// A clip record with one recipe, one measured block and these events.
    fn clip_with_events(events: Vec<AnimEvent>) -> Sidecar {
        let mut sidecar = Sidecar::new(Kind::Clip, "test");
        sidecar.recipe = Some(ClipRecipe {
            trim_start_s: 0.25,
            ..ClipRecipe::default()
        });
        sidecar.measured = Some(Measured {
            fps: Some(20.0),
            duration_s: Some(2.0),
            ..Measured::default()
        });
        sidecar.events = Some(events);
        sidecar
    }

    fn event(t: f32, t_src: Option<f32>, name: &str) -> AnimEvent {
        AnimEvent {
            t,
            t_src,
            name: name.to_owned(),
            origin: EventOrigin::Human,
            audio: None,
        }
    }

    #[test]
    fn a_true_event_record_passes_and_the_two_empties_both_pass() {
        // t_src 1.0 through trim_start 0.25 lands at 0.75 on the built clip.
        let mut fire = event(0.75, Some(1.0), "fire");
        fire.audio = AudioRef::parse("sfx:blip");
        let (_dir, project) = library_with(&clip_with_events(vec![fire]));
        let report = library(&project);
        assert!(report.ok(), "{report}");

        let (_dir, project) = library_with(&clip_with_events(Vec::new()));
        assert!(library(&project).ok(), "examined-and-empty is clean");
    }

    #[test]
    fn an_event_off_the_clip_or_off_the_alphabet_is_a_failure() {
        let (_dir, project) = library_with(&clip_with_events(vec![event(2.4, None, "fire")]));
        assert!(!library(&project).ok(), "2.4s on a 2.0s clip must fail");

        let (_dir, project) = library_with(&clip_with_events(vec![event(-0.1, None, "fire")]));
        assert!(!library(&project).ok(), "negative time must fail");

        let (_dir, project) = library_with(&clip_with_events(vec![event(0.5, None, "Fire!")]));
        let report = library(&project);
        assert!(!report.ok(), "an unusable name must fail");
        assert!(report.findings.iter().any(|f| f.detail.contains("Fire!")));
    }

    #[test]
    fn a_contacts_event_must_speak_the_footstep_vocabulary() {
        let mut step = event(0.5, None, "footstep_l");
        step.origin = EventOrigin::Contacts;
        let (_dir, project) = library_with(&clip_with_events(vec![step]));
        assert!(library(&project).ok());

        let mut fire = event(0.5, None, "fire");
        fire.origin = EventOrigin::Contacts;
        let (_dir, project) = library_with(&clip_with_events(vec![fire]));
        assert!(!library(&project).ok());
    }

    #[test]
    fn a_built_time_that_disagrees_with_its_take_time_is_a_failure() {
        // t_src 1.0 maps to 0.75; claiming 0.5 is a 250 ms lie, ten times the
        // tolerance.
        let (_dir, project) = library_with(&clip_with_events(vec![event(0.5, Some(1.0), "fire")]));
        let report = library(&project);
        assert!(!report.ok());
        assert!(
            report.findings.iter().any(|f| f.detail.contains("maps to")),
            "{report}"
        );

        // And a take time inside the trimmed lead-in has no home at all.
        let (_dir, project) = library_with(&clip_with_events(vec![event(0.1, Some(0.1), "fire")]));
        assert!(!library(&project).ok());
    }

    #[test]
    fn a_dangling_audio_link_fails_the_library() {
        let mut fire = event(0.5, None, "fire");
        fire.audio = AudioRef::parse("sfx:missing_shot");
        let (_dir, project) = library_with(&clip_with_events(vec![fire]));
        let report = library(&project);
        assert!(!report.ok(), "{report}");
        assert!(report.render().contains("missing_shot"), "{report}");
    }

    #[test]
    fn a_missing_or_changed_take_is_a_failure_and_a_stale_hash_too() {
        let (_dir, project) = library_with(&clip_with_events(Vec::new()));
        assert!(library(&project).ok());
        let take = project.takes_dir().join("test.npz");
        std::fs::write(&take, b"different").expect("rewrite");
        let report = library(&project);
        assert!(!report.ok(), "{report}");
        assert!(report.render().contains("has changed"), "{report}");
        std::fs::remove_file(&take).expect("remove");
        assert!(library(&project).render().contains("is gone"));

        std::fs::write(project.kind_dir(Kind::Clip).join("test.glb"), b"other").expect("rewrite");
        assert!(
            library(&project)
                .render()
                .contains("content hash does not match")
        );
    }

    #[test]
    fn verify_glb_reads_the_container_and_refuses_an_external_uri() {
        let rig = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/humanoid/rig.glb"),
        )
        .expect("rig.glb");
        let summary = verify_glb(&rig).expect("self-contained");
        assert!(summary.nodes > 50, "{summary:?}");
        assert_eq!(summary.bytes, rig.len());
        assert!(verify_glb(b"not a glb").is_err());
        let mut truncated = rig.clone();
        truncated.truncate(rig.len() - 8);
        assert!(
            verify_glb(&truncated)
                .expect_err("truncated")
                .contains("truncated")
        );

        // Hand-build the smallest container claiming a uri buffer.
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
        let error = verify_glb(&glb).expect_err("must refuse");
        assert!(error.contains("external.bin"), "{error}");
    }

    #[test]
    fn a_png_without_a_ledger_row_fails_and_a_row_passes() {
        let (_dir, project) = temp_project();
        assert_eq!(refs(&project).checked, 0, "no refs, nothing to check");
        let props = project.refs_dir().join("props");
        std::fs::create_dir_all(&props).expect("mkdir");
        std::fs::write(props.join("barrel.png"), b"\x89PNG").expect("png");
        let report = refs(&project);
        assert!(!report.ok(), "{report}");
        assert!(report.render().contains("does not exist"), "{report}");

        std::fs::write(
            project.sources_ledger(),
            "# Sources\n\n| file | origin | licence |\n|---|---|---|\n| props/barrel.png | drawn | CC0 |\n",
        )
        .expect("ledger");
        let report = refs(&project);
        assert!(report.ok(), "{report}");

        std::fs::write(props.join("crate.png"), b"\x89PNG").expect("png");
        let report = refs(&project);
        assert!(!report.ok(), "{report}");
        assert!(report.render().contains("crate.png"), "{report}");
        assert_eq!(report.failures(), 1);
    }

    /// A `voice` generator record for `assets-src/voices/<name>/ref.wav`,
    /// its output hash being whatever `clip_bytes` hashes to.
    fn voice_record(name: &str, clip_bytes: &[u8]) -> String {
        format!(
            r#"{{"forge_record": 1, "kind": "voice", "tool": "moss_voice_generator",
                "created": "2026-08-23", "created_by": "human",
                "backend": {{"name": "moss_tts", "model": "OpenMOSS-Team/MOSS-VoiceGenerator"}},
                "params": {{"instruction": "deep and slow", "seed": 7}},
                "outputs": [{{"path": "assets-src/voices/{name}/ref.wav", "sha256": "{}"}}]}}"#,
            hash::sha256_bytes(clip_bytes),
        )
    }

    #[test]
    fn a_designed_voice_needs_its_record_or_a_ledger_row() {
        let (_dir, project) = temp_project();
        assert_eq!(voices(&project).checked, 0, "no voices, nothing to check");
        let warden = project.voices_dir().join("crypt_warden");
        std::fs::create_dir_all(&warden).expect("mkdir");
        std::fs::write(warden.join("ref.wav"), b"RIFFwarden").expect("clip");

        // Neither: a failure that names both fixes.
        let report = voices(&project);
        assert!(!report.ok(), "{report}");
        assert_eq!(report.checked, 1);
        let text = report.render();
        assert!(
            text.contains("voice.json") && text.contains("SOURCES.md"),
            "{text}"
        );
        assert!(text.contains("forge gen voice crypt_warden"), "{text}");

        // The record branch: a voice record whose output hash is the clip.
        std::fs::write(
            warden.join("voice.json"),
            voice_record("crypt_warden", b"RIFFwarden"),
        )
        .expect("record");
        let report = voices(&project);
        assert!(report.ok(), "{report}");

        // The clip edited by hand no longer hashes to what the record says.
        std::fs::write(warden.join("ref.wav"), b"RIFFedited").expect("edit");
        let report = voices(&project);
        assert!(!report.ok(), "{report}");
        assert!(report.render().contains("never edited"), "{report}");

        // A record of another kind beside the clip is not a voice's record.
        std::fs::write(warden.join("ref.wav"), b"RIFFwarden").expect("restore");
        std::fs::write(
            warden.join("voice.json"),
            voice_record("crypt_warden", b"RIFFwarden").replacen(
                "\"kind\": \"voice\"",
                "\"kind\": \"speech\"",
                1,
            ),
        )
        .expect("record");
        let report = voices(&project);
        assert!(!report.ok(), "{report}");
        assert!(report.render().contains("speech run"), "{report}");
        std::fs::remove_file(warden.join("voice.json")).expect("rm");

        // The ledger branch: a brought clip with a row.
        std::fs::write(
            project.sources_ledger(),
            "# Sources\n\n| file | origin | licence |\n|---|---|---|\n| voices/crypt_warden/ref.wav | recorded by the user | CC0 |\n",
        )
        .expect("ledger");
        let report = voices(&project);
        assert!(report.ok(), "{report}");

        // And the whole report carries it.
        std::fs::remove_file(project.sources_ledger()).expect("rm");
        assert!(!all(&project).ok());
    }

    #[test]
    fn a_voice_line_whose_reference_moved_or_vanished_says_so() {
        let (_dir, project) = temp_project();
        let warden = project.voices_dir().join("crypt_warden");
        std::fs::create_dir_all(&warden).expect("mkdir");
        std::fs::write(warden.join("ref.wav"), b"RIFFwarden").expect("clip");
        std::fs::write(
            warden.join("voice.json"),
            voice_record("crypt_warden", b"RIFFwarden"),
        )
        .expect("record");
        let lines = project.kind_dir(Kind::Voice);
        std::fs::write(lines.join("greeting.wav"), b"RIFFline").expect("line");
        let mut sidecar = Sidecar::new(Kind::Voice, "greeting");
        sidecar.content_hash = hash::sha256_bytes(b"RIFFline");
        sidecar.source.path = project.rel_to_root(&warden.join("ref.wav"));
        sidecar.source.sha256 = Some(hash::sha256_bytes(b"RIFFwarden"));
        crate::sidecar::save(&lines.join("greeting.json"), &sidecar).expect("sidecar");
        let report = all(&project);
        assert!(report.ok(), "{report}");

        // The voice re-designed: the line still carries the old one — a warning.
        std::fs::write(warden.join("ref.wav"), b"RIFFredesigned").expect("redesign");
        std::fs::write(
            warden.join("voice.json"),
            voice_record("crypt_warden", b"RIFFredesigned"),
        )
        .expect("record");
        let report = all(&project);
        assert!(report.ok(), "{report}");
        assert_eq!(report.warnings(), 1, "{report}");
        assert!(report.render().contains("has changed"), "{report}");

        // The voice gone: a failure.
        std::fs::remove_dir_all(&warden).expect("rm");
        let report = all(&project);
        assert!(!report.ok(), "{report}");
        assert!(report.render().contains("is gone"), "{report}");
    }

    #[test]
    fn the_shipped_profile_has_not_drifted() {
        let (_dir, project) = temp_project();
        let report = profile(&project);
        assert!(report.ok(), "{report}");
        // Touch the blend: a warning, not a failure.
        std::fs::write(project.rig_dir().join("rig.blend"), b"edited").expect("touch");
        let report = profile(&project);
        assert!(report.ok(), "{report}");
        assert_eq!(report.warnings(), 1, "{report}");
        // Touch the glb: a failure.
        std::fs::write(project.rig_dir().join("rig.glb"), b"edited").expect("touch");
        assert!(!profile(&project).ok());
    }

    #[test]
    fn an_empty_project_passes_every_check() {
        let (_dir, project) = temp_project();
        let report = all(&project);
        assert!(report.ok(), "{report}");
        assert_eq!(report.checked, 1, "only the profile");
    }

    #[test]
    fn a_sidecar_whose_asset_is_gone_is_an_orphan_and_fails() {
        let (_dir, project) = library_with(&clip_with_events(Vec::new()));
        assert!(orphans(&project).ok(), "nothing is orphaned yet");

        // The hand-deleted asset: the record stays behind, and the catalog
        // (which scans assets, not records) can no longer see it.
        std::fs::remove_file(project.kind_dir(Kind::Clip).join("test.glb")).expect("rm");
        let report = orphans(&project);
        assert!(!report.ok(), "{report}");
        assert!(report.render().contains("orphan record"), "{report}");
        assert!(report.render().contains("test.json"), "{report}");
        assert!(!all(&project).ok(), "verify as a whole fails on the orphan");

        // A stray record that never had an asset beside it fails the same way.
        std::fs::write(project.kind_dir(Kind::Clip).join("test.glb"), b"glb").expect("restore");
        std::fs::copy(
            project.kind_dir(Kind::Clip).join("test.json"),
            project.kind_dir(Kind::Clip).join("ghost.json"),
        )
        .expect("stray record");
        let report = orphans(&project);
        assert!(!report.ok(), "{report}");
        assert!(report.render().contains("ghost.json"), "{report}");
    }

    #[test]
    fn a_ledger_row_accounts_for_exactly_one_path_under_refs() {
        let (_dir, project) = temp_project();
        let props = project.refs_dir().join("props");
        let characters = project.refs_dir().join("characters");
        std::fs::create_dir_all(&props).expect("mkdir");
        std::fs::create_dir_all(&characters).expect("mkdir");
        std::fs::write(props.join("barrel.png"), b"\x89PNG").expect("png");
        std::fs::write(
            project.sources_ledger(),
            "# Sources\n\n| file | origin | licence |\n|---|---|---|\n\
             | `props/barrel.png` | drawn | CC0 |\n",
        )
        .expect("ledger");
        assert!(
            refs(&project).ok(),
            "a backticked path-qualified row counts"
        );

        // A copy under another refs/ subdirectory is NOT covered by the
        // props/ row: the cell is matched as the whole path under refs/,
        // not as a file-name substring.
        std::fs::copy(props.join("barrel.png"), characters.join("barrel.png")).expect("copy");
        let report = refs(&project);
        assert!(!report.ok(), "{report}");
        assert!(
            report.render().contains("characters/barrel.png"),
            "{report}"
        );
        assert_eq!(report.failures(), 1, "{report}");
    }
}
