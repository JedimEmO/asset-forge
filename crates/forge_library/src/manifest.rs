//! Projecting the library into the consumer manifest.
//!
//! Games never parse sidecars. A sidecar answers authoring questions and
//! changes shape when authoring learns something new; what a game reads is
//! `assets/library.json` in the shape [`forge_manifest`] pins down. This
//! module is the one place that shape is produced, and [`build`] is a
//! projection in the strict sense: every clip, model and audio field comes
//! from the record's sidecar, and a value the sidecar does not carry stays
//! `null`. A manifest that guessed a duration would be the same bug as a
//! sidecar recording an argparse default. The one exception is the hash on
//! every entry, which is read from the file because it is the one claim that
//! must be true of the bytes shipped rather than of the record.
//!
//! The rig block comes from the profile — bones, sockets, the `.glb`'s hash —
//! so a consumer that bundles the rig can prove it bundled the one the clips
//! were baked against without learning the profile's on-disk layout.
//!
//! The catalog is scan-derived and cannot go stale; the manifest is a
//! committed file and can. [`check`] is what keeps it honest — rebuild from
//! a fresh scan and compare byte-for-byte, so `just ci` refuses a library
//! whose manifest tells yesterday's story.

use forge_manifest::{
    AudioEntry, BodyEntry, ClipEntry, Manifest, ManifestEvent, ModelEntry, RigBone, RigInfo,
    RigSocket, RootMotionInfo, RootMotionMode,
};

use crate::catalog::AssetRecord;
use crate::report::Report;
use crate::schema::Kind;
use crate::{Catalog, LibraryError, Project, Result, clock, hash, write_atomic};

/// Project the catalog into a manifest.
///
/// Pure per asset — nothing here re-measures anything beyond hashing each
/// file. An empty library projects to a valid manifest with empty arrays.
///
/// # Errors
///
/// Fails when the profile cannot be loaded or its rig `.glb` cannot be
/// hashed — a manifest that cannot say which rig its clips bind to would be
/// making the library's central promise with nothing behind it — or when an
/// asset file cannot be hashed.
pub fn build(project: &Project, catalog: &Catalog) -> Result<Manifest> {
    let mut bodies = Vec::new();
    let mut models = Vec::new();
    let mut clips = Vec::new();
    let mut audio = Vec::new();
    for record in catalog.records() {
        let sha256 = hash::sha256_file(&record.path)?;
        match record.kind {
            Kind::Clip => clips.push(clip_entry(record, sha256)),
            Kind::Body => bodies.push(BodyEntry {
                name: record.name.clone(),
                path: record.rel_path.clone(),
                sha256,
                tags: record.tags(),
            }),
            Kind::Model => models.push(model_entry(record, sha256)),
            Kind::Sfx | Kind::Music | Kind::Voice => audio.push(audio_entry(record, sha256)),
        }
    }
    Ok(Manifest {
        schema: forge_manifest::SCHEMA,
        forge_version: String::from(env!("CARGO_PKG_VERSION")),
        library_version: project.library_version.clone(),
        generated: clock::today_iso(),
        rig: rig_info(project)?,
        bodies,
        models,
        clips,
        audio,
    })
}

/// The rig block: the profile's contract and sockets, and the hash of the
/// `.glb` as it sits on disk.
fn rig_info(project: &Project) -> Result<RigInfo> {
    let profile = project.profile()?;
    let contract = &profile.contract;
    Ok(RigInfo {
        profile: contract.name.clone(),
        version: u64::from(contract.version),
        bone_count: u32::try_from(contract.bones.len()).unwrap_or(u32::MAX),
        glb_sha256: hash::sha256_file(&profile.glb_path())?,
        bones: contract
            .bones
            .iter()
            .map(|bone| RigBone {
                name: bone.name.clone(),
                parent: bone.parent,
                driven: bone.driven,
            })
            .collect(),
        sockets: profile
            .sockets
            .sockets
            .iter()
            .map(|socket| RigSocket {
                name: socket.name.clone(),
                bone: socket.bone.clone(),
                translation: socket.translation,
                rotation: socket.rotation,
            })
            .collect(),
    })
}

/// Rebuild the manifest from a fresh scan and write it atomically to
/// `<assets>/library.json`. Returns what was written.
///
/// # Errors
///
/// As [`build`], plus the file being unwritable.
pub fn write(project: &Project) -> Result<Manifest> {
    let catalog = Catalog::scan(project);
    let manifest = build(project, &catalog)?;
    let bytes = manifest
        .to_vec_pretty()
        .map_err(|e| LibraryError::rejected(format!("the manifest will not serialise: {e}")))?;
    write_atomic(&project.manifest_path(), &bytes)?;
    Ok(manifest)
}

/// Compare the committed manifest against a rebuild from a fresh scan.
///
/// The `generated` date and `forge_version` are adopted from the committed
/// file before comparing: they record when and by what the manifest was last
/// written, and a comparison that included them would start failing at the
/// next midnight or the next toolkit bump with the library untouched — the
/// check is about the library, not the calendar.
#[must_use]
pub fn check(project: &Project) -> Report {
    let mut report = Report {
        checked: 1,
        ..Report::default()
    };
    let manifest_path = project.manifest_path();
    let subject = crate::project::MANIFEST_FILE;
    let committed = match std::fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            report.fail(
                subject,
                format!(
                    "{} does not exist — run `forge manifest`",
                    manifest_path.display()
                ),
            );
            return report;
        }
        Err(err) => {
            report.fail(subject, format!("cannot be read: {err}"));
            return report;
        }
    };
    let committed_manifest = match Manifest::from_slice(&committed) {
        Ok(manifest) => manifest,
        Err(err) => {
            report.fail(subject, err.to_string());
            return report;
        }
    };
    let catalog = Catalog::scan(project);
    let mut rebuilt = match build(project, &catalog) {
        Ok(manifest) => manifest,
        Err(err) => {
            report.fail(subject, format!("cannot be rebuilt: {err}"));
            return report;
        }
    };
    rebuilt.generated.clone_from(&committed_manifest.generated);
    rebuilt
        .forge_version
        .clone_from(&committed_manifest.forge_version);
    match rebuilt.to_vec_pretty() {
        Ok(expected) if expected == committed => {}
        Ok(_) => report.fail(
            subject,
            "does not match a rebuild from the library — run `forge manifest`",
        ),
        Err(err) => report.fail(subject, format!("the rebuild will not serialise: {err}")),
    }
    report
}

/// One clip, projected. Everything optional stays `None` when the sidecar
/// does not carry it. The two non-optional judgements a consumer needs a
/// decision on — `looped` and the root-motion mode — read a missing recipe
/// as "not baked as a loop" and "travel left as generated", which is what an
/// unprocessed clip actually is.
fn clip_entry(record: &AssetRecord, sha256: String) -> ClipEntry {
    let sidecar = record.sidecar.as_ref();
    let measured = sidecar.and_then(|s| s.measured.as_ref());
    let recipe = sidecar.and_then(|s| s.recipe.as_ref());
    ClipEntry {
        name: record.name.clone(),
        path: record.rel_path.clone(),
        sha256,
        duration_s: measured.and_then(|m| m.duration_s),
        fps: measured.and_then(|m| m.fps),
        frames: measured.and_then(|m| m.frames),
        looped: recipe.is_some_and(|r| r.looping),
        tags: record.tags(),
        root_motion: RootMotionInfo {
            mode: recipe.map_or(RootMotionMode::Off, |r| r.in_place.root_motion_mode()),
            avg_speed_mps: measured.and_then(|m| m.avg_speed_mps),
            // The track's clock is the built clip's frame rate: the sidecar
            // records one pre-strip sample per built frame, so the same fps
            // that times the clip times the track.
            fps: measured.and_then(|m| m.fps),
            track_xz_m: measured
                .and_then(|m| m.root_motion.as_ref())
                .map(|root| root.track_xz_m.clone())
                .unwrap_or_default(),
        },
        // The manifest's event vocabulary is smaller than the sidecar's on
        // purpose: a game needs the name, the built-clip time and the sound
        // the instant plays; take times and origins are authoring facts. A
        // sidecar that was never examined projects the same empty list as
        // one examined and found silent — the distinction matters to
        // authoring, not to a game.
        events: sidecar
            .and_then(|s| s.events.as_ref())
            .map(|events| {
                events
                    .iter()
                    .map(|event| ManifestEvent {
                        name: event.name.clone(),
                        time_s: event.t,
                        audio: event.audio.as_ref().map(ToString::to_string),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// One model, projected: its bounds come from the measured block, and stay
/// `null` when the mesh was never measured.
fn model_entry(record: &AssetRecord, sha256: String) -> ModelEntry {
    ModelEntry {
        name: record.name.clone(),
        path: record.rel_path.clone(),
        sha256,
        tags: record.tags(),
        bounds_m: record
            .sidecar
            .as_ref()
            .and_then(|s| s.measured.as_ref())
            .and_then(|m| m.mesh)
            .map(|mesh| mesh.bounds),
    }
}

/// One sound, projected. `kind` comes from where the record lives, which is
/// the same rule the catalog itself uses.
fn audio_entry(record: &AssetRecord, sha256: String) -> AudioEntry {
    let sidecar = record.sidecar.as_ref();
    AudioEntry {
        name: record.name.clone(),
        path: record.rel_path.clone(),
        sha256,
        kind: record
            .kind
            .audio_kind()
            .unwrap_or(forge_manifest::AudioKind::Sfx),
        duration_s: sidecar
            .and_then(|s| s.measured.as_ref())
            .and_then(|m| m.duration_s),
        tags: record.tags(),
    }
}
