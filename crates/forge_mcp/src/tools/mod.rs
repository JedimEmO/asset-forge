//! The tools, one file per verb an agent is using.
//!
//! Each file exposes one `pub(crate) fn router() -> ToolRouter<ForgeServer>`
//! and [`router`] here sums them. The split is by what the agent is doing —
//! looking, rendering, inspecting, diagnosing, making, shipping — rather
//! than by asset type, because `render_model` and `render_clip_strip` share
//! the renderer and the refusal shapes, not the catalog kind.
//!
//! `jobs` is the queue an agent lives with — `wait`, `cancel`, `status`,
//! `list_runs` — and it is in its own file for the same reason: what an
//! agent is doing when it calls those is waiting, not generating.

use rmcp::handler::server::router::tool::ToolRouter;

use crate::server::ForgeServer;

pub(crate) mod audio;
mod bundle;
mod checks;
mod doctor;
mod generate;
mod jobs;
mod list;
mod mesh;
mod promote;
mod reference;
mod render;
mod setup;

/// Every tool the server offers.
pub(crate) fn router() -> ToolRouter<ForgeServer> {
    list::router()
        + mesh::router()
        + render::router()
        + audio::router()
        + doctor::router()
        + generate::router()
        + jobs::router()
        + promote::router()
        + reference::router()
        + bundle::router()
        + setup::router()
        + checks::router()
}
