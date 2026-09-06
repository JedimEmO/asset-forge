//! Does the contract check say the right thing about a real spawned mesh?
//!
//! The unit tests beside [`forge_studio::rig_findings`] feed the checks
//! hand-built data, which is where the message wording is pinned. What they
//! cannot answer is whether the hierarchy Bevy actually builds out of a `.glb`
//! is the one the contract describes — importer quirks, mesh nodes hanging
//! beside the armature, targets installed or not — so the tests here run the
//! whole `forge rig check` path on the real thing:
//!
//! * the fixture mannequin passes every check, and the reference clip drives
//!   all 27 of its driven bones;
//! * a copy of it with one bone inserted above `Hips` fails, in one named
//!   finding rather than fifty-five, and stops binding the reference clip;
//! * a copy with a scaled armature fails the identity check;
//! * a contract whose stature band the mannequin falls outside fails it, with
//!   the band's numbers in the line;
//! * a library that has not promoted the reference clip gets a warning, not
//!   a failure.
//!
//! Every test builds its own project with `Project::init`, installs the
//! toolkit's `rigs/humanoid` profile, writes the mannequin from the contract
//! and bakes the reference clip from a fixture take — no sample library, no
//! window, no rendering, no GPU: the check runs on `MinimalPlugins`.

use std::path::{Path, PathBuf};

use forge_library::Project;
use forge_library::promote::{PromoteClip, promote_clip};
use forge_library::schema::{Actor, ClipRecipe};
use forge_rig::Contract;
use forge_studio::rig_check::{self, CheckReport};
use forge_studio::rig_findings::Severity;

/// The toolkit checkout this crate lives in.
fn toolkit(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// An empty project with the humanoid profile installed and the mannequin
/// written beside it (not promoted: it is the subject under check).
fn temp_project() -> (tempfile::TempDir, Project, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = Project::init(dir.path(), "rig_check_test").expect("init");
    project
        .install_profile(&toolkit("rigs/humanoid"))
        .expect("install the profile");
    let profile = project.profile().expect("profile");
    let mannequin = dir.path().join("mannequin.glb");
    forge_rig::fixture::write_mannequin(&profile, &mannequin).expect("mannequin");
    (dir, project, mannequin)
}

/// Promote the contract's reference clip — the walk fixture take, baked
/// fresh — so the check has something to bind.
fn promote_reference(project: &Project) {
    let name = project
        .profile()
        .expect("profile")
        .contract
        .reference_clip
        .clone();
    promote_clip(
        project,
        &PromoteClip {
            name,
            take_path: toolkit("crates/forge_motion/tests/fixtures/blender/gen_walk.npz"),
            recipe: ClipRecipe::default(),
            prompt: None,
            tags: Vec::new(),
            note: None,
            events: Vec::new(),
            created_by: Actor::Agent(String::from("tester")),
            take_record: None,
            overwrite: false,
        },
    )
    .expect("promote the reference clip");
}

fn failures(report: &CheckReport) -> Vec<&str> {
    report
        .findings
        .iter()
        .filter(|finding| !finding.passed())
        .map(|finding| finding.text.as_str())
        .collect()
}

fn lines_of(report: &CheckReport, severity: Severity) -> Vec<&str> {
    report
        .findings
        .iter()
        .filter(|finding| finding.severity == severity)
        .map(|finding| finding.text.as_str())
        .collect()
}

// ------------------------------------------------------------ glb surgery ---

/// Rewrite a `.glb`'s JSON chunk in place, leaving the binary chunk alone.
///
/// The smallest possible editor: a glb is a 12-byte header, a JSON chunk and
/// a BIN chunk, and the node graph lives entirely in the JSON. Editing the
/// ECS after the spawn would test Bevy's hierarchy, not the importer's; this
/// tests what an artist's export actually looks like.
fn rewrite_glb(from: &Path, to: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let bytes = std::fs::read(from).expect("read glb");
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().expect("chunk length")) as usize;
    let mut json: serde_json::Value =
        serde_json::from_slice(&bytes[20..20 + json_len]).expect("glb JSON");
    let rest = &bytes[20 + json_len..];
    edit(&mut json);
    let mut text = serde_json::to_vec(&json).expect("serialise");
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let mut out = Vec::with_capacity(bytes.len() + 64);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&((12 + 8 + text.len() + rest.len()) as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    out.extend_from_slice(&text);
    out.extend_from_slice(rest);
    std::fs::write(to, out).expect("write glb");
}

fn node_index(json: &serde_json::Value, name: &str) -> usize {
    json["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .position(|node| node["name"] == name)
        .unwrap_or_else(|| panic!("no node named {name}"))
}

/// The defect the whole tool exists for: a `Root` node between the armature
/// and `Hips`, the way a rig built by parenting the contract skeleton under a
/// control bone comes out.
fn inject_root_above_hips(json: &mut serde_json::Value) {
    let hips = node_index(json, "Hips");
    let nodes = json["nodes"].as_array_mut().expect("nodes");
    let root = nodes.len();
    nodes.push(serde_json::json!({ "name": "Root", "children": [hips] }));
    // Every node but the new one: the armature's child slot is re-pointed
    // at Root, and Root keeps Hips.
    for node in nodes.iter_mut().take(root) {
        if let Some(children) = node["children"].as_array_mut()
            && let Some(slot) = children
                .iter_mut()
                .find(|c| c.as_u64() == Some(hips as u64))
        {
            *slot = serde_json::json!(root);
        }
    }
}

/// An armature exported without applying its scale.
fn scale_armature(json: &mut serde_json::Value) {
    let armature = node_index(json, "Armature");
    json["nodes"][armature]["scale"] = serde_json::json!([0.01, 0.01, 0.01]);
}

// ----------------------------------------------------------------- tests ---

/// The body every test in the workspace stands on has to pass its own
/// contract, or the contract is describing something nobody ships.
#[test]
fn the_mannequin_passes_every_finding_and_binds_the_reference_clip() {
    let (_dir, project, mannequin) = temp_project();
    promote_reference(&project);

    let report = rig_check::run(&project, &mannequin, None).expect("the check runs");
    println!("{report}");
    assert!(!report.failed(), "{:?}", failures(&report));
    assert_eq!(report.reference.as_deref(), Some("clips/walk.glb"));
    // The twelve checks `forge rig check` prints, plus the one note: the
    // whole-clip lowest vertex, which is a different question from the
    // planted foot's own and is said beside it rather than instead of it.
    assert_eq!(
        report.findings.len(),
        13,
        "a check appeared or went missing"
    );
    assert_eq!(report.count(Severity::Ok), 12);
    assert_eq!(report.count(Severity::Note), 1);
    let directions = lines_of(&report, Severity::Ok)
        .into_iter()
        .find(|line| line.starts_with("rest translation directions"))
        .expect("the direction line");
    assert!(
        directions.contains("match the contract (worst "),
        "{directions}"
    );
    let planted = lines_of(&report, Severity::Ok)
        .into_iter()
        .find(|line| line.contains("planted foot"))
        .expect("the contact line");
    assert!(
        planted.starts_with("the planted foot's own lowest vertex stays within"),
        "the gate says whose vertex it measured: {planted}"
    );
    let binding = lines_of(&report, Severity::Ok)
        .into_iter()
        .find(|line| line.contains("bone(s) driven"))
        .expect("the binding line");
    assert!(
        binding.starts_with("reference clip: 27 bone(s) driven"),
        "{binding}"
    );
    let text = report.text();
    assert!(
        text.contains("\nok:   reference clip: 27 bone(s) driven"),
        "{text}"
    );
    assert!(
        text.ends_with("12 finding(s) passed, 0 failed, 1 note(s), 0 warning(s)"),
        "{text}"
    );
}

/// One bone above `Hips`: everything still spawns, everything still renders,
/// and every clip in the library silently binds to nothing — which is the
/// second half of what this asserts.
#[test]
fn a_root_above_hips_fails_in_one_named_finding_and_nothing_binds() {
    let (dir, project, mannequin) = temp_project();
    promote_reference(&project);
    let injected = dir.path().join("injected.glb");
    rewrite_glb(&mannequin, &injected, inject_root_above_hips);

    let report = rig_check::run(&project, &injected, None).expect("the check runs");
    println!("{report}");
    assert!(report.failed());
    let failed = failures(&report);
    assert_eq!(
        failed.first().copied(),
        Some(
            "Hips found at depth 3, expected 2 — a parent above Hips rewrites every \
             AnimationTargetId and every clip binds to nothing"
        ),
        "the inserted root was not named as the cause: {failed:?}"
    );
    // One cause, one finding: the fifty-four bones under Hips moved with it
    // and must not each report a depth of their own.
    assert_eq!(
        failed.len(),
        2,
        "the injection should read as its cause and its consequence: {failed:?}"
    );
    assert!(
        failed[1].starts_with("reference clip: 0 bone(s) driven")
            && failed[1].contains("27 orphaned curve(s)"),
        "the consequence — nothing binds — is the point of the finding above: {}",
        failed[1]
    );
    assert!(failed[1].contains("nothing binds"), "{}", failed[1]);
    assert!(report.text().contains("\nFAIL: Hips found at depth 3"));
}

/// A scaled armature bakes a hidden scale into every pose; the file still
/// binds perfectly, which is why nothing in the engine complains.
#[test]
fn a_scaled_armature_fails_the_identity_check() {
    let (dir, project, mannequin) = temp_project();
    promote_reference(&project);
    let scaled = dir.path().join("scaled.glb");
    rewrite_glb(&mannequin, &scaled, scale_armature);

    let report = rig_check::run(&project, &scaled, None).expect("the check runs");
    println!("{report}");
    let failed = failures(&report);
    assert_eq!(
        failed,
        vec![
            "armature transform is not identity — it would bake a hidden offset or scale \
             into every pose; apply transforms before export"
        ],
        "{failed:?}"
    );
}

/// The band is the contract's: narrow it past the mannequin and the same
/// file fails, with the new numbers in the line.
#[test]
fn a_stature_outside_the_contract_band_fails() {
    let (_dir, project, mannequin) = temp_project();
    promote_reference(&project);
    let contract_path = project.rig_dir().join(forge_rig::CONTRACT_FILE);
    let mut contract = Contract::load(&contract_path).expect("contract");
    contract.stature_m.min = 2.0;
    std::fs::write(&contract_path, contract.to_json().expect("json")).expect("write");

    let report = rig_check::run(&project, &mannequin, None).expect("the check runs");
    println!("{report}");
    let failed = failures(&report);
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert!(
        failed[0].starts_with("height 1.") && failed[0].contains("expected 2-2.2 m"),
        "{}",
        failed[0]
    );
}

/// A library that has not promoted the reference clip yet is not a failure
/// of the mesh — but it is said out loud, and the binding line is absent
/// rather than faked.
#[test]
fn a_library_without_the_reference_clip_warns_instead_of_binding() {
    let (_dir, project, mannequin) = temp_project();

    let report = rig_check::run(&project, &mannequin, None).expect("the check runs");
    println!("{report}");
    assert!(!report.failed(), "{:?}", failures(&report));
    assert_eq!(report.reference, None);
    assert_eq!(report.findings.len(), 12);
    // Two warnings, because two different checks wanted that clip: the
    // binding diff and the planted foot. Each says which one it is, so a
    // reader is not left guessing what "not checked" covered.
    let warnings = lines_of(&report, Severity::Warn);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(
        warnings[0].starts_with("the planted foot's own lowest vertex was not measured"),
        "{}",
        warnings[0]
    );
    assert!(
        warnings[1]
            .starts_with("no reference clip 'walk' in the library; walk binding not checked"),
        "{}",
        warnings[1]
    );
    assert!(
        !report.text().contains("bone(s) driven"),
        "a binding line with no clip behind it"
    );
    assert!(report.text().contains("reference: none in the library"));
}

/// The same check on any mesh, for a human: a body under `out/`, an export
/// fresh from Blender, a copy with a defect injected on purpose.
///
/// ```sh
/// FORGE_RIG_CHECK_SUBJECT=out/p3/vex_runner.glb \
///   cargo test -p forge_studio --test rig_findings -- --ignored --nocapture check_a_subject
/// ```
///
/// The subject is held to the toolkit's humanoid contract with the walk
/// fixture as the reference clip, exactly as the tests above do; set
/// `FORGE_RIG_CHECK_OUT` to a PNG path to render the sheet as well, which
/// needs a wgpu adapter (`env -u DISPLAY -u WAYLAND_DISPLAY` for offscreen).
/// The report is printed; the test fails when a finding does, so the exit
/// code means what `forge rig check`'s does.
#[test]
#[ignore = "a demo against a mesh named by FORGE_RIG_CHECK_SUBJECT, not a fixture"]
fn check_a_subject_from_the_environment() {
    let subject = PathBuf::from(
        std::env::var_os("FORGE_RIG_CHECK_SUBJECT").expect("FORGE_RIG_CHECK_SUBJECT=<mesh.glb>"),
    );
    let out = std::env::var_os("FORGE_RIG_CHECK_OUT").map(PathBuf::from);
    let (_dir, project, _mannequin) = temp_project();
    promote_reference(&project);

    let report = rig_check::run(&project, &subject, out.as_deref()).expect("the check runs");
    println!("{report}");
    assert!(!report.failed(), "{:?}", failures(&report));
}

#[test]
fn explicit_reference_is_used_and_missing_selection_is_refused() {
    let (_dir, project, mannequin) = temp_project();
    promote_clip(
        &project,
        &PromoteClip {
            name: "jog".into(),
            take_path: toolkit("crates/forge_motion/tests/fixtures/blender/gen_walk.npz"),
            recipe: ClipRecipe::default(),
            prompt: None,
            tags: Vec::new(),
            note: None,
            events: Vec::new(),
            created_by: Actor::Agent("tester".into()),
            take_record: None,
            overwrite: false,
        },
    )
    .expect("promote jog without a walk entry");
    let report =
        rig_check::run_with_reference(&project, &mannequin, None, Some("jog")).expect("check jog");
    assert_eq!(report.reference.as_deref(), Some("clips/jog.glb"));
    assert!(!report.failed(), "{report}");
    assert_eq!(report.count(Severity::Warn), 0, "{report}");
    assert!(matches!(
        rig_check::run_with_reference(&project, &mannequin, None, Some("missing")),
        Err(rig_check::RigCheckError::MissingReference(_))
    ));
}
