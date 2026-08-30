//! `forge init`: a project where there was a directory, and the three
//! questions that shape everything after it.
//!
//! Writes `forge.toml` and the convention directories, installs the rig
//! profile, writes the reference ledger's header and an empty manifest — so
//! the very next `forge manifest --check` and `forge verify` pass on a
//! library that holds nothing, which is the honest starting state rather
//! than a broken one.
//!
//! # The three questions
//!
//! Asked once, on a terminal, and phrased as **what you make** and **what
//! card you have** — never as model names, because a model name is a fact
//! about this month's backends and "characters" is a fact about the game.
//!
//! 1. *What will you make here?* The six kinds, defaulting to props,
//!    characters and clips.
//! 2. *What card is this?* Detected from `nvidia-smi` — 22 GB or more is
//!    `full`, 14 or more is `lean`, none is `fake` — and **offered, not
//!    assumed**: the answer is printed with the detected one as the default.
//! 3. *Where is `ComfyUI`?* Only when a chosen kind runs in the comfy
//!    executor; another machine's URL is fine.
//!
//! `--make`, `--tier`, `--comfy-url` and `--yes` are the same three answers
//! given up front, and are what the MCP `init_project` passes. **With no
//! terminal and no flags it takes the defaults and prints one line naming
//! each assumption** — it never hangs on a prompt, which is the trap
//! `hf auth login` taught this repo (`designs/hosting.md`, Common).

use std::io::{IsTerminal, Write};
use std::path::Path;

use clap::Args;
use forge_library::project::{Hardware, MakeKind, MakeKinds, Tier};

use crate::cli::InitArgs;
use crate::outcome::{Failure, Outcome};
use crate::toolkit;

/// The three answers, as flags.
///
/// Flattened into `forge init`'s arguments, and passed verbatim by the MCP
/// `init_project` tool, so a terminal and an agent make a project the same
/// way.
#[derive(Debug, Clone, Default, Args)]
pub(crate) struct MakeFlags {
    /// What you will make here: any of props, characters, clips, sfx,
    /// music, voice as a comma-separated list — or `all`, or `none`.
    /// Unstated on a terminal you are asked; unstated with no terminal it
    /// is props,characters,clips.
    #[arg(long, value_name = "KINDS")]
    pub(crate) make: Option<String>,
    /// The register this machine runs in: full (24 GB), lean (16 GB) or
    /// fake (no card, every generator writes a placeholder). Unstated, it
    /// is detected from nvidia-smi and offered.
    #[arg(long, value_name = "TIER")]
    pub(crate) tier: Option<String>,
    /// Where the `ComfyUI` service answers. Asked only when a chosen kind
    /// runs in the comfy executor; another machine's is fine.
    #[arg(long, value_name = "URL")]
    pub(crate) comfy_url: Option<String>,
    /// Take the answers as given (or as detected) without asking, even on a
    /// terminal.
    #[arg(long, short = 'y')]
    pub(crate) yes: bool,
}

/// Make a project at `root` (the `--project` directory, else the working
/// directory).
pub(crate) fn run(root: Option<&Path>, args: &InitArgs) -> Outcome {
    // The flag table grows `--make`, `--tier`, `--comfy-url` and `--yes`
    // beside the other init flags; until it does, the answers here are the
    // detected ones and `init_project` over MCP is the door that states
    // them.
    run_with(root, args, &MakeFlags::default())
}

/// [`run`] with the three answers already in hand.
pub(crate) fn run_with(root: Option<&Path>, args: &InitArgs, flags: &MakeFlags) -> Outcome {
    let root = match root {
        Some(dir) => dir.to_path_buf(),
        None => crate::cwd()?,
    };
    let name = match &args.name {
        Some(name) => name.clone(),
        None => root
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .ok_or_else(|| {
                Failure::refused(format!(
                    "{} has no name to take — pass --name",
                    root.display()
                ))
            })?,
    };
    let (make, hardware) = ask(flags)?;
    let profile = match &args.rig_dir {
        Some(dir) => Some(dir.clone()),
        None => toolkit::profile_dir(&project_rig()),
    };
    let (project, lines) =
        forge_library::project::create(&root, &name, make, &hardware, profile.as_deref())?;
    let mut lines = lines.into_iter();
    if let Some(first) = lines.next() {
        println!("{first}");
    }
    for line in lines {
        println!("  {line}");
    }
    if profile.is_none() {
        // Exit non-zero: without the profile the project is half-made — the
        // very next `forge verify` and `forge manifest` both exit 1 on it —
        // and an `init` that said ok anyway buried the one message that
        // names the fix.
        return Err(Failure::refused(format!(
            "no rig profile installed — the toolkit's rigs/{} could not be found from this \
             executable. Set {} to the asset-forge checkout (or pass --rig-dir), then run \
             `forge init` here again; forge.toml and the directories are already in place",
            project.rig_name,
            forge_library::backends::TOOLKIT_ENV,
        )));
    }
    println!("next: {}", project.next_step());
    Ok(())
}

/// The rig profile a new project is given. Named here because the profile
/// has to be found *before* the project exists to name it.
fn project_rig() -> String {
    String::from(forge_library::project::DEFAULT_RIG)
}

/// Ask the three questions, or take the answers as given.
///
/// Every path through this ends with an answer: a flag, a prompt, or the
/// default with a line saying so. Nothing waits on a terminal that is not
/// there.
fn ask(flags: &MakeFlags) -> Result<(MakeKinds, Hardware), Failure> {
    let interactive = flags.make.is_none() && !flags.yes && std::io::stdin().is_terminal();
    let detected = Tier::detect();

    let make = match &flags.make {
        Some(value) => MakeKinds::parse_list(value)?,
        None if interactive => ask_make()?,
        None => MakeKinds::DEFAULT,
    };
    let tier = match &flags.tier {
        Some(value) => Tier::parse(value)?,
        None if interactive => ask_tier(detected)?,
        None => detected,
    };
    let wants_comfy = make
        .backends()
        .contains(&forge_library::project::COMFY_BACKEND);
    let comfy_url = match &flags.comfy_url {
        Some(url) => url.trim().to_string(),
        None if interactive && wants_comfy => ask_comfy_url()?,
        None => String::from(forge_library::project::DEFAULT_COMFY_URL),
    };
    if !interactive && flags.make.is_none() {
        // The one line that names each assumption. A silent default is the
        // thing a stranger discovers three commands later.
        println!(
            "no terminal and no --make: assuming you make {} (--make), tier {} ({}) \
             (--tier), comfy at {} (--comfy-url)",
            make.chosen()
                .iter()
                .map(|kind| kind.as_str())
                .collect::<Vec<_>>()
                .join(","),
            tier,
            if flags.tier.is_some() {
                "as stated"
            } else if detected.is_fake() {
                "no card answered nvidia-smi"
            } else {
                "detected from nvidia-smi"
            },
            comfy_url
        );
    }
    Ok((
        make,
        Hardware {
            tier: Some(tier),
            comfy_url,
        },
    ))
}

/// Question one: what will you make here?
fn ask_make() -> Result<MakeKinds, Failure> {
    println!("What will you make here? (the answer shapes everything after it)");
    for kind in MakeKind::ALL {
        println!(
            "  {:<11} {:<45} {}",
            kind.as_str(),
            kind.blurb(),
            if MakeKinds::DEFAULT.has(kind) {
                "(default)"
            } else {
                ""
            }
        );
    }
    let answer = prompt("kinds, comma-separated [props,characters,clips]")?;
    if answer.is_empty() {
        return Ok(MakeKinds::DEFAULT);
    }
    MakeKinds::parse_list(&answer).map_err(Into::into)
}

/// Question two: what card is this? Offered, never assumed.
fn ask_tier(detected: Tier) -> Result<Tier, Failure> {
    println!("What card is this?");
    for tier in Tier::ALL {
        println!(
            "  {:<6} {:<70} {}",
            tier.as_str(),
            tier.blurb(),
            if tier == detected { "(detected)" } else { "" }
        );
    }
    let answer = prompt(&format!("tier [{detected}]"))?;
    if answer.is_empty() {
        return Ok(detected);
    }
    Tier::parse(&answer).map_err(Into::into)
}

/// Question three: where is `ComfyUI`? Only asked when something needs it.
fn ask_comfy_url() -> Result<String, Failure> {
    println!("Where is ComfyUI? (a chosen kind runs inside it; another machine's is fine)");
    let default = forge_library::project::DEFAULT_COMFY_URL;
    let answer = prompt(&format!("url [{default}]"))?;
    Ok(if answer.is_empty() {
        String::from(default)
    } else {
        answer
    })
}

/// One line read from the terminal, with the question in front of it.
fn prompt(question: &str) -> Result<String, Failure> {
    print!("{question}: ");
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

    #[test]
    fn no_terminal_and_no_flags_takes_the_defaults_rather_than_waiting() {
        // The trap this guards against is `hf auth login`: a prompt with no
        // TTY in front of it hangs, and an agent's turn is the thing that
        // ends. Every path through `ask` returns an answer.
        let (make, hardware) = ask(&MakeFlags::default()).expect("defaults");
        assert_eq!(make, MakeKinds::DEFAULT);
        assert!(
            hardware.tier.is_some(),
            "the tier is written, not left to drift"
        );
        assert_eq!(
            hardware.comfy_url,
            forge_library::project::DEFAULT_COMFY_URL
        );
    }

    #[test]
    fn the_flags_are_the_same_three_answers() {
        let flags = MakeFlags {
            make: Some(String::from("sfx,voice")),
            tier: Some(String::from("lean")),
            comfy_url: Some(String::from("http://box:8188")),
            yes: true,
        };
        let (make, hardware) = ask(&flags).expect("flags");
        assert_eq!(make.chosen(), vec![MakeKind::Sfx, MakeKind::Voice]);
        assert_eq!(hardware.tier, Some(Tier::Lean));
        assert_eq!(hardware.comfy_url, "http://box:8188");

        let bad = MakeFlags {
            make: Some(String::from("widgets")),
            ..MakeFlags::default()
        };
        let error = ask(&bad).expect_err("refuse");
        assert!(
            error.message().contains("characters"),
            "{}",
            error.message()
        );
    }

    #[test]
    fn make_none_is_a_real_answer_and_says_what_to_do_next() {
        let flags = MakeFlags {
            make: Some(String::from("none")),
            tier: Some(String::from("fake")),
            ..MakeFlags::default()
        };
        let (make, _) = ask(&flags).expect("none");
        assert!(make.is_empty());
        assert!(make.backends().is_empty(), "every doctor row reads off");
    }
}
