//! The server state, and the one router every tool file feeds.
//!
//! The tools live in `tools/`, one file per verb, each declaring its own
//! [`ToolRouter`] over this same type. `ToolRouter` implements `Add`, so
//! the whole surface is the sum of those — which is what lets a tool move
//! between files without anything here changing, and what keeps a
//! 900-line `impl` from reappearing by accretion.

use std::sync::Arc;

use forge_serve::Queue;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{AnnotateAble, ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, tool_handler};

use crate::config::Config;
use crate::tools;

type ActivatedProject = (Config, Arc<dyn Queue>);

const GUIDE_URI: &str = "forge://guides/v1/workflow";

/// The MCP server: the configuration, the queue, and every tool.
#[derive(Clone)]
pub(crate) struct ForgeServer {
    /// Where the project is and what renders.
    pub(crate) config: Config,
    /// The queue every generate goes through — a daemon's if one is up,
    /// else this process's own. The server never learns which, which is
    /// what makes a stranger's first session and a busy machine's tenth
    /// the same code path.
    pub(crate) queue: Arc<dyn Queue>,
    /// Every tool, summed from the per-file routers.
    tool_router: ToolRouter<Self>,
    /// A project created after startup activates once, including its queue.
    activation: Arc<tokio::sync::Mutex<Option<ActivatedProject>>>,
}

impl ForgeServer {
    /// Assemble the server over a queue.
    pub(crate) fn new(config: Config, queue: Arc<dyn Queue>) -> Self {
        Self {
            config,
            queue,
            tool_router: tools::router(),
            activation: Arc::default(),
        }
    }

    /// The names of every tool the server offers, sorted — what the
    /// handshake advertises, for the test that pins the surface.
    #[cfg(test)]
    pub(crate) fn tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect();
        names.sort();
        names
    }

    /// The instructions the client shows its model at the start of every
    /// session: what the library is, and which tool does what.
    ///
    /// Written for an agent that has never seen this server. The verbs are
    /// grouped the way the work goes — look, make, wait, ship — and the two
    /// rules that are not obvious from any one tool's description are said
    /// here: a promote writes the library *now*, and looking is not one of
    /// the gates, which is why it has to be said out loud.
    pub(crate) fn instructions(&self) -> String {
        if !self.config.project_found {
            return format!(
                "There is no forge project at {root} yet — no forge.toml — so this server \
                 offers three tools and refuses the rest by name.\n\
                 \n\
                 - init_project {{\"path\": \"{root}\", \"make\": {{…}}, \"tier\": \"full|lean|fake\"}} \
                 writes forge.toml and the asset directories. It is the first call, and after \
                 it this session continues immediately when the path is {root}. A different path \
                 needs its own server with --project <path>.\n\
                 - licences {{}} returns every licence a kind carries, in full. The ids are \
                 the toolkit's, not a project's, so they answer before there is one.\n\
                 - doctor {{}} says what this machine can run at all.\n\
                 \n\
                 Everything else — the lists, the renders, the generators, the promotes — \
                 works on a library, and there is none here yet.",
                root = self.config.project.root.display(),
            );
        }
        let stage = self.config.stage_body.as_deref().map_or_else(
            || {
                String::from(
                    "the library's first body (set [studio] stage_body in forge.toml to choose)",
                )
            },
            |body| format!("bodies/{body}.glb"),
        );
        let generation = if self.config.toolkit.is_some() {
            "generate_* run the local backends through this toolkit; doctor says which are installed."
        } else {
            "no toolkit checkout was found from this executable, so generate_* will refuse; doctor says how to point at one."
        };
        format!(
            "Make, judge and ship game assets in the forge project at {root}. Everything \
             promoted lands in {assets} with a sidecar record beside it and a manifest a \
             game reads; nothing here has a review queue — a promote is a direct write.\n\
             \n\
             LOOK BEFORE YOU PROMOTE. Numbers say an asset is wired; a picture says what it \
             is, and a human's eye in the studio outranks both.\n\
             \n\
             LOOKING — no GPU for the lists, a headless render for the rest:\n\
             - list_models / list_clips / list_audio: what the library holds, with each \
             record's prompt, tags, provenance and recipe. Call these first: every other tool \
             takes the names they print, and a wrong name costs a turn.\n\
             - render_model: a body or model from the angles a reviewer walks to (front, back, \
             both sides, three head close-ups), as one inline contact sheet. A path under out/ \
             renders with culling off, so a raw lift's missing back shows as the inside of its \
             front.\n\
             - render_clip_strip: a clip posed on {stage} as a labelled grid of poses — judge \
             silhouette, foot contact, limbs through the body. A report line saying 0 bones \
             driven means the clip does not match this skeleton at all, and an identical grid \
             means it never moves.\n\
             - inspect_audio: you cannot hear a file, but the plot shows clipping as \
             flat-topping, dead air as a gap and a truncated tail as a cliff; the numbers \
             carry the rest.\n\
             \n\
             MAKING — writes only under out/ and assets-src/, never the library: \
             import_reference brings a PNG you drew into assets-src/refs/, checked and \
             recorded (no image model runs here); generate_mesh lifts that PNG; prepare_prop \
             normalises a prop lift to metres and its resting origin; prepare_body \
             normalises a character lift and puts a skeleton in it; skin_body weights it and fits \
             that skeleton to this body's own proportions, which is what lets a short \
             character and a giant play the same clips; generate_clips draws motion takes \
             from a prompt and hands back a review sheet of every take; generate_audio starts \
             a sound, a track or a spoken line and hands back a JOB. {generation}\n\
             \n\
             WAITING — a generate takes minutes and one card is shared by every door, so a \
             generate is queued: generate_audio returns a job id and the literal wait call to \
             make next; wait returns the finished job (and, for a sound, its measurements and \
             plot) or a successful \"still running\" frame you call again; cancel stops one and \
             gives the card back; status says who holds the card, what is queued and what is \
             running; list_runs is everything the generators have left under out/, with \
             whether each has been promoted.\n\
             \n\
             SHIPPING — direct writes: promote_clip bakes one take with a recipe you state in \
             full into the library; promote_audio copies the sound you auditioned; \
             promote_body files a rigged body behind the export gate and rig check; \
             promote_model files a normalised prop. Every one REFUSES a name that is already \
             taken unless you pass overwrite, and then echoes the record it replaced. Nothing \
             here replaces the eye: render_model and render_clip_strip exist beside these \
             doors, not instead of them.\n\
             \n\
             VALIDATING — verify checks integrity and provenance; audit checks full clip \
             reproduction and body conformance; manifest_check detects stale projections. \
             All three are read-only and return structured passed/exit_code/report fields.\n\
             When unsure what this machine can run, call doctor first. Refusals come back as \
             error results that name what would have worked; read them and correct the call \
             rather than retrying it.",
            root = self.config.project.root.display(),
            assets = self.config.project.assets.display(),
        )
    }
}

/// The three tools a session with no project may call.
///
/// `init_project` makes one, `licences` needs none — the ids and their
/// texts are the toolkit's, not a project's — and `doctor` answers "what
/// can this machine run", which is a fair question to ask before choosing a
/// directory. Everything else works on a library that does not exist yet.
const WITHOUT_A_PROJECT: [&str; 3] = ["init_project", "licences", "doctor"];

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ForgeServer {
    /// Every tool call, with the no-project gate in front of it.
    ///
    /// The macro writes this one for us when we do not; we do, because a
    /// server started where there is no `forge.toml` has to answer
    /// something better than a protocol error for the twenty-two tools that
    /// need a library. A refusal is a successful frame naming the tool that
    /// fixes it.
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
        // Serialize bootstrap calls so generation cannot see a half-written init.
        // The bound root never changes, and activation creates only one queue.
        let mut active = self.clone();
        let mut activation = if self.config.project_found {
            None
        } else {
            Some(self.activation.lock().await)
        };
        if let Some(state) = activation.as_mut() {
            if state.is_none() && self.config.project.root.join("forge.toml").is_file() {
                let project = match forge_library::Project::load(&self.config.project.root) {
                    Ok(project) => project,
                    Err(err) => {
                        return Ok(crate::util::refuse(format!(
                            "cannot activate this project: {err}"
                        )));
                    }
                };
                let queue = match forge_serve::discovery::queue_with(
                    &project,
                    forge_serve::LocalQueueOptions::for_project(
                        &project,
                        self.config.renderer.clone(),
                    ),
                ) {
                    Ok(queue) => queue,
                    Err(err) => {
                        return Ok(crate::util::refuse(format!(
                            "cannot activate this project's queue: {err}"
                        )));
                    }
                };
                let config = Config::with_renderer(project, self.config.renderer.clone());
                **state = Some((config, queue));
            }
            if let Some((config, queue)) = state.as_ref() {
                active.config = config.clone();
                active.queue = queue.clone();
            }
        }
        if active.config.project_found {
            drop(activation);
            activation = None;
        }
        if !active.config.project_found && !WITHOUT_A_PROJECT.contains(&request.name.as_ref()) {
            return Ok(crate::util::refuse(format!(
                "{} needs a project and there is no forge.toml at {}.\ncall init_project \
                 {{\"path\": \"{}\", \"make\": {{\"props\": true}}, \"tier\": \"full\"}} to make one \
                 here — licences and doctor also work without one, and every other tool \
                 refuses like this until there is a project. nothing was written.",
                request.name,
                self.config.project.root.display(),
                self.config.project.root.display(),
            )));
        }
        let tcc = rmcp::handler::server::tool::ToolCallContext::new(&active, request, context);
        let result = active.tool_router.call(tcc).await;
        drop(activation);
        result
    }

    async fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListResourcesResult, rmcp::ErrorData> {
        let resources = vec![
            rmcp::model::RawResource::new(GUIDE_URI, "Asset Forge workflow v1")
                .with_description(
                    "Local project setup, asset workflows, review, recovery and validation",
                )
                .with_mime_type("text/markdown")
                .no_annotation(),
        ];
        Ok(rmcp::model::ListResourcesResult::with_all_items(resources))
    }

    async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ReadResourceResult, rmcp::ErrorData> {
        if request.uri != GUIDE_URI {
            return Err(rmcp::ErrorData::resource_not_found(
                format!("unknown resource; available: {GUIDE_URI}"),
                None,
            ));
        }
        Ok(rmcp::model::ReadResourceResult::new(vec![
            rmcp::model::ResourceContents::text(crate::workflow_guide(), GUIDE_URI)
                .with_mime_type("text/markdown"),
        ]))
    }

    fn get_info(&self) -> ServerInfo {
        // ServerInfo is #[non_exhaustive], so build from the default rather
        // than a struct literal.
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .build();
        // Without this the handshake reports the SDK's crate name, not ours.
        env!("CARGO_PKG_NAME").clone_into(&mut info.server_info.name);
        env!("CARGO_PKG_VERSION").clone_into(&mut info.server_info.version);
        info.instructions = Some(format!(
            "{}\nRead the workflow guide through resources/read at {GUIDE_URI} (also discoverable with resources/list).",
            self.instructions()
        ));
        info
    }
}
