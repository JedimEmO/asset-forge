//! Turning a `forge gen` command line into a job spec, in one place.
//!
//! Both doors submit the same generators — a human's `just sfx` and an
//! agent's `generate_audio` — so what a command line is *called* and which
//! paths it claims are one fact, not two. `kind` is `<tool>.<verb>` so
//! `list_runs`, `status` and the monitor can group without parsing `argv`;
//! `outputs_claimed` is what the out-path lease is taken on.
//!
//! Nothing here knows a generator's flags beyond the three that name a
//! destination. The queue schedules a command line; it does not compose
//! one.

use std::path::Path;

use crate::job::JobSpec;

/// The flags that name where a generator writes.
const OUT_FLAGS: [&str; 3] = ["--out", "--out-dir", "--record"];

/// `<tool>.<verb>` for a `forge gen` command line.
///
/// The tool name is the MCP tool an agent would have called for the same
/// work, so a job a human queued and a job an agent queued group together
/// in `status` and `list_runs` instead of reading as two systems.
#[must_use]
pub fn kind_of(argv: &[String]) -> String {
    let first = argv.first().map_or("", String::as_str);
    let second = argv.get(1).map_or("", String::as_str);
    match (first, second) {
        ("sfx" | "music" | "speech" | "voice", _) => format!("generate_audio.{first}"),
        ("motion", "sweep" | "keys") => format!("generate_clips.{second}"),
        ("motion", verb) => format!("motion.{verb}"),
        ("mesh", _) => String::from("generate_mesh.lift"),
        ("prop", _) => String::from("generate_mesh.prop"),
        ("rig" | "export" | "rig-build" | "skin" | "prepare", _) => format!("rig.{first}"),
        ("", _) => String::from("forge_gen.none"),
        (verb, _) => format!("forge_gen.{verb}"),
    }
}

/// The paths a command line says it will write, relative to the project
/// root — what the out-path lease is taken on.
///
/// A path outside the project is claimed as it was written: a lease is
/// about two doors wanting one file, and a file is a file wherever it is.
#[must_use]
pub fn outputs_claimed(argv: &[String], project_root: &Path) -> Vec<String> {
    let mut claimed = Vec::new();
    let mut arguments = argv.iter();
    while let Some(argument) = arguments.next() {
        let value = if let Some((flag, inline)) = argument.split_once('=') {
            OUT_FLAGS.contains(&flag).then(|| inline.to_owned())
        } else if OUT_FLAGS.contains(&argument.as_str()) {
            arguments.next().cloned()
        } else {
            None
        };
        let Some(value) = value else { continue };
        let path = Path::new(&value);
        let relative = if path.is_absolute() {
            path.strip_prefix(project_root)
                .map_or_else(|_| value.clone(), |rest| rest.display().to_string())
        } else {
            value.clone()
        };
        if !claimed.contains(&relative) {
            claimed.push(relative);
        }
    }
    claimed
}

/// The spec for a `forge gen` command line from this project.
#[must_use]
pub fn spec_for(argv: &[String], project_root: &Path, created_by: &str) -> JobSpec {
    let claimed = outputs_claimed(argv, project_root);
    // The record is claimed like any other output — two jobs writing one
    // record is the same collision — but it is also named on the row, so a
    // reader does not have to guess which of the claims it was.
    let record = claimed
        .iter()
        .find(|path| {
            Path::new(path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .cloned();
    JobSpec {
        kind: kind_of(argv),
        backend: None,
        argv: argv.to_vec(),
        outputs_claimed: claimed,
        record,
        created_by: created_by.to_owned(),
        // The asking process's own answer, because a daemon's environment
        // is not the caller's: `FORGE_FAKE=1 just sfx …` against a daemon
        // somebody else started must still write a placeholder.
        fake: std::env::var("FORGE_FAKE")
            .ok()
            .map(|value| value == "1" || value == "true"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn a_command_line_is_named_after_the_tool_that_would_have_called_it() {
        assert_eq!(kind_of(&argv("sfx --prompt x")), "generate_audio.sfx");
        assert_eq!(kind_of(&argv("voice warden")), "generate_audio.voice");
        assert_eq!(
            kind_of(&argv("motion sweep --samples 8")),
            "generate_clips.sweep"
        );
        assert_eq!(kind_of(&argv("motion review a.npz")), "motion.review");
        assert_eq!(kind_of(&argv("mesh --image a.png")), "generate_mesh.lift");
        assert_eq!(kind_of(&argv("prop --image a.png")), "generate_mesh.prop");
        assert_eq!(kind_of(&argv("export vex")), "rig.export");
        assert_eq!(kind_of(&argv("doctor")), "forge_gen.doctor");
        assert_eq!(kind_of(&[]), "forge_gen.none");
    }

    #[test]
    fn the_lease_is_taken_on_what_the_flags_name() {
        let root = Path::new("/home/me/game");
        let spec = spec_for(
            &argv(
                "sfx --prompt door --out /home/me/game/out/audio/sfx/door.wav \
                 --record=out/audio/sfx/door.json --created-by cli",
            ),
            root,
            "cli",
        );
        assert_eq!(
            spec.outputs_claimed,
            vec![
                String::from("out/audio/sfx/door.wav"),
                String::from("out/audio/sfx/door.json")
            ],
            "an absolute path under the project is claimed relative to it, and --flag=value counts"
        );
        assert_eq!(spec.record.as_deref(), Some("out/audio/sfx/door.json"));
        assert_eq!(spec.kind, "generate_audio.sfx");
        assert!(
            outputs_claimed(&argv("doctor --json"), root).is_empty(),
            "a command line that names no destination claims none"
        );
    }
}
