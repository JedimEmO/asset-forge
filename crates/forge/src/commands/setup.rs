//! `forge setup [kind…]`: **one screen before a byte downloads**, then the
//! installers, in order, skipping what is already there.
//!
//! What the screen says: per chosen kind the backends it needs, what each
//! costs on disk, the total, and every licence fact those carry — the
//! nvdiffrast non-commercial clause, the `DINOv3` gated login, Llama 3's
//! attribution, the `SkinTokens` encoder question, `ComfyUI`'s GPL — each in
//! the words a human is asked to accept, not a summary of them. Then it
//! asks once.
//!
//! # `--yes` takes a name, never a blanket
//!
//! `--yes nvdiffrast --yes llama3` accepts by id and is repeatable. **A
//! bare `--yes` is refused**, because a blanket yes to a list nobody read
//! is exactly what this gate exists to prevent: the whole point of showing
//! the text is that the person saying yes has seen it. On a terminal with
//! no `--yes` at all the screen is printed and one question is asked.
//!
//! Acceptances append to `$FORGE_BACKENDS_HOME/licences.json` — beside the
//! installs, because the install is what is licensed. Never to `forge.toml`,
//! which is hand-edited and would let an acceptance be typed rather than
//! given.
//!
//! # Resumable
//!
//! Every installer is idempotent and writes `installed.json`. Before
//! installing anything this asks `forge gen doctor --json`, and a backend
//! that already reads `ok` is skipped with one line saying so — which is a
//! stronger question than "does the receipt match the pin", because a
//! receipt says an env was made and `ok` says the weights are there too.

use std::io::{IsTerminal, Write};
use std::process::Command;

use clap::Args;
use forge_library::Project;
use forge_library::project::{BLENDER_BACKEND, Licence, MakeKind, MakeKinds, SetupPlan, licences};
use serde_json::Value;

use crate::commands::generate;
use crate::outcome::{Failure, Outcome};

/// `forge setup`.
#[derive(Debug, Clone, Default, Args)]
pub(crate) struct SetupArgs {
    /// Which kinds to install for: props, characters, clips, sfx, music,
    /// voice. Unstated, everything the project's `[make]` chose.
    #[arg(value_name = "KIND")]
    pub(crate) kinds: Vec<String>,
    /// Accept one licence by id: `--yes nvdiffrast --yes llama3`.
    /// Repeatable. A bare `--yes` is refused — the ids are what you are
    /// agreeing to, and a blanket yes to a list nobody read is the thing
    /// this gate exists to prevent.
    #[arg(long = "yes", value_name = "LICENCE")]
    pub(crate) accept: Vec<String>,
    /// Print the screen and stop. Nothing is fetched, nothing is accepted.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Make the environments now and leave every weight to the first
    /// generate. Doctor will read `partial` until they arrive.
    #[arg(long)]
    pub(crate) no_models: bool,
}

/// Print the screen, ask once, then install what is not there.
pub(crate) fn run(project: &Project, args: &SetupArgs) -> Outcome {
    let kinds = wanted_kinds(project, &args.kinds)?;
    let plan = SetupPlan::for_kinds(&kinds, project.tier());
    print!("{}", plan.screen());
    if plan.kinds.is_empty() {
        return Ok(());
    }
    if args.dry_run {
        println!("--dry-run: nothing was fetched and nothing was accepted.");
        return Ok(());
    }

    // -- the gate ---------------------------------------------------------
    let receipt = licences::load()?;
    for licence in &plan.licences {
        if let Some(row) = receipt.row(licence.id) {
            println!(
                "already accepted: {} — by {} at {} ({})",
                licence.id,
                row.by,
                row.at,
                row.via.as_str()
            );
        }
    }
    let missing = plan.missing_accepts(&receipt, &args.accept);
    let mut accepted: Vec<&'static Licence> = plan
        .required()
        .into_iter()
        .filter(|licence| args.accept.iter().any(|id| id == licence.id))
        .collect();
    if !missing.is_empty() {
        if !std::io::stdin().is_terminal() {
            return Err(Failure::refused(SetupPlan::refusal(&missing)));
        }
        // One question, on the terminal, naming exactly what is being
        // agreed to. Not one question per licence: the screen above showed
        // them all, and a ladder of prompts is how people stop reading.
        let ids: Vec<&str> = missing.iter().map(|licence| licence.id).collect();
        let answer = ask(&format!(
            "Accept {} ({})? [y/N]",
            if ids.len() == 1 {
                "this licence"
            } else {
                "these licences"
            },
            ids.join(", ")
        ))?;
        if !matches!(answer.to_ascii_lowercase().as_str(), "y" | "yes") {
            return Err(Failure::refused(format!(
                "declined: nothing was installed. {} still needs accepting; \
                 re-run with --yes <id> once you have read {}",
                ids.join(", "),
                if ids.len() == 1 { "it" } else { "them" }
            )));
        }
        accepted.extend(missing);
    }
    if !accepted.is_empty() {
        let ids: Vec<&str> = accepted.iter().map(|licence| licence.id).collect();
        let (_, added) = licences::accept(&ids, "human", licences::Via::Cli)?;
        if !added.is_empty() {
            println!(
                "recorded in {}: {}",
                licences::path().display(),
                added.join(", ")
            );
        }
    }

    // Everything this machine has agreed to, by id: what was on the receipt
    // before this call and what this call just recorded. It is what decides
    // whether an installer may be handed `--yes`.
    let mut accepted_ids: Vec<String> = accepted
        .iter()
        .map(|licence| String::from(licence.id))
        .collect();
    let receipt = licences::load()?;
    for licence in &plan.licences {
        if receipt.has(licence.id) && !accepted_ids.iter().any(|id| id == licence.id) {
            accepted_ids.push(String::from(licence.id));
        }
    }

    // -- what is already there --------------------------------------------
    let installed = already_ok(project);
    let mut failed: Vec<String> = Vec::new();
    for need in &plan.backends {
        if need.name == BLENDER_BACKEND {
            println!(
                "  {:<12} host program — install Blender >= 4.2 and put it on PATH, or set \
                 $BLENDER_BIN; nothing here installs it",
                need.name
            );
            continue;
        }
        if installed.contains(&need.name.to_string()) {
            println!("  {:<12} already ok — skipped", need.name);
            continue;
        }
        let Some(dir) = project.backends_dir.as_ref() else {
            failed.push(format!("{}: no backends directory", need.name));
            continue;
        };
        let script = dir.join(need.name).join("install.sh");
        if !script.is_file() {
            failed.push(format!("{}: no {}", need.name, script.display()));
            continue;
        }
        println!("== {} ({:.1} GB)", need.name, need.disk_gb());
        let mut command = Command::new("bash");
        command.arg(&script);
        for flag in installer_flags(need, &plan, &accepted_ids) {
            command.arg(flag);
        }
        if args.no_models {
            command.arg("--no-models");
        }
        match command.status() {
            Ok(status) if status.success() => {}
            Ok(status) => failed.push(format!("{}: install.sh exited {status}", need.name)),
            Err(err) => failed.push(format!("{}: install.sh did not start: {err}", need.name)),
        }
    }
    if failed.is_empty() {
        println!(
            "setup: every backend for {} is in place. `forge doctor` says what each one sees.",
            label(&plan.kinds)
        );
        return Ok(());
    }
    Err(Failure::failed(format!(
        "setup did not finish:\n  {}\n`forge doctor` names what each one is missing.",
        failed.join("\n  ")
    )))
}

/// The flags one installer is run with — and **`--yes` only when this
/// machine has agreed to everything that installer stops and asks about**.
///
/// `forge setup` used to pass a blanket `--yes` to every `install.sh`: the
/// door the CLI itself refuses from a human ("a blanket yes to a list
/// nobody read is exactly what the gate exists to prevent"), handed to a
/// script whose `confirm_license` takes it. `backends/comfy/install.sh`
/// then accepted the Shakker-Labs `FLUX.1-dev` `ControlNet` — **a
/// non-commercial licence**, not one of the five ids, never on the screen,
/// in no receipt — on the strength of a `--yes` about nvdiffrast
/// (2026-08-30). That prompt and the image weights behind it have since
/// left the host with the reference door, but the rule they bought stays:
///
/// `--yes` is passed only when every id in `BackendNeed::installer_prompts`
/// is on this machine's receipt or in this call's `--yes`. An installer
/// that would still ask gets no `--yes` and asks — on a TTY it is answered,
/// and without one it stops, which is the correct end of a licence nobody
/// has agreed to.
fn installer_flags(
    need: &'static forge_library::project::BackendNeed,
    _plan: &SetupPlan,
    accepted: &[String],
) -> Vec<String> {
    let mut flags: Vec<String> = Vec::new();
    let prompts: Vec<&str> = need.installer_prompts.to_vec();
    let covered = prompts
        .iter()
        .all(|id| accepted.iter().any(|given| given == id));
    if covered {
        flags.push(String::from("--yes"));
    } else {
        println!(
            "  {:<12} no --yes: {} is not accepted on this machine, so its installer asks \
             for itself",
            need.name,
            prompts.join(", ")
        );
    }
    flags
}

/// The kinds this call is about: the ones named, else the project's own.
fn wanted_kinds(project: &Project, named: &[String]) -> Result<Vec<MakeKind>, Failure> {
    if named.is_empty() {
        return Ok(project.make.chosen());
    }
    Ok(MakeKinds::parse_list(&named.join(","))?.chosen())
}

/// The backends `forge gen doctor` already calls `ok`.
///
/// A probe pass before a download is the cheap half of "look before you
/// spend": seconds each, against tens of GB.
fn already_ok(project: &Project) -> Vec<String> {
    let Ok(result) = generate::spawn(project, &["doctor", "--no-host"], false) else {
        return Vec::new();
    };
    let Some(report) = result.payload else {
        return Vec::new();
    };
    report
        .get("backends")
        .and_then(Value::as_object)
        .map(|entries| {
            entries
                .iter()
                .filter(|(_, entry)| entry.get("status").and_then(Value::as_str) == Some("ok"))
                .map(|(name, _)| name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// The kinds, as a sentence.
fn label(kinds: &[MakeKind]) -> String {
    kinds
        .iter()
        .map(|kind| kind.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// One line read from the terminal.
fn ask(question: &str) -> Result<String, Failure> {
    print!("{question} ");
    std::io::stdout()
        .flush()
        .map_err(|e| Failure::failed(e.to_string()))?;
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| Failure::failed(e.to_string()))?;
    Ok(line.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_library::project::Tier;

    #[test]
    fn a_gated_kind_with_nothing_accepted_refuses_and_names_the_ids() {
        let plan = SetupPlan::for_kinds(&[MakeKind::Props], Tier::Full);
        let missing = plan.missing_accepts(&licences::Receipt::default(), &[]);
        let refusal = SetupPlan::refusal(&missing);
        assert!(
            refusal.starts_with("refused: nothing was installed."),
            "{refusal}"
        );
        for id in ["nvdiffrast", "dinov3"] {
            assert!(refusal.contains(id), "{refusal}");
        }
        assert!(refusal.contains("call licences first"), "{refusal}");
    }

    #[test]
    fn an_installer_is_handed_yes_only_for_what_this_machine_agreed_to() {
        let plan = SetupPlan::for_kinds(&[MakeKind::Music], Tier::Full);
        let comfy = plan
            .backends
            .iter()
            .find(|need| need.name == "comfy")
            .expect("music runs on the host");
        assert_eq!(
            installer_flags(comfy, &plan, &[]),
            vec!["--yes"],
            "the host asks about nothing since the image models left it, so there is nothing \
             to decline and no group to name"
        );

        let props = SetupPlan::for_kinds(&[MakeKind::Props], Tier::Full);
        assert!(
            !props.backends.iter().any(|need| need.name == "comfy"),
            "props need no host: the reference image is brought, not generated"
        );

        let trellis = props
            .backends
            .iter()
            .find(|need| need.name == "trellis2")
            .expect("props lift");
        assert!(
            !installer_flags(trellis, &props, &[]).contains(&String::from("--yes")),
            "nvdiffrast is not accepted here, so its installer asks for itself rather than \
             being told yes on behalf of somebody who read nothing"
        );
        assert_eq!(
            installer_flags(trellis, &props, &[String::from("nvdiffrast")]),
            vec![String::from("--yes")],
            "and once it is accepted by name, the installer needs no TTY"
        );
    }

    #[test]
    fn the_screen_comes_before_anything_and_names_its_disk() {
        let plan = SetupPlan::for_kinds(&[MakeKind::Clips], Tier::Lean);
        let screen = plan.screen();
        assert!(screen.starts_with("setup — what this installs, before a byte downloads"));
        assert!(screen.contains("ardy"), "{screen}");
        assert!(screen.contains("Built with Meta Llama 3"), "{screen}");
        assert!(screen.contains("35.0 GB"), "{screen}");

        // And the bill a music project is shown is the one its installers
        // actually spend: one checkpoint and the host, not the image stack.
        let music = SetupPlan::for_kinds(&[MakeKind::Music], Tier::Full).screen();
        assert!(
            music.contains("10.0 GB"),
            "acestep's own checkpoint: {music}"
        );
        assert!(music.contains("total          12.0 GB"), "{music}");
    }
}
