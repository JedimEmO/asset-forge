//! Does `forge audit` catch a record that stopped describing its file?
//!
//! The engine-free claims are proved in `forge_library`'s own tests. These
//! are about the two claims that need an animation player: a promoted clip
//! poses the fixture mannequin exactly as its rebuild does, a promoted body
//! conforms to the contract — and, when a sidecar's recipe is edited by hand
//! after the promote, the audit fails and names the clip. With `--fit` it
//! also names the recipe that *would* reproduce the file.
//!
//! Every test builds its own project with `Project::init`, installs the
//! toolkit's `rigs/humanoid` profile and promotes fixtures through the real
//! doors — no sample library, no window, no GPU.

use std::path::{Path, PathBuf};

use forge_library::promote::{PromoteBody, PromoteClip, promote_body, promote_clip};
use forge_library::schema::{Actor, AutoTrim, ClipRecipe, InPlaceMode};
use forge_library::{Kind, Project, Severity};
use forge_studio::audit;

/// The toolkit checkout this crate lives in.
fn toolkit(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn fixture_take(name: &str) -> PathBuf {
    toolkit(&format!(
        "crates/forge_motion/tests/fixtures/blender/{name}.npz"
    ))
}

/// An empty project with the humanoid profile installed.
fn temp_project() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = Project::init(dir.path(), "audit_test").expect("init");
    project
        .install_profile(&toolkit("rigs/humanoid"))
        .expect("install the profile");
    (dir, project)
}

/// A recipe that exercises the knobs a rebuild has to agree on: trims, root
/// travel detrended, a lean, an exaggeration.
fn roll_recipe() -> ClipRecipe {
    ClipRecipe {
        trim_start_s: 0.25,
        trim_end_s: 1.3,
        auto_trim: Some(AutoTrim::Action),
        in_place: InPlaceMode::Detrend,
        exaggerate: 1.15,
        lean_deg: 4.0,
        clip: Some(String::from("Roll")),
        ..ClipRecipe::default()
    }
}

fn promote(project: &Project, name: &str, take: &str, recipe: ClipRecipe) {
    promote_clip(
        project,
        &PromoteClip {
            name: name.to_owned(),
            take_path: fixture_take(take),
            recipe,
            prompt: None,
            tags: Vec::new(),
            note: None,
            events: Vec::new(),
            created_by: Actor::Agent(String::from("tester")),
            take_record: None,
            overwrite: false,
        },
    )
    .unwrap_or_else(|error| panic!("promote {name}: {error}"));
}

/// The fixture mannequin, promoted as a body so claim 6 has a subject.
fn promote_mannequin(project: &Project, dir: &Path) {
    let profile = project.profile().expect("profile");
    let glb = dir.join("mannequin.glb");
    forge_rig::fixture::write_mannequin(&profile, &glb).expect("mannequin");
    promote_body(
        project,
        &PromoteBody {
            name: String::from("mannequin"),
            glb_path: glb,
            blend_path: None,
            lift_record: None,
            rig_record: None,
            export_record: None,
            prompt: Some(String::from("the fixture mannequin")),
            tags: Vec::new(),
            note: None,
            created_by: Actor::Human,
            overwrite: false,
        },
    )
    .expect("promote the mannequin");
}

/// Edit one recipe field in a clip's sidecar by hand — the thing the
/// one-way rule forbids, done on purpose so the audit can catch it.
fn corrupt_recipe(project: &Project, name: &str, edit: impl FnOnce(&mut ClipRecipe)) {
    let path = project.kind_dir(Kind::Clip).join(format!("{name}.json"));
    let mut sidecar = forge_library::sidecar::load(&path).expect("sidecar");
    let recipe = sidecar.recipe.as_mut().expect("a clip has a recipe");
    edit(recipe);
    std::fs::write(&path, sidecar.to_bytes().expect("bytes")).expect("write");
}

fn failures_naming<'a>(report: &'a forge_library::Report, subject: &str) -> Vec<&'a str> {
    report
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Failure && f.subject == subject)
        .map(|f| f.detail.as_str())
        .collect()
}

/// The happy path: a clip through the door, the walk beside it as the
/// reference, the mannequin as a body. Every claim holds, and the pose
/// compare says so in numbers.
#[test]
fn a_promoted_library_reproduces_posed_and_its_body_conforms() {
    let (dir, project) = temp_project();
    promote(&project, "roll", "gen_roll", roll_recipe());
    promote(&project, "walk", "gen_walk", ClipRecipe::default());
    promote_mannequin(&project, dir.path());

    let audit = audit::run(&project, false);
    println!("{}", audit.render());
    assert!(audit.ok(), "{}", audit.render());
    assert_eq!(audit.clips, 2);
    assert_eq!(audit.poses_ok, 2);
    assert_eq!(audit.bodies_checked, 1);
    assert_eq!(audit.bodies_ok, 1);
    assert_eq!(audit.bodies.warnings(), 0, "the walk is there to bind");
    assert!(audit.combined().ok());
    let text = audit.render();
    assert!(
        text.contains("2/2 clips pose the mannequin exactly"),
        "{text}"
    );
    assert!(text.contains("1/1 bodies conform"), "{text}");
}

/// A trim edited by hand after the promote: the file beside the record no
/// longer comes from it, and the audit says which one.
#[test]
fn a_corrupted_recipe_fails_the_pose_compare_naming_the_clip() {
    let (_dir, project) = temp_project();
    promote(&project, "roll", "gen_roll", roll_recipe());
    assert!(audit::run(&project, false).ok(), "conforms before the edit");

    corrupt_recipe(&project, "roll", |recipe| recipe.trim_start_s += 0.5);

    let audit = audit::run(&project, false);
    println!("{}", audit.render());
    assert!(!audit.ok());
    assert!(
        !audit.poses.ok(),
        "the pose compare must catch it on its own"
    );
    let named = failures_naming(&audit.poses, "roll");
    assert_eq!(named.len(), 1, "{named:?}");
    assert!(
        named[0].starts_with(
            "the rebuild from its own take and recipe poses the mannequin differently"
        ),
        "{}",
        named[0]
    );
    assert!(named[0].contains("mm"), "{}", named[0]);
    assert_eq!(audit.poses_ok, 0);
    assert!(audit.render().contains("FAIL roll"), "{}", audit.render());
}

/// `--fit`: a wrap blend mis-recorded after the fact is found again, and the
/// failure names the value that reproduces the file.
#[test]
fn fit_names_the_recipe_that_reproduces_a_misrecorded_loop() {
    let (_dir, project) = temp_project();
    promote(
        &project,
        "walk",
        "gen_walk",
        ClipRecipe {
            looping: true,
            loop_blend_s: 0.1,
            ..ClipRecipe::default()
        },
    );
    corrupt_recipe(&project, "walk", |recipe| recipe.loop_blend_s = 0.25);

    let audit = audit::run(&project, true);
    println!("{}", audit.render());
    assert!(audit.fitted);
    assert!(!audit.poses.ok());
    let named = failures_naming(&audit.poses, "walk");
    assert_eq!(named.len(), 1, "{named:?}");
    assert!(
        named[0].contains("reproduces with in_place Off, loop_blend_s 0.1"),
        "{}",
        named[0]
    );
    assert!(
        named[0].contains("the record is wrong, not the file"),
        "{}",
        named[0]
    );
}
