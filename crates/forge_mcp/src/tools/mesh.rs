//! The three steps that turn a reference PNG into a rigged body, each a job.
//!
//! `generate_mesh` lifts the picture, `prepare_body` normalises the lift and
//! puts a skeleton in it, `skin_body` hands that to `SkinTokens` and fits the
//! skeleton to the body its own weights describe. All three write under
//! `out/` and none of them touches the library: `promote_body` is the door
//! for that, and it is a separate decision on purpose.
//!
//! # Why they are jobs and not calls
//!
//! Each of the three wants the card for minutes — a 1024³ lift, two
//! `SkinTokens` passes — and one card is shared by every door in the toolkit.
//! So each returns a **job id** and the literal `wait` call to make next,
//! exactly as `generate_audio` does, and the queue is what decides when the
//! card is free. A tool that blocked for four minutes would hold an agent's
//! whole turn and still be racing the terminal.
//!
//! # One binary, one launcher
//!
//! Nothing here composes a generator's flags beyond the handful it exposes:
//! the argv is `forge gen <verb> …`, the same line `just character` types,
//! and the queue spawns it. So the MCP door and the terminal door read one
//! backend table, write one record shape, and cannot drift into two
//! behaviours for one piece of work.

use forge_library::project::OutKind;
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

/// This file's tools, for the server to sum.
pub(crate) fn router() -> ToolRouter<ForgeServer> {
    ForgeServer::mesh_router()
}

/// Arguments for `generate_mesh`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct GenerateMeshArgs {
    /// The reference PNG, as `import_reference` reported it — normally
    /// `assets-src/refs/characters/<name>.png`. A relative path is read
    /// against the project root.
    pub(crate) image: String,
    /// `snake_case` stem for the lift under out/lifts/. Default: the
    /// image's own stem. Becomes the suggested library name.
    pub(crate) name: Option<String>,
    /// `character` (a body, to be prepared and skinned) or `prop` (a static
    /// mesh, to be normalised and promoted as a model). Default `character`.
    pub(crate) kind: Option<String>,
    /// The register: `character`, `prop` or `detail`. Omitted, the
    /// generator's own default for the kind.
    pub(crate) preset: Option<String>,
    /// Sampler seed. One front view underdetermines a shape, so the seed is
    /// the knob to turn when the back of a head comes back hollow.
    pub(crate) seed: Option<i64>,
    /// Where the picture came from, in the words you would put in
    /// SOURCES.md. Recorded on the lift.
    pub(crate) source: Option<String>,
    /// The prompt the picture was drawn with, when you have it.
    pub(crate) prompt: Option<String>,
    /// Seconds to wait inline before answering. Omitted, the tool returns a
    /// job id at once and `wait` is the next call.
    pub(crate) wait_s: Option<f64>,
}

/// Arguments for `prepare_body`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PrepareBodyArgs {
    /// The raw lift, as `generate_mesh` reported it — `out/lifts/<name>.glb`.
    pub(crate) glb: String,
    /// `snake_case` stem for the prepared mesh under out/prepare/. Default:
    /// the input's stem.
    pub(crate) name: Option<String>,
    /// Height to scale the body to, metres. Omitted, the profile's own
    /// reference stature.
    pub(crate) stature_m: Option<f32>,
    /// Degrees to turn the body about +Y so it faces the rig's front.
    pub(crate) yaw_deg: Option<f32>,
    /// A skeleton to insert instead of the profile's reference rig — what
    /// the second pass of a fit uses. Omitted, the profile's.
    pub(crate) skeleton: Option<String>,
    /// Seconds to wait inline before answering.
    pub(crate) wait_s: Option<f64>,
}

/// Arguments for `skin_body`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct SkinBodyArgs {
    /// The prepared mesh, as `prepare_body` reported it —
    /// `out/prepare/<name>.glb`.
    pub(crate) glb: String,
    /// `snake_case` library name for the body. Default: the input's stem.
    /// The rig record and the working `.blend` are written under it.
    pub(crate) name: Option<String>,
    /// Refuse to start with less free VRAM than this. Omitted, the door's
    /// own floor; doctor says who is holding the card.
    pub(crate) min_free_gb: Option<f32>,
    /// Seconds to wait inline before answering. A skin is two `SkinTokens`
    /// passes and two Blender steps — about a minute on a free card.
    pub(crate) wait_s: Option<f64>,
}

#[tool_router(router = mesh_router, vis = "pub(crate)")]
impl ForgeServer {
    /// Lift a reference PNG to a textured mesh.
    #[tool(
        description = "Lift one reference PNG to a textured mesh with TRELLIS.2. This returns \
                       a JOB, not a file: a lift wants the card for a minute or two and the \
                       card is shared, so the frame carries the job id and the literal `wait` \
                       call to make next. The mesh and its lift record land under \
                       out/lifts/ — never in the library. LOOK AT IT NEXT: render_model on \
                       the path under out/ draws it from seven angles with culling off, which \
                       is how a hollow head shows. A character then goes through prepare_body \
                       and skin_body; a prop goes straight to promote_model once it has been \
                       normalised. One front view underdetermines a shape — when the back \
                       comes back wrong, change the seed or redraw the picture, never repair \
                       the mesh. Refuses with the install line when TRELLIS.2 is not set up \
                       (doctor has the full table). The texture baker is nvdiffrast, which is \
                       non-commercial; the lift record says so and keeps saying so."
    )]
    pub(crate) async fn generate_mesh(
        &self,
        Parameters(args): Parameters<GenerateMeshArgs>,
    ) -> CallToolResult {
        let project = &self.config.project;
        let image = resolve_path(project, &args.image);
        if !image.is_file() {
            return util::refuse(format!(
                "no reference PNG at {} — import_reference is the door that puts one under \
                 assets-src/refs/, and it prints the path it wrote. nothing was written.",
                image.display()
            ));
        }
        let kind = match stated(args.kind.as_deref()).as_deref() {
            None | Some("character") => "character",
            Some("prop") => "prop",
            Some(other) => {
                return util::refuse(format!(
                    "{other:?} is not a mesh kind — use character (a body to rig) or prop (a \
                     static mesh)"
                ));
            }
        };
        if let Some(refusal) = self.backend_refusal("trellis2", "generate_mesh") {
            return refusal;
        }
        let name = match named(args.name.as_deref(), &image) {
            Ok(name) => name,
            Err(refusal) => return util::refuse(refusal),
        };

        let dir = project.out_dir(OutKind::Lifts);
        if let Err(err) = std::fs::create_dir_all(&dir) {
            return util::refuse(format!("cannot create {}: {err}", dir.display()));
        }
        let out = dir.join(format!("{name}.glb"));
        let record = dir.join(format!("{name}.lift.json"));

        let mut argv = vec![
            String::from("mesh"),
            image.display().to_string(),
            String::from("--out"),
            out.display().to_string(),
            String::from("--record"),
            record.display().to_string(),
        ];
        push_stated(&mut argv, "--preset", args.preset.as_deref());
        push_stated(&mut argv, "--source", args.source.as_deref());
        push_stated(&mut argv, "--prompt", args.prompt.as_deref());
        if let Some(seed) = args.seed {
            argv.push(String::from("--seed"));
            argv.push(seed.to_string());
        }
        argv.push(String::from("--created-by"));
        argv.push(String::from(ACTOR));

        let then = if kind == "prop" {
            format!(
                "when it is done, render_model {{\"name_or_path\": {:?}}} to look at it from \
                 every side, then normalise it and promote_model",
                out.display().to_string()
            )
        } else {
            format!(
                "when it is done, render_model {{\"name_or_path\": {:?}}} to look at it from \
                 every side, then prepare_body {{\"glb\": {:?}}}",
                out.display().to_string(),
                out.display().to_string()
            )
        };
        self.queue_gen("generate_mesh", argv, args.wait_s, &then)
            .await
    }

    /// Normalise a lift and put the profile's skeleton in it.
    #[tool(
        description = "Normalise a raw lift and insert a skeleton, in headless Blender: scale \
                       to the profile's stature, face the rig's front, drop generation debris, \
                       hold the triangle budget, and refuse a mesh the skinner cannot work \
                       with. This returns a JOB; the prepared mesh and its record land under \
                       out/prepare/. TWO GATES LIVE HERE and both name the reference PNG when \
                       they refuse: the arms must be horizontal against the body's OWN \
                       shoulder line (a weight cannot fix a pose), and a limb must not be \
                       thinner than the bone it hangs on (a posterized picture gives the lift \
                       no shading to take volume from). Both refusals print the numbers they \
                       measured. The fix for either is upstream — redraw the picture, or \
                       re-lift at another seed — never a patch in Blender. Next is skin_body."
    )]
    pub(crate) async fn prepare_body(
        &self,
        Parameters(args): Parameters<PrepareBodyArgs>,
    ) -> CallToolResult {
        let project = &self.config.project;
        let glb = resolve_path(project, &args.glb);
        if !glb.is_file() {
            return util::refuse(format!(
                "no mesh at {} — pass the path generate_mesh reported, under out/lifts/. \
                 nothing was written.",
                glb.display()
            ));
        }
        if let Some(refusal) = self.backend_refusal("blender", "prepare_body") {
            return refusal;
        }
        let name = match named(args.name.as_deref(), &glb) {
            Ok(name) => name,
            Err(refusal) => return util::refuse(refusal),
        };
        // `out/prepare/` has no `OutKind` yet; the directory is the one
        // `forge gen prepare` writes by default, named here so the job's
        // claim and the tool's report agree.
        let dir = project.out.join("prepare");
        if let Err(err) = std::fs::create_dir_all(&dir) {
            return util::refuse(format!("cannot create {}: {err}", dir.display()));
        }
        let out = dir.join(format!("{name}.glb"));

        let mut argv = vec![
            String::from("prepare"),
            glb.display().to_string(),
            String::from("--out"),
            out.display().to_string(),
        ];
        if let Some(stature) = args.stature_m {
            argv.push(String::from("--stature"));
            argv.push(stature.to_string());
        }
        if let Some(yaw) = args.yaw_deg {
            argv.push(String::from("--yaw-deg"));
            argv.push(yaw.to_string());
        }
        if let Some(skeleton) = stated(args.skeleton.as_deref()) {
            argv.push(String::from("--skeleton"));
            argv.push(resolve_path(project, &skeleton).display().to_string());
        }
        argv.push(String::from("--created-by"));
        argv.push(String::from(ACTOR));

        let then = format!(
            "when it is done, skin_body {{\"glb\": {:?}}} weights it and fits the skeleton to \
             this body's own proportions",
            out.display().to_string()
        );
        self.queue_gen("prepare_body", argv, args.wait_s, &then)
            .await
    }

    /// Skin a prepared mesh, and fit the skeleton to it.
    #[tool(
        description = "Weight a prepared mesh to the profile's skeleton with SkinTokens, then \
                       fit that skeleton to THIS body — every bone keeps its name, its parent \
                       and its rest rotation, and takes its length from where the weights say \
                       the joint is. That is what lets a four-head character and a giant play \
                       the same clips with nothing rebaked. This returns a JOB; about a \
                       minute on a free card, because the door skins, fits, re-prepares and \
                       re-skins. The result lands as a working .blend under assets-src/ with \
                       its rig record, and the frame carries the fit table and the \
                       motion_scale a consumer applies to a root track. A run outside the \
                       symmetry or ratio bands is refused with the numbers it measured. \
                       Refuses with the install line when SkinTokens is not set up; the \
                       encoder carries an open licence question, which the record names. \
                       Next: export the body, then promote_body, which runs the export gate \
                       and rig check for you."
    )]
    pub(crate) async fn skin_body(
        &self,
        Parameters(args): Parameters<SkinBodyArgs>,
    ) -> CallToolResult {
        let project = &self.config.project;
        let glb = resolve_path(project, &args.glb);
        if !glb.is_file() {
            return util::refuse(format!(
                "no prepared mesh at {} — pass the path prepare_body reported, under \
                 out/prepare/. nothing was written.",
                glb.display()
            ));
        }
        if let Some(refusal) = self.backend_refusal("skintokens", "skin_body") {
            return refusal;
        }
        let name = match named(args.name.as_deref(), &glb) {
            Ok(name) => name,
            Err(refusal) => return util::refuse(refusal),
        };
        let dir = project.out.join("skin");
        if let Err(err) = std::fs::create_dir_all(&dir) {
            return util::refuse(format!("cannot create {}: {err}", dir.display()));
        }
        let out = dir.join(format!("{name}.skinned.glb"));
        let record = dir.join(format!("{name}.rig.json"));

        let mut argv = vec![
            String::from("skin"),
            glb.display().to_string(),
            String::from("--out"),
            out.display().to_string(),
            String::from("--record"),
            record.display().to_string(),
            String::from("--name"),
            name.clone(),
        ];
        if let Some(free) = args.min_free_gb {
            argv.push(String::from("--min-free-gb"));
            argv.push(free.to_string());
        }
        argv.push(String::from("--created-by"));
        argv.push(String::from(ACTOR));

        let then = format!(
            "when it is done, read the fit table and the motion_scale, export the body, and \
             promote_body {{\"glb\": \"out/export/{name}.glb\", \"name\": {name:?}, \
             \"rig_record\": {:?}}} ships it",
            record.display().to_string()
        );
        self.queue_gen("skin_body", argv, args.wait_s, &then).await
    }
}

/// A stated flag, or nothing at all — never an empty string, which a
/// generator would read as a value somebody meant.
fn push_stated(argv: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(value) = stated(value) {
        argv.push(String::from(flag));
        argv.push(value);
    }
}

/// The library name for a step: what the caller stated, else the input
/// file's own stem, held to the same rule a promote holds a name to — so a
/// name that cannot be a library asset is refused here rather than four
/// minutes of card later.
fn named(stated_name: Option<&str>, input: &std::path::Path) -> Result<String, String> {
    let raw = match stated(stated_name) {
        Some(stated) => stated,
        None => input
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .ok_or_else(|| {
                format!(
                    "{} has no file stem to name the output after; pass name",
                    input.display()
                )
            })?,
    };
    validate_name(&raw).map_err(|err| err.to_string())
}

impl ForgeServer {
    /// Submit one generator command line and answer with its job — the
    /// shape `generate_audio` established, in one place because three tools
    /// here answer identically and a fourth copy would be where they drift.
    async fn queue_gen(
        &self,
        tool: &str,
        argv: Vec<String>,
        wait_s: Option<f64>,
        then: &str,
    ) -> CallToolResult {
        let project = &self.config.project;
        let spec = forge_serve::JobSpec {
            backend: forge_serve::spec::backend_of(&argv).map(str::to_owned),
            ..forge_serve::spec::spec_for(&argv, &project.root, ACTOR)
        };
        let job = match self.queue.submit(spec) {
            Ok(job) => job,
            Err(err) => return self.queue_refusal(&err),
        };
        if job.state == forge_serve::JobState::Refused {
            // Admission refused it before it could cost a card minute — a
            // backend that is not installed, in the backend table's own
            // words, with the hint that fixes it.
            return util::refuse(format!(
                "{tool} refused: {}{}\n\nnothing was written.",
                job.message.as_deref().unwrap_or("(no message)"),
                job.hint
                    .as_deref()
                    .map_or_else(String::new, |hint| format!("\nhint: {hint}"))
            ));
        }
        if let Some(seconds) = wait_s
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
            frame
                .content
                .insert(0, Content::text(format!("{tool} is job {}", job.id)));
            return frame;
        }
        let position = self.position_of(&job.id);
        let mut frame = ForgeServer::job_frame(&job, position, None);
        frame["then"] = Value::from(then);
        CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&frame).unwrap_or_else(|_| frame.to_string()),
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{empty_project, server};
    use crate::util::first_line;
    use crate::util::frame_text;

    /// Every refusal is a successful frame that names what would have
    /// worked. A missing input names the door that makes one; a missing
    /// backend prints doctor's own line and points at doctor.
    #[tokio::test]
    async fn each_mesh_tool_refuses_by_naming_the_door_that_fixes_it() {
        let (dir, project) = empty_project();
        let server = server(project);

        let lift = server
            .generate_mesh(Parameters(GenerateMeshArgs {
                image: String::from("assets-src/refs/characters/nobody.png"),
                ..GenerateMeshArgs::default()
            }))
            .await;
        let text = frame_text(&lift);
        assert!(text.contains("import_reference"), "{text}");
        assert!(text.contains("nothing was written"), "{text}");

        let prepare = server
            .prepare_body(Parameters(PrepareBodyArgs {
                glb: String::from("out/lifts/nobody.glb"),
                ..PrepareBodyArgs::default()
            }))
            .await;
        let text = frame_text(&prepare);
        assert!(text.contains("generate_mesh reported"), "{text}");

        let skin = server
            .skin_body(Parameters(SkinBodyArgs {
                glb: String::from("out/prepare/nobody.glb"),
                ..SkinBodyArgs::default()
            }))
            .await;
        let text = frame_text(&skin);
        assert!(text.contains("prepare_body reported"), "{text}");

        // A file that does exist gets past the input check and lands on the
        // backend one, which is where a machine with no TRELLIS.2 stops —
        // with doctor named, not with a stack trace.
        let png = dir.path().join("ref.png");
        std::fs::write(&png, b"not really a png").expect("write");
        let lift = server
            .generate_mesh(Parameters(GenerateMeshArgs {
                image: png.display().to_string(),
                name: Some(String::from("hero")),
                ..GenerateMeshArgs::default()
            }))
            .await;
        let text = frame_text(&lift);
        assert!(text.contains("doctor"), "{text}");
        assert!(
            text.contains("generate_mesh is off") || text.contains("refused"),
            "{text}"
        );
        assert!(!first_line(&text, 200).is_empty());
    }

    /// A kind the door does not have is refused by name with the two it
    /// does, rather than being taken as the default.
    #[tokio::test]
    async fn a_mesh_kind_that_is_not_a_kind_is_refused_with_the_two_that_are() {
        let (dir, project) = empty_project();
        let png = dir.path().join("ref.png");
        std::fs::write(&png, b"not really a png").expect("write");
        let server = server(project);
        let refused = server
            .generate_mesh(Parameters(GenerateMeshArgs {
                image: png.display().to_string(),
                kind: Some(String::from("vehicle")),
                ..GenerateMeshArgs::default()
            }))
            .await;
        let text = frame_text(&refused);
        assert!(text.contains("character"), "{text}");
        assert!(text.contains("prop"), "{text}");
    }

    #[test]
    fn a_name_falls_back_to_the_input_stem_and_is_held_to_the_library_rule() {
        let stem = std::path::Path::new("/tmp/out/lifts/ember_knight.glb");
        assert_eq!(named(None, stem).expect("a stem"), "ember_knight");
        assert_eq!(named(Some("hero"), stem).expect("stated"), "hero");
        assert!(
            named(Some("../escape"), stem).is_err(),
            "a name that cannot be a path is refused here, not after the card"
        );
    }
}
