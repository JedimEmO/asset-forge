//! Seeing one asset: a mesh from every angle, a clip as a strip of poses.
//!
//! Both shell out to the binary that serves this — [`Config::renderer`]
//! is `current_exe()` — with exactly the arguments a human would type:
//! `forge views …` and `forge sheet …`. Nothing is rendered in-process, so
//! Bevy is linked once, in the binary, and the sheet an agent sees is the
//! sheet `forge sheet` writes.
//!
//! # What the exit code means
//!
//! The binary's codes are the library's: `0` a picture worth looking at,
//! `1` a gate that did not hold, `2` the call refused as written. For a
//! sheet, exit 1 still writes the PNG — it means the clip drives none of
//! the body's bones (the grid shows the rest pose) or every sampled pose is
//! identical (the clip never moves) — and the agent needs to see *that*
//! picture with the verdict beside it, so it comes back as an error result
//! carrying the image rather than as a refusal that hides it.
//!
//! [`Config::renderer`]: crate::config::Config::renderer

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use forge_library::{Catalog, Kind};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::server::ForgeServer;
use crate::util::{self, Captured, RENDER_TIMEOUT};

/// Arguments for `render_model`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(crate) struct RenderModelArgs {
    /// A body or model by library name (as `list_models` prints it), or a
    /// path to any .glb — a raw lift under out/lifts, an export under
    /// out/export, a file from elsewhere.
    pub(crate) name_or_path: String,
    /// Comma-separated views, or "all". Known: `three_quarter`, front, back,
    /// left, right, top. Default: front, back, left, right — the walk a
    /// reviewer takes around a mesh.
    #[serde(default)]
    pub(crate) views: Option<String>,
    /// Add three head close-ups under the views. Default true: a
    /// whole-body cell leaves a face two dozen pixels tall. Pass false for
    /// a prop.
    #[serde(default)]
    pub(crate) head_row: Option<bool>,
    /// Switch back-face culling off on every material, so a missing rear
    /// surface shows as the inside of the front one. Default: on for a
    /// path under the project's out/ (that is where raw lifts live), off
    /// for a library asset.
    #[serde(default)]
    pub(crate) cull_off: Option<bool>,
    /// Set false to get only the report and the path — for a sweep over
    /// many meshes where inlining every image would flood the context.
    #[serde(default)]
    pub(crate) return_image: Option<bool>,
}

/// Arguments for `render_clip_strip`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(crate) struct RenderClipArgs {
    /// The clip: a library name, file name or stem from `list_clips`, or a
    /// path to a clip .glb under the asset root.
    pub(crate) clip: String,
    /// The body to pose it on: a library name, a file name, or a path to a
    /// rigged .glb anywhere. Default: the project's `stage_body`, else the
    /// first body (said on stderr), else the fixture mannequin.
    #[serde(default)]
    pub(crate) body: Option<String>,
    /// Poses sampled across the window. Default 8.
    #[serde(default)]
    pub(crate) frames: Option<u32>,
    /// Comma-separated views, one band each, or "all". Default
    /// `three_quarter`. Known: `three_quarter`, front, back, left, right, top.
    #[serde(default)]
    pub(crate) views: Option<String>,
    /// Cells per row. Default 4.
    #[serde(default)]
    pub(crate) columns: Option<u32>,
    /// Cell size as `WxH` pixels. Default 384x512, clamped to keep the sheet
    /// under the resolution a vision model downscales past.
    #[serde(default)]
    pub(crate) cell: Option<String>,
    /// Window start as a fraction of clip length, to zoom in on a moment.
    #[serde(default)]
    pub(crate) t0: Option<f32>,
    /// Window end as a fraction of clip length.
    #[serde(default)]
    pub(crate) t1: Option<f32>,
    /// Add a band of head close-ups under the view bands. Default false.
    #[serde(default)]
    pub(crate) head_row: Option<bool>,
    /// Set false to get only the report and the path, for sweeps over many
    /// clips.
    #[serde(default)]
    pub(crate) return_image: Option<bool>,
}

#[tool_router(router = render_tools, vis = "pub(crate)")]
impl ForgeServer {
    /// Render a mesh from several angles and return the sheet inline.
    #[tool(
        description = "Render a body or model from the angles a reviewer walks to — front, \
                       back, both sides, plus three head close-ups — composed into one \
                       labelled contact sheet returned as an inline image. Takes a library \
                       name from list_models or a path to any .glb; a path under out/ (a raw \
                       lift) renders with culling off, so a missing back shows as the inside \
                       of the front. Use it to judge proportion, silhouette, face, and \
                       whether the surface is actually there. Headless; takes 10-60s."
    )]
    async fn render_model(&self, Parameters(args): Parameters<RenderModelArgs>) -> CallToolResult {
        let catalog = Catalog::scan(&self.config.project);
        let target = match resolve_mesh(&self.config.project, &catalog, &args.name_or_path) {
            Ok(target) => target,
            Err(refusal) => return refusal,
        };
        let out_path = self.config.scratch_png(&format!("views-{}", target.stem()));

        let mut cmd = self.forge_command();
        cmd.arg("views")
            .arg(target.argument())
            .arg("--out")
            .arg(&out_path);
        if let Some(views) = clean(args.views.as_deref()) {
            cmd.arg("--views").arg(views);
        }
        if !args.head_row.unwrap_or(true) {
            cmd.arg("--no-head");
        }
        if args.cull_off.unwrap_or(false) {
            cmd.arg("--cull-off");
        }

        let what = format!("render of {}", target.label());
        let captured = match util::run(&mut cmd, RENDER_TIMEOUT).await.or_refuse(
            &what,
            &self.config.renderer,
            RENDER_TIMEOUT,
        ) {
            Ok(captured) => captured,
            Err(refusal) => return refusal,
        };

        let mut text = format!("{}\n{}", target.describe(), captured.stdout.trim_end());
        if let Some(note) = note_line(&captured) {
            let _ = write!(text, "\n{note}");
        }
        let mut blocks = vec![Content::text(text)];
        if args.return_image.unwrap_or(true) {
            blocks.push(util::inline_image(&out_path).into_content(&out_path, "sheet"));
        }
        CallToolResult::success(blocks)
    }

    /// Render a clip as a contact sheet and return it inline.
    #[tool(
        description = "Render evenly spaced poses of an animation clip playing on a body, \
                       composed into one labelled contact sheet (frame and time under each \
                       cell) returned as an inline image. Use it to judge whether a clip \
                       reads as the action it claims: silhouette, foot contact, limbs through \
                       the body. The report's bones line is the binding verdict — 0 driven \
                       means the clip does not match this skeleton, and the sheet shows the \
                       rest pose; an identical grid means the clip never moves. Both come \
                       back as an error result with the picture attached. Headless; takes \
                       10-60s."
    )]
    async fn render_clip_strip(
        &self,
        Parameters(args): Parameters<RenderClipArgs>,
    ) -> CallToolResult {
        let catalog = Catalog::scan(&self.config.project);
        let clip = match resolve_clip(&self.config.project, &catalog, &args.clip) {
            Ok(clip) => clip,
            Err(refusal) => return refusal,
        };
        if let Some(body) = clean(args.body.as_deref())
            && let Err(refusal) = check_body(&catalog, body)
        {
            return refusal;
        }
        let out_path = self.config.scratch_png(&format!("sheet-{}", clip.name));

        let mut cmd = self.forge_command();
        cmd.arg("sheet")
            .arg(&clip.rel_path)
            .arg("--out")
            .arg(&out_path);
        if let Some(body) = clean(args.body.as_deref()) {
            cmd.arg("--body").arg(body);
        }
        if let Some(frames) = args.frames {
            cmd.arg("--frames").arg(frames.to_string());
        }
        if let Some(views) = clean(args.views.as_deref()) {
            cmd.arg("--views").arg(views);
        }
        if let Some(columns) = args.columns {
            cmd.arg("--columns").arg(columns.to_string());
        }
        if let Some(cell) = clean(args.cell.as_deref()) {
            cmd.arg("--cell").arg(cell);
        }
        if let Some(t0) = args.t0 {
            cmd.arg("--t0").arg(t0.to_string());
        }
        if let Some(t1) = args.t1 {
            cmd.arg("--t1").arg(t1.to_string());
        }
        if args.head_row.unwrap_or(false) {
            cmd.arg("--head-row");
        }

        let what = format!("render of {}", clip.rel_path);
        let ran = util::run(&mut cmd, RENDER_TIMEOUT).await;
        // Exit 1 with a sheet on disk is a verdict, not a failure to
        // render: the agent needs the picture and the reason together.
        let (captured, gate) = match ran {
            util::Ran::Failed(captured) if captured.code == Some(1) && out_path.is_file() => {
                let verdict = gate_verdict(&captured);
                (captured, Some(verdict))
            }
            other => match other.or_refuse(&what, &self.config.renderer, RENDER_TIMEOUT) {
                Ok(captured) => (captured, None),
                Err(refusal) => return refusal,
            },
        };

        let mut text = format!(
            "clip: {}{}\n{}",
            clip.rel_path,
            clean(args.body.as_deref()).map_or(String::new(), |b| format!("  body: {b}")),
            captured.stdout.trim_end()
        );
        if let Some(line) = events_line(&clip.events) {
            let _ = write!(text, "\n{line}");
        }
        if let Some(note) = note_line(&captured) {
            let _ = write!(text, "\n{note}");
        }
        if let Some(verdict) = &gate {
            let _ = write!(text, "\nGATE (exit 1): {verdict}");
        }
        let mut blocks = vec![Content::text(text)];
        if args.return_image.unwrap_or(true) {
            blocks.push(util::inline_image(&out_path).into_content(&out_path, "sheet"));
        }
        if gate.is_some() {
            CallToolResult::error(blocks)
        } else {
            CallToolResult::success(blocks)
        }
    }
}

impl ForgeServer {
    /// `forge --project <root>` — the renderer, pointed at this project
    /// explicitly so it never has to discover one from whatever working
    /// directory the MCP client gave us.
    pub(crate) fn forge_command(&self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.config.renderer);
        cmd.arg("--project").arg(&self.config.project.root);
        cmd
    }
}

/// The module's router, for `tools::router` to sum.
pub(crate) fn router() -> rmcp::handler::server::router::tool::ToolRouter<ForgeServer> {
    ForgeServer::render_tools()
}

/// A mesh to render: in the library by name, or a file somewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MeshTarget {
    /// A library asset: the kind, the name, the asset-relative path.
    InLibrary {
        /// Body or model.
        kind: Kind,
        /// The asset's name.
        name: String,
        /// Relative to the asset root.
        rel_path: String,
    },
    /// An existing `.glb` outside the library, by absolute path.
    File(PathBuf),
}

impl MeshTarget {
    /// What `forge views` is given.
    fn argument(&self) -> String {
        match self {
            Self::InLibrary { name, .. } => name.clone(),
            Self::File(path) => path.display().to_string(),
        }
    }

    /// The stem a scratch file takes.
    fn stem(&self) -> String {
        match self {
            Self::InLibrary { name, .. } => name.clone(),
            Self::File(path) => path.file_stem().map_or_else(
                || String::from("mesh"),
                |s| s.to_string_lossy().into_owned(),
            ),
        }
    }

    /// For a message.
    fn label(&self) -> String {
        match self {
            Self::InLibrary { rel_path, .. } => rel_path.clone(),
            Self::File(path) => path.display().to_string(),
        }
    }

    /// The first line of the report: what was rendered and where it is.
    fn describe(&self) -> String {
        match self {
            Self::InLibrary {
                kind,
                name,
                rel_path,
            } => format!("{kind}: {name} ({rel_path})"),
            Self::File(path) => format!("file: {}", path.display()),
        }
    }
}

/// Resolve `render_model`'s target: a file that exists is that file (and,
/// under the asset root, its library record); otherwise a library body or
/// model by name; else a refusal listing every mesh the library holds.
///
/// The file check comes first on purpose: `out/export/hero.glb` and the
/// shipped `bodies/hero.glb` share a file name, and the export is exactly
/// what a caller wants to look at before promoting over the body.
pub(crate) fn resolve_mesh(
    project: &forge_library::Project,
    catalog: &Catalog,
    wanted: &str,
) -> Result<MeshTarget, CallToolResult> {
    let wanted = wanted.trim();
    let in_library = |record: &forge_library::AssetRecord| MeshTarget::InLibrary {
        kind: record.kind,
        name: record.name.clone(),
        rel_path: record.rel_path.clone(),
    };
    let lookup = |name: &str| {
        catalog
            .resolve(name, Some(Kind::Body))
            .or_else(|| catalog.resolve(name, Some(Kind::Model)))
    };
    if let Some(absolute) = existing_file(wanted)? {
        if let Some(rel) = project.rel_to_assets(&absolute)
            && let Some(record) = lookup(&rel)
        {
            return Ok(in_library(record));
        }
        return Ok(MeshTarget::File(absolute));
    }
    if let Some(record) = lookup(wanted) {
        return Ok(in_library(record));
    }
    let mut names = catalog.names(Some(Kind::Body));
    names.extend(catalog.names(Some(Kind::Model)));
    Err(util::refuse_unknown("mesh", wanted, &names))
}

/// `wanted` as an absolute path when it names an existing file, `None`
/// when it does not (a library name, a typo), a refusal when the operating
/// system cannot make it absolute.
pub(crate) fn existing_file(wanted: &str) -> Result<Option<PathBuf>, CallToolResult> {
    let path = Path::new(wanted);
    if wanted.is_empty() || !path.is_file() {
        return Ok(None);
    }
    std::path::absolute(path)
        .map(Some)
        .map_err(|e| util::refuse(format!("{}: {e}", path.display())))
}

/// A clip to render, and what its record says about its timeline.
#[derive(Debug, Clone)]
pub(crate) struct ClipTarget {
    /// The asset's name (or the file stem, for a path under the asset root).
    pub(crate) name: String,
    /// Relative to the asset root — what `forge sheet` takes.
    pub(crate) rel_path: String,
    /// The record's events, when it has any.
    pub(crate) events: Vec<forge_library::schema::AnimEvent>,
}

/// Resolve a clip: a library name first, then a clip .glb under the asset
/// root by relative path; else a refusal listing every clip.
pub(crate) fn resolve_clip(
    project: &forge_library::Project,
    catalog: &Catalog,
    wanted: &str,
) -> Result<ClipTarget, CallToolResult> {
    if let Some(record) = catalog.resolve(wanted, Some(Kind::Clip)) {
        return Ok(ClipTarget {
            name: record.name.clone(),
            rel_path: record.rel_path.clone(),
            events: record
                .sidecar
                .as_ref()
                .and_then(|s| s.events.clone())
                .unwrap_or_default(),
        });
    }
    let rel = wanted.trim().trim_start_matches("./").replace('\\', "/");
    if !rel.is_empty()
        && !Path::new(&rel).is_absolute()
        && !rel.starts_with("../")
        && project.assets.join(&rel).is_file()
    {
        let name = Path::new(&rel)
            .file_stem()
            .map_or_else(|| rel.clone(), |s| s.to_string_lossy().into_owned());
        return Ok(ClipTarget {
            name,
            rel_path: rel,
            events: Vec::new(),
        });
    }
    Err(util::refuse_unknown(
        "clip",
        wanted,
        &catalog.names(Some(Kind::Clip)),
    ))
}

/// A stated body must be a library body, a library model, or a file — the
/// same rule `forge sheet --body` applies — and a refusal here lists the
/// bodies before a render is spent finding out.
fn check_body(catalog: &Catalog, wanted: &str) -> Result<(), CallToolResult> {
    if catalog.resolve(wanted, Some(Kind::Body)).is_some()
        || catalog.resolve(wanted, Some(Kind::Model)).is_some()
        || Path::new(wanted).is_file()
    {
        return Ok(());
    }
    Err(util::refuse_unknown(
        "body",
        wanted,
        &catalog.names(Some(Kind::Body)),
    ))
}

/// The binary's own reason for exit 1, from its last stderr line — "sheet:
/// the clip drives none of this skeleton's bones" — with the `forge: `
/// prefix dropped.
fn gate_verdict(captured: &Captured) -> String {
    captured
        .stderr
        .lines()
        .rev()
        .find(|l| l.starts_with("forge:"))
        .map_or_else(
            || String::from("the sheet did not pass the binary's gate"),
            |l| l.trim_start_matches("forge:").trim().to_owned(),
        )
}

/// The binary's `note:` and `warning:` lines from stderr — which body it
/// chose when none was stated, that culling is off — relayed because they
/// change how the picture should be read.
fn note_line(captured: &Captured) -> Option<String> {
    let lines: Vec<&str> = captured
        .stderr
        .lines()
        .filter(|l| l.starts_with("note:") || l.starts_with("warning:"))
        .collect();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

/// The clip's recorded timeline events as one report line, or `None` when
/// there are none to state. Times are the built clip's seconds — the clock
/// the strip's cell labels quote.
fn events_line(events: &[forge_library::schema::AnimEvent]) -> Option<String> {
    if events.is_empty() {
        return None;
    }
    let listed: Vec<String> = events
        .iter()
        .map(|event| format!("{} @ {:.3}s", event.name, event.t))
        .collect();
    Some(format!("events:  {}", listed.join(", ")))
}

/// A string argument trimmed, `None` when it is empty.
fn clean(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;
    use crate::util::frame_text;
    use forge_library::schema::{AnimEvent, AudioRef, EventOrigin};

    #[test]
    fn a_mesh_resolves_by_name_by_file_name_and_as_a_file() {
        let (dir, project) = testing::library();
        let catalog = Catalog::scan(&project);
        let shipped = project.kind_dir(Kind::Body).join("mannequin.glb");
        let shipped = shipped.to_string_lossy().into_owned();
        for wanted in [
            "mannequin",
            "MANNEQUIN.glb",
            "bodies/mannequin.glb",
            shipped.as_str(),
        ] {
            let target = resolve_mesh(&project, &catalog, wanted).expect(wanted);
            assert_eq!(
                target,
                MeshTarget::InLibrary {
                    kind: Kind::Body,
                    name: String::from("mannequin"),
                    rel_path: String::from("bodies/mannequin.glb"),
                },
                "{wanted}"
            );
        }
        // An export under out/ that shares the body's file name is the
        // export, not the body: the path was typed on purpose.
        let export = project.out.join("export/mannequin.glb");
        std::fs::create_dir_all(export.parent().expect("parent")).expect("mkdir");
        std::fs::write(&export, b"not a glb, but a file").expect("write");
        let target = resolve_mesh(&project, &catalog, &export.to_string_lossy()).expect("file");
        assert!(matches!(&target, MeshTarget::File(p) if p.is_absolute()));
        assert!(target.describe().starts_with("file:"));
        assert_eq!(target.stem(), "mannequin");

        let refusal = resolve_mesh(&project, &catalog, "nobody").expect_err("refused");
        let text = frame_text(&refusal);
        assert!(text.contains("no mesh matching \"nobody\""), "{text}");
        assert!(text.contains("  mannequin"), "{text}");
        drop(dir);
    }

    #[test]
    fn a_clip_resolves_by_name_or_by_a_path_under_the_asset_root() {
        let (_dir, project) = testing::library();
        let catalog = Catalog::scan(&project);
        for wanted in ["roll", "roll.glb", "clips/roll.glb", "./clips/roll.glb"] {
            let clip = resolve_clip(&project, &catalog, wanted).expect(wanted);
            assert_eq!(clip.rel_path, "clips/roll.glb");
            assert_eq!(clip.name, "roll");
            assert!(
                !clip.events.is_empty(),
                "the roll take has contacts, so the record has footsteps"
            );
        }
        // A clip file under the asset root with no record still renders.
        let stray = project.assets.join("clips/stray.glb");
        std::fs::write(&stray, b"glb").expect("write");
        let clip = resolve_clip(&project, &catalog, "clips/stray.glb").expect("stray");
        assert_eq!(clip.name, "stray");
        assert!(clip.events.is_empty());
        assert!(resolve_clip(&project, &catalog, &stray.to_string_lossy()).is_err());

        let refusal = resolve_clip(&project, &catalog, "walk").expect_err("refused");
        let text = frame_text(&refusal);
        assert!(text.contains("no clip matching \"walk\""), "{text}");
        assert!(text.contains("  roll"), "{text}");
    }

    #[test]
    fn a_stated_body_is_checked_before_the_render_is_spent() {
        let (_dir, project) = testing::library();
        let catalog = Catalog::scan(&project);
        assert!(check_body(&catalog, "mannequin").is_ok());
        let refusal = check_body(&catalog, "vex").expect_err("refused");
        let text = frame_text(&refusal);
        assert!(text.contains("no body matching \"vex\""), "{text}");
        assert!(text.contains("mannequin"), "{text}");
    }

    #[test]
    fn the_events_line_lists_each_event_on_the_built_clock() {
        let events = vec![
            AnimEvent {
                t: 0.35,
                t_src: Some(0.85),
                name: String::from("footstep_l"),
                origin: EventOrigin::Contacts,
                audio: None,
            },
            AnimEvent {
                t: 1.2,
                t_src: None,
                name: String::from("fire"),
                origin: EventOrigin::Agent(String::from("claude")),
                audio: AudioRef::parse("sfx:pistol_shot"),
            },
        ];
        let line = events_line(&events).expect("two events make a line");
        assert_eq!(line, "events:  footstep_l @ 0.350s, fire @ 1.200s");
        assert_eq!(events_line(&[]), None);
    }

    #[test]
    fn the_gate_verdict_and_the_notes_come_from_stderr() {
        let captured = Captured {
            code: Some(1),
            stdout: String::new(),
            stderr: String::from(
                "warning: no [studio] stage_body in forge.toml — posing on the first body, mannequin\n\
                 forge: sheet: the clip drives none of this skeleton's bones\n",
            ),
        };
        assert_eq!(
            gate_verdict(&captured),
            "sheet: the clip drives none of this skeleton's bones"
        );
        assert!(
            note_line(&captured)
                .expect("a warning")
                .contains("posing on the first body")
        );
        let quiet = Captured {
            code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
        };
        assert_eq!(note_line(&quiet), None);
        assert!(gate_verdict(&quiet).contains("gate"));
    }

    #[tokio::test]
    async fn an_unknown_name_is_refused_before_the_renderer_is_touched() {
        let (_dir, project) = testing::library();
        let server = testing::server(project);
        let result = server
            .render_model(Parameters(RenderModelArgs {
                name_or_path: String::from("ghost"),
                views: None,
                head_row: None,
                cull_off: None,
                return_image: None,
            }))
            .await;
        assert_eq!(result.is_error, Some(true));
        let text = frame_text(&result);
        assert!(
            text.contains("available meshs") || text.contains("mannequin"),
            "{text}"
        );

        let result = server
            .render_clip_strip(Parameters(RenderClipArgs {
                clip: String::from("ghost"),
                body: None,
                frames: None,
                views: None,
                columns: None,
                cell: None,
                t0: None,
                t1: None,
                head_row: None,
                return_image: None,
            }))
            .await;
        assert_eq!(result.is_error, Some(true));
        assert!(frame_text(&result).contains("roll"));
    }

    #[tokio::test]
    async fn a_missing_renderer_is_a_refusal_naming_it() {
        let (_dir, project) = testing::library();
        let server = testing::server(project);
        let result = server
            .render_model(Parameters(RenderModelArgs {
                name_or_path: String::from("mannequin"),
                views: None,
                head_row: None,
                cull_off: None,
                return_image: Some(false),
            }))
            .await;
        assert_eq!(result.is_error, Some(true));
        let text = frame_text(&result);
        assert!(text.contains("could not run"), "{text}");
        assert!(text.contains("forge-for-tests"), "{text}");
    }
}
