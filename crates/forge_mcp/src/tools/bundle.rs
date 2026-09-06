//! Handing an asset out: one body and its clips as a single `.glb`.
//!
//! The only tool here that writes a file nobody in this project will read
//! again. A bundle is an export — it is not filed, not named in the catalog
//! and not in the manifest — so unlike the promote doors it takes a path
//! rather than a name, and refuses nothing about collisions: the library's
//! copy of every input is still the truth, and a bundle is regenerated, never
//! repaired.
//!
//! The merge is [`forge_library::bundle`], the same function `forge bundle`
//! calls, so what an agent exports is byte-for-byte what a human at the
//! terminal would have exported with the same arguments. The frame it hands
//! back is the record that was written beside the file.

use std::path::PathBuf;

use forge_library::bundle::{BundleRequest, write};
use forge_library::schema::Actor;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::server::ForgeServer;
use crate::util;

/// Arguments for `export_bundle`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ExportBundleArgs {
    /// The body: a library body name as `list_models` prints it, or a path
    /// to any rigged glb — an export under out/ that is not in the library.
    pub(crate) body: String,
    /// The clips, in the order they should appear in the file: library clip
    /// names as `list_clips` prints them, or paths to clip glbs.
    pub(crate) clips: Vec<String>,
    /// Where to write it. A relative path is taken from the project root;
    /// out/bundles/<name>.glb is the convention.
    pub(crate) out: String,
    /// Multiply the root travel by this — the body's own root height against
    /// the rig profile's, so a body fitted to its own proportions travels
    /// its own stride. Omit it and the body's record is read, which is
    /// almost always what you want; rotations are never touched either way,
    /// and the bundle record names which number it used and where it came
    /// from.
    #[serde(default)]
    pub(crate) motion_scale: Option<f64>,
}

#[tool_router(router = bundle_tools, vis = "pub(crate)")]
impl ForgeServer {
    /// Merge a body and its clips into one file.
    #[tool(
        description = "Export one self-contained glb carrying a body's skin and any number of \
                       clips as named animations — what you hand to somebody outside this \
                       toolkit. It is a merge, not a bake: every clip's channels are \
                       re-pointed at the body's bones by name and their values copied \
                       unchanged, so a clip that drives a bone the body does not have is \
                       refused, naming the bone. Writes the glb and a <stem>.bundle.json \
                       record beside it, and returns that record: the inputs hashed, and the \
                       animation names an engine will bind by (a clip's name inside its own \
                       file, which need not be the library name you asked for). Nothing is \
                       filed in the library."
    )]
    async fn export_bundle(
        &self,
        Parameters(args): Parameters<ExportBundleArgs>,
    ) -> CallToolResult {
        let project = self.config.project.clone();
        let out = PathBuf::from(args.out.trim());
        let out = if out.is_absolute() {
            out
        } else {
            project.root.join(out)
        };
        let request = BundleRequest {
            body: args.body,
            clips: args.clips,
            out,
            motion_scale: args.motion_scale,
            created_by: Actor::Agent(String::from("mcp")),
        };
        // Reading a body, hashing it and writing a few megabytes is blocking
        // work; it is milliseconds, but it is not the runtime's.
        let bundled = tokio::task::spawn_blocking(move || {
            write(&project, &request).map_err(|e| e.to_string())
        })
        .await
        .unwrap_or_else(|err| Err(format!("the bundle task failed: {err}")));
        let bundled = match bundled {
            Ok(bundled) => bundled,
            // A refused name arrives here already carrying the list of what
            // the library does hold; a merge that refused names the bone.
            Err(message) => return util::refuse(message),
        };
        let record = match serde_json::to_string_pretty(&bundled.record) {
            Ok(record) => record,
            Err(err) => return util::refuse(format!("the record could not be rendered: {err}")),
        };
        util::report(format!(
            "wrote {} with {} animation(s): {}\nrecord: {}\n\n{record}",
            bundled.record.output.path,
            bundled.record.output.animations.len(),
            bundled.record.output.animations.join(", "),
            bundled
                .record_path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        ))
    }
}

/// The module's router, for `tools::router` to sum.
pub(crate) fn router() -> rmcp::handler::server::router::tool::ToolRouter<ForgeServer> {
    ForgeServer::bundle_tools()
}
