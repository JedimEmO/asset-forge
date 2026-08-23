//! The server state, and the one router every tool file feeds.
//!
//! The tools live in `tools/`, one file per verb, each declaring its own
//! [`ToolRouter`] over this same type. `ToolRouter` implements `Add`, so
//! the whole surface is the sum of those — which is what lets a tool move
//! between files without anything here changing, and what keeps a
//! 900-line `impl` from reappearing by accretion.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, tool_handler};

use crate::config::Config;
use crate::tools;

/// The MCP server: the configuration, and every tool.
#[derive(Clone)]
pub(crate) struct ForgeServer {
    /// Where the project is and what renders.
    pub(crate) config: Config,
    /// Every tool, summed from the per-file routers.
    tool_router: ToolRouter<Self>,
}

impl ForgeServer {
    /// Assemble the server.
    pub(crate) fn new(config: Config) -> Self {
        Self {
            config,
            tool_router: tools::router(),
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
    /// grouped the way the work goes — look, make, ship — and the two rules
    /// that are not obvious from a tool's own description are said here:
    /// promote writes the library *now*, and a mesh has no promote at all.
    pub(crate) fn instructions(&self) -> String {
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
             MAKING — writes only under out/, never the library: generate_clips draws motion \
             takes from a prompt and hands back a review sheet of every take; generate_audio \
             draws a sound, a track or a spoken line. {generation}\n\
             \n\
             SHIPPING — direct writes: promote_clip bakes one take with a recipe you state in \
             full into the library; promote_audio copies the sound you auditioned. Both REFUSE \
             a name that is already taken unless you pass overwrite, and then echo the record \
             they replaced. There is no promote for a body or a model: those go through the \
             forge-character and forge-prop skills with a human looking at every step.\n\
             \n\
             When unsure what this machine can run, call doctor first. Refusals come back as \
             error results that name what would have worked; read them and correct the call \
             rather than retrying it.",
            root = self.config.project.root.display(),
            assets = self.config.project.assets.display(),
        )
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ForgeServer {
    fn get_info(&self) -> ServerInfo {
        // ServerInfo is #[non_exhaustive], so build from the default rather
        // than a struct literal.
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        // Without this the handshake reports the SDK's crate name, not ours.
        env!("CARGO_PKG_NAME").clone_into(&mut info.server_info.name);
        env!("CARGO_PKG_VERSION").clone_into(&mut info.server_info.version);
        info.instructions = Some(self.instructions());
        info
    }
}
