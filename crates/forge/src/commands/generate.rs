//! `forge gen <cmd> [args…]`: the Python layer, run whole.
//!
//! The binary knows nothing about a generator's flags and wants to know
//! nothing: the command line after `gen` is handed to `python3
//! <toolkit>/python/forge_gen` verbatim, with `--project <root>` appended so
//! the record it writes carries paths relative to this project, and
//! `--json` so its last stdout line is one object this side can read. The
//! lines before it — progress, the fit lines, the "keyed background" line —
//! stream through as they arrive; stderr is the child's own and is not
//! touched.
//!
//! On success the object is summarised (record, outputs) unless `--json` was
//! among the arguments, in which case the object itself is the last stdout
//! line here too — the MCP server and a skill that parses read the same line
//! either way. On a refusal the exit code is relayed unchanged: 3 install
//! something, 4 fix the input, 5 read the log, 6 put a tool on PATH. The
//! toolkit not being findable is a 6 of this side's own — the tool that is
//! missing is `python/forge_gen`, and the hint names `FORGE_HOME`.

use std::io::{Read, Write as _};
use std::process::{Command, Stdio};

use forge_library::Project;
use forge_library::backends::{Backends, GenExit, HOME_ENV};
use serde_json::Value;

use crate::cli::GenArgs;
use crate::outcome::{Failure, Outcome};

/// What a run of the Python layer left behind.
pub(crate) struct GenResult {
    /// The exit code, as the table reads it; `None` when it was none of them.
    pub(crate) exit: Option<GenExit>,
    /// The raw code, for the message when it is not in the table.
    pub(crate) raw_code: Option<i32>,
    /// The object on the last stdout line, when there was one.
    pub(crate) payload: Option<Value>,
}

/// Whether the command line is only asking for help text.
pub(crate) fn wants_help(args: &GenArgs) -> bool {
    args.rest.iter().any(|a| a == "--help" || a == "-h")
}

/// Print a generator's help without a project.
///
/// `forge gen sfx --help` is an agent reading a flag table before choosing
/// a directory; argparse answers it without ever touching a library, so the
/// project discovery the real call needs is not a reason to refuse. The
/// toolkit still has to be findable — the help lives in the Python layer.
pub(crate) fn help(args: &GenArgs) -> Outcome {
    let Some(toolkit) = crate::toolkit::gen_dir() else {
        return Err(Failure::from_gen(
            GenExit::MissingTool,
            format!(
                "the toolkit's python/forge_gen was not found from this executable — set \
                 {HOME_ENV} to the asset-forge checkout"
            ),
        ));
    };
    let mut command = Command::new("python3");
    command.arg(toolkit.join("python").join("forge_gen"));
    command.args(&args.rest);
    let status = command.status().map_err(|e| {
        Failure::from_gen(
            GenExit::MissingTool,
            format!(
                "python3 could not be started: {e} — the Python layer needs python3 >= 3.11 on PATH"
            ),
        )
    })?;
    match status.code() {
        Some(0) | None => Ok(()),
        Some(code) => Err(Failure::from_gen(
            GenExit::from_code(code).unwrap_or(GenExit::Usage),
            format!("forge-gen exited {code}"),
        )),
    }
}

/// Run one generator command and relay its verdict.
pub(crate) fn run(project: &Project, args: &GenArgs) -> Outcome {
    let wants_json = args.rest.iter().any(|a| a == "--json");
    let command_line: Vec<&str> = args
        .rest
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "--json")
        .collect();
    // `forge gen doctor` with no --json is a person asking: the Python
    // table is the answer, and there is no object to summarise. Every other
    // command gets --json so the last line can be read back here.
    let is_doctor = command_line.first().copied() == Some("doctor");
    let result = spawn_with(project, &command_line, true, wants_json || !is_doctor)?;
    match result.exit {
        Some(GenExit::Ok) => {
            if let Some(payload) = &result.payload {
                if wants_json {
                    println!("{payload}");
                } else {
                    print!("{}", summary(payload));
                }
            }
            Ok(())
        }
        Some(exit) => {
            if wants_json && let Some(payload) = &result.payload {
                println!("{payload}");
            }
            Err(Failure::from_gen(
                exit,
                refusal(exit, result.payload.as_ref()),
            ))
        }
        None => {
            if wants_json && let Some(payload) = &result.payload {
                println!("{payload}");
            }
            match result.raw_code {
                // Exit 1 is not in the generator table: it is `forge-gen
                // doctor` saying a check did not hold, and it stays a 1 here
                // — the same code every other check in this binary uses.
                Some(1) => Err(Failure::failed(
                    result
                        .payload
                        .as_ref()
                        .and_then(|p| p.get("message").or_else(|| p.get("error")))
                        .and_then(Value::as_str)
                        .map_or_else(
                            || String::from("forge-gen: a check did not hold (exit 1)"),
                            str::to_owned,
                        ),
                )),
                Some(code) => Err(Failure::from_gen(
                    GenExit::BackendFailed,
                    format!("forge-gen exited {code}, which is not a code it speaks"),
                )),
                None => Err(Failure::from_gen(
                    GenExit::BackendFailed,
                    "forge-gen was killed by a signal",
                )),
            }
        }
    }
}

/// Read a child's stdout, splitting on `\n` at the byte level rather than
/// `BufRead::lines()`, whose UTF-8 check is all-or-nothing: one invalid byte
/// anywhere in the stream discards every line already buffered, not just
/// the bad one — measured losing a whole ~80s WSL2 probe's JSON this way,
/// with the underlying bytes themselves perfectly valid UTF-8 on a second,
/// whole-stream read (`read_to_end`), so the corruption was in the
/// incremental reader, not the data. Each line is decoded lossily instead
/// (any actually-bad byte becomes U+FFFD, never a lost line) and the last
/// one is held back rather than relayed, so the caller can parse it as a
/// possible JSON payload without it being printed twice.
fn relay_lines<R: Read>(mut reader: R, relay: bool) -> Option<String> {
    let mut held: Option<String> = None;
    let mut carry: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    let mut out = std::io::stdout().lock();
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        carry.extend_from_slice(&buf[..n]);
        while let Some(pos) = carry.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = carry.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line_bytes[..line_bytes.len() - 1]).into_owned();
            if let Some(previous) = held.replace(line)
                && relay
            {
                let _ = writeln!(out, "{previous}");
                let _ = out.flush();
            }
        }
    }
    if !carry.is_empty()
        && let Some(previous) = held.replace(String::from_utf8_lossy(&carry).into_owned())
        && relay
    {
        let _ = writeln!(out, "{previous}");
        let _ = out.flush();
    }
    held
}

/// Spawn `python3 <toolkit>/python/forge_gen <argv> --project <root> --json`,
/// stream its stdout through (holding the last line back), and return what
/// it exited with and the object on that last line.
///
/// With `relay` off nothing is printed: what `doctor` wants, since its
/// `--json` output is the one line.
pub(crate) fn spawn(project: &Project, argv: &[&str], relay: bool) -> Result<GenResult, Failure> {
    spawn_with(project, argv, relay, true)
}

/// As [`spawn`], with `--json` optional: without it nothing is held back and
/// there is no payload, only the exit code.
fn spawn_with(
    project: &Project,
    argv: &[&str],
    relay: bool,
    json: bool,
) -> Result<GenResult, Failure> {
    let mut command = launcher(project)?;
    command.args(argv);
    command.arg("--project").arg(&project.root);
    // The Python side defaults its rig profile to the toolkit's own
    // rigs/humanoid; the project's forge.toml may name a different one (or an
    // edited copy under assets-src/rigs). Rigging against one contract and
    // checking against another is the kind of disagreement nobody notices
    // until a body fails rig check, so the project's profile is handed over
    // explicitly. A user's own FORGE_RIG_PROFILE still wins.
    if std::env::var_os("FORGE_RIG_PROFILE").is_none() {
        command.env("FORGE_RIG_PROFILE", project.rig_dir());
    }
    if json {
        command.arg("--json");
    }
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::inherit());
    let mut child = command.spawn().map_err(|e| {
        Failure::from_gen(
            GenExit::MissingTool,
            format!(
                "python3 could not be started: {e} — the Python layer needs python3 >= 3.11 on PATH"
            ),
        )
    })?;
    let stdout = child.stdout.take();
    let held = if let Some(stdout) = stdout {
        relay_lines(stdout, relay)
    } else {
        None
    };
    let status = child
        .wait()
        .map_err(|e| Failure::from_gen(GenExit::BackendFailed, format!("forge-gen: {e}")))?;
    let payload = held.as_deref().and_then(parse_object);
    if payload.is_none()
        && let Some(last) = &held
        && relay
    {
        println!("{last}");
    }
    let raw_code = status.code();
    Ok(GenResult {
        exit: raw_code.and_then(GenExit::from_code),
        raw_code,
        payload,
    })
}

/// The launcher command, or the missing-tool refusal naming `FORGE_HOME`.
fn launcher(project: &Project) -> Result<Command, Failure> {
    Backends::python_launcher(project).ok_or_else(|| {
        Failure::from_gen(
            GenExit::MissingTool,
            format!(
                "the toolkit's python/forge_gen was not found from this executable or from \
                 {} — set {HOME_ENV} to the asset-forge checkout",
                project.root.display()
            ),
        )
    })
}

/// A line that is one JSON object, or nothing.
fn parse_object(line: &str) -> Option<Value> {
    let trimmed = line.trim();
    if !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
        return None;
    }
    serde_json::from_str::<Value>(trimmed)
        .ok()
        .filter(Value::is_object)
}

/// The human summary of a success object: the record, the outputs, the
/// time, and any scalar the command added (a seed, a model).
fn summary(payload: &Value) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if let Some(record) = payload.get("record").and_then(Value::as_str) {
        let _ = writeln!(out, "record   {record}");
    }
    if let Some(outputs) = payload.get("outputs").and_then(Value::as_array) {
        for output in outputs.iter().filter_map(Value::as_str) {
            let _ = writeln!(out, "output   {output}");
        }
    }
    if let Some(object) = payload.as_object() {
        for (key, value) in object {
            if matches!(
                key.as_str(),
                "ok" | "record" | "outputs" | "elapsed_s" | "fake" | "records"
            ) {
                continue;
            }
            match value {
                Value::String(text) => {
                    let _ = writeln!(out, "{key:<8} {text}");
                }
                Value::Number(number) => {
                    let _ = writeln!(out, "{key:<8} {number}");
                }
                Value::Bool(flag) => {
                    let _ = writeln!(out, "{key:<8} {flag}");
                }
                _ => {}
            }
        }
    }
    // Whether the run was a --fake one is the record's word, not the
    // summary object's: not every command repeats it there, and the record
    // is what the promote will read.
    let fake = payload.get("fake").and_then(Value::as_bool) == Some(true)
        || payload
            .get("record")
            .and_then(Value::as_str)
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|record| record.get("fake").and_then(Value::as_bool))
            == Some(true);
    if fake {
        out.push_str(
            "fake     true — placeholder outputs that pass the validators and nothing else\n",
        );
    }
    if let Some(elapsed) = payload.get("elapsed_s").and_then(Value::as_f64) {
        let _ = writeln!(out, "elapsed  {elapsed:.1} s");
    }
    out
}

/// The refusal's text from its object: the message (or reason), the hint,
/// and the tail of the log when the backend failed.
fn refusal(exit: GenExit, payload: Option<&Value>) -> String {
    use std::fmt::Write as _;
    let Some(payload) = payload else {
        return format!("{exit} (forge-gen printed no JSON line)");
    };
    let text = payload
        .get("reason")
        .or_else(|| payload.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("(no message)");
    let mut out = format!("{exit}: {text}");
    if let Some(backend) = payload.get("backend").and_then(Value::as_str) {
        let _ = write!(out, " [backend {backend}]");
    }
    if let Some(tool) = payload.get("tool").and_then(Value::as_str) {
        let _ = write!(out, " [tool {tool}]");
    }
    if let Some(hint) = payload.get("hint").and_then(Value::as_str) {
        let _ = write!(out, "\n  hint: {hint}");
    }
    if let Some(tail) = payload.get("log_tail").and_then(Value::as_array) {
        let lines: Vec<&str> = tail.iter().filter_map(Value::as_str).collect();
        if !lines.is_empty() {
            let keep = lines.len().saturating_sub(12);
            out.push_str("\n  log tail:");
            for line in &lines[keep..] {
                out.push_str("\n    ");
                out.push_str(line);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_json_object_on_the_last_line_is_the_payload() {
        assert!(parse_object("{\"ok\": true}").is_some());
        assert!(parse_object("  {\"ok\": true}  ").is_some());
        assert!(parse_object("[1, 2]").is_none());
        assert!(parse_object("fit: 0.83 of the frame").is_none());
        assert!(parse_object("{not json}").is_none());
    }

    #[test]
    fn the_summary_names_record_outputs_and_scalars() {
        let payload: Value = serde_json::from_str(
            "{\"ok\":true,\"record\":\"/r.json\",\"outputs\":[\"/a.glb\"],\"seed\":7,\
             \"model\":\"m\",\"metrics\":{\"x\":1},\"fake\":true,\"elapsed_s\":1.25}",
        )
        .expect("json");
        let text = summary(&payload);
        assert!(text.contains("record   /r.json"));
        assert!(text.contains("output   /a.glb"));
        assert!(text.contains("seed     7"));
        assert!(text.contains("model    m"));
        assert!(!text.contains("metrics"), "objects are not summarised");
        assert!(text.contains("fake     true"));
        assert!(text.contains("elapsed  1.2 s") || text.contains("elapsed  1.3 s"));
    }

    #[test]
    fn a_refusal_carries_reason_hint_and_the_log_tail() {
        let payload: Value = serde_json::from_str(
            "{\"ok\":false,\"error\":\"input_rejected\",\"reason\":\"no flat border\",\
             \"hint\":\"fix the PNG\",\"log_tail\":[\"a\",\"b\"]}",
        )
        .expect("json");
        let text = refusal(GenExit::InputRejected, Some(&payload));
        assert!(text.starts_with("input_rejected: no flat border"));
        assert!(text.contains("hint: fix the PNG"));
        assert!(text.contains("log tail:"));
        assert!(text.ends_with("    b"));
        assert!(refusal(GenExit::BackendFailed, None).contains("no JSON line"));
    }
}
