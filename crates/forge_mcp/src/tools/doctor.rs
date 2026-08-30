//! `doctor`: what this machine can do, relayed from `forge doctor --json`.
//!
//! The binary already knows the answer — the project, the rig profile and
//! its drift, the library and whether its manifest is current, every
//! backend probed inside its own interpreter, the GPU and who holds it —
//! and prints it as one JSON object. This tool runs that, turns the object
//! into the lines an agent reads, and adds the three facts only the server
//! knows: which binary renders, which body clips are posed on, and what the
//! library holds right now.
//!
//! Exit 1 from the binary is a backend that is not ok, and the report is
//! still the answer — so it comes back as a successful frame with the
//! verdict in it, not as a refusal that hides the table.

use std::fmt::Write as _;

use forge_library::{Catalog, Kind};
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde_json::Value;

use crate::server::ForgeServer;
use crate::util::{self, DOCTOR_TIMEOUT, Ran};

#[tool_router(router = doctor_tools, vis = "pub(crate)")]
impl ForgeServer {
    /// What this machine can do.
    #[tool(
        description = "What this machine can do: the project and its rig profile, what the \
                       library holds and whether its manifest is current, the GPU and who \
                       holds it, and every generator backend probed in its own environment \
                       (installed, partial, missing — with the install hint). Call it first \
                       when unsure whether generate_* can run, and when a generate or a \
                       render times out. Takes 10-60s: each backend's probe imports torch."
    )]
    async fn doctor(&self) -> CallToolResult {
        let mut cmd = self.forge_command();
        cmd.arg("doctor").arg("--json");
        let ran = util::run(&mut cmd, DOCTOR_TIMEOUT).await;
        let own = self.own_lines();

        // Exit 1 is "something is not ok", said inside the JSON; the
        // object is still on the last line and is still the report.
        let captured = match ran {
            Ran::Ok(captured) => captured,
            Ran::Failed(captured) if captured.code == Some(1) => captured,
            other => match other.or_message("doctor", &self.config.renderer, DOCTOR_TIMEOUT) {
                Ok(captured) => captured,
                // The server's own facts are still worth having when the
                // probe could not run.
                Err(message) => return util::refuse(format!("{message}\n\n{own}")),
            },
        };

        let report = captured
            .last_stdout_line()
            .and_then(|line| serde_json::from_str::<Value>(line).ok());
        let Some(report) = report else {
            return util::refuse(format!(
                "forge doctor --json printed no JSON object on its last line.\n{}\n\n{}\n\n{own}",
                captured.stderr_tail(),
                captured.stdout.trim_end()
            ));
        };
        util::report(format!("{}\n{own}", render(&report)))
    }
}

impl ForgeServer {
    /// The facts only the server has: the renderer, the stage body, the
    /// library as scanned this instant.
    fn own_lines(&self) -> String {
        let catalog = Catalog::scan(&self.config.project);
        let counts: Vec<String> = Kind::ALL
            .iter()
            .map(|kind| {
                format!(
                    "{} {kind}",
                    catalog.records().iter().filter(|r| r.kind == *kind).count()
                )
            })
            .collect();
        let stage = match self.config.stage_body.as_deref() {
            Some(body) if catalog.resolve(body, Some(Kind::Body)).is_some() => {
                format!("{body} (forge.toml [studio] stage_body)")
            }
            Some(body) => format!(
                "{body} is set in forge.toml but is NOT in the library — renders fall back to the first body"
            ),
            None => catalog
                .records()
                .iter()
                .find(|r| r.kind == Kind::Body)
                .map_or_else(
                    || {
                        String::from(
                            "none — no body in the library; clips pose on the fixture mannequin",
                        )
                    },
                    |first| format!("unset — the first body, {}", first.name),
                ),
        };
        format!(
            "mcp       renderer {}\n          stage body: {stage}\n          library now: {}\n          toolkit: {}",
            self.config.renderer.display(),
            counts.join(", "),
            self.config.toolkit.as_deref().map_or_else(
                || String::from("not found — generate_* will refuse"),
                |t| t.display().to_string()
            )
        )
    }
}

/// The module's router, for `tools::router` to sum.
pub(crate) fn router() -> rmcp::handler::server::router::tool::ToolRouter<ForgeServer> {
    ForgeServer::doctor_tools()
}

/// The JSON report as the lines an agent reads: verdict first, then the
/// project, the rig, the library, the host, one row per backend with what
/// failed under it and the hint that fixes it.
pub(crate) fn render(report: &Value) -> String {
    let mut out = String::new();
    let ok = report.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let mut problems: Vec<&str> = Vec::new();
    for key in ["broken", "not_ok"] {
        problems.extend(strings(report.get(key)));
    }
    if ok {
        out.push_str("doctor: ok — every backend ");
        out.push_str(if report.get("gen").is_some_and(Value::is_object) {
            "probed ok\n"
        } else {
            "found (not probed)\n"
        });
    } else if problems.is_empty() {
        out.push_str("doctor: NOT OK\n");
    } else {
        let _ = writeln!(out, "doctor: NOT OK — {}", problems.join("; "));
    }

    if let Some(project) = report.get("project") {
        let _ = writeln!(
            out,
            "project   {} at {} (library {})",
            text(project, "name"),
            text(project, "root"),
            text(project, "library_version")
        );
    }
    if let Some(rig) = report.get("rig") {
        if let Some(error) = rig.get("error").and_then(Value::as_str) {
            let _ = writeln!(
                out,
                "rig       {}: does not load — {error}",
                text(rig, "name")
            );
        } else {
            let _ = writeln!(
                out,
                "rig       {} v{}: {} bones ({} driven), {} socket(s) — {}",
                text(rig, "name"),
                text(rig, "version"),
                text(rig, "bones"),
                text(rig, "driven"),
                text(rig, "sockets"),
                strings(rig.get("status")).join(", ")
            );
            for line in strings(rig.get("drift")) {
                let _ = writeln!(out, "          drift: {line}");
            }
        }
    }
    if let Some(library) = report.get("library") {
        let counts = library
            .get("counts")
            .and_then(Value::as_object)
            .map(|counts| {
                Kind::ALL
                    .iter()
                    .map(|kind| {
                        format!(
                            "{} {kind}",
                            counts
                                .get(kind.as_str())
                                .and_then(Value::as_u64)
                                .unwrap_or(0)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let _ = writeln!(out, "library   {counts}; {}", text(library, "manifest"));
    }

    match report.get("gen").filter(|g| g.is_object()) {
        Some(probe) => {
            host_lines(probe, &mut out);
            let _ = writeln!(
                out,
                "backends  {} ({})",
                text(probe, "backends_dir"),
                text(report, "backends_origin")
            );
            if let Some(error) = probe.get("error").and_then(Value::as_str) {
                let _ = writeln!(out, "          error: {error}");
            }
            if let Some(entries) = probe.get("backends").and_then(Value::as_object) {
                for (name, entry) in entries {
                    backend_lines(name, entry, &mut out);
                }
            }
        }
        None => {
            let _ = writeln!(
                out,
                "backends  not probed ({}){}",
                text(report, "backends_origin"),
                if report.get("quick").and_then(Value::as_bool) == Some(true) {
                    " — quick mode"
                } else {
                    " — no python/forge_gen was found, so generation is off"
                }
            );
        }
    }
    out
}

/// The host rows: GPU and who holds it, Blender, ffmpeg, python.
fn host_lines(probe: &Value, out: &mut String) {
    if let Some(gpu) = probe.get("host").and_then(|h| h.get("gpu")) {
        if gpu.get("ok").and_then(Value::as_bool) == Some(true) {
            let _ = writeln!(
                out,
                "gpu       {}  {} / {} MiB in use",
                text(gpu, "name"),
                text(gpu, "used_mb"),
                text(gpu, "total_mb")
            );
            if let Some(warn) = gpu.get("warn").and_then(Value::as_str) {
                let _ = writeln!(out, "          warn: {warn}");
            }
        } else {
            let _ = writeln!(out, "gpu       {}", text(gpu, "error"));
        }
    }
    if let Some(blender) = probe.get("blender").filter(|b| b.is_object()) {
        let ok = blender.get("ok").and_then(Value::as_bool) == Some(true);
        let _ = writeln!(
            out,
            "blender   {} {}",
            text(blender, "version"),
            if ok {
                String::from("ok")
            } else {
                format!("{} — {}", text(blender, "error"), text(blender, "hint"))
            }
        );
    }
    if let Some(ffmpeg) = probe.get("ffmpeg").filter(|f| f.is_object()) {
        let _ = writeln!(
            out,
            "ffmpeg    {}",
            ffmpeg
                .get("version")
                .or_else(|| ffmpeg.get("error"))
                .and_then(Value::as_str)
                .unwrap_or("?")
        );
    }
}

/// One backend's rows: the status, then what failed, then notices (a
/// licence fact is not a detail) and hints (the next command to type).
fn backend_lines(name: &str, entry: &Value, out: &mut String) {
    let status = text(entry, "status");
    if status == "off" {
        // Not a probe result and not a problem: the project's [make] did
        // not choose the kind, so nothing was run and nothing is wrong.
        let _ = writeln!(
            out,
            "  {name:<12} off — {}",
            entry
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("not chosen")
        );
        return;
    }
    let executor = entry
        .get("executor")
        .and_then(Value::as_str)
        .map(|word| format!(" [{word}]"))
        .unwrap_or_default();
    let _ = writeln!(out, "  {name:<12} {status}{executor}");
    for check in entry
        .get("checks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if check.get("ok").and_then(Value::as_bool) != Some(true) {
            let _ = writeln!(
                out,
                "               FAIL {}: {}",
                text(check, "name"),
                text(check, "detail")
            );
        }
    }
    for notice in strings(entry.get("notices")) {
        let _ = writeln!(out, "               notice: {notice}");
    }
    for hint in strings(entry.get("hints")) {
        let _ = writeln!(out, "               hint: {hint}");
    }
}

/// A field as text, `?` when absent — a report line with a hole reads
/// better than a report with a line missing.
fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        None | Some(Value::Null) => String::from("?"),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// An array of strings, or nothing.
fn strings(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;
    use crate::util::frame_text;
    use serde_json::json;

    #[test]
    fn a_report_renders_verdict_rows_and_hints() {
        let report = json!({
            "ok": false,
            "project": {"name": "forge", "root": "/p", "library_version": "0.1.0"},
            "rig": {"name": "humanoid", "version": 1, "bones": 55, "driven": 27, "sockets": 6,
                    "status": ["glb sha ok", "no drift"], "drift": []},
            "library": {"counts": {"clip": 4, "body": 1, "model": 2, "sfx": 0, "music": 0, "voice": 0},
                        "manifest": "manifest current"},
            "backends_origin": "the toolkit root",
            "toolkit": "/p",
            "gen": {
                "host": {"gpu": {"ok": true, "name": "RTX 4090", "used_mb": 512, "total_mb": 24564}},
                "blender": {"ok": false, "error": "not found", "hint": "install Blender 5.2"},
                "backends_dir": "/p/backends",
                "backends": {
                    "ardy": {"status": "ok", "checks": [{"name": "probe", "ok": true, "detail": "torch"}]},
                    "trellis2": {"status": "missing",
                                 "checks": [{"name": "env", "ok": false, "detail": "no .env"}],
                                 "notices": ["nvdiffrast is non-commercial"],
                                 "hints": ["just setup trellis2"]}
                }
            },
            "quick": false,
            "broken": [],
            "not_ok": ["trellis2 missing"]
        });
        let text = render(&report);
        assert!(
            text.starts_with("doctor: NOT OK — trellis2 missing"),
            "{text}"
        );
        assert!(
            text.contains("project   forge at /p (library 0.1.0)"),
            "{text}"
        );
        assert!(
            text.contains(
                "rig       humanoid v1: 55 bones (27 driven), 6 socket(s) — glb sha ok, no drift"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "library   4 clip, 1 body, 2 model, 0 sfx, 0 music, 0 voice; manifest current"
            ),
            "{text}"
        );
        assert!(
            text.contains("gpu       RTX 4090  512 / 24564 MiB in use"),
            "{text}"
        );
        assert!(
            text.contains("blender   ? not found — install Blender 5.2"),
            "{text}"
        );
        assert!(text.contains("  ardy         ok"), "{text}");
        assert!(text.contains("  trellis2     missing"), "{text}");
        assert!(text.contains("FAIL env: no .env"), "{text}");
        assert!(
            text.contains("notice: nvdiffrast is non-commercial"),
            "{text}"
        );
        assert!(text.contains("hint: just setup trellis2"), "{text}");
        assert!(
            !text.contains("FAIL probe"),
            "passing checks are not rows: {text}"
        );
    }

    #[test]
    fn an_unprobed_report_says_why() {
        let report = json!({"ok": true, "backends_origin": "nowhere", "quick": true});
        let text = render(&report);
        assert!(
            text.starts_with("doctor: ok — every backend found (not probed)"),
            "{text}"
        );
        assert!(text.contains("quick mode"), "{text}");
        let report = json!({"ok": false, "rig": {"name": "humanoid", "error": "no contract.json"}});
        let text = render(&report);
        assert!(
            text.contains("rig       humanoid: does not load — no contract.json"),
            "{text}"
        );
        assert!(text.contains("generation is off"), "{text}");
    }

    #[tokio::test]
    async fn a_missing_binary_still_reports_what_the_server_knows() {
        let (_dir, project) = testing::library();
        let server = testing::server(project);
        let result = server.doctor().await;
        assert_eq!(result.is_error, Some(true));
        let text = frame_text(&result);
        assert!(text.contains("could not run"), "{text}");
        assert!(text.contains("forge-for-tests"), "{text}");
        assert!(
            text.contains("stage body: unset — the first body, mannequin"),
            "{text}"
        );
        assert!(text.contains("1 clip, 1 body"), "{text}");
    }
}
