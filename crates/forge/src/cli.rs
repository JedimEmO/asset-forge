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
    /// Every clip rebuilds from its own record, by bytes and by pose; every
    /// body is what it claims and conforms to the rig
    Audit(AuditArgs),
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
    /// Contact sheet of one clip on a body: poses across the clip, one band
    /// per view. Exits 1 when the clip drives no bone or never moves
    Sheet(SheetArgs),
    /// One mesh from the angles a reviewer would walk to: front, back, both
    /// sides and three head close-ups, culling off for a raw lift
    Views(ViewsArgs),
    /// Every view of a body, head row included, posed on the reference clip
    Turntable(TurntableArgs),
    /// Which bones a clip drives on a body — driven, at rest, orphaned — with
    /// no GPU. Exits 1 when nothing binds
    Bones(BonesArgs),
    /// Open the viewer window: library browser, stage, transport, metadata,
    /// audio
    Studio(StudioArgs),
    /// Serve the MCP tools over stdio (not yet: lands in P4)
    Mcp(Later),
}

/// `forge audit`.
#[derive(Debug, Args)]
pub(crate) struct AuditArgs {
    /// When a clip does not reproduce as recorded, search the in-place modes
    /// and a ladder of wrap blends and name the recipe that does. The
    /// failure stands either way.
    #[arg(long)]
    pub(crate) fit: bool,
}

/// What `sheet`, `turntable` and `bones` pose a clip on.
///
/// Unstated, the project's `stage_body`; a library without it falls back to
/// its first body with a warning, and one with no body at all to the fixture
/// mannequin written under out/ — a clip is never judged on an empty stage.
#[derive(Debug, Args)]
pub(crate) struct BodyArg {
    /// The body to pose on: a library name, a file name, or a path to a
    /// rigged glb anywhere.
    #[arg(long, value_name = "BODY")]
    pub(crate) body: Option<String>,
}

/// `forge sheet <clip>`.
#[derive(Debug, Args)]
pub(crate) struct SheetArgs {
    /// The clip: a library name, or a path to a clip glb under the asset
    /// root.
    pub(crate) clip: String,

    #[command(flatten)]
    pub(crate) body: BodyArg,

    /// Poses sampled across the window.
    #[arg(long, default_value_t = 8, value_name = "N")]
    pub(crate) frames: u32,
    /// Comma-separated views, one band each, or `all`: `three_quarter`,
    /// front, back, left, right, top.
    #[arg(long, default_value = "three_quarter", value_name = "LIST")]
    pub(crate) views: String,
    /// Cells per row.
    #[arg(long, default_value_t = 4, value_name = "N")]
    pub(crate) columns: u32,
    /// Cell pixels, clamped to fit the vision budget.
    #[arg(long, default_value = "384x512", value_name = "WxH")]
    pub(crate) cell: String,
    /// Window start, as a fraction of clip length.
    #[arg(long, default_value_t = 0.0, value_name = "0..1")]
    pub(crate) t0: f32,
    /// Window end, as a fraction of clip length.
    #[arg(long, default_value_t = 1.0, value_name = "0..1")]
    pub(crate) t1: f32,
    /// Add a band of head close-ups under the view bands.
    #[arg(long)]
    pub(crate) head_row: bool,
    /// Where to write the PNG. Default: out/sheets/<clip>.png.
    #[arg(long, value_name = "PNG")]
    pub(crate) out: Option<PathBuf>,
}

/// `forge views <name|path.glb>`.
#[derive(Debug, Args)]
pub(crate) struct ViewsArgs {
    /// A library body or model by name, or a path to any glb — a raw lift
    /// under out/, an export, a file from elsewhere.
    pub(crate) target: String,
    /// Switch back-face culling off on every material, so a missing rear
    /// surface shows as the inside of the front one. On by default for a
    /// path under the project's out/ — that is where raw lifts live.
    #[arg(long)]
    pub(crate) cull_off: bool,
    /// Leave the three head close-ups out — for a prop.
    #[arg(long)]
    pub(crate) no_head: bool,
    /// Comma-separated views, or `all`. Default: front, back, left, right.
    #[arg(long, value_name = "LIST")]
    pub(crate) views: Option<String>,
    /// Where to write the PNG. Default: out/views/<stem>.png.
    #[arg(long, value_name = "PNG")]
    pub(crate) out: Option<PathBuf>,
}

/// `forge turntable <body>`.
#[derive(Debug, Args)]
pub(crate) struct TurntableArgs {
    /// The body, by library name.
    pub(crate) body: String,
    /// Where to write the PNG. Default: out/views/<body>.png.
    #[arg(long, value_name = "PNG")]
    pub(crate) out: Option<PathBuf>,
}

/// `forge bones <clip>`.
#[derive(Debug, Args)]
pub(crate) struct BonesArgs {
    /// The clip: a library name, or a path to a clip glb under the asset
    /// root.
    pub(crate) clip: String,

    #[command(flatten)]
    pub(crate) body: BodyArg,
}

/// `forge studio`.
#[derive(Debug, Args)]
pub(crate) struct StudioArgs {
    /// The body or model to open on: a library name, a file name, or a path
    /// under the asset root. Default: the project's `stage_body`, else the
    /// first body, else the fixture mannequin.
    #[arg(long, value_name = "MODEL")]
    pub(crate) model: Option<String>,
    /// Open on the first sound rather than the first clip.
    #[arg(long)]
    pub(crate) audio: bool,
    /// Put a raw ARDY take on the stage body, beside the shipped clips.
    #[arg(long, value_name = "NPZ")]
    pub(crate) take: Option<PathBuf>,
    /// A recipe to apply to the take once, at load: a bare recipe or a
    /// sidecar holding one. Ignored without --take.
    #[arg(long, value_name = "JSON", requires = "take")]
    pub(crate) recipe: Option<PathBuf>,
    /// Capture the window to this PNG once the scene has settled, then quit.
    #[arg(long, value_name = "PNG")]
    pub(crate) screenshot: Option<PathBuf>,
    /// Play every audio asset in turn, then quit. Exits non-zero if one
    /// will not play.
    #[arg(long)]
    pub(crate) selftest: bool,
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
    /// Hold one rigged glb to the profile's contract: every bone at its
    /// depth, the rest pose, the weights, stature, feet, the reference clip
    /// binding. Exits 1 on any FAIL
    Check(RigCheckArgs),
}

/// `forge rig check <glb>`.
#[derive(Debug, Args)]
pub(crate) struct RigCheckArgs {
    /// The rigged glb to check — an export under out/, or a shipped body.
    pub(crate) glb: PathBuf,
    /// Also render it playing the reference clip (or at rest, when the
    /// library has none) to this PNG. Needs a wgpu adapter.
    #[arg(long, value_name = "PNG")]
    pub(crate) out: Option<PathBuf>,
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
    fn recipe_needs_a_take() {
        let parsed = Cli::try_parse_from(["forge", "studio", "--recipe", "r.json"]);
        assert!(parsed.is_err());
        let parsed =
            Cli::try_parse_from(["forge", "studio", "--take", "t.npz", "--recipe", "r.json"]);
        assert!(parsed.is_ok());
    }

    #[test]
    fn sheet_defaults_are_the_documented_ones() {
        let cli = Cli::try_parse_from(["forge", "sheet", "walk"]).expect("parses");
        let Command::Sheet(args) = cli.command else {
            panic!("not a sheet");
        };
        assert_eq!(args.frames, 8);
        assert_eq!(args.columns, 4);
        assert_eq!(args.views, "three_quarter");
        assert_eq!(args.cell, "384x512");
        assert!(!args.head_row);
        assert!(args.body.body.is_none());
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
