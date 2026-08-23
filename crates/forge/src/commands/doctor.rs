//! `forge doctor`: what this machine can do, in one table.
//!
//! Two halves, one report. This side knows the project: the root, the rig
//! profile and whether it has drifted from its artifacts, what the library
//! holds and whether the manifest is current, where the backends directory
//! is. The Python side knows the environments: `forge gen doctor --json`
//! runs each backend's probe inside its own interpreter — imports, torch,
//! CUDA, weights on disk, licence notices — and reports the host tools (the
//! GPU and who holds it, Blender, ffmpeg). The table here is both, merged:
//! one row per backend with its status, then its failures, notices and
//! hints, because a licence fact is not a detail and an install hint is the
//! next command to type.
//!
//! The exit code is 1 when any backend is not `ok`, or when the profile is
//! broken. A `partial` backend — the env runs, a weight is not cached — is
//! not ok: the first `forge gen` through it would download for minutes, and
//! doctor's job is to say so first. `--quick` skips the probes and reports
//! only what the directory says (found / missing / broken), in well under a
//! second.

use forge_library::backends::{BackendState, Backends, KNOWN};
use forge_library::{Catalog, Kind, Project, manifest};
use serde_json::{Value, json};

use crate::cli::DoctorArgs;
use crate::commands::generate;
use crate::outcome::{Failure, Outcome};

/// Print the table (or the object) and carry the verdict out.
pub(crate) fn run(project: &Project, args: &DoctorArgs) -> Outcome {
    let mut broken: Vec<String> = Vec::new();
    let mut lines: Vec<String> = Vec::new();

    // -- project ----------------------------------------------------------
    lines.push(format!(
        "project   {} at {} ({})",
        project.name,
        project.root.display(),
        forge_library::project::PROJECT_FILE
    ));
    lines.push(format!(
        "          assets {}  sources {}  out {}  library {}",
        rel(project, &project.assets),
        rel(project, &project.sources),
        rel(project, &project.out),
        project.library_version
    ));
    let project_json = json!({
        "name": project.name,
        "root": project.root,
        "assets": project.assets,
        "sources": project.sources,
        "out": project.out,
        "library_version": project.library_version,
    });

    // -- rig --------------------------------------------------------------
    let rig_json = rig_section(project, &mut lines, &mut broken);

    // -- library ----------------------------------------------------------
    let catalog = Catalog::scan(project);
    let mut counts = serde_json::Map::new();
    let count_text: Vec<String> = Kind::ALL
        .iter()
        .map(|kind| {
            let n = catalog.records().iter().filter(|r| r.kind == *kind).count();
            counts.insert(kind.to_string(), json!(n));
            format!("{n} {kind}")
        })
        .collect();
    let manifest_state = if project.manifest_path().is_file() {
        if manifest::check(project).ok() {
            "manifest current"
        } else {
            "manifest STALE — run `forge manifest`"
        }
    } else {
        "no manifest — run `forge manifest`"
    };
    lines.push(format!(
        "library   {}; {manifest_state}",
        count_text.join(", ")
    ));
    let library_json = json!({ "counts": counts, "manifest": manifest_state });

    // -- backends ---------------------------------------------------------
    let backends = Backends::discover(project);
    let toolkit = Backends::toolkit_root(project);
    let gen_report: Option<Value> = if args.quick || toolkit.is_none() {
        None
    } else {
        match generate::spawn(project, &["doctor"], false) {
            Ok(result) => result.payload,
            Err(failure) => {
                lines.push(format!(
                    "          forge gen doctor did not run: {}",
                    failure.message()
                ));
                None
            }
        }
    };

    let mut not_ok: Vec<String> = Vec::new();
    if let Some(report) = &gen_report {
        probed_lines(report, &backends.origin, &mut lines, &mut not_ok);
    } else {
        unprobed_lines(&backends, args.quick, &mut lines, &mut not_ok);
    }
    if let Some(root) = &toolkit {
        lines.push(format!("toolkit   {} (python/forge_gen)", root.display()));
    } else {
        lines.push(format!(
            "toolkit   not found from this executable — generation is off; set {} to the checkout",
            forge_library::backends::HOME_ENV
        ));
        not_ok.push(String::from("toolkit missing"));
    }

    let ok = broken.is_empty() && not_ok.is_empty();
    let verdict = if ok && gen_report.is_none() {
        String::from("doctor: every backend found (not probed)")
    } else if ok {
        String::from("doctor: every backend ok")
    } else {
        let mut parts = broken.clone();
        parts.extend(not_ok.iter().cloned());
        format!("doctor: {}", parts.join("; "))
    };

    if args.json {
        let object = json!({
            "ok": ok,
            "project": project_json,
            "rig": rig_json,
            "library": library_json,
            "backends_origin": backends.origin,
            "toolkit": toolkit,
            "gen": gen_report,
            "quick": args.quick,
            "broken": broken,
            "not_ok": not_ok,
        });
        println!("{object}");
    } else {
        for line in &lines {
            println!("{line}");
        }
        println!("{verdict} (exit {})", u8::from(!ok));
    }
    if ok {
        Ok(())
    } else {
        Err(Failure::failed(verdict))
    }
}

/// [`KNOWN`] as sort keys that order before any other name.
const KNOWN_ORDER: [&str; 5] = ["0", "1", "2", "3", "4"];

/// The backend rows from the Python report: host first, then one row per
/// backend with what failed under it.
fn probed_lines(report: &Value, origin: &str, lines: &mut Vec<String>, not_ok: &mut Vec<String>) {
    host_lines(report, lines);
    lines.push(format!(
        "backends  {} ({origin})",
        report
            .get("backends_dir")
            .and_then(Value::as_str)
            .unwrap_or("?")
    ));
    if let Some(error) = report.get("error").and_then(Value::as_str) {
        lines.push(format!("          error: {error}"));
        not_ok.push(String::from("backends"));
    }
    if let Some(entries) = report.get("backends").and_then(Value::as_object) {
        // The object's keys come back sorted; the rows go in doctor's order —
        // the known five, then anything else the directory described.
        let mut names: Vec<&String> = entries.keys().collect();
        names.sort_by_key(|name| {
            KNOWN
                .iter()
                .position(|k| k == name)
                .map_or((1, name.as_str()), |i| (0, KNOWN_ORDER[i]))
        });
        for name in names {
            let entry = &entries[name];
            backend_lines(name, entry, lines);
            let status = entry.get("status").and_then(Value::as_str).unwrap_or("?");
            if status != "ok" {
                not_ok.push(format!("{name} {status}"));
            }
        }
    }
}

/// The backend rows when no probe ran: what the directory says, and why
/// that is all.
fn unprobed_lines(
    backends: &Backends,
    quick: bool,
    lines: &mut Vec<String>,
    not_ok: &mut Vec<String>,
) {
    lines.push(format!(
        "backends  {} ({})",
        backends
            .dir
            .as_ref()
            .map_or_else(|| String::from("none"), |d| d.display().to_string()),
        backends.origin
    ));
    for backend in &backends.backends {
        lines.push(format!(
            "  {:<10} {:<8} {}",
            backend.name,
            backend.state.as_str(),
            match &backend.state {
                BackendState::Broken(why) => why.clone(),
                _ => backend.dir.display().to_string(),
            }
        ));
        if backend.state != BackendState::Found {
            not_ok.push(format!("{} {}", backend.name, backend.state.as_str()));
        }
    }
    if quick {
        lines.push(String::from(
            "          --quick: found is not checked; drop the flag to run every probe",
        ));
    } else {
        lines.push(String::from(
            "          the probes did not run: no python/forge_gen was found — generation is off",
        ));
    }
}

/// The profile row: bones, sockets, the source hashes, the drift.
fn rig_section(project: &Project, lines: &mut Vec<String>, broken: &mut Vec<String>) -> Value {
    match project.profile() {
        Ok(profile) => {
            let contract = &profile.contract;
            let mut status = Vec::new();
            match profile.check_sources() {
                Ok(sources) => {
                    if sources.glb_matches {
                        status.push(String::from("glb sha ok"));
                    } else {
                        status.push(String::from("glb sha MISMATCH"));
                        broken.push(String::from(
                            "rig.glb is not the file the contract was derived from",
                        ));
                    }
                    match sources.blend_matches {
                        Some(true) => status.push(String::from("blend sha ok")),
                        Some(false) => status.push(String::from("blend sha differs (warning)")),
                        None => status.push(String::from("no blend")),
                    }
                }
                Err(err) => {
                    status.push(format!("sources unreadable: {err}"));
                    broken.push(String::from("the profile's rig.glb cannot be read"));
                }
            }
            let mut drift_lines = Vec::new();
            match std::fs::read(profile.glb_path())
                .map_err(|e| e.to_string())
                .and_then(|bytes| forge_rig::derive_from_glb(&bytes).map_err(|e| e.to_string()))
            {
                Ok(derived) => {
                    let drift = forge_rig::check_drift(contract, &derived.bones);
                    if drift.is_empty() {
                        status.push(String::from("no drift"));
                    } else {
                        status.push(format!("DRIFT in {} bone(s)", drift.len()));
                        drift_lines.clone_from(&drift);
                        broken.push(String::from("the contract no longer derives from rig.glb"));
                    }
                }
                Err(err) => {
                    status.push(format!("cannot derive: {err}"));
                    broken.push(String::from("the profile's rig.glb does not derive"));
                }
            }
            lines.push(format!(
                "rig       {} v{}: {} bones ({} driven by {}), {} socket(s), {} — {}",
                contract.name,
                contract.version,
                contract.bones.len(),
                contract.driven().count(),
                contract.driven_layout,
                profile.sockets.sockets.len(),
                rel(project, &profile.dir),
                status.join(", ")
            ));
            for line in &drift_lines {
                lines.push(format!("          drift: {line}"));
            }
            json!({
                "name": contract.name,
                "version": contract.version,
                "bones": contract.bones.len(),
                "driven": contract.driven().count(),
                "sockets": profile.sockets.sockets.len(),
                "dir": profile.dir,
                "status": status,
                "drift": drift_lines,
            })
        }
        Err(err) => {
            lines.push(format!(
                "rig       {} at {}: does not load — {err}",
                project.rig_name,
                rel(project, &project.rig_dir())
            ));
            broken.push(String::from("the rig profile does not load"));
            json!({ "name": project.rig_name, "error": err.to_string() })
        }
    }
}

/// The host rows from the Python report: GPU, Blender, ffmpeg, python3.
fn host_lines(report: &Value, lines: &mut Vec<String>) {
    let host = report.get("host").cloned().unwrap_or(Value::Null);
    if let Some(gpu) = host.get("gpu") {
        if gpu.get("ok").and_then(Value::as_bool) == Some(true) {
            lines.push(format!(
                "gpu       {}  {} / {} MiB in use",
                gpu.get("name").and_then(Value::as_str).unwrap_or("?"),
                gpu.get("used_mb").and_then(Value::as_u64).unwrap_or(0),
                gpu.get("total_mb").and_then(Value::as_u64).unwrap_or(0),
            ));
            if let Some(warn) = gpu.get("warn").and_then(Value::as_str) {
                lines.push(format!("          warn: {warn}"));
            }
        } else {
            lines.push(format!(
                "gpu       {}",
                gpu.get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("not reported")
            ));
        }
    }
    if let Some(blender) = report.get("blender").filter(|b| b.is_object()) {
        let ok = blender.get("ok").and_then(Value::as_bool) == Some(true);
        lines.push(format!(
            "blender   {} {}  {}",
            blender
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("?"),
            blender.get("bin").and_then(Value::as_str).unwrap_or(""),
            if ok {
                "ok"
            } else {
                blender
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("not found")
            }
        ));
        if !ok && let Some(hint) = blender.get("hint").and_then(Value::as_str) {
            lines.push(format!("          hint: {hint}"));
        }
    }
    if let Some(ffmpeg) = report.get("ffmpeg").filter(|f| f.is_object()) {
        lines.push(format!(
            "ffmpeg    {}",
            ffmpeg
                .get("version")
                .or_else(|| ffmpeg.get("error"))
                .and_then(Value::as_str)
                .unwrap_or("?")
        ));
    }
    if let Some(python) = host.get("python3") {
        lines.push(format!(
            "python3   {} {}",
            python.get("version").and_then(Value::as_str).unwrap_or("?"),
            python.get("bin").and_then(Value::as_str).unwrap_or("")
        ));
    }
}

/// One backend's rows: the status line, then what failed, then the
/// notices and hints. Checks that passed are in `--json` and not here.
fn backend_lines(name: &str, entry: &Value, lines: &mut Vec<String>) {
    let status = entry.get("status").and_then(Value::as_str).unwrap_or("?");
    let checks: Vec<&Value> = entry
        .get("checks")
        .and_then(Value::as_array)
        .map(|c| c.iter().collect())
        .unwrap_or_default();
    let detail_of = |check_name: &str| -> Option<&str> {
        checks
            .iter()
            .find(|c| c.get("name").and_then(Value::as_str) == Some(check_name))
            .and_then(|c| c.get("detail"))
            .and_then(Value::as_str)
    };
    let summary = detail_of("probe")
        .filter(|d| !d.starts_with("no probe"))
        .or_else(|| detail_of("binary"))
        .or_else(|| detail_of("python"))
        .or_else(|| detail_of("toml"))
        .unwrap_or("");
    lines.push(format!("  {name:<10} {status:<8} {summary}"));
    for check in &checks {
        let ok = check.get("ok").and_then(Value::as_bool).unwrap_or(false);
        let detail = check.get("detail").and_then(Value::as_str).unwrap_or("");
        let check_name = check.get("name").and_then(Value::as_str).unwrap_or("?");
        if !ok {
            lines.push(format!("             FAIL {check_name}: {detail}"));
        } else if detail.starts_with("warn:") || detail.contains("; dirty") {
            lines.push(format!("             warn {check_name}: {detail}"));
        }
    }
    for notice in entry
        .get("notices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        lines.push(format!("             warn notice: {notice}"));
    }
    for hint in entry
        .get("hints")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        lines.push(format!("             hint: {hint}"));
    }
}

/// A path as the table shows it: relative to the root when it is under it.
fn rel(project: &Project, path: &std::path::Path) -> String {
    project
        .rel_to_root(path)
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_backend_row_shows_failures_notices_and_hints_but_not_passing_checks() {
        let entry: Value = serde_json::from_str(
            "{\"status\":\"partial\",\"checks\":[\
               {\"name\":\"toml\",\"ok\":true,\"detail\":\"sfx, venv py3.12\"},\
               {\"name\":\"probe\",\"ok\":true,\"detail\":\"torch 2.9.0, cuda yes\"},\
               {\"name\":\"checkout\",\"ok\":true,\"detail\":\"abc; dirty (5 tracked files modified)\"},\
               {\"name\":\"model:x\",\"ok\":false,\"detail\":\"absent\"}],\
             \"notices\":[\"a licence\"],\"hints\":[\"install it\"]}",
        )
        .expect("json");
        let mut lines = Vec::new();
        backend_lines("moss_sfx", &entry, &mut lines);
        assert_eq!(lines[0], "  moss_sfx   partial  torch 2.9.0, cuda yes");
        assert!(
            lines
                .iter()
                .any(|l| l.contains("warn checkout: abc; dirty"))
        );
        assert!(lines.iter().any(|l| l.contains("FAIL model:x: absent")));
        assert!(lines.iter().any(|l| l.contains("warn notice: a licence")));
        assert!(lines.iter().any(|l| l.contains("hint: install it")));
        assert!(
            !lines.iter().any(|l| l.contains("venv py3.12")),
            "{lines:?}"
        );
    }
}
