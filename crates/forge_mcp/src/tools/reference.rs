//! The one door a reference PNG comes through.
//!
//! No image model ships in this toolkit. A reference is **brought** — drawn
//! wherever the person drawing it works — and everything downstream of the
//! PNG is what the toolkit reproduces. So the record claims the picture's
//! integrity (a hash and a row in `SOURCES.md`) and never its regeneration,
//! which is the first entry in the lessons ledger and still the right one.
//!
//! What the door adds is the checking that used to happen after a GPU
//! minute: the format, the keyer, and the silhouette. A drawn floor band
//! lifts as a slab under the feet; a faint contact shadow keys as a
//! detached island above the dust threshold and rides a foot bone; a
//! flood-through hole in a pale jacket punches the subject open. All three
//! were measured riding through to a finished body on 2026-08-30, and all
//! three are refusals here, before anything costs the card.
//!
//! # The format text has one home
//!
//! `python/forge_gen/reference.py`'s `FORMAT`, which is the door that
//! measures against it. [`REFERENCE_FORMAT`] is a **generated** copy — see
//! `build.rs` — so the words an agent reads in this tool's description and
//! the words the importer holds a picture to cannot drift, and neither has
//! to be kept equal to the other by a test that fails on a rewrap.

use forge_library::promote::validate_name;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::server::ForgeServer;
use crate::tools::promote::{ACTOR, resolve_path, stated};
use crate::util;

/// The reference format, taken at build time from the importer's own
/// `FORMAT`. Empty when this crate is built without the Python source
/// beside it, which is a description missing a paragraph and never a second
/// copy of one.
pub(crate) const REFERENCE_FORMAT: &str =
    include_str!(concat!(env!("OUT_DIR"), "/reference_format.txt"));

/// What `import_reference` is, in its own words. The format paragraph is
/// **not** here: it is [`REFERENCE_FORMAT`], generated from the importer,
/// and [`description`] joins the two.
const LEAD: &str = "Bring one reference PNG into assets-src/refs/ with its record and its \
     SOURCES.md row. NO IMAGE MODEL RUNS HERE: a reference is drawn wherever you draw \
     pictures and brought through this door, which hashes it, states the source you give and \
     checks it — the format, the background keyer (a drawn floor band, a contact shadow or a \
     hole flooded through a pale garment is refused BY NAME, because all three ride through \
     to a finished body), and the silhouette. Every refusal names what it measured and what \
     to redraw; the fix is always the picture. The file stored is your ORIGINAL bytes, never \
     the keyed image, because the lift keys again and a keyed PNG in the source tree would be \
     a derived artefact whose hash describes something you never drew. Returns a JOB, so one \
     door owns every write under assets-src/. Next: generate_mesh on the path it reports.";

/// The tool's whole description: what it does, then the format it holds a
/// picture to.
///
/// Assembled here rather than written into `#[tool(description = …)]`
/// because that attribute takes a literal and the format text is generated
/// from the Python door. The one thing that must not happen is a second
/// hand-written copy of the format, so the join happens at runtime and the
/// generated block travels whole.
fn description() -> String {
    let format = REFERENCE_FORMAT.trim();
    if format.is_empty() {
        return String::from(LEAD);
    }
    format!("{LEAD}\n\nTHE FORMAT A PICTURE IS HELD TO:\n{format}")
}

/// This file's tools, for the server to sum.
pub(crate) fn router() -> ToolRouter<ForgeServer> {
    let mut router = ForgeServer::reference_router();
    if let Some(route) = router.map.get_mut("import_reference") {
        route.attr.description = Some(std::borrow::Cow::Owned(description()));
    }
    router
}

/// Arguments for `import_reference`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ImportReferenceArgs {
    /// The PNG to import, wherever it is now — a download, a path under
    /// out/. It is copied, not moved, and its ORIGINAL bytes are what land
    /// under assets-src/refs/.
    pub(crate) png: String,
    /// `snake_case` name for the reference. Becomes the file stem, and is
    /// the name the lift and the body will carry.
    pub(crate) name: String,
    /// `character` or `prop`. Decides which subdirectory it lands in and
    /// which silhouette checks run: a character is held to a T-pose, a prop
    /// to a three-quarter view inside the frame.
    pub(crate) kind: Option<String>,
    /// Where the picture came from, in your own words — the model, the
    /// tool, the artist, the licence. This is written into the record and
    /// into the SOURCES.md row verbatim, and it is the only provenance a
    /// brought picture has.
    pub(crate) source: String,
    /// Seconds to wait inline before answering. An import touches no card
    /// and normally finishes inside a second, so waiting is usually right.
    pub(crate) wait_s: Option<f64>,
}

#[tool_router(router = reference_router, vis = "pub(crate)")]
impl ForgeServer {
    /// Bring one reference PNG into the source tree, checked. The
    /// description clients see is [`description`], assembled in
    /// [`router`] so the generated format text can travel with it.
    #[tool]
    pub(crate) async fn import_reference(
        &self,
        Parameters(args): Parameters<ImportReferenceArgs>,
    ) -> CallToolResult {
        let project = &self.config.project;
        let png = resolve_path(project, &args.png);
        if !png.is_file() {
            return util::refuse(format!(
                "no file at {} — pass the path to the PNG you drew. nothing was written.",
                png.display()
            ));
        }
        let kind = match stated(args.kind.as_deref()).as_deref() {
            None | Some("character") => "character",
            Some("prop") => "prop",
            Some(other) => {
                return util::refuse(format!(
                    "{other:?} is not a reference kind — use character (a body in a T-pose) or \
                     prop (a three-quarter view)"
                ));
            }
        };
        let name = match validate_name(&args.name) {
            Ok(name) => name,
            Err(err) => return util::refuse(err.to_string()),
        };
        let Some(source) = stated(Some(&args.source)) else {
            return util::refuse(
                "source is empty — say where the picture came from in your own words (the \
                 model, the tool, the artist, the licence). A brought picture's provenance is \
                 what you state and nothing else, so an empty one is not a reference. nothing \
                 was written.",
            );
        };

        let dir = project.sources.join("refs").join(format!("{kind}s"));
        let out = dir.join(format!("{name}.png"));
        let record = dir.join(format!("{name}.ref.json"));
        if out.is_file() {
            return util::refuse(format!(
                "refused: {} already holds a reference called {name}. pick another name, or \
                 remove that one first — a reference is the durable source a body is \
                 re-derived from, so this door will not overwrite one. nothing was written.",
                project
                    .rel_to_root(&dir)
                    .unwrap_or_else(|| dir.display().to_string())
            ));
        }
        if let Err(err) = std::fs::create_dir_all(&dir) {
            return util::refuse(format!("cannot create {}: {err}", dir.display()));
        }

        let argv = vec![
            String::from("ref-import"),
            png.display().to_string(),
            String::from("--name"),
            name.clone(),
            String::from("--kind"),
            String::from(kind),
            String::from("--source"),
            source,
            // The door derives all three paths — the PNG, the record and the
            // ledger row's key — from one directory, so that is what it is
            // given. Naming the PNG and the record separately would let this
            // caller put a file somewhere the row it writes does not point.
            String::from("--sources"),
            project.sources.display().to_string(),
            String::from("--created-by"),
            String::from(ACTOR),
        ];

        // The door derives every path it writes from one directory, so the
        // command line names none of them and `outputs_claimed` would come
        // back empty — the queue would hold no lease on the one door that
        // owns writes under assets-src/. This caller already knows both
        // paths: it refused a taken name with them a dozen lines up. So it
        // states the claim rather than leaving it unstated.
        let mut spec = forge_serve::spec::spec_for(&argv, &project.root, ACTOR);
        let claim = |path: &std::path::Path| {
            project
                .rel_to_root(path)
                .unwrap_or_else(|| path.display().to_string())
        };
        spec.record = Some(claim(&record));
        spec.outputs_claimed = vec![claim(&out), claim(&record)];
        let job = match self.queue.submit(spec) {
            Ok(job) => job,
            Err(err) => return self.queue_refusal(&err),
        };
        if job.state == forge_serve::JobState::Refused {
            return util::refuse(format!(
                "import_reference refused: {}{}\n\nnothing was written.",
                job.message.as_deref().unwrap_or("(no message)"),
                job.hint
                    .as_deref()
                    .map_or_else(String::new, |hint| format!("\nhint: {hint}"))
            ));
        }

        if let Some(seconds) = args.wait_s
            && seconds > 0.0
        {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a wait ceiling in seconds; a negative one is filtered above"
            )]
            let max_s = seconds as u64;
            let mut frame = self
                .wait(Parameters(crate::tools::jobs::WaitArgs {
                    job: job.id.to_string(),
                    max_s: Some(max_s),
                }))
                .await;
            frame.content.insert(
                0,
                Content::text(format!("import_reference is job {}", job.id)),
            );
            return frame;
        }

        let position = self.position_of(&job.id);
        let mut frame = ForgeServer::job_frame(&job, position, None);
        frame["then"] = Value::from(format!(
            "when it is done, generate_mesh {{\"image\": {:?}, \"kind\": {kind:?}}} lifts it",
            out.display().to_string()
        ));
        CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&frame).unwrap_or_else(|_| frame.to_string()),
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{empty_project, server};
    use crate::util::frame_text;

    /// Three refusals, each a successful frame naming what would have
    /// worked: a picture that is not there, a kind the door does not have,
    /// and a provenance nobody stated.
    #[tokio::test]
    async fn the_door_refuses_a_missing_png_an_unknown_kind_and_an_empty_source() {
        let (dir, project) = empty_project();
        let server = server(project);

        let missing = server
            .import_reference(Parameters(ImportReferenceArgs {
                png: String::from("nowhere.png"),
                name: String::from("hero"),
                source: String::from("drawn in Grok"),
                ..ImportReferenceArgs::default()
            }))
            .await;
        let text = frame_text(&missing);
        assert!(text.contains("nowhere.png"), "{text}");
        assert!(text.contains("nothing was written"), "{text}");

        let png = dir.path().join("hero.png");
        std::fs::write(&png, b"not really a png").expect("write");

        let kind = server
            .import_reference(Parameters(ImportReferenceArgs {
                png: png.display().to_string(),
                name: String::from("hero"),
                kind: Some(String::from("vehicle")),
                source: String::from("drawn in Grok"),
                ..ImportReferenceArgs::default()
            }))
            .await;
        let text = frame_text(&kind);
        assert!(text.contains("character"), "{text}");
        assert!(text.contains("prop"), "{text}");

        let unsourced = server
            .import_reference(Parameters(ImportReferenceArgs {
                png: png.display().to_string(),
                name: String::from("hero"),
                source: String::from("   "),
                ..ImportReferenceArgs::default()
            }))
            .await;
        let text = frame_text(&unsourced);
        assert!(text.contains("source is empty"), "{text}");
    }

    /// A reference that already exists is not silently replaced: it is the
    /// durable source a body is re-derived from, and there is no
    /// `overwrite` on this door for the same reason there is no repairing a
    /// derived artefact.
    #[tokio::test]
    async fn a_reference_of_that_name_is_never_overwritten() {
        let (dir, project) = empty_project();
        let refs = project.sources.join("refs/characters");
        std::fs::create_dir_all(&refs).expect("refs");
        std::fs::write(refs.join("hero.png"), b"already here").expect("write");
        let png = dir.path().join("new.png");
        std::fs::write(&png, b"not really a png").expect("write");
        let server = server(project);

        let taken = server
            .import_reference(Parameters(ImportReferenceArgs {
                png: png.display().to_string(),
                name: String::from("hero"),
                source: String::from("drawn in Grok"),
                ..ImportReferenceArgs::default()
            }))
            .await;
        let text = frame_text(&taken);
        assert!(
            text.contains("already holds a reference called hero"),
            "{text}"
        );
        assert!(text.contains("nothing was written"), "{text}");
        assert_eq!(
            std::fs::read(refs.join("hero.png")).expect("still there"),
            b"already here",
            "the reference on disk is untouched"
        );
    }
}
