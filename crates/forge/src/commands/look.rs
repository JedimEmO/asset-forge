//! `forge sheet`, `forge views`, `forge turntable`, `forge bones`: the
//! looks an agent takes at an asset, each a render or a report from
//! `forge_studio` printed and turned into an exit code.
//!
//! The three that pose a clip share one rule for the body under it, in
//! [`default_body`]: the project's `stage_body`, else the first body with a
//! warning, else the fixture mannequin written under `out/` — a clip is never
//! judged on an empty stage, because an empty stage says nothing about the
//! clip and a sheet of nothing is easy to mistake for a sheet of a T-pose.
//!
//! Bevy loads nothing from outside an asset root — an absolute path or a
//! `../` never spawns and never fails, it just sits there — so a body that
//! is not in the library is staged beside a copy of its clip in a scratch
//! root that removes itself ([`Subject`]). The library's own bodies stand on
//! the asset root as they are.
//!
//! `views` is the only one that takes a path from anywhere: a raw lift under
//! `out/` is outside the library and is still the thing to look at before
//! the rig minute is spent. Under `out/`, culling defaults to off, because a
//! lift's whole question is whether its back is there.

use std::path::{Path, PathBuf};

use forge_library::project::OutKind;
use forge_library::{Catalog, Kind, Project};
use forge_studio::{
    SheetRequest, Shot, Stage, UVec2, View, ViewsRequest, bones_report, render_clip_sheet,
    render_views,
};

use crate::cli::{BonesArgs, SheetArgs, TurntableArgs, ViewsArgs};
use crate::outcome::{Failure, Outcome};

/// The fixture mannequin's place under `out/`, when a library has no body to
/// pose a clip on.
const MANNEQUIN_OUT: &str = "fixture/mannequin.glb";

/// A mesh chosen for the stage, and where it lives.
#[derive(Debug, Clone)]
pub(crate) enum Chosen {
    /// In the library: the path relative to the asset root, and the name.
    InLibrary {
        /// Relative to the asset root, forward-slashed.
        rel_path: String,
        /// The asset's name.
        name: String,
    },
    /// A file somewhere else — the fixture mannequin, an export — by
    /// absolute path.
    Elsewhere(PathBuf),
}

impl Chosen {
    /// The name a default output file takes.
    fn stem(&self) -> String {
        match self {
            Self::InLibrary { name, .. } => name.clone(),
            Self::Elsewhere(path) => path.file_stem().map_or_else(
                || String::from("mesh"),
                |s| s.to_string_lossy().into_owned(),
            ),
        }
    }
}

/// The body a clip goes on when nobody said: the project's `stage_body`,
/// then the first body (said on stderr, because a sheet on the wrong body
/// is a sheet that lies quietly), then the fixture mannequin, written fresh
/// under `out/fixture/` from the profile so the command still answers on a
/// library with no body at all. `verb` is the command's own word for what
/// it does with the body — "posing on", "opening on" — so the warning reads
/// as that command's.
pub(crate) fn default_body(
    project: &Project,
    catalog: &Catalog,
    verb: &str,
) -> Result<Chosen, Failure> {
    let in_library = |record: &forge_library::AssetRecord| Chosen::InLibrary {
        rel_path: record.rel_path.clone(),
        name: record.name.clone(),
    };
    let first_body = catalog.records().iter().find(|r| r.kind == Kind::Body);
    if let Some(name) = project.stage_body.as_deref() {
        if let Some(record) = catalog.resolve(name, Some(Kind::Body)) {
            return Ok(in_library(record));
        }
        if let Some(first) = first_body {
            eprintln!(
                "warning: stage_body {name:?} is not in the library — {verb} the first body, {}",
                first.name
            );
            return Ok(in_library(first));
        }
    } else if let Some(first) = first_body {
        eprintln!(
            "warning: no [studio] stage_body in forge.toml — {verb} the first body, {}",
            first.name
        );
        return Ok(in_library(first));
    }
    let mannequin = write_mannequin(project)?;
    eprintln!(
        "warning: the library has no body — {verb} the fixture mannequin at {}",
        mannequin.display()
    );
    Ok(Chosen::Elsewhere(mannequin))
}

/// Resolve a named body or model: in the library by name, file name or
/// relative path; failing that, an existing glb by path. Refused with what
/// the library holds when it is neither.
pub(crate) fn named_mesh(catalog: &Catalog, wanted: &str) -> Result<Chosen, Failure> {
    if let Some(record) = catalog
        .resolve(wanted, Some(Kind::Body))
        .or_else(|| catalog.resolve(wanted, Some(Kind::Model)))
    {
        return Ok(Chosen::InLibrary {
            rel_path: record.rel_path.clone(),
            name: record.name.clone(),
        });
    }
    let path = Path::new(wanted);
    if path.is_file() {
        let absolute = std::path::absolute(path)
            .map_err(|e| Failure::refused(format!("{}: {e}", path.display())))?;
        return Ok(Chosen::Elsewhere(absolute));
    }
    Err(Failure::refused(format!(
        "{} — and it is not a file either",
        catalog.refusal(wanted, Some(Kind::Body))
    )))
}

/// Write the profile's fixture mannequin under `out/fixture/` and return its
/// absolute path. Byte-deterministic, so rewriting it is cheap and never
/// stale.
pub(crate) fn write_mannequin(project: &Project) -> Result<PathBuf, Failure> {
    let profile = project.profile()?;
    let out = std::path::absolute(project.out.join(MANNEQUIN_OUT))
        .map_err(|e| Failure::failed(format!("{MANNEQUIN_OUT}: {e}")))?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Failure::failed(format!("{}: {e}", parent.display())))?;
    }
    forge_rig::fixture::write_mannequin(&profile, &out)?;
    Ok(out)
}

/// A body on a stage with a clip beside it, wherever the two came from.
///
/// A library body stands on the asset root and the clip is named relative
/// to it. A body from elsewhere and the clip are both copied into a scratch
/// root, because that is the only way Bevy will load the two together; the
/// root is removed when the subject is dropped.
struct Subject {
    /// The asset root and the model on it.
    stage: Stage,
    /// The clip file, relative to the stage's root.
    clip_file: String,
    /// The scratch root, while one is needed.
    _scratch: Option<tempfile::TempDir>,
}

impl Subject {
    /// Put `body` and the library clip at `clip_rel` on one stage.
    fn new(project: &Project, body: Chosen, clip_rel: &str) -> Result<Self, Failure> {
        match body {
            Chosen::InLibrary { rel_path, .. } => Ok(Self {
                stage: Stage::new(project.assets.clone(), rel_path),
                clip_file: clip_rel.to_owned(),
                _scratch: None,
            }),
            Chosen::Elsewhere(path) => {
                let scratch = tempfile::Builder::new()
                    .prefix("forge_stage_")
                    .tempdir()
                    .map_err(|e| Failure::failed(format!("cannot make a scratch root: {e}")))?;
                let copy = |from: &Path, name: &str| {
                    std::fs::copy(from, scratch.path().join(name))
                        .map(|_| ())
                        .map_err(|e| Failure::failed(format!("{}: {e}", from.display())))
                };
                let body_name = file_name(&path);
                let clip_name = file_name(Path::new(clip_rel));
                copy(&path, &body_name)?;
                copy(&project.assets.join(clip_rel), &clip_name)?;
                Ok(Self {
                    stage: Stage::new(scratch.path(), body_name),
                    clip_file: clip_name,
                    _scratch: Some(scratch),
                })
            }
        }
    }
}

/// The last path segment, lossy.
fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || String::from("file.glb"),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Resolve `--body` for the commands that pose a clip: named, through
/// [`named_mesh`]; unstated, through [`default_body`].
fn body_for(project: &Project, catalog: &Catalog, wanted: Option<&str>) -> Result<Chosen, Failure> {
    match wanted {
        Some(wanted) => named_mesh(catalog, wanted),
        None => default_body(project, catalog, "posing on"),
    }
}

/// Resolve a clip: a library name first, else a clip glb under the asset
/// root by relative path. Returns the asset-relative path and the name.
fn clip_file(
    project: &Project,
    catalog: &Catalog,
    wanted: &str,
) -> Result<(String, String), Failure> {
    if let Some(record) = catalog.resolve(wanted, Some(Kind::Clip)) {
        return Ok((record.rel_path.clone(), record.name.clone()));
    }
    if let Some(rel) = under_assets(project, wanted) {
        let name = Path::new(&rel)
            .file_stem()
            .map_or_else(|| rel.clone(), |s| s.to_string_lossy().into_owned());
        return Ok((rel, name));
    }
    Err(Failure::refused(catalog.refusal(wanted, Some(Kind::Clip))))
}

/// `wanted` as a path relative to the asset root, when it is one and the
/// file exists. An absolute path is never that: `Path::join` would hand it
/// back whole, and Bevy refuses to load it anyway.
pub(crate) fn under_assets(project: &Project, wanted: &str) -> Option<String> {
    let rel = wanted.trim_start_matches("./").replace('\\', "/");
    if rel.is_empty() || Path::new(&rel).is_absolute() || rel.starts_with("../") {
        return None;
    }
    project.assets.join(&rel).is_file().then_some(rel)
}

/// Parse a `--views` list: comma-separated names, or `all`.
fn parse_views(spec: &str) -> Result<Vec<View>, Failure> {
    if spec.trim().eq_ignore_ascii_case("all") {
        return Ok(View::ALL.to_vec());
    }
    spec.split(',')
        .map(|name| {
            View::parse(name).ok_or_else(|| {
                Failure::refused(format!(
                    "{:?} is not a view — the views are {}, or all",
                    name.trim(),
                    View::ALL.map(View::name).join(", ")
                ))
            })
        })
        .collect()
}

/// Parse a `--cell WxH`.
fn parse_cell(spec: &str) -> Result<UVec2, Failure> {
    let refuse = || {
        Failure::refused(format!(
            "{spec:?} is not a cell size — say WxH, as in 384x512"
        ))
    };
    let (w, h) = spec.split_once(['x', 'X', ',']).ok_or_else(refuse)?;
    let (w, h): (u32, u32) = (
        w.trim().parse().map_err(|_| refuse())?,
        h.trim().parse().map_err(|_| refuse())?,
    );
    if w == 0 || h == 0 {
        return Err(refuse());
    }
    Ok(UVec2::new(w, h))
}

/// Write a shot and print its summary and where it went.
fn save(shot: &Shot, out: &Path, what: &str) -> Outcome {
    if let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| Failure::failed(format!("{}: {e}", parent.display())))?;
    }
    shot.save_png(out)
        .map_err(|e| Failure::failed(format!("{}: {e}", out.display())))?;
    print!("{}", shot.summary());
    println!(
        "{what}: {} ({}x{})",
        out.display(),
        shot.sheet.width(),
        shot.sheet.height()
    );
    Ok(())
}

/// `forge sheet <clip>`: poses across the clip on the stage body.
///
/// A sheet nothing bound to, or one where every pose is identical, is not a
/// successful render even though a PNG exists — exit 1 so this can gate a
/// recipe rather than quietly producing a picture of a T-pose.
pub(crate) fn sheet(project: &Project, args: &SheetArgs) -> Outcome {
    let catalog = Catalog::scan(project);
    let (clip_rel, clip_name) = clip_file(project, &catalog, &args.clip)?;
    let body = body_for(project, &catalog, args.body.body.as_deref())?;
    let subject = Subject::new(project, body, &clip_rel)?;
    let request = SheetRequest {
        frames: args.frames.max(1),
        views: parse_views(&args.views)?,
        cell: parse_cell(&args.cell)?,
        columns: args.columns.max(1),
        t0: args.t0.clamp(0.0, 1.0),
        t1: args.t1.clamp(0.0, 1.0),
        head_row: args.head_row,
        ..SheetRequest::new(subject.clip_file.clone())
    };
    let out = args.out.clone().unwrap_or_else(|| {
        project
            .out_dir(OutKind::Sheets)
            .join(format!("{clip_name}.png"))
    });
    let shot =
        render_clip_sheet(&subject.stage, &request).map_err(|e| Failure::failed(e.to_string()))?;
    save(&shot, &out, "sheet")?;
    match shot.failure() {
        Some(why) => Err(Failure::failed(format!("sheet: {why}"))),
        None => Ok(()),
    }
}

/// `forge views <name|path.glb>`: one mesh from every angle.
pub(crate) fn views(project: &Project, args: &ViewsArgs) -> Outcome {
    let catalog = Catalog::scan(project);
    let chosen = named_mesh(&catalog, &args.target)?;
    let stem = chosen.stem();
    let (stage, under_out) = match chosen {
        Chosen::InLibrary { rel_path, .. } => (Stage::new(project.assets.clone(), rel_path), false),
        Chosen::Elsewhere(path) => (
            Stage::from_path(&path).map_err(|e| Failure::refused(e.to_string()))?,
            is_under(&path, &project.out),
        ),
    };
    let request = ViewsRequest {
        views: match &args.views {
            Some(spec) => parse_views(spec)?,
            None => View::WALK_AROUND.to_vec(),
        },
        head_row: !args.no_head,
        cull_off: args.cull_off || under_out,
        ..ViewsRequest::default()
    };
    if under_out && !args.cull_off {
        eprintln!(
            "note: under out/, so culling is off — a face's inside showing means the surface is missing"
        );
    }
    let out = args
        .out
        .clone()
        .unwrap_or_else(|| project.out_dir(OutKind::Views).join(format!("{stem}.png")));
    let shot = render_views(&stage, &request).map_err(|e| Failure::failed(e.to_string()))?;
    save(&shot, &out, "views")
}

/// `forge turntable <body>`: every view and the head row, posed on the
/// profile's reference clip at its first frame when the library has it —
/// the pose a body is judged in — and at rest otherwise, said on stderr.
pub(crate) fn turntable(project: &Project, args: &TurntableArgs) -> Outcome {
    let catalog = Catalog::scan(project);
    let Some(record) = catalog.resolve(&args.body, Some(Kind::Body)) else {
        return Err(Failure::refused(
            catalog.refusal(&args.body, Some(Kind::Body)),
        ));
    };
    let stage = Stage::new(project.assets.clone(), record.rel_path.clone());
    let reference = project.profile()?.contract.reference_clip;
    let pose = catalog
        .resolve(&reference, Some(Kind::Clip))
        .map(|clip| (clip.rel_path.clone(), 0.0));
    if pose.is_none() {
        eprintln!("note: no reference clip {reference:?} in the library — rendering the rest pose");
    }
    let request = ViewsRequest {
        views: View::ALL.to_vec(),
        head_row: true,
        pose,
        ..ViewsRequest::default()
    };
    let out = args.out.clone().unwrap_or_else(|| {
        project
            .out_dir(OutKind::Views)
            .join(format!("{}.png", record.name))
    });
    let shot = render_views(&stage, &request).map_err(|e| Failure::failed(e.to_string()))?;
    save(&shot, &out, "turntable")?;
    if shot.is_total_mismatch() {
        return Err(Failure::failed(
            "turntable: the reference clip drives none of this body's bones — the pose is the rest pose",
        ));
    }
    Ok(())
}

/// `forge bones <clip>`: the binding report, no GPU.
pub(crate) fn bones(project: &Project, args: &BonesArgs) -> Outcome {
    let catalog = Catalog::scan(project);
    let (clip_rel, _) = clip_file(project, &catalog, &args.clip)?;
    let body = body_for(project, &catalog, args.body.body.as_deref())?;
    let subject = Subject::new(project, body, &clip_rel)?;
    let report = bones_report(&subject.stage, &subject.clip_file, 0)
        .map_err(|e| Failure::failed(e.to_string()))?;
    print!("{}", report.text());
    if report.is_total_mismatch() {
        return Err(Failure::failed(
            "bones: the clip drives none of this skeleton's bones",
        ));
    }
    Ok(())
}

/// Whether `path` lies under `dir`, by absolute paths.
pub(crate) fn is_under(path: &Path, dir: &Path) -> bool {
    let (Ok(path), Ok(dir)) = (std::path::absolute(path), std::path::absolute(dir)) else {
        return false;
    };
    path.starts_with(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_parse_names_and_all() {
        assert_eq!(parse_views("all").expect("all"), View::ALL.to_vec());
        assert_eq!(
            parse_views("front, back").expect("two"),
            vec![View::Front, View::Back]
        );
        let error = parse_views("front,sideways").expect_err("refused");
        assert!(error.message().contains("sideways"), "{}", error.message());
    }

    #[test]
    fn cells_parse_wxh_and_refuse_zero() {
        assert_eq!(parse_cell("384x512").expect("cell"), UVec2::new(384, 512));
        assert_eq!(parse_cell("256,256").expect("cell"), UVec2::new(256, 256));
        assert!(parse_cell("0x512").is_err());
        assert!(parse_cell("wide").is_err());
    }

    #[test]
    fn a_path_under_out_is_under_out() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("out");
        assert!(is_under(&out.join("lifts/x.glb"), &out));
        assert!(!is_under(&dir.path().join("assets/x.glb"), &out));
    }

    #[test]
    fn a_library_with_no_body_poses_on_the_mannequin_in_a_scratch_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = Project::init(dir.path(), "t").expect("project");
        let humanoid = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/humanoid");
        project.install_profile(&humanoid).expect("profile");
        let catalog = Catalog::scan(&project);
        let chosen = default_body(&project, &catalog, "posing on").expect("mannequin");
        let mannequin = dir.path().join("out").join(MANNEQUIN_OUT);
        assert!(
            matches!(&chosen, Chosen::Elsewhere(path) if *path == mannequin),
            "{chosen:?}"
        );
        assert!(mannequin.is_file());

        // The clip is copied beside it, and both go when the subject does.
        let clip = project.assets.join("clips/walk.glb");
        std::fs::create_dir_all(clip.parent().expect("parent")).expect("clips dir");
        std::fs::write(&clip, b"not really a glb").expect("clip");
        let subject = Subject::new(&project, chosen, "clips/walk.glb").expect("subject");
        assert_eq!(subject.stage.model, "mannequin.glb");
        assert_eq!(subject.clip_file, "walk.glb");
        let root = subject.stage.asset_root.clone();
        assert!(root.join("mannequin.glb").is_file());
        assert!(root.join("walk.glb").is_file());
        drop(subject);
        assert!(!root.exists());
    }

    #[test]
    fn an_absolute_path_is_never_under_the_asset_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = Project::init(dir.path(), "t").expect("project");
        let clip = project.assets.join("clips/walk.glb");
        std::fs::create_dir_all(clip.parent().expect("parent")).expect("clips dir");
        std::fs::write(&clip, b"not really a glb").expect("clip");
        assert_eq!(
            under_assets(&project, "./clips/walk.glb").as_deref(),
            Some("clips/walk.glb")
        );
        assert!(under_assets(&project, &clip.to_string_lossy()).is_none());
        assert!(under_assets(&project, "../assets/clips/walk.glb").is_none());
        assert!(under_assets(&project, "clips/run.glb").is_none());
    }

    #[test]
    fn a_body_that_does_not_exist_is_refused_by_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let project = Project::init(dir.path(), "t").expect("project");
        let catalog = Catalog::scan(&project);
        let error = body_for(&project, &catalog, Some("nobody")).expect_err("refused");
        assert!(error.message().contains("nobody"), "{}", error.message());
    }
}
