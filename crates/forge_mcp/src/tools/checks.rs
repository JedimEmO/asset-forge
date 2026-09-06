//! Read-only validation using the same executable and gates as the CLI.

use crate::server::ForgeServer;
use crate::util::{self, Ran};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

const CHECK_TIMEOUT: Duration = Duration::from_mins(5);

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuditArgs {
    /// Include measured body-fit checks, exactly as CLI `audit --fit`.
    #[serde(default)]
    fit: bool,
}

#[tool_router(router = check_tools, vis = "pub(crate)")]
impl ForgeServer {
    /// Check library integrity and provenance.
    #[tool(
        description = "Verify the project's accepted library, sidecar hashes, reference provenance and rig profile integrity. Read-only; runs the same checks as forge verify. Returns passed, exit_code and the full report. A failed check is an error result; repair through the upstream generation or promotion door, never edit a record to pass. Does not judge visual or audio quality."
    )]
    async fn verify(&self) -> CallToolResult {
        self.check("verify", &["verify"]).await
    }

    /// Check complete clip reproduction and body conformance.
    #[tool(
        description = "Audit clip reproduction by bytes and posed animation, and check body conformance, exactly as forge audit. Optional fit enables forge audit --fit. Read-only, no generator or GPU required; may take minutes. Returns passed, exit_code and the full report. A failed audit is an error result. Does not replace visual review."
    )]
    async fn audit(&self, Parameters(args): Parameters<AuditArgs>) -> CallToolResult {
        let args = if args.fit {
            vec!["audit", "--fit"]
        } else {
            vec!["audit"]
        };
        self.check("audit", &args).await
    }

    /// Compare the manifest with a fresh in-memory projection.
    #[tool(
        description = "Check that assets/library.json matches a fresh projection of the accepted library, exactly as forge manifest --check. Read-only: never rewrites the manifest or accepted assets. Returns passed, exit_code and the report; drift is an error result."
    )]
    async fn manifest_check(&self) -> CallToolResult {
        self.check("manifest_check", &["manifest", "--check"]).await
    }
}

impl ForgeServer {
    async fn check(&self, name: &str, args: &[&str]) -> CallToolResult {
        let mut cmd = self.forge_command();
        cmd.args(args);
        let (passed, exit_code, report, code) = match util::run(&mut cmd, CHECK_TIMEOUT).await {
            Ran::Ok(captured) => (true, captured.code, captured.stdout, "ok"),
            Ran::Failed(captured) => (
                false,
                captured.code,
                format!(
                    "{}\n{}",
                    captured.stdout.trim_end(),
                    captured.stderr.trim_end()
                ),
                "check_failed",
            ),
            Ran::TimedOut => (
                false,
                None,
                format!(
                    "{name} exceeded {} seconds; inspect library size and diagnostics before retrying",
                    CHECK_TIMEOUT.as_secs()
                ),
                "check_timeout",
            ),
            Ran::Unlaunchable(err) => (
                false,
                None,
                format!("could not launch {}: {err}", self.config.renderer.display()),
                "check_unavailable",
            ),
        };
        let mut result = if passed {
            util::report(&report)
        } else {
            util::refuse(&report)
        };
        result.structured_content = Some(json!({"check": name, "passed": passed,
            "exit_code": exit_code, "code": code, "report": report}));
        result
    }
}

pub(crate) fn router() -> rmcp::handler::server::router::tool::ToolRouter<ForgeServer> {
    ForgeServer::check_tools()
}
