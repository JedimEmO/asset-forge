//! The four doors and the checks, end to end, in temporary projects.
//!
//! No GPU, no Blender, no sample library: every test builds a project with
//! `Project::init`, installs the toolkit's `rigs/humanoid` profile into it,
//! and promotes fixtures — a take pinned under `forge_motion`'s tests, the
//! fixture mannequin `forge_rig` writes from the contract, and a WAV written
//! by hand. The claims are then the library's own: the record parses, the
//! manifest checks, the catalog lists it, verify passes, audit rebuilds it.

use std::path::{Path, PathBuf};

use forge_library::promote::{
    PromoteAudio, PromoteBody, PromoteClip, PromoteModel, promote_audio, promote_body,
    promote_clip, promote_model,
};
use forge_library::schema::{
    Actor, AnimEvent, AutoTrim, ClipRecipe, EventOrigin, Generator, InPlaceMode, Provenance,
};
use forge_library::{
    Catalog, GeneratorRecord, Kind, LibraryError, Project, Query, audit, manifest, migrate, rebake,
    sidecar, verify,
};

/// The toolkit checkout this crate lives in.
fn toolkit(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// A pinned copy of the roll take — NOT a live source, which a promote
/// rewrites in place. The recipe below and its 49-frame arithmetic are
/// written down against this exact file.
fn take_fixture() -> PathBuf {
    toolkit("crates/forge_motion/tests/fixtures/blender/gen_roll.npz")
}

/// An empty project with the humanoid profile installed.
fn temp_project() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = Project::init(dir.path(), "test_library").expect("init");
    project
        .install_profile(&toolkit("rigs/humanoid"))
        .expect("install the profile");
    (dir, project)
}

/// The roll's own recipe. Trimming 0.25 s .. 1.3 s off a 20 fps take leaves
/// 49 frames — which is what makes this a check that the flags arrived rather
/// than that two empty bakes agree.
fn roll_recipe() -> ClipRecipe {
    ClipRecipe {
        trim_start_s: 0.25,
        trim_end_s: 1.3,
        // Provenance, not instruction: the trims above are what the search
        // resolved to, so a re-bake must not run it again.
        auto_trim: Some(AutoTrim::Action),
        // The one field measured to matter: strip lands 417 mm from detrend
        // on this clip.
        in_place: InPlaceMode::Detrend,
        exaggerate: 1.15,
        lean_deg: 4.0,
        clip: Some(String::from("Roll")),
        ..ClipRecipe::default()
    }
}

fn clip_request(name: &str, recipe: ClipRecipe, overwrite: bool) -> PromoteClip {
    PromoteClip {
        name: name.to_owned(),
        take_path: take_fixture(),
        recipe,
        prompt: None,
        tags: Vec::new(),
        note: None,
        events: Vec::new(),
        created_by: Actor::Agent(String::from("tester")),
        take_record: None,
        overwrite,
    }
}

/// A 16-bit PCM mono WAV of `seconds` of a 440 Hz sine at 22050 Hz, written
/// by hand: the 44-byte RIFF header and the samples, nothing else.
fn write_sine_wav(path: &Path, seconds: f32) {
    let sample_rate: u32 = 22_050;
    let frames = (seconds * sample_rate as f32).round() as u32;
    let data_bytes = frames * 2;
    let mut bytes = Vec::with_capacity(44 + data_bytes as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    for i in 0..frames {
        let t = i as f32 / sample_rate as f32;
        let sample = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
        bytes.extend_from_slice(&((sample * 32_767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, bytes).expect("write wav");
}

/// The fixture mannequin for the project's profile, as a file to promote.
fn mannequin(project: &Project, dir: &Path) -> PathBuf {
    let profile = project.profile().expect("profile");
    let path = dir.join("mannequin.glb");
    forge_rig::fixture::write_mannequin(&profile, &path).expect("mannequin");
    path
}

// ------------------------------------------------------------------ clips ---

#[test]
fn a_promoted_clip_is_listed_checked_verified_and_audited() {
    let (_dir, project) = temp_project();
    let promoted =
        promote_clip(&project, &clip_request("roll", roll_recipe(), false)).expect("promote");
    assert_eq!(promoted.rel_path, "clips/roll.glb");
    assert!(promoted.replaced.is_none());
    assert!(promoted.report.contains("49 frames"), "{}", promoted.report);
    assert!(
        promoted.report.contains("manifest refreshed"),
        "{}",
        promoted.report
    );

    // The sidecar parses at schema 1 and says what the bake did.
    let record = sidecar::load(&promoted.sidecar).expect("sidecar parses");
    assert_eq!(record, promoted.record);
    assert_eq!(record.kind, Kind::Clip);
    assert_eq!(record.rig.as_deref(), Some("humanoid"));
    assert_eq!(
        record.source.path.as_deref(),
        Some("assets-src/takes/roll.npz")
    );
    assert!(record.source.sha256.is_some());
    assert_eq!(record.source.skeleton.as_deref(), Some("cskel27"));
    assert_eq!(record.provenance, Provenance::Reconstructed);
    assert!(record.prompt.is_some(), "the take's own prompt stands");
    assert_eq!(record.created_by, Actor::Agent(String::from("tester")));
    let measured = record.measured.as_ref().expect("measured");
    assert_eq!(measured.frames, Some(49));
    assert!(measured.root_motion.is_some());
    assert!(
        record.events.is_some(),
        "the take has contacts, so footsteps were examined"
    );
    assert_eq!(
        record.recipe.as_ref().expect("recipe").clip.as_deref(),
        Some("Roll")
    );
    match record.generator.as_ref().expect("generator") {
        Generator::Ardy(params) => {
            assert_eq!(params.sweep_take.as_deref(), Some("gen_roll.npz"));
            assert_eq!(params.seed, None, "never invented");
        }
        other => panic!("{other:?}"),
    }
    assert!(project.takes_dir().join("roll.npz").is_file());

    // The catalog lists it under every spelling, the manifest checks, and
    // every engine-free claim holds.
    let catalog = Catalog::scan(&project);
    assert_eq!(catalog.len(), 1);
    for wanted in ["roll", "roll.glb", "clips/roll.glb"] {
        assert!(catalog.resolve(wanted, None).is_some(), "{wanted}");
    }
    assert_eq!(catalog.find(&Query::of_kind(Kind::Clip)).len(), 1);
    let check = manifest::check(&project);
    assert!(check.ok(), "{check}");
    let report = verify::all(&project);
    assert!(report.ok(), "{report}");
    let audit = audit::run(&project);
    assert!(audit.ok(), "{}", audit.render());
    assert_eq!(audit.rebuilt_ok, 1);
    assert_eq!(audit.roots_ok, 1);
    assert_eq!(audit.events_ok, 1);

    // The manifest carries the clip and the rig block from the profile.
    let bytes = std::fs::read(project.manifest_path()).expect("manifest");
    let manifest = forge_manifest::Manifest::from_slice(&bytes).expect("parses");
    let clip = manifest.clip("roll").expect("listed");
    assert_eq!(clip.frames, Some(49));
    assert_eq!(
        clip.root_motion.mode,
        forge_manifest::RootMotionMode::Detrend
    );
    assert_eq!(clip.root_motion.track_xz_m.len(), 49);
    assert_eq!(
        clip.events.len(),
        record.events.as_ref().map_or(0, Vec::len)
    );
    assert_eq!(manifest.rig.profile, "humanoid");
    assert_eq!(manifest.rig.bone_count, 55);
}

#[test]
fn promoting_twice_refuses_by_name_and_overwrite_returns_both_recipes() {
    let (_dir, project) = temp_project();
    promote_clip(&project, &clip_request("roll", roll_recipe(), false)).expect("first");

    let error = promote_clip(&project, &clip_request("roll", roll_recipe(), false))
        .expect_err("must refuse");
    assert!(
        matches!(&error, LibraryError::WouldOverwrite { name, .. } if name == "roll"),
        "{error}"
    );
    assert!(
        error.to_string().starts_with("roll already exists"),
        "{error}"
    );

    let stripped = ClipRecipe {
        in_place: InPlaceMode::Strip,
        ..roll_recipe()
    };
    let promoted =
        promote_clip(&project, &clip_request("roll", stripped.clone(), true)).expect("overwrite");
    let previous = promoted
        .previous_recipe()
        .expect("the old recipe comes back");
    assert_eq!(previous.in_place, InPlaceMode::Detrend);
    let current = promoted.recipe().expect("the new recipe too");
    assert_eq!(current.in_place, InPlaceMode::Strip);
    assert_eq!(current.clip.as_deref(), Some("Roll"));
    assert!(verify::all(&project).ok());
}

#[test]
fn the_recipe_reaches_the_bake_and_curation_survives_a_rebake() {
    let (_dir, project) = temp_project();
    let first = promote_clip(&project, &clip_request("roll", roll_recipe(), true)).expect("one");
    let asked_for = std::fs::read(&first.asset).expect("read");
    assert!(first.record.tags.is_empty());

    // Root travel, the knob whose two settings differ by 417 mm on this clip.
    let second = promote_clip(
        &project,
        &clip_request(
            "roll",
            ClipRecipe {
                in_place: InPlaceMode::Strip,
                ..roll_recipe()
            },
            true,
        ),
    )
    .expect("two");
    assert!(
        asked_for != std::fs::read(&second.asset).expect("read"),
        "changing the root-travel treatment changed nothing, so the flags are not reaching the bake"
    );

    // Tags are curation: stated once, they survive a re-promote that states
    // none, and `loop` follows the recipe.
    let mut tagged = clip_request(
        "roll",
        ClipRecipe {
            looping: true,
            loop_blend_s: 0.15,
            ..roll_recipe()
        },
        true,
    );
    tagged.tags = vec![String::from("hero")];
    let third = promote_clip(&project, &tagged).expect("three");
    assert_eq!(third.record.tags, ["hero", "loop"]);
    assert_eq!(
        third
            .record
            .recipe
            .as_ref()
            .expect("recipe")
            .clip
            .as_deref(),
        Some("Roll-loop")
    );
    let fourth = promote_clip(&project, &clip_request("roll", roll_recipe(), true)).expect("four");
    assert_eq!(
        fourth.record.tags,
        ["hero"],
        "the stated tag survives, the loop tag follows the recipe"
    );
}

#[test]
fn two_promotes_of_the_same_take_into_two_projects_are_byte_identical() {
    let (_a, project_a) = temp_project();
    let (_b, project_b) = temp_project();
    let a = promote_clip(&project_a, &clip_request("parity", roll_recipe(), false)).expect("a");
    let b = promote_clip(&project_b, &clip_request("parity", roll_recipe(), false)).expect("b");
    let bytes_a = std::fs::read(&a.asset).expect("read a");
    let bytes_b = std::fs::read(&b.asset).expect("read b");
    assert!(
        bytes_a == bytes_b,
        "the same take and recipe produced different bytes"
    );

    // And they equal a direct forge_motion bake of the same inputs: the
    // door adds nothing to the file.
    let take = forge_motion::Take::read(take_fixture()).expect("take");
    let edit = roll_recipe().to_edit(take.fps).expect("edit");
    let rig = forge_motion::RigDef::from_glb(
        &std::fs::read(project_a.profile().expect("profile").glb_path()).expect("rig"),
    )
    .expect("rig");
    let baked = forge_motion::bake(&take, &edit, &rig, "Roll").expect("bake");
    assert!(
        baked == bytes_a,
        "the promoted file is not forge_motion::bake's output"
    );

    // The two records differ only in what they must: nothing, in fact, when
    // the clock is the same day.
    let mut record_a = a.record;
    let mut record_b = b.record;
    record_a.created = String::new();
    record_b.created = String::new();
    assert_eq!(record_a, record_b);
}

#[test]
fn authored_events_land_on_the_built_clock_and_a_stated_footstep_is_refused() {
    let (_dir, project) = temp_project();
    let mut request = clip_request("roll", roll_recipe(), false);
    request.events = vec![AnimEvent {
        t: 0.0,
        t_src: Some(1.0),
        name: String::from("impact"),
        origin: EventOrigin::Unknown,
        audio: None,
    }];
    let promoted = promote_clip(&project, &request).expect("promote");
    let events = promoted.record.events.as_ref().expect("events");
    let impact = events
        .iter()
        .find(|e| e.name == "impact")
        .expect("authored");
    assert!(
        (impact.t - 0.75).abs() < 1e-4,
        "1.0 s through a 0.25 s trim: {}",
        impact.t
    );
    assert_eq!(impact.origin, EventOrigin::Agent(String::from("tester")));
    assert!(verify::all(&project).ok());
    assert!(audit::run(&project).ok());

    let mut stated = clip_request("roll", roll_recipe(), true);
    stated.events = vec![AnimEvent {
        t: 0.0,
        t_src: Some(0.5),
        name: String::from("footstep_l"),
        origin: EventOrigin::Contacts,
        audio: None,
    }];
    assert!(promote_clip(&project, &stated).is_err());
}

#[test]
fn a_take_record_makes_the_provenance_recorded() {
    let (_dir, project) = temp_project();
    let record_text = r#"{"forge_record": 1, "kind": "take", "tool": "ardy",
        "created": "2026-08-23", "created_by": "human",
        "backend": {"name": "ardy", "model": "core", "commit": "693f74d"},
        "inputs": [{"role": "prompt", "prompt": "A person dives into a roll."}],
        "params": {"seed": 7, "duration_s": 4.0, "cfg": 2.0, "sample": 1},
        "outputs": [{"path": "out/sweeps/roll/roll__d4_c2_s7_1.npz"}]}"#;
    let mut request = clip_request("roll", roll_recipe(), false);
    request.take_record = Some(
        GeneratorRecord::from_slice(record_text.as_bytes(), Path::new("t.json")).expect("record"),
    );
    request.prompt = None;
    let promoted = promote_clip(&project, &request).expect("promote");
    assert_eq!(promoted.record.provenance, Provenance::Recorded);
    match promoted.record.generator.expect("generator") {
        Generator::Ardy(params) => {
            assert_eq!(params.seed, Some(7));
            assert_eq!(params.sweep_take.as_deref(), Some("roll__d4_c2_s7_1.npz"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn rebake_rebuilds_every_clip_and_skips_meshes_loudly() {
    let (dir, project) = temp_project();
    promote_clip(&project, &clip_request("roll", roll_recipe(), false)).expect("clip");
    let before = sidecar::load(&project.kind_dir(Kind::Clip).join("roll.json")).expect("record");
    let body = mannequin(&project, dir.path());
    promote_body(&project, &body_request("dummy", &body)).expect("body");

    let dry = rebake::run(&project, true);
    assert!(dry.ok(), "{}", dry.render(true));
    assert_eq!(dry.baked.len(), 1);
    assert_eq!(dry.skipped.len(), 1);
    assert!(dry.skipped[0].contains("dummy: body"), "{:?}", dry.skipped);

    let wet = rebake::run(&project, false);
    assert!(wet.ok(), "{}", wet.render(false));
    let after = sidecar::load(&project.kind_dir(Kind::Clip).join("roll.json")).expect("record");
    assert_eq!(
        after, before,
        "a re-bake of an unchanged clip changes nothing in its record"
    );
    assert!(audit::run(&project).ok());
}

// ----------------------------------------------------------------- bodies ---

fn body_request(name: &str, glb: &Path) -> PromoteBody {
    PromoteBody {
        name: name.to_owned(),
        glb_path: glb.to_path_buf(),
        blend_path: None,
        lift_record: None,
        rig_record: None,
        export_record: None,
        prompt: Some(String::from("the fixture mannequin")),
        tags: vec![String::from("fixture")],
        note: None,
        created_by: Actor::Human,
        overwrite: false,
    }
}

#[test]
fn the_fixture_mannequin_promotes_as_a_body_and_the_manifest_rig_matches_the_profile() {
    let (dir, project) = temp_project();
    let glb = mannequin(&project, dir.path());
    let promoted = promote_body(&project, &body_request("mannequin", &glb)).expect("promote");
    assert_eq!(promoted.rel_path, "bodies/mannequin.glb");
    assert_eq!(promoted.record.rig.as_deref(), Some("humanoid"));
    assert_eq!(promoted.record.provenance, Provenance::Reconstructed);
    assert!(
        promoted.record.generator.is_none(),
        "no lift record, no generator block"
    );
    let mesh = promoted
        .record
        .measured
        .as_ref()
        .and_then(|m| m.mesh)
        .expect("measured");
    assert_eq!(mesh.bones_skinned, 55);
    assert!(promoted.report.contains("55 bones"), "{}", promoted.report);

    let bytes = std::fs::read(project.manifest_path()).expect("manifest");
    let manifest = forge_manifest::Manifest::from_slice(&bytes).expect("parses");
    let profile = project.profile().expect("profile");
    assert_eq!(manifest.rig.profile, profile.contract.name);
    assert_eq!(manifest.rig.version, u64::from(profile.contract.version));
    assert_eq!(manifest.rig.bones.len(), profile.contract.bones.len());
    for (published, bone) in manifest.rig.bones.iter().zip(&profile.contract.bones) {
        assert_eq!(published.name, bone.name);
        assert_eq!(published.parent, bone.parent);
        assert_eq!(published.driven, bone.driven);
    }
    assert_eq!(manifest.rig.sockets.len(), profile.sockets.sockets.len());
    assert_eq!(
        manifest.rig.glb_sha256,
        forge_library::hash::prefixed(&profile.contract.sources.glb_sha256)
    );
    let body = manifest.body("mannequin").expect("listed");
    assert_eq!(body.tags, ["fixture"]);

    assert!(manifest::check(&project).ok());
    let report = verify::all(&project);
    assert!(report.ok(), "{report}");
    let audit = audit::run(&project);
    assert!(audit.ok(), "{}", audit.render());
    assert_eq!(audit.meshes_skipped, 1);
    assert!(
        audit.render().contains("1 bodies/models skipped"),
        "{}",
        audit.render()
    );
}

#[test]
fn a_body_with_a_blend_and_records_is_recorded_and_held_to_its_source() {
    let (dir, project) = temp_project();
    let glb = mannequin(&project, dir.path());
    let blend = project.blender_dir().join("mannequin.blend");
    std::fs::write(&blend, b"BLENDER-v502").expect("blend");
    let lift = GeneratorRecord::from_slice(
        br#"{"forge_record": 1, "kind": "lift", "tool": "trellis2", "created": "2026-08-23",
            "created_by": "agent:claude", "backend": {"model": "microsoft/TRELLIS.2-4B"},
            "inputs": [{"role": "image", "path": "assets-src/refs/characters/m.png", "sha256": "sha256:00"}],
            "params": {"seed": 42, "resolution": 1024, "texture_baker": "nvdiffrast (non-commercial)"},
            "outputs": [{"path": "out/lifts/m.glb", "sha256": "sha256:11"}]}"#,
        Path::new("m.lift.json"),
    )
    .expect("lift record");
    let rig = GeneratorRecord::from_slice(
        br#"{"forge_record": 1, "kind": "rig", "tool": "blender", "created": "2026-08-23",
            "created_by": "agent:claude"}"#,
        Path::new("m.rig.json"),
    )
    .expect("rig record");
    let request = PromoteBody {
        blend_path: Some(blend.clone()),
        lift_record: Some(lift),
        rig_record: Some(rig),
        prompt: None,
        tags: Vec::new(),
        created_by: Actor::Unknown,
        ..body_request("mannequin", &glb)
    };
    let promoted = promote_body(&project, &request).expect("promote");
    assert_eq!(promoted.record.provenance, Provenance::Recorded);
    assert_eq!(
        promoted.record.created_by,
        Actor::Agent(String::from("claude"))
    );
    assert_eq!(
        promoted.record.source.path.as_deref(),
        Some("assets-src/blender/mannequin.blend")
    );
    match promoted.record.generator.as_ref().expect("generator") {
        Generator::Trellis2(params) => {
            assert_eq!(params.seed, Some(42));
            assert_eq!(
                params.texture_baker.as_deref(),
                Some("nvdiffrast (non-commercial)")
            );
            let post = params.post.as_ref().expect("post step");
            assert_eq!(post.script.as_deref(), Some("rig"));
            assert_eq!(post.version.as_deref(), Some(forge_rig::fixture::GENERATOR));
        }
        other => panic!("{other:?}"),
    }
    assert!(verify::all(&project).ok());

    // Iterating on the .blend is a warning; losing it is a failure.
    std::fs::write(&blend, b"BLENDER-v502 edited").expect("edit");
    let report = verify::all(&project);
    assert!(report.ok(), "{report}");
    assert!(report.render().contains("has changed"), "{report}");
    std::fs::remove_file(&blend).expect("rm");
    assert!(!verify::all(&project).ok());
}

#[test]
fn a_mesh_that_is_not_on_the_contract_is_refused() {
    let (dir, project) = temp_project();
    // The profile's own fixture clip: skinned, but to 27 joints, not 55.
    let clip = toolkit("rigs/humanoid/fixture/cskel27_idle.glb");
    let error = promote_body(&project, &body_request("idle", &clip)).expect_err("refuse");
    assert!(error.to_string().contains("missing"), "{error}");
    // A .blend outside the project is not a provenance claim.
    let glb = mannequin(&project, dir.path());
    let outside = tempfile::tempdir().expect("tempdir");
    let blend = outside.path().join("m.blend");
    std::fs::write(&blend, b"x").expect("blend");
    let request = PromoteBody {
        blend_path: Some(blend),
        ..body_request("mannequin", &glb)
    };
    let error = promote_body(&project, &request).expect_err("refuse");
    assert!(error.to_string().contains("outside the project"), "{error}");
    assert!(Catalog::scan(&project).is_empty(), "nothing was written");
}

// ----------------------------------------------------------------- models ---

#[test]
fn a_model_promotes_with_its_bounds_and_needs_no_rig() {
    let (dir, project) = temp_project();
    // The mannequin is a perfectly good static mesh when nobody claims a
    // contract for it.
    let glb = mannequin(&project, dir.path());
    let promoted = promote_model(
        &project,
        &PromoteModel {
            name: String::from("statue"),
            glb_path: glb,
            blend_path: None,
            lift_record: None,
            prop_record: None,
            prompt: Some(String::from("a statue")),
            tags: vec![String::from("decor")],
            note: None,
            created_by: Actor::Human,
            overwrite: false,
        },
    )
    .expect("promote");
    assert_eq!(promoted.rel_path, "models/statue.glb");
    assert_eq!(promoted.record.rig, None);
    let bytes = std::fs::read(project.manifest_path()).expect("manifest");
    let manifest = forge_manifest::Manifest::from_slice(&bytes).expect("parses");
    let model = manifest.model("statue").expect("listed");
    let bounds = model.bounds_m.expect("bounds from the measurement");
    assert!(bounds[1][1] - bounds[0][1] > 1.4, "{bounds:?}");
    assert!(manifest::check(&project).ok());
    assert!(verify::all(&project).ok());
    assert!(audit::run(&project).ok());
}

// ------------------------------------------------------------------ audio ---

fn audio_request(kind: Kind, name: &str, file: &Path, overwrite: bool) -> PromoteAudio {
    PromoteAudio {
        kind,
        name: name.to_owned(),
        file: file.to_path_buf(),
        record: None,
        prompt: Some(String::from("a test tone")),
        tags: Vec::new(),
        note: None,
        created_by: Actor::Human,
        overwrite,
    }
}

#[test]
fn a_sound_promotes_with_its_duration_measured_and_a_stem_collision_is_refused() {
    let (dir, project) = temp_project();
    let wav = dir.path().join("tone.wav");
    write_sine_wav(&wav, 0.5);
    let promoted =
        promote_audio(&project, &audio_request(Kind::Sfx, "tone", &wav, false)).expect("promote");
    assert_eq!(promoted.rel_path, "audio/sfx/tone.wav");
    assert_eq!(promoted.record.kind, Kind::Sfx);
    assert_eq!(promoted.record.provenance, Provenance::Unknown);
    let duration = promoted
        .record
        .measured
        .as_ref()
        .and_then(|m| m.duration_s)
        .expect("measured");
    assert!((duration - 0.5).abs() < 0.01, "{duration}");
    assert!(promoted.report.contains("0.50s"), "{}", promoted.report);

    // Same kind, same stem: refused by name unless overwrite.
    let error = promote_audio(&project, &audio_request(Kind::Sfx, "tone", &wav, false))
        .expect_err("refuse");
    assert!(matches!(&error, LibraryError::WouldOverwrite { name, .. } if name == "tone"));
    assert!(promote_audio(&project, &audio_request(Kind::Sfx, "tone", &wav, true)).is_ok());

    // Another kind, same stem: refused regardless — a game's audio map is by
    // stem.
    let error = promote_audio(&project, &audio_request(Kind::Voice, "tone", &wav, true))
        .expect_err("refuse");
    assert!(error.to_string().contains("already a sfx"), "{error}");

    let manifest_bytes = std::fs::read(project.manifest_path()).expect("manifest");
    let manifest = forge_manifest::Manifest::from_slice(&manifest_bytes).expect("parses");
    let sound = manifest
        .audio
        .iter()
        .find(|a| a.name == "tone")
        .expect("listed");
    assert_eq!(sound.kind, forge_manifest::AudioKind::Sfx);
    assert!(sound.duration_s.is_some());
    assert!(manifest::check(&project).ok());
    assert!(verify::all(&project).ok());
}

#[test]
fn a_sound_with_a_record_is_recorded_and_a_record_of_the_wrong_kind_is_refused() {
    let (dir, project) = temp_project();
    let wav = dir.path().join("boom.wav");
    write_sine_wav(&wav, 0.25);
    let record = GeneratorRecord::from_slice(
        br#"{"forge_record": 1, "kind": "sfx", "tool": "moss_sound_effect", "created": "2026-08-23",
            "created_by": "agent:claude", "backend": {"model": "OpenMOSS-Team/MOSS-SoundEffect-v2.0"},
            "inputs": [{"role": "prompt", "prompt": "a dry wooden thwack"}],
            "params": {"seed": 424242, "duration_s": 3.0, "steps": 100, "cfg": 4.0},
            "outputs": [{"path": "out/audio/boom.wav"}]}"#,
        Path::new("boom.json"),
    )
    .expect("record");
    let mut request = audio_request(Kind::Sfx, "boom", &wav, false);
    request.record = Some(record.clone());
    request.prompt = None;
    let promoted = promote_audio(&project, &request).expect("promote");
    assert_eq!(promoted.record.provenance, Provenance::Recorded);
    assert_eq!(
        promoted.record.prompt.as_deref(),
        Some("a dry wooden thwack")
    );
    match promoted.record.generator.expect("generator") {
        Generator::MossSoundEffect(params) => assert_eq!(params.seed, Some(424_242)),
        other => panic!("{other:?}"),
    }

    let mut wrong = audio_request(Kind::Music, "boom_theme", &wav, false);
    wrong.record = Some(record);
    let error = promote_audio(&project, &wrong).expect_err("refuse");
    assert!(error.to_string().contains("sfx run"), "{error}");
}

// --------------------------------------------------------- the empty case ---

#[test]
fn an_empty_project_projects_a_valid_manifest_and_passes_every_check() {
    let (_dir, project) = temp_project();
    let written = manifest::write(&project).expect("write");
    assert!(written.clips.is_empty() && written.bodies.is_empty());
    assert_eq!(written.rig.bone_count, 55);
    let bytes = std::fs::read(project.manifest_path()).expect("manifest");
    assert!(forge_manifest::Manifest::from_slice(&bytes).is_ok());
    assert!(manifest::check(&project).ok());
    assert!(verify::all(&project).ok());
    let audit = audit::run(&project);
    assert!(audit.ok(), "{}", audit.render());
    assert!(migrate::run(&project, false).expect("migrate").ok());
    assert!(rebake::run(&project, false).ok());
}

#[test]
fn a_reference_png_without_a_ledger_row_fails_verify() {
    let (_dir, project) = temp_project();
    let chars = project.refs_dir().join("characters");
    std::fs::create_dir_all(&chars).expect("mkdir");
    std::fs::write(chars.join("hero.png"), b"\x89PNG\r\n").expect("png");
    let report = verify::all(&project);
    assert!(!report.ok(), "{report}");
    assert!(report.render().contains("hero.png"), "{report}");
    std::fs::write(
        project.sources_ledger(),
        "| file | origin |\n|---|---|\n| characters/hero.png | drawn by hand |\n",
    )
    .expect("ledger");
    assert!(verify::all(&project).ok());
}

#[test]
fn a_stale_manifest_fails_the_check_until_rewritten() {
    let (_dir, project) = temp_project();
    promote_clip(&project, &clip_request("roll", roll_recipe(), false)).expect("clip");
    assert!(manifest::check(&project).ok());
    std::fs::write(project.kind_dir(Kind::Clip).join("loose.glb"), b"glb").expect("loose");
    assert!(
        !manifest::check(&project).ok(),
        "a file nobody projected is exactly the staleness"
    );
    manifest::write(&project).expect("rewrite");
    assert!(manifest::check(&project).ok());
}
