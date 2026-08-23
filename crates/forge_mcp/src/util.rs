//! The small pieces every tool needs: refusals, inline images, subprocesses.
//!
//! Nothing here knows what a clip is. It exists because the same three
//! mistakes were available at every call site — swallowing the list of
//! valid names in a refusal, inlining an image large enough to blow the
//! frame, and awaiting a subprocess with no ceiling — and one copy of each
//! is easier to keep right than ten.

use std::path::Path;
use std::time::Duration;

use base64::{Engine, prelude::BASE64_STANDARD};
use rmcp::model::{CallToolResult, Content};

/// Inline images larger than this are refused rather than silently
/// truncated.
///
/// Roughly what a vision model accepts before the client downscales the
/// sheet past the point where a foot contact is visible, which is the whole
/// reason for rendering one.
pub(crate) const MAX_IMAGE_BYTES: usize = 3_500_000;

/// Hard ceiling on a render, so a wedged GPU cannot hang the agent's turn.
/// A sheet under llvmpipe is a minute; three is a wedge.
pub(crate) const RENDER_TIMEOUT: Duration = Duration::from_mins(3);

/// Hard ceiling on a generator. A motion sweep loads a 15 GB text encoder
/// before it draws anything; a music track renders for minutes. The
/// generate tools' ceiling, defined beside the render one so the two are
/// read together.
pub(crate) const GENERATE_TIMEOUT: Duration = Duration::from_mins(20);

/// Hard ceiling on `doctor`: every backend is probed inside its own
/// interpreter, seconds each, and a probe that imports torch on a cold
/// cache is tens of seconds.
pub(crate) const DOCTOR_TIMEOUT: Duration = Duration::from_mins(5);

/// How many lines of a failing child's stderr a refusal quotes.
const STDERR_TAIL_LINES: usize = 12;

/// A refusal: an *error* result inside a successful protocol frame.
///
/// The distinction matters. An `Err(ErrorData)` is rendered opaquely by the
/// client and teaches the agent nothing, so it burns a turn and then repeats
/// the same call. This shape puts the reason — and, where there is one, the
/// list of things that would have worked — into the conversation.
pub(crate) fn refuse(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![Content::text(message.into())])
}

/// A plain successful text frame.
pub(crate) fn report(message: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![Content::text(message.into())])
}

/// A refusal that names everything that *would* have worked.
///
/// `noun` is the singular thing being looked up — `clip`, `body`, `sound`
/// — because "available clips (3)" reads and "available (3)" does not.
pub(crate) fn refuse_unknown(noun: &str, wanted: &str, valid: &[String]) -> CallToolResult {
    let plural = plural(noun);
    if valid.is_empty() {
        return refuse(format!(
            "no {noun} matching {wanted:?} — and there are no {plural} at all yet"
        ));
    }
    refuse(format!(
        "no {noun} matching {wanted:?}.\navailable {plural} ({}):\n  {}",
        valid.len(),
        valid.join("\n  ")
    ))
}

/// The plural of a lookup noun: `mesh` takes -es, everything else here -s.
/// One rule, because "available meshs" shipped once.
fn plural(noun: &str) -> String {
    if noun.ends_with('s') || noun.ends_with("sh") || noun.ends_with("ch") || noun.ends_with('x') {
        format!("{noun}es")
    } else {
        format!("{noun}s")
    }
}

/// What became of an attempt to inline an image.
///
/// Three outcomes rather than an `Option` because the call sites report
/// them differently: a renderer that succeeded and then produced an
/// unreadable file is a failure of the tool, while a plot that came out too
/// large is still a useful answer with a path attached.
pub(crate) enum Inline {
    /// Small enough to send.
    Image(Box<Content>),
    /// Over [`MAX_IMAGE_BYTES`]; the caller says where the file is instead.
    TooLarge {
        /// Size on disk, for the message.
        bytes: usize,
    },
    /// The file could not be read at all.
    Unreadable {
        /// What the operating system said.
        error: String,
    },
}

impl Inline {
    /// Megabytes, for the two messages that quote a size.
    pub(crate) fn megabytes(bytes: usize) -> f64 {
        bytes as f64 / 1e6
    }

    /// The content block for a frame, whatever happened: the image, or a
    /// line saying why it is not here and where it is. An unreadable file
    /// after a successful render is still reported as a line rather than a
    /// refusal — the report text beside it is the part the agent came for.
    pub(crate) fn into_content(self, path: &Path, what: &str) -> Content {
        match self {
            Self::Image(image) => *image,
            Self::TooLarge { bytes } => Content::text(format!(
                "{what} is {:.1} MB, over the {:.1} MB inline limit — read it from {}",
                Self::megabytes(bytes),
                Self::megabytes(MAX_IMAGE_BYTES),
                path.display()
            )),
            Self::Unreadable { error } => Content::text(format!(
                "{what} was reported written but {} could not be read: {error}",
                path.display()
            )),
        }
    }
}

/// Read a PNG and turn it into a content block, capped.
pub(crate) fn inline_image(path: &Path) -> Inline {
    match std::fs::read(path) {
        Ok(bytes) if bytes.len() > MAX_IMAGE_BYTES => Inline::TooLarge { bytes: bytes.len() },
        Ok(bytes) => Inline::Image(Box::new(Content::image(
            BASE64_STANDARD.encode(&bytes),
            "image/png",
        ))),
        Err(err) => Inline::Unreadable {
            error: err.to_string(),
        },
    }
}

/// What a finished subprocess left behind, as text.
#[derive(Debug, Clone)]
pub(crate) struct Captured {
    /// The exit code; `None` when a signal ended it.
    pub(crate) code: Option<i32>,
    /// Everything it printed, lossily decoded.
    pub(crate) stdout: String,
    /// Everything it said, lossily decoded.
    pub(crate) stderr: String,
}

impl Captured {
    fn from_output(output: &std::process::Output) -> Self {
        Self {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// The last stdout line, which is where `forge gen` and `forge doctor
    /// --json` put their one JSON object.
    pub(crate) fn last_stdout_line(&self) -> Option<&str> {
        self.stdout.lines().rev().find(|l| !l.trim().is_empty())
    }

    /// The tail of stderr, which is where a Python trace puts the line that
    /// says why. Quoting the whole of it into a tool frame buries that line.
    pub(crate) fn stderr_tail(&self) -> String {
        stderr_tail(&self.stderr, STDERR_TAIL_LINES)
    }
}

/// What running a subprocess did.
#[derive(Debug)]
pub(crate) enum Ran {
    /// It exited zero.
    Ok(Captured),
    /// It exited non-zero; the output carries the diagnostics.
    Failed(Captured),
    /// It never started — missing, not executable, wrong interpreter.
    Unlaunchable(std::io::Error),
    /// It outlived its ceiling and was abandoned.
    TimedOut,
}

impl Ran {
    /// The output, or the refusal that says what went wrong: a non-zero
    /// exit quoted with its code and the stderr tail, a binary that would
    /// not launch named by path, a timeout with its ceiling. `what` is the
    /// phrase the message starts with — "render of clips/walk.glb", "doctor".
    pub(crate) fn or_refuse(
        self,
        what: &str,
        exe: &Path,
        limit: Duration,
    ) -> Result<Captured, CallToolResult> {
        self.or_message(what, exe, limit).map_err(refuse)
    }

    /// As [`Self::or_refuse`], with the message as text — for a tool that
    /// has more to say beside it.
    pub(crate) fn or_message(
        self,
        what: &str,
        exe: &Path,
        limit: Duration,
    ) -> Result<Captured, String> {
        match self {
            Self::Ok(captured) => Ok(captured),
            Self::Failed(captured) => Err(failed_message(what, &captured)),
            Self::Unlaunchable(err) => {
                Err(format!("could not run {} for {what}: {err}", exe.display()))
            }
            Self::TimedOut => Err(format!(
                "{what} timed out after {} min — the GPU may be held by another process; \
                 the doctor tool says who holds it",
                minutes(limit)
            )),
        }
    }
}

/// The message for a child that exited non-zero: the code, the stderr
/// tail, and the stdout so far, since a renderer prints its summary before
/// it decides to fail.
fn failed_message(what: &str, captured: &Captured) -> String {
    let code = captured.code.map_or_else(
        || String::from("killed by a signal"),
        |c| format!("exit {c}"),
    );
    let mut text = format!("{what} failed ({code}).\n{}", captured.stderr_tail());
    let stdout = captured.stdout.trim();
    if !stdout.is_empty() {
        text.push_str("\n\n");
        text.push_str(stdout);
    }
    text
}

/// Run a command with a hard ceiling, capturing both streams.
///
/// Every subprocess this server launches is minutes long and at least one
/// of them loads a 15 GB text encoder, so a wedged one is not hypothetical.
/// The ceiling is what keeps a wedged GPU from consuming the agent's whole
/// turn. `DISPLAY` and `WAYLAND_DISPLAY` are removed so a render never
/// depends on a login session being present — the child picks a headless
/// adapter the way CI does.
pub(crate) async fn run(command: &mut tokio::process::Command, limit: Duration) -> Ran {
    command.env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
    command.kill_on_drop(true);
    match tokio::time::timeout(limit, command.output()).await {
        Ok(Ok(output)) if output.status.success() => Ran::Ok(Captured::from_output(&output)),
        Ok(Ok(output)) => Ran::Failed(Captured::from_output(&output)),
        Ok(Err(err)) => Ran::Unlaunchable(err),
        Err(_) => Ran::TimedOut,
    }
}

/// The last `lines` non-empty lines of a stderr stream, or a placeholder
/// when it said nothing.
pub(crate) fn stderr_tail(stderr: &str, lines: usize) -> String {
    let kept: Vec<&str> = stderr
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if kept.is_empty() {
        String::from("(no diagnostics on stderr)")
    } else {
        kept.join("\n")
    }
}

/// A duration in whole minutes, for a timeout message.
pub(crate) fn minutes(limit: Duration) -> u64 {
    limit.as_secs() / 60
}

/// One line of a longer string, trimmed and capped, for a table cell.
pub(crate) fn first_line(text: &str, limit: usize) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    if line.chars().count() <= limit {
        return line.to_owned();
    }
    let kept: String = line.chars().take(limit.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// The text of a frame, for tests and for the one place a tool quotes
/// another tool's answer.
#[cfg(test)]
pub(crate) fn frame_text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_lists_what_would_have_worked() {
        let names = vec![String::from("roll"), String::from("walk")];
        let refusal = refuse_unknown("clip", "rol", &names);
        let text = frame_text(&refusal);
        assert!(text.contains("no clip matching \"rol\""), "{text}");
        assert!(text.contains("available clips (2)"), "{text}");
        assert!(text.contains("  walk"), "{text}");
        assert_eq!(refusal.is_error, Some(true));
    }

    #[test]
    fn an_empty_library_says_so_rather_than_listing_nothing() {
        let refusal = refuse_unknown("sound", "bark", &[]);
        let text = frame_text(&refusal);
        assert!(text.contains("no sounds at all"), "{text}");
        assert_eq!(refusal.is_error, Some(true));
    }

    #[test]
    fn a_report_is_not_an_error() {
        let frame = report("3 clips");
        assert_ne!(frame.is_error, Some(true));
        assert_eq!(frame_text(&frame), "3 clips");
    }

    #[test]
    fn a_failed_child_is_refused_with_its_code_and_its_stderr_tail() {
        let captured = Captured {
            code: Some(2),
            stdout: String::from("summary line\n"),
            stderr: String::from("noise\n\nforge: no clip named \"x\"\n"),
        };
        let refusal = Ran::Failed(captured)
            .or_refuse("render of x", Path::new("/x"), RENDER_TIMEOUT)
            .expect_err("refused");
        let text = frame_text(&refusal);
        assert!(text.starts_with("render of x failed (exit 2)."), "{text}");
        assert!(text.contains("forge: no clip named"), "{text}");
        assert!(text.contains("summary line"), "{text}");
        assert_eq!(refusal.is_error, Some(true));

        let silent = Captured {
            code: None,
            stdout: String::new(),
            stderr: String::new(),
        };
        let text = frame_text(
            &Ran::Failed(silent)
                .or_refuse("doctor", Path::new("/x"), RENDER_TIMEOUT)
                .expect_err("refused"),
        );
        assert!(text.contains("killed by a signal"), "{text}");
        assert!(text.contains("no diagnostics"), "{text}");
    }

    #[test]
    fn the_stderr_tail_keeps_the_last_lines_in_order() {
        let text = "a\nb\n\nc\nd\n";
        assert_eq!(stderr_tail(text, 2), "c\nd");
        assert_eq!(stderr_tail(text, 10), "a\nb\nc\nd");
        assert!(stderr_tail("\n\n", 3).contains("no diagnostics"));
    }

    #[test]
    fn the_last_stdout_line_skips_trailing_blanks() {
        let captured = Captured {
            code: Some(0),
            stdout: String::from("progress\n{\"ok\":true}\n\n"),
            stderr: String::new(),
        };
        assert_eq!(captured.last_stdout_line(), Some("{\"ok\":true}"));
    }

    #[test]
    fn or_refuse_names_the_binary_and_the_ceiling() {
        let exe = Path::new("/nowhere/forge");
        let text = frame_text(
            &Ran::TimedOut
                .or_refuse("render of walk", exe, RENDER_TIMEOUT)
                .expect_err("refused"),
        );
        assert!(text.contains("timed out after 3 min"), "{text}");
        let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let text = frame_text(
            &Ran::Unlaunchable(missing)
                .or_refuse("doctor", exe, DOCTOR_TIMEOUT)
                .expect_err("refused"),
        );
        assert!(text.contains("/nowhere/forge"), "{text}");
        let ok = Ran::Ok(Captured {
            code: Some(0),
            stdout: String::from("x"),
            stderr: String::new(),
        });
        assert!(ok.or_refuse("x", exe, RENDER_TIMEOUT).is_ok());
    }

    #[test]
    fn an_oversized_image_becomes_a_line_naming_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let png = dir.path().join("big.png");
        std::fs::write(&png, vec![0u8; MAX_IMAGE_BYTES + 1]).expect("write");
        match inline_image(&png) {
            Inline::TooLarge { bytes } => assert_eq!(bytes, MAX_IMAGE_BYTES + 1),
            _ => panic!("should be too large"),
        }
        let text = inline_image(&png).into_content(&png, "sheet");
        let text = text.as_text().expect("a text block").text.clone();
        assert!(text.contains("over the 3.5 MB inline limit"), "{text}");
        assert!(text.contains("big.png"), "{text}");

        let small = dir.path().join("small.png");
        std::fs::write(&small, b"\x89PNG").expect("write");
        assert!(matches!(inline_image(&small), Inline::Image(_)));
        assert!(matches!(
            inline_image(&dir.path().join("missing.png")),
            Inline::Unreadable { .. }
        ));
    }

    #[test]
    fn a_table_cell_takes_the_first_line_and_says_when_it_cut() {
        assert_eq!(first_line("  one  \ntwo", 40), "one");
        assert_eq!(first_line("abcdef", 4), "abc…");
        assert_eq!(first_line("", 10), "");
    }

    #[test]
    fn minutes_round_down() {
        assert_eq!(minutes(RENDER_TIMEOUT), 3);
        assert_eq!(minutes(GENERATE_TIMEOUT), 20);
        assert_eq!(minutes(Duration::from_secs(59)), 0);
    }
}
