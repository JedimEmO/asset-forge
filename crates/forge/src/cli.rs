//! The flag table: every subcommand and every argument, as clap derives.
//!
//! Kept in one file so `forge --help` and this module read the same way.
//! Parsing stops here — a flag becomes a typed value, never a decision; the
//! decisions are in `commands/`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use forge_library::schema::{InPlaceMode, RootYMode};

/// Local generation and previews of game assets, for any game.
#[derive(Debug, Parser)]
#[command(
    name = "forge",
    version,
    about = "Make, judge and ship game assets: meshes, rigged characters, clips, audio — every generator local",
    long_about = "Make, judge and ship game assets — meshes, rigged characters, animation clips, \
                  audio — with every generator on your GPU and every output an engine-agnostic \
                  file plus one manifest.\n\n\
                  Exit codes: 0 ok; 1 a gate did not hold (verify, audit, a clipped sound); \
                  2 the call was refused as written (unknown kind, missing file, name in use, no \
                  project). A refusal says what does exist.",
    propagate_version = true
)]
pub(crate) struct Cli {
    /// The project root (the directory holding forge.toml). Default: walk
    /// up from the working directory to the first forge.toml.
    #[arg(long, global = true, value_name = "DIR")]
    pub(crate) project: Option<PathBuf>,

    #[command(subcommand)]
    pub(crate) command: Command,
}

/// Every verb.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Make a project here: forge.toml, the asset directories, the rig profile
    Init(InitArgs),
    /// What the library holds, one line per asset
    Catalog(CatalogArgs),
    /// Project the library into assets/library.json, or check the committed one
    Manifest(ManifestArgs),
    /// Every engine-free check: sidecars, hashes, the reference ledger, profile drift
    Verify,
    /// Every clip rebuilds from its own record; every body is what it claims
    Audit,
    /// Re-bake every shipped clip from its own record (bodies are skipped, loudly)
    Rebake(DryRunArgs),
    /// Bring every sidecar up to the current schema, idempotently
    Migrate(DryRunArgs),
    /// Ship an asset into the library: clip, body, model or audio
    #[command(subcommand)]
    Promote(PromoteDoor),
    /// Measure a sound, or every sound under a directory
    #[command(subcommand)]
    Audio(AudioCommand),
    /// The rig profile: export its contract, write its fixture mannequin
    #[command(subcommand)]
    Rig(RigCommand),
    /// Run a generator through the Python layer: mesh, prop, rig, export,
    /// rig-build, motion sweep|keys|review, sfx, music, speech, doctor
    Gen(GenArgs),
    /// What this machine can do: project, profile drift, library counts, host
    /// tools, every backend probed in its own environment
    Doctor(DoctorArgs),
    /// Who holds the GPU right now, and whether the largest backend would fit
    Gpu(GpuArgs),
    /// Open the viewer window (not yet: lands in P3)
    Studio(Later),
    /// Serve the MCP tools over stdio (not yet: lands in P4)
    Mcp(Later),
}

/// Arguments swallowed by a subcommand whose phase has not landed, so a
/// skill written against the final flag table is refused with the phase
/// rather than with a parse error.
#[derive(Debug, Args)]
pub(crate) struct Later {
    /// Ignored until the subcommand lands.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub(crate) rest: Vec<String>,
}

/// `forge gen <cmd> [args…]`: the command line is handed to
/// `python3 <toolkit>/python/forge_gen` whole, with `--project <root>` and
/// `--json` appended; the flags are that program's (`forge gen <cmd> --help`
/// shows them). Exit codes are its table: 0 ok, 2 usage, 3 missing backend,
/// 4 input rejected, 5 backend failed, 6 missing tool.
#[derive(Debug, Args)]
pub(crate) struct GenArgs {
    /// The forge-gen command and its arguments, verbatim. `--json` among
    /// them makes the last stdout line the JSON object instead of a summary;
    /// `--fake` (or `FORGE_FAKE=1`) writes placeholders that pass the same
    /// validators with no backend.
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        required = true,
        value_name = "CMD [ARGS]..."
    )]
    pub(crate) rest: Vec<String>,
}

/// `forge doctor`.
#[derive(Debug, Args)]
pub(crate) struct DoctorArgs {
    /// One JSON object instead of the table: the project, the profile, the
    /// library, and `forge gen doctor --json`'s report under "gen".
    #[arg(long)]
    pub(crate) json: bool,
    /// Skip the in-environment probes (seconds each) and report only what
    /// the directory says: found, missing, broken.
    #[arg(long)]
    pub(crate) quick: bool,
}

/// `forge gpu`.
#[derive(Debug, Args)]
pub(crate) struct GpuArgs {
    /// One JSON object instead of the lines.
    #[arg(long)]
    pub(crate) json: bool,
}

/// `forge init`.
#[derive(Debug, Args)]
pub(crate) struct InitArgs {
    /// The library's name, written into library.json. Default: the
    /// directory's name.
    #[arg(long)]
    pub(crate) name: Option<String>,
    /// A rig profile directory to install instead of the toolkit's own
    /// rigs/<rig>.
    #[arg(long, value_name = "DIR")]
    pub(crate) rig_dir: Option<PathBuf>,
}

/// `forge catalog`.
#[derive(Debug, Args)]
pub(crate) struct CatalogArgs {
    /// Restrict to one kind: clip, body, model, sfx, music, voice.
    #[arg(long)]
    pub(crate) kind: Option<String>,
    /// Case-insensitive substring of the name or the prompt.
    #[arg(long)]
    pub(crate) filter: Option<String>,
    /// Exact tag, case-insensitive.
    #[arg(long)]
    pub(crate) tag: Option<String>,
}

/// `forge manifest`.
#[derive(Debug, Args)]
pub(crate) struct ManifestArgs {
    /// Hold the committed manifest to a rebuild instead of writing it.
    #[arg(long)]
    pub(crate) check: bool,
}

/// `forge rebake`, `forge migrate`.
#[derive(Debug, Args)]
pub(crate) struct DryRunArgs {
    /// Report what would change without writing anything.
    #[arg(long, short = 'n')]
    pub(crate) dry_run: bool,
}

/// The four doors.
#[derive(Debug, Subcommand)]
pub(crate) enum PromoteDoor {
    /// Bake a take into a clip with a recipe, and file it
    Clip(Box<PromoteClipArgs>),
    /// File a rigged body .glb with its records
    Body(PromoteBodyArgs),
    /// File a static mesh .glb — a prop, a weapon — with its records
    Model(PromoteModelArgs),
    /// File a sound as sfx, music or voice
    Audio(PromoteAudioArgs),
}

/// What every door takes about who and why.
#[derive(Debug, Args)]
pub(crate) struct Curation {
    /// What the asset is, for the catalog. Unstated: the record's own prompt
    /// stands; nothing is invented.
    #[arg(long)]
    pub(crate) prompt: Option<String>,
    /// A curation tag; repeat for more. Unstated: a replaced asset's tags
    /// survive.
    #[arg(long = "tag", value_name = "TAG")]
    pub(crate) tags: Vec<String>,
    /// Anything the next reader should know.
    #[arg(long)]
    pub(crate) note: Option<String>,
    /// Who is promoting: human, agent:<name>, unknown.
    #[arg(long, default_value = "human", value_name = "WHO")]
    pub(crate) created_by: String,
    /// Replace an existing asset of this name. Refused otherwise.
    #[arg(long)]
    pub(crate) overwrite: bool,
}

/// `forge promote clip <take> <name>`.
#[derive(Debug, Args)]
pub(crate) struct PromoteClipArgs {
    /// The raw ARDY .npz take.
    pub(crate) take: PathBuf,
    /// The clip's name: lower-case letters, digits, underscores.
    pub(crate) name: String,

    /// Seconds cut from the start of the take.
    #[arg(long, value_name = "S")]
    pub(crate) trim_start: Option<f32>,
    /// Seconds cut from the end of the take.
    #[arg(long, value_name = "S")]
    pub(crate) trim_end: Option<f32>,
    /// Root travel treatment: off keeps it, strip pins the hips, detrend
    /// removes only the linear drift.
    #[arg(long, value_name = "off|strip|detrend", value_parser = parse_in_place)]
    pub(crate) in_place: Option<InPlaceMode>,
    /// Root height treatment, the same three words.
    #[arg(long, value_name = "off|strip|detrend", value_parser = parse_y_mode)]
    pub(crate) y_mode: Option<RootYMode>,
    /// Bake as a loop: the clip name gets -loop, the tail blends to frame 0
    /// over --loop-blend seconds.
    #[arg(long, conflicts_with = "no_loop")]
    pub(crate) r#loop: bool,
    /// Turn an inherited loop off.
    #[arg(long)]
    pub(crate) no_loop: bool,
    /// Seconds of tail blended toward frame 0. A value above zero makes the
    /// clip a loop; zero makes it not one.
    #[arg(long, value_name = "S")]
    pub(crate) loop_blend: Option<f32>,
    /// Scale on each arm joint's swing about its clip-mean pose. 1.0 is
    /// unchanged.
    #[arg(long, value_name = "X")]
    pub(crate) exaggerate: Option<f32>,
    /// Constant forearm bend, degrees — runner arms.
    #[arg(long, value_name = "DEG")]
    pub(crate) arm_bend: Option<f32>,
    /// Forward lean, degrees, split down the spine.
    #[arg(long, value_name = "DEG")]
    pub(crate) lean: Option<f32>,
    /// Upper-arm pull-back, degrees, so the hands ride beside the hips.
    #[arg(long, value_name = "DEG")]
    pub(crate) shoulder_back: Option<f32>,
    /// Piecewise time remap as src:dst,src:dst,… in seconds. An empty string
    /// removes an inherited one.
    #[arg(long, value_name = "SPEC")]
    pub(crate) retime: Option<String>,
    /// The animation's name inside the .glb, which is what an engine binds
    /// by. Default: the clip's name.
    #[arg(long, value_name = "NAME")]
    pub(crate) clip: Option<String>,
    /// An authored event as T:NAME, T in seconds on the raw take; add
    /// :KIND:SOUND to link a sound (0.4:swing:sfx:whoosh). Repeat for more.
    /// Footsteps are derived from the take and must not be stated.
    #[arg(long = "event", value_name = "T:NAME")]
    pub(crate) events: Vec<String>,
    /// The ARDY run's record (take.json). With it the provenance is
    /// recorded; without it, reconstructed.
    #[arg(long, value_name = "JSON")]
    pub(crate) record: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) curation: Curation,
}

/// `forge promote body <glb> <name>`.
#[derive(Debug, Args)]
pub(crate) struct PromoteBodyArgs {
    /// The exported, self-contained .glb.
    pub(crate) glb: PathBuf,
    /// The body's name.
    pub(crate) name: String,
    /// The committed .blend it was exported from, under the project.
    #[arg(long, value_name = "BLEND")]
    pub(crate) blend: Option<PathBuf>,
    /// The TRELLIS.2 lift record (<name>.lift.json).
    #[arg(long, value_name = "JSON")]
    pub(crate) lift_record: Option<PathBuf>,
    /// The auto-rig's record.
    #[arg(long, value_name = "JSON")]
    pub(crate) rig_record: Option<PathBuf>,
    /// The export's record.
    #[arg(long, value_name = "JSON")]
    pub(crate) export_record: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) curation: Curation,
}

/// `forge promote model <glb> <name>`.
#[derive(Debug, Args)]
pub(crate) struct PromoteModelArgs {
    /// The normalized .glb: metres, origin where the profile says, matte.
    pub(crate) glb: PathBuf,
    /// The model's name.
    pub(crate) name: String,
    /// A committed .blend, when the prop has one.
    #[arg(long, value_name = "BLEND")]
    pub(crate) blend: Option<PathBuf>,
    /// The TRELLIS.2 lift record (<name>.lift.json).
    #[arg(long, value_name = "JSON")]
    pub(crate) lift_record: Option<PathBuf>,
    /// The prop normalizer's record.
    #[arg(long, value_name = "JSON")]
    pub(crate) prop_record: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) curation: Curation,
}

/// `forge promote audio <kind> <file> <name>`.
#[derive(Debug, Args)]
pub(crate) struct PromoteAudioArgs {
    /// sfx, music or voice.
    pub(crate) kind: String,
    /// The sound: wav, ogg, mp3 or flac.
    pub(crate) file: PathBuf,
    /// The sound's name — one name across every audio kind.
    pub(crate) name: String,
    /// The generator run's record. Without it the provenance is unknown.
    #[arg(long, value_name = "JSON")]
    pub(crate) record: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) curation: Curation,
}

/// `forge audio`.
#[derive(Debug, Subcommand)]
pub(crate) enum AudioCommand {
    /// Measure one file and, with --out, draw it. Exits 1 if silent or clipped
    Inspect(AudioInspectArgs),
    /// Measure every sound under a directory. Exits 1 if any is defective
    List(AudioListArgs),
}

/// `forge audio inspect <file>`.
#[derive(Debug, Args)]
pub(crate) struct AudioInspectArgs {
    /// The sound to measure.
    pub(crate) file: PathBuf,
    /// Write the waveform-and-spectrogram plot here.
    #[arg(long, value_name = "PNG")]
    pub(crate) out: Option<PathBuf>,
    /// Plot width in pixels.
    #[arg(long, value_name = "PX")]
    pub(crate) width: Option<u32>,
}

/// `forge audio list [dir]`.
#[derive(Debug, Args)]
pub(crate) struct AudioListArgs {
    /// The directory to walk. Default: the project's assets/audio.
    pub(crate) dir: Option<PathBuf>,
}

/// `forge rig`.
#[derive(Debug, Subcommand)]
pub(crate) enum RigCommand {
    /// Derive contract.json from a profile's rig.glb and its motion skeleton
    ExportContract(ExportContractArgs),
    /// Write the fixture mannequin: a capsule figure skinned to the contract
    Fixture(FixtureArgs),
}

/// `forge rig export-contract <dir>`.
///
/// The scalars the artifact cannot decide default to what the directory's
/// existing `contract.json` says, so a re-export keeps them; the toolkit's
/// humanoid values stand in when there is no contract yet.
#[derive(Debug, Args)]
pub(crate) struct ExportContractArgs {
    /// The profile directory holding rig.glb and `motion_skeleton.json`.
    pub(crate) dir: PathBuf,
    /// The profile's name.
    #[arg(long)]
    pub(crate) name: Option<String>,
    /// The contract's version.
    #[arg(long)]
    pub(crate) version: Option<u32>,
    /// The axis the rig faces in rest, e.g. +Z.
    #[arg(long)]
    pub(crate) front: Option<String>,
    /// Reference stature, metres.
    #[arg(long, value_name = "M")]
    pub(crate) stature: Option<f32>,
    /// Shortest body the contract accepts, metres.
    #[arg(long, value_name = "M")]
    pub(crate) stature_min: Option<f32>,
    /// Tallest body the contract accepts, metres.
    #[arg(long, value_name = "M")]
    pub(crate) stature_max: Option<f32>,
    /// How far from the ground the lowest vertex may sit, metres.
    #[arg(long, value_name = "M")]
    pub(crate) foot_tolerance: Option<f32>,
    /// Rest-rotation tolerance when a body is held to the contract.
    #[arg(long)]
    pub(crate) rest_rotation_tolerance: Option<f32>,
    /// The clip a body is bound to when the rig is checked.
    #[arg(long)]
    pub(crate) reference_clip: Option<String>,
    /// The rig glb, relative to the profile directory.
    #[arg(long, value_name = "FILE")]
    pub(crate) glb: Option<String>,
    /// The rig blend, relative to the profile directory.
    #[arg(long, value_name = "FILE")]
    pub(crate) blend: Option<String>,
}

/// `forge rig fixture <out.glb>`.
#[derive(Debug, Args)]
pub(crate) struct FixtureArgs {
    /// Where to write the mannequin.
    pub(crate) out: PathBuf,
    /// A profile directory to build it from instead of the project's.
    #[arg(long, value_name = "DIR")]
    pub(crate) rig_dir: Option<PathBuf>,
}

/// Read an `--in-place` word, refusing with the three valid ones.
fn parse_in_place(text: &str) -> Result<InPlaceMode, String> {
    InPlaceMode::parse(text).ok_or_else(|| format!("{text:?} is not off, strip or detrend"))
}

/// Read a `--y-mode` word, refusing with the three valid ones.
fn parse_y_mode(text: &str) -> Result<RootYMode, String> {
    RootYMode::parse(text).ok_or_else(|| format!("{text:?} is not off, strip or detrend"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_is_well_formed() {
        use clap::CommandFactory as _;
        Cli::command().debug_assert();
    }

    #[test]
    fn loop_and_no_loop_conflict() {
        let parsed = Cli::try_parse_from([
            "forge",
            "promote",
            "clip",
            "t.npz",
            "walk",
            "--loop",
            "--no-loop",
        ]);
        assert!(parsed.is_err());
    }

    #[test]
    fn in_place_refuses_a_fourth_word() {
        let parsed = Cli::try_parse_from([
            "forge",
            "promote",
            "clip",
            "t.npz",
            "walk",
            "--in-place",
            "sideways",
        ]);
        let error = parsed.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("off, strip or detrend"), "{error}");
    }
}
