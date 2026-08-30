//! Setting up, from a client with no shell: `init_project`, `licences`,
//! `setup`.
//!
//! The three doors a stranger's agent needs before it can make anything.
//! They run in this process through [`forge_library`] — the same functions
//! `forge init` and `forge setup` call — so a project an agent made and a
//! project a person made are the same project, down to the ledger's
//! wording.
//!
//! # The gate
//!
//! `setup` refuses a kind whose licences have not been accepted **by
//! name**, and the refusal lists exactly the ids that are missing and the
//! sentence that fixes the call. That is the gate: a tool that cannot
//! succeed, rather than a prompt asking an agent to behave. `licences`
//! exists so the ids can be accepted knowingly — it returns each notice in
//! full, because an agent cannot accept what it was not shown, and a
//! summary is not the thing anybody is agreeing to.
//!
//! The `DINOv3` weights are gated behind a token only a human holds. The
//! tool says so and stops with the two commands to run; there is no
//! argument that gets past it, and that is deliberate.
//!
//! Every refusal here is a successful frame carrying an error result
//! ([`util::refuse`]), never an `Err(ErrorData)`: a client renders the
//! latter opaquely, the agent learns nothing, and the next turn is the same
//! call again.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use forge_library::Project;
use forge_library::project::{
    Hardware, Licence, MakeKind, MakeKinds, PROJECT_FILE, SetupPlan, Tier, licences,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::server::ForgeServer;
use crate::util;

/// How an acceptance made through this server is recorded. The receipt has
/// to be able to answer "who agreed to this", and "an agent, on this date,
/// through the MCP" is a different answer from "a human at a terminal".
pub(crate) const ACTOR: &str = "agent:claude";

/// This file's tools, for the server to sum.
pub(crate) fn router() -> ToolRouter<ForgeServer> {
    ForgeServer::setup_router()
}

/// The six kinds, as an object an agent fills in.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct MakeArg {
    /// Static props from a reference image (trellis2 + `qwen_image`).
    #[serde(default)]
    pub(crate) props: bool,
    /// Rigged characters from a reference image (+ skintokens).
    #[serde(default)]
    pub(crate) characters: bool,
    /// Animation clips from a prompt (ardy).
    #[serde(default)]
    pub(crate) clips: bool,
    /// Sound effects (`moss_sfx`, in the comfy executor).
    #[serde(default)]
    pub(crate) sfx: bool,
    /// Music (acestep, in the comfy executor).
    #[serde(default)]
    pub(crate) music: bool,
    /// Spoken lines and the voices they are cloned from (`moss_tts`).
    #[serde(default)]
    pub(crate) voice: bool,
}

impl From<MakeArg> for MakeKinds {
    fn from(arg: MakeArg) -> Self {
        Self {
            props: arg.props,
            characters: arg.characters,
            clips: arg.clips,
            sfx: arg.sfx,
            music: arg.music,
            voice: arg.voice,
        }
    }
}

/// `init_project`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct InitArgs {
    /// The directory to make the project in. It is created if it is not
    /// there.
    pub(crate) path: String,
    /// The library's name, written into `library.json`. Omitted, the
    /// directory's name.
    pub(crate) name: Option<String>,
    /// What you will make here. Omitted, props + characters + clips.
    pub(crate) make: Option<MakeArg>,
    /// `full` (24 GB card), `lean` (16 GB) or `fake` (no card: every
    /// generator writes a branded placeholder through the same
    /// validators). Omitted, detected from `nvidia-smi`.
    pub(crate) tier: Option<String>,
    /// Where the `ComfyUI` service answers. Omitted,
    /// `http://127.0.0.1:8188`; another machine's is fine.
    pub(crate) comfy_url: Option<String>,
    /// `true` to re-answer the three questions for a directory that is
    /// already a project. Only `[make]` and `[hardware]` are rewritten;
    /// every other line, comments included, is left exactly as it was.
    #[serde(default)]
    pub(crate) adopt: bool,
}

/// `licences`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct LicencesArgs {
    /// Which kinds to show the licences for: any of props, characters,
    /// clips, sfx, music, voice. Omitted, everything this project's
    /// `[make]` chose.
    pub(crate) kinds: Option<Vec<String>>,
}

/// `setup`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct SetupToolArgs {
    /// Which kinds to install for. Omitted, everything this project's
    /// `[make]` chose.
    pub(crate) kinds: Option<Vec<String>>,
    /// One backend by name, instead of whole kinds — for repairing a single
    /// install that doctor calls partial or broken.
    pub(crate) backend: Option<String>,
    /// The licence ids you accept, by name, after `licences` showed you
    /// their text. Every id whose `needs_accept` is true must be here or
    /// the call is refused and nothing is installed.
    #[serde(default)]
    pub(crate) accept: Vec<String>,
    /// `true` makes the environments now and leaves every weight to the
    /// first generate. Doctor reads `partial` until they arrive.
    #[serde(default)]
    pub(crate) no_models: bool,
    /// `true` returns the plan and stops: nothing fetched, nothing
    /// accepted, nothing recorded.
    #[serde(default)]
    pub(crate) dry_run: bool,
}

#[tool_router(router = setup_router, vis = "pub(crate)")]
impl ForgeServer {
    /// Make a project.
    #[tool(
        description = "Make a forge project in a directory: forge.toml with what you make and \
                       what card this is, the asset directories, the reference ledger, the rig \
                       profile and an empty manifest — so verify and manifest --check pass on \
                       it immediately. The three answers are phrased as what you MAKE (props, \
                       characters, clips, sfx, music, voice) and what CARD you have (full 24 \
                       GB, lean 16 GB, fake no card), never as model names. Everything after \
                       reads them: setup installs only what a chosen kind needs, and doctor \
                       prints `off` for a kind you did not choose rather than probing it. Tier \
                       fake is a first-class answer — every generator writes a branded \
                       placeholder through the same validators — and chooses no backend at \
                       all. REFUSES a directory that is already a project unless adopt:true, \
                       which rewrites only [make] and [hardware] and leaves every other line \
                       alone. Returns the forge.toml that was written and the next step."
    )]
    async fn init_project(&self, Parameters(args): Parameters<InitArgs>) -> CallToolResult {
        let root = PathBuf::from(&args.path);
        let make: MakeKinds = args.make.map_or(MakeKinds::DEFAULT, Into::into);
        let tier = match args.tier.as_deref() {
            Some(word) => match Tier::parse(word) {
                Ok(tier) => tier,
                Err(err) => return util::refuse(err.to_string()),
            },
            None => Tier::detect(),
        };
        let hardware = Hardware {
            tier: Some(tier),
            comfy_url: args
                .comfy_url
                .unwrap_or_else(|| String::from(forge_library::project::DEFAULT_COMFY_URL)),
        };

        // An existing project is only re-answered when the caller said so.
        // Rewriting a forge.toml somebody wrote, uninvited, is the one
        // thing this door must not do.
        if root.join(PROJECT_FILE).is_file() {
            if !args.adopt {
                return util::refuse(format!(
                    "{} is already a project — nothing was written. Pass adopt:true to \
                     re-answer the three questions, which rewrites only [make] and \
                     [hardware] and leaves every other line, comments included, exactly as \
                     it is.",
                    root.join(PROJECT_FILE).display()
                ));
            }
            let project = match Project::load(&root) {
                Ok(project) => project,
                Err(err) => return util::refuse(err.to_string()),
            };
            return match project.set_make_hardware(make, &hardware) {
                Ok(project) => util::report(format!(
                    "adopted {}\n  {}\n{}\n{}next: {}",
                    project.root.display(),
                    project.answered_line(),
                    toml_tables(&project),
                    self.reconnect_note(&project.root),
                    project.next_step()
                )),
                Err(err) => util::refuse(err.to_string()),
            };
        }

        let name = match args.name {
            Some(name) => name,
            None => match root
                .canonicalize()
                .ok()
                .and_then(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
                .or_else(|| {
                    root.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .filter(|n| !n.is_empty())
                }) {
                Some(name) => name,
                None => {
                    return util::refuse(format!(
                        "{} has no name to take — pass name",
                        root.display()
                    ));
                }
            },
        };
        let profile = self.profile_dir(forge_library::project::DEFAULT_RIG);
        if profile.is_none() {
            return util::refuse(format!(
                "no rig profile to install: this server cannot find the toolkit's \
                 rigs/{} from its own executable, so a project made here would be \
                 half-made and the very next verify would fail. Set {} to the \
                 asset-forge checkout and restart the server.",
                forge_library::project::DEFAULT_RIG,
                forge_library::backends::HOME_ENV
            ));
        }
        match forge_library::project::create(&root, &name, make, &hardware, profile.as_deref()) {
            Ok((project, lines)) => util::report(format!(
                "{}\n{}\n{}next: {}",
                lines.join("\n  "),
                toml_tables(&project),
                self.reconnect_note(&project.root),
                project.next_step()
            )),
            Err(err) => util::refuse(err.to_string()),
        }
    }

    /// What a caller has to do before this session can work in the project
    /// it just made — when it is not the one this server is serving.
    ///
    /// A running server holds one project, settled at startup: it cannot
    /// follow `init_project` to another directory, and it never said so. A
    /// project was made from the toolkit's own server and every tool after
    /// it went on answering about the toolkit (2026-08-30).
    fn reconnect_note(&self, made: &std::path::Path) -> String {
        if self.config.project_found && made == self.config.project.root {
            return String::new();
        }
        let serving = if self.config.project_found {
            format!(
                "this server is still serving {}",
                self.config.project.root.display()
            )
        } else {
            String::from("this server started with no project and holds none")
        };
        format!(
            "NOTE: {serving}. Reconnect it with `--project {}` (or FORGE_PROJECT={}) to work \
             there; until then every tool but init_project, licences and doctor answers about \
             what this server was started with.\n",
            made.display(),
            made.display()
        )
    }

    /// What the chosen kinds ask of you, in full.
    #[tool(
        description = "The licences the chosen kinds carry, each with ITS WHOLE NOTICE — the \
                       text, not a summary, because you cannot accept what you were not \
                       shown. Per component: the id `setup`'s accept takes, what it is \
                       called, which backend carries it, whether it needs your yes, and the \
                       URL of the complete legal text. Call this BEFORE setup: setup refuses \
                       until accept names every id whose needs_accept is true, and the ids \
                       are here. Some facts are told rather than asked — ComfyUI's GPL, the \
                       SkinTokens encoder question — and those have needs_accept false; they \
                       still travel with every asset you make."
    )]
    async fn licences(&self, Parameters(args): Parameters<LicencesArgs>) -> CallToolResult {
        let kinds = match self.kinds_of(args.kinds.as_deref()) {
            Ok(kinds) => kinds,
            Err(refusal) => return refusal,
        };
        let plan = SetupPlan::for_kinds(&kinds, self.current_project().tier());
        if plan.licences.is_empty() {
            return util::report(format!(
                "no licence here asks anything of you.\nkinds: {}\n(that is the whole \
                 answer: nothing in {} carries a licence fact you have to weigh)",
                label(&kinds),
                label(&kinds)
            ));
        }
        let receipt = match licences::load() {
            Ok(receipt) => receipt,
            Err(err) => return util::refuse(err.to_string()),
        };
        let mut out = format!(
            "{} licence fact(s) for {} — the whole notice each time.\nreceipt: {}\n\n",
            plan.licences.len(),
            label(&kinds),
            licences::path().display()
        );
        for licence in &plan.licences {
            let state = if receipt.has(licence.id) {
                let row = receipt.row(licence.id).expect("just checked");
                format!(
                    "already accepted by {} at {} ({})",
                    row.by,
                    row.at,
                    row.via.as_str()
                )
            } else if licence.needs_accept {
                String::from("NEEDS YOUR YES — pass this id in setup's accept")
            } else {
                String::from("told, not asked — nothing waits on it")
            };
            let _ = write!(
                out,
                "── id: {}\n   name: {}\n   backend: {}\n   needs_accept: {}\n   {}\n\n{}\n\n\
                 full text: {}\n\n",
                licence.id,
                licence.name,
                licence.backend,
                licence.needs_accept,
                state,
                licence.terms,
                licence.full_text
            );
        }
        let missing = plan.missing_accepts(&receipt, &[]);
        if missing.is_empty() {
            out.push_str("every licence that needs a yes already has one; setup will proceed.\n");
        } else {
            let _ = writeln!(
                out,
                "to install: setup(kinds: [{}], accept: [{}])",
                kinds
                    .iter()
                    .map(|kind| format!("\"{kind}\""))
                    .collect::<Vec<_>>()
                    .join(", "),
                missing
                    .iter()
                    .map(|licence| format!("\"{}\"", licence.id))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        util::report(out)
    }

    /// Install what the chosen kinds need, after the gate.
    #[tool(
        description = "Plan the install the chosen kinds need, hold it to its licence gate, \
                       and record what you accept. IT DOES NOT INSTALL ANYTHING — the last \
                       line of a cleared call is the one command a human runs, and there is \
                       no tool that runs it for you. What it returns is the plan: per kind \
                       the backends, what each costs on disk, the total, and every licence \
                       fact they carry, because nothing should download tens of GB on a call \
                       nobody read. REFUSED unless accept names every licence id whose \
                       needs_accept is true; the refusal lists exactly the ids that are \
                       missing, and `licences` is where their text is. An accepted id is \
                       written to this machine's receipt as soon as this call clears, so the \
                       human's install will not ask again. dry_run:true plans and records \
                       nothing. The DINOv3 weights are gated behind a Hugging Face token that \
                       only a human holds — no argument gets past that, and the answer names \
                       the two commands a human runs."
    )]
    async fn setup(&self, Parameters(args): Parameters<SetupToolArgs>) -> CallToolResult {
        let kinds = match self.kinds_of(args.kinds.as_deref()) {
            Ok(kinds) => kinds,
            Err(refusal) => return refusal,
        };
        let plan = SetupPlan::for_kinds(&kinds, self.current_project().tier());
        if plan.kinds.is_empty() && args.backend.is_none() {
            return util::report(plan.screen());
        }
        if let Some(name) = &args.backend
            && !plan.backends.iter().any(|need| need.name == *name)
        {
            return util::refuse(format!(
                "{name:?} is not a backend the chosen kinds need.\nfor {}: {}",
                label(&kinds),
                plan.backends
                    .iter()
                    .map(|need| need.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        let receipt = match licences::load() {
            Ok(receipt) => receipt,
            Err(err) => return util::refuse(err.to_string()),
        };
        let missing = plan.missing_accepts(&receipt, &args.accept);
        if !missing.is_empty() {
            // The gate. A refusal, not a prompt: this call cannot succeed
            // until the ids are in it, and the ids are in the message.
            return util::refuse(format!(
                "{}\n\n{}",
                SetupPlan::refusal(&missing),
                plan.screen()
            ));
        }
        let unknown: Vec<&String> = args
            .accept
            .iter()
            .filter(|id| forge_library::project::licence(id).is_none())
            .collect();
        if !unknown.is_empty() {
            return util::refuse(format!(
                "accept names {} licence(s) this toolkit does not know: {}.\nthe ids are: \
                 {}\nnothing was installed and nothing was recorded.",
                unknown.len(),
                unknown
                    .iter()
                    .map(|id| format!("{id:?}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                forge_library::project::LICENCES
                    .iter()
                    .map(|licence| licence.id)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if args.dry_run {
            return util::report(format!(
                "{}\ndry_run: nothing was fetched, nothing was accepted, nothing was \
                 recorded.",
                plan.screen()
            ));
        }

        let accepted: Vec<&str> = plan
            .required()
            .into_iter()
            .filter(|licence| args.accept.iter().any(|id| id == licence.id))
            .map(|licence| licence.id)
            .collect();
        let mut out = plan.screen();
        if !accepted.is_empty() {
            match licences::accept(&accepted, ACTOR, licences::Via::Mcp) {
                Ok((_, added)) if added.is_empty() => {}
                Ok((_, added)) => {
                    let _ = writeln!(
                        out,
                        "\naccepted and recorded in {} as {ACTOR}: {}",
                        licences::path().display(),
                        added.join(", ")
                    );
                }
                Err(err) => return util::refuse(err.to_string()),
            }
        }
        if let Some(gated) = plan
            .licences
            .iter()
            .find(|licence| licence.id == "dinov3" && !hf_token_present())
        {
            let _ = write!(
                out,
                "\nSTOPPED before installing: {} is gated, and the token that proves the \
                 acceptance is a thing only a human holds. Two commands, and neither can be \
                 run for you:\n  1. accept at {}\n  2. hf auth login --token <tok>\n\
                 Never the interactive `hf auth login`: with no TTY it hangs. Ask the person \
                 you are working with to run those, then call setup again.\n",
                gated.id, gated.full_text
            );
            return util::report(out);
        }

        // **This tool installs nothing, and says so.** `serve.md` §7 has it
        // returning a job; the queue schedules `forge gen` command lines,
        // and an installer is not one — making it a job means a third
        // executor shape with its own log, exit-code and cancel story, and
        // that is a decision of its own rather than a line added at the end
        // of a fix. Until it is taken, the honest frame is this: the gate
        // is what the tool is for, the acceptance is recorded where the
        // installs are, and the install is a command a human runs.
        let _ = write!(
            out,
            "\nthe gate is clear — every licence that needs a yes has one, and the \
             acceptance is recorded. NOTHING HAS BEEN INSTALLED: this tool plans and \
             gates, and the install is a command a person runs in a shell —\n  forge setup \
             {}{}{}\nit skips every backend doctor already calls ok, so it is safe to \
             re-run, and it will not ask about the licences above again. Ask the person you \
             are working with to run it, then call doctor to see what landed.\n",
            label(&kinds).replace(", ", " "),
            accepted.iter().fold(String::new(), |mut line, id| {
                let _ = write!(line, " --yes {id}");
                line
            }),
            if args.no_models { " --no-models" } else { "" }
        );
        util::report(out)
    }
}

impl ForgeServer {
    /// The kinds a call is about: the ones named, else the project's own.
    fn kinds_of(&self, named: Option<&[String]>) -> Result<Vec<MakeKind>, CallToolResult> {
        let Some(named) = named else {
            return Ok(self.current_project().make.chosen());
        };
        MakeKinds::parse_list(&named.join(","))
            .map(|make| make.chosen())
            .map_err(|err| util::refuse(err.to_string()))
    }

    /// The project as it is on disk *now*.
    ///
    /// `init_project` with `adopt: true` rewrites `[make]` and
    /// `[hardware]` inside a live session, and the copy the server read at
    /// startup is then stale — a `setup` that went on printing the old tier
    /// would be telling the caller something that stopped being true one
    /// call ago. A file that has since become unreadable falls back to the
    /// startup copy rather than refusing: the answer is still mostly right,
    /// and doctor is the door that says a project is broken.
    fn current_project(&self) -> Project {
        Project::load(&self.config.project.root).unwrap_or_else(|_| self.config.project.clone())
    }

    /// The toolkit's rig profile directory, when this executable can find
    /// the checkout.
    fn profile_dir(&self, rig: &str) -> Option<PathBuf> {
        let dir = self.config.toolkit.as_ref()?.join("rigs").join(rig);
        dir.is_dir().then_some(dir)
    }
}

/// The two tables as they were written, so the answer shows the file rather
/// than describing it.
fn toml_tables(project: &Project) -> String {
    format!("{}\n{}", project.make.to_toml(), project.hardware.to_toml())
}

/// Whether a Hugging Face token is stored or exported. The same rule
/// `python/forge_gen/doctor.py` uses, because the two must not disagree
/// about whether a human has logged in.
fn hf_token_present() -> bool {
    if std::env::var_os("HF_TOKEN").is_some() {
        return true;
    }
    let path = if let Some(explicit) = std::env::var_os("HF_TOKEN_PATH") {
        PathBuf::from(explicit)
    } else if let Some(home) = std::env::var_os("HF_HOME") {
        Path::new(&home).join("token")
    } else if let Some(home) = std::env::var_os("HOME") {
        Path::new(&home).join(".cache/huggingface/token")
    } else {
        return false;
    };
    path.is_file()
}

/// The kinds, as a sentence.
fn label(kinds: &[MakeKind]) -> String {
    if kinds.is_empty() {
        return String::from("nothing");
    }
    kinds
        .iter()
        .map(|kind| kind.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Kept so the licence table cannot be trimmed without this file noticing.
#[allow(dead_code)]
const KNOWN_LICENCE: fn(&str) -> Option<&'static Licence> = forge_library::project::licence;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_refuses_a_gated_kind_without_accept() {
        // The refusal is the gate. It has to name the ids — an agent that
        // is told "no" without being told which words to say next burns a
        // turn and then repeats the same call.
        let plan = SetupPlan::for_kinds(&[MakeKind::Characters], Tier::Full);
        let missing = plan.missing_accepts(&licences::Receipt::default(), &[]);
        let refusal = SetupPlan::refusal(&missing);
        assert!(refusal.contains("nvdiffrast"), "{refusal}");
        assert!(refusal.contains("dinov3"), "{refusal}");
        assert!(
            refusal.contains("call licences first and pass each id in accept"),
            "{refusal}"
        );
        assert!(refusal.contains("nothing was installed"), "{refusal}");

        // sfx carries a fact that is told, not asked, so it is not gated.
        let sfx = SetupPlan::for_kinds(&[MakeKind::Sfx], Tier::Fake);
        assert!(
            sfx.missing_accepts(&licences::Receipt::default(), &[])
                .is_empty(),
            "sfx must not be gated: its one licence fact needs no yes"
        );
        assert!(
            sfx.licences
                .iter()
                .any(|licence| licence.id == "comfyui_gpl"),
            "and it is still shown"
        );

        // Naming one id leaves the other named.
        let missing =
            plan.missing_accepts(&licences::Receipt::default(), &[String::from("nvdiffrast")]);
        assert_eq!(
            missing.iter().map(|l| l.id).collect::<Vec<_>>(),
            vec!["dinov3"]
        );
    }

    #[test]
    fn the_make_argument_is_the_same_six_kinds_the_toml_has() {
        let arg = MakeArg {
            sfx: true,
            voice: true,
            ..MakeArg::default()
        };
        let make: MakeKinds = arg.into();
        assert_eq!(make.chosen(), vec![MakeKind::Sfx, MakeKind::Voice]);
        assert_eq!(make.backends(), vec!["comfy", "moss_sfx", "moss_tts"]);
        assert_eq!(MakeKinds::from(MakeArg::default()), MakeKinds::none());
    }
}
