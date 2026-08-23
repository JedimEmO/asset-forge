//! The tools, one file per verb an agent is using.
//!
//! Each file exposes one `pub(crate) fn router() -> ToolRouter<ForgeServer>`
//! and [`router`] here sums them. The split is by what the agent is doing —
//! looking, rendering, inspecting, diagnosing, making, shipping — rather
//! than by asset type, because `render_model` and `render_clip_strip` share
//! the renderer and the refusal shapes, not the catalog kind.
//!
//! `generate` and `promote` are the other half of the P4 work and land in
//! their own files; an empty router from either is a valid summand.

use rmcp::handler::server::router::tool::ToolRouter;

use crate::server::ForgeServer;

mod audio;
mod doctor;
mod generate;
mod list;
mod promote;
mod render;

/// Every tool the server offers.
pub(crate) fn router() -> ToolRouter<ForgeServer> {
    list::router()
        + render::router()
        + audio::router()
        + doctor::router()
        + generate::router()
        + promote::router()
}
