//! Shipping what was made: a take baked into a clip, a sound copied in.
//!
//! Both doors are direct — there is no review queue — and both write the
//! library through [`forge_library::promote`] in this process: the native
//! bake, the same function `forge promote clip` calls, so what an agent
//! ships is byte-for-byte what a human at the terminal would have shipped
//! with the same arguments. What stays here is the conversation: reading
//! the caller's knobs onto the recipe, refusing an existing name unless
//! `overwrite` was said, and echoing every recipe involved.
//!
//! # The overlay rule
//!
//! When the name already exists, the shipped clip's recorded recipe is the
//! starting point and only the knobs the caller *stated* are replaced —
//! [`forge_library::schema::overlay_recipe`] is the one implementation, and
//! the CLI uses the same one. An unstated knob is read from the shipped
//! record, never defaulted: the failure this guards against was a
//! re-promote that looked plausible right up until somebody watched it,
//! because "unstated" had reached a generator as "absent" and absent meant
//! "whatever the old clip had". The response carries both recipes in full
//! under the headings [`SHIPPED_RECIPE_HEADING`] (what the library held
//! before this call) and [`EFFECTIVE_RECIPE_HEADING`] (what was baked
//! now), so the caller sees what it replaced rather than assuming.
//!
//! # The mesh door, and what actually guards it
//!
//! `promote_body` and `promote_model` are here, and the first form of this
//! server deliberately had neither. The rule went because the human was
//! never absent: the harness that issues every one of these calls is a
//! human in the loop, which is the same ground "no review queue" already
//! stands on, and a doorman who only makes the agent ask a human to type
//! the command the agent composed guards nothing. What guards the library
//! is the gates — the export gate on the file, `forge rig check` on the
//! walk, and a taken name that must be told `overwrite` and then echoes
//! what it replaced. `promote_body` runs all three, and none of them is
//! weaker for being called by an agent.
//!
//! What stays un-automatable is a different thing entirely: accepting a
//! licence. And the eye is the other — "look before you promote" is not a
//! gate and never was, which is why `render_model` exists beside this door
//! rather than instead of it.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use forge_library::generator_record::RecordKind;
use forge_library::promote::{
    PromoteAudio, PromoteBody, PromoteClip, PromoteModel, Promoted, promote_audio, promote_body,
    promote_clip, promote_model, validate_name,
};
use forge_library::schema::{
    Actor, AnimEvent, AudioRef, ClipRecipe, DEFAULT_FPS, EventOrigin, InPlaceMode, PartialRecipe,
    RootYMode, overlay_recipe, parse_retime, valid_event_name,
};
use forge_library::{AssetRecord, Catalog, GeneratorRecord, Kind, Project};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::server::ForgeServer;
use crate::util;

/// How this server names itself in every record it writes.
///
/// Recorded rather than inferred: a library where half the assets say
/// "unknown" cannot answer "which of these did an agent make", which is the
/// first question a reviewer asks.
pub(crate) const ACTOR: &str = "agent:claude";

/// The bake is native and fast; the ceiling bounds a stuck disk or a
/// pathological take, not a generator launch.
const PROMOTE_TIMEOUT: Duration = Duration::from_mins(5);

/// A rig check spawns a headless Bevy app, loads a body and a clip and
/// walks the clip on the CPU: seconds on any machine. The ceiling bounds a
/// binary that never comes back, not the work.
const RIG_CHECK_TIMEOUT: Duration = Duration::from_mins(5);

/// The heading over the recipe the library held before an overwrite.
pub(crate) const SHIPPED_RECIPE_HEADING: &str = "shipped recipe";
/// The heading over the recipe a promote baked with.
pub(crate) const EFFECTIVE_RECIPE_HEADING: &str = "effective recipe";

/// This file's tools, for the server to sum.
pub(crate) fn router() -> ToolRouter<ForgeServer> {
    ForgeServer::promote_router()
}

/// A rigged body or a normalised model to file, with the records that say
/// how it was made.
///
/// One argument type for both doors: `prop_record` is a model's and
/// `rig_record`/`export_record` are a body's, and each is refused when it
/// names a run of the wrong kind. Splitting them into two types would give
/// an agent two schemas to learn for one shape of work and would not stop
/// the mistake either type is guarding against.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PromoteBodyArgs {
    /// `snake_case` name for the asset, e.g. `vex_runner`. Becomes the file
    /// stem and the key everything else refers to it by.
    pub(crate) name: String,
    /// Path to the exported, self-contained `.glb`. A relative path is read
    /// against the project root.
    pub(crate) glb: String,
    /// The committed `.blend` it was exported from, under the project. It
    /// is the provenance claim: hashed into the record, and `verify`
    /// expects it to still be there.
    pub(crate) blend: Option<String>,
    /// The TRELLIS.2 lift's record (`<name>.lift.json`). With it the
    /// generator block is recorded; without it there is none, and the
    /// record says `reconstructed`, which is the truth.
    pub(crate) lift_record: Option<String>,
    /// The skinner's record (`<name>.rig.json`) — a body only.
    pub(crate) rig_record: Option<String>,
    /// The export's record — a body only.
    pub(crate) export_record: Option<String>,
    /// The prop normaliser's record — a model only.
    pub(crate) prop_record: Option<String>,
    /// What it is, in plain English. Omitted, the lift's own prompt stands;
    /// nothing is invented.
    pub(crate) prompt: Option<String>,
    /// Curation tags. Omitted, an existing asset's tags survive the
    /// re-promote.
    pub(crate) tags: Option<Vec<String>>,
    /// Anything the next reader should know.
    pub(crate) note: Option<String>,
    /// Allow replacing an existing asset of this name. Default false,
    /// because guessing a name that is taken costs somebody else's asset.
    pub(crate) overwrite: Option<bool>,
}

/// Which take to keep, what to call it, and how to cut it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PromoteClipArgs {
    /// `snake_case` name for the clip, e.g. `wave`. Becomes the file stem
    /// and the asset key. If a clip of this name exists, its recorded recipe
    /// is the starting point and only the knobs you state below change.
    pub(crate) name: String,
    /// Path to the `.npz` take, exactly as `generate_clips` reported it. A
    /// relative path is read against the project root.
    pub(crate) take: String,
    /// The take's generator record (`<take>.take.json`). Omitted, the record
    /// beside the take is used when there is one; without any record the
    /// clip's provenance is `reconstructed`, which is the truth.
    pub(crate) record: Option<String>,
    /// What the motion is, in plain English — normally the prompt you
    /// generated from. Omitted, the take's own prompt stands; nothing is
    /// invented.
    pub(crate) prompt: Option<String>,
    /// Curation tags. Omitted, an existing clip's tags survive the
    /// re-promote.
    pub(crate) tags: Option<Vec<String>>,
    /// Anything the next reader should know: why this take, what to watch.
    pub(crate) note: Option<String>,
    /// Seconds trimmed from the start.
    pub(crate) trim_start_s: Option<f32>,
    /// Seconds trimmed from the end.
    pub(crate) trim_end_s: Option<f32>,
    /// Root travel: `off`, `strip` (pin hips X/Z, for locomotion loops the
    /// game drives) or `detrend` (remove drift, keep surges, for travelling
    /// one-shots).
    pub(crate) in_place: Option<String>,
    /// Root height: `off`, `strip` (pin to frame 0's height) or `detrend`
    /// (remove net rise/fall, keep dips). For jumps and vaults the game's
    /// capsule owns.
    pub(crate) y_mode: Option<String>,
    /// `true` makes the clip a loop by name even with no blend; `false`
    /// turns an inherited loop off, blend included.
    #[serde(rename = "loop")]
    pub(crate) looping: Option<bool>,
    /// Seconds of tail blended back to frame 0 to close a loop. Stating any
    /// value above zero makes the clip a loop; stating zero makes it not
    /// one. Capped at half the clip by the bake.
    pub(crate) loop_blend_s: Option<f32>,
    /// Scale on arm-swing deviation from the clip mean. 1.0 leaves it alone.
    pub(crate) exaggerate: Option<f32>,
    /// Elbow bend in degrees.
    pub(crate) arm_bend_deg: Option<f32>,
    /// Forward lean in degrees, spread down the spine.
    pub(crate) lean_deg: Option<f32>,
    /// Upper-arm pull-back in degrees.
    pub(crate) shoulder_back_deg: Option<f32>,
    /// Piecewise time remap as `src:dst,src:dst,…` in seconds. An empty
    /// string removes an inherited one.
    pub(crate) retime: Option<String>,
    /// Gameplay moments to record on the clip — the fire frame, an impact —
    /// timed in seconds on the RAW TAKE (the timeline the sheets label).
    /// The bake maps them onto the built clip through the recipe. Footsteps
    /// are derived from the take's contact labels; never state them.
    pub(crate) events: Option<Vec<ProposedEvent>>,
    /// Allow replacing an existing clip of this name. Default false, because
    /// guessing a name that is taken costs somebody else's asset.
    pub(crate) overwrite: Option<bool>,
}

/// One event a caller wants on a clip, timed against the raw take.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ProposedEvent {
    /// Seconds on the raw take.
    pub(crate) t_src: f32,
    /// Event name, lower-case `[a-z0-9_]+`, e.g. `fire` or `impact`.
    pub(crate) name: String,
    /// A sound to hang off the event, as `kind:name`, e.g. `sfx:pistol_shot`.
    pub(crate) audio: Option<String>,
}

/// A sound to copy into the library.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PromoteAudioArgs {
    /// Which audio directory: `sfx`, `music` or `voice` (`speech` is read
    /// as `voice`, the word `generate_audio` uses).
    pub(crate) kind: String,
    /// `snake_case` name it ships under. Becomes the file stem — and a
    /// game's audio map is by stem, so it must be free in every audio kind.
    pub(crate) name: String,
    /// Path to the audio file, exactly as `generate_audio` reported it. A
    /// relative path is read against the project root.
    pub(crate) file: String,
    /// The generator record `generate_audio` wrote beside it. Omitted, the
    /// `<stem>.json` beside the file is used when there is one; without a
    /// record the provenance is `unknown`, which is the truth.
    pub(crate) record: Option<String>,
    /// What it is: the description it was generated from, or for voice,
    /// the spoken line. Omitted, the record's prompt stands.
    pub(crate) prompt: Option<String>,
    /// Curation tags.
    pub(crate) tags: Option<Vec<String>>,
    /// Anything the next reader should know.
    pub(crate) note: Option<String>,
    /// Allow replacing an existing sound of this name in this kind. Default
    /// false.
    pub(crate) overwrite: Option<bool>,
    /// Ship the sound even when the measurements call it defective (silent,
    /// or clipped hard enough to distort). Default false: the door refuses
    /// with the defect named, and the fix is a regenerate, not a flag.
    pub(crate) allow_defective: Option<bool>,
}

#[tool_router(router = promote_router, vis = "pub(crate)")]
impl ForgeServer {
    /// Bake a take into the library, now.
    #[tool(
        description = "Bake one generated .npz take into a shipped clip, with an edit recipe \
                       (trim, root travel, height, arm/lean styling, loop blend, retime) and \
                       optional take-time events. THIS WRITES THE LIBRARY DIRECTLY — there is \
                       no review queue — so judge the take on generate_clips' sheet first, and \
                       render_clip_strip afterwards to see the clip on the real body. An \
                       existing name is refused unless overwrite:true; \
                       when you do overwrite, the shipped clip's recorded recipe is the starting \
                       point and only the knobs you state change, and the response carries both \
                       the 'shipped recipe' it replaced and the 'effective recipe' it baked with. \
                       The take's .take.json beside it is read automatically, so the record says \
                       how the motion was made. Returns the sidecar path and the catalog line."
    )]
    pub(crate) async fn promote_clip(
        &self,
        Parameters(args): Parameters<PromoteClipArgs>,
    ) -> CallToolResult {
        let project = self.config.project.clone();
        let name = match validate_name(&args.name) {
            Ok(name) => name,
            Err(err) => return util::refuse(err.to_string()),
        };
        let take = resolve_path(&project, &args.take);
        if !take.is_file() {
            return util::refuse(format!(
                "no take at {} — pass the path exactly as generate_clips reported it",
                take.display()
            ));
        }

        let catalog = Catalog::scan(&project);
        let shipped = catalog.resolve(&name, Some(Kind::Clip));
        let base = shipped.and_then(|r| r.sidecar.as_ref()?.recipe.clone());
        let recipe = match effective_recipe(base.as_ref(), &args) {
            Ok(recipe) => recipe,
            Err(message) => return util::refuse(message),
        };

        let mut preamble = String::new();
        if let Some(shipped) = shipped {
            let _ = writeln!(
                preamble,
                "{name} already exists as {}; {}",
                shipped.rel_path,
                if base.is_some() {
                    "the knobs you did not state were read from its recipe, not defaulted"
                } else {
                    "it records no recipe, so the knobs you did not state are identity"
                }
            );
        }
        let overwrite = args.overwrite.unwrap_or(false);
        if let Some(shipped) = shipped
            && !overwrite
        {
            let mut text = format!(
                "{preamble}refused: promoting {name} would replace {}. pass overwrite:true if \
                 replacing it is the intent, or pick another name (list_clips shows what is \
                 taken). nothing was written.\n\n",
                shipped.rel_path
            );
            let _ = write!(
                text,
                "{SHIPPED_RECIPE_HEADING} (what {name} is baked with now):\n{}\n",
                base.as_ref()
                    .map_or_else(|| String::from("  (none recorded)\n"), ClipRecipe::lines)
            );
            let _ = write!(
                text,
                "{EFFECTIVE_RECIPE_HEADING} (what it would have been baked with):\n{}",
                recipe.lines()
            );
            return util::refuse(text);
        }

        let events = match proposed_events(args.events.as_deref(), &recipe) {
            Ok(events) => events,
            Err(message) => return util::refuse(message),
        };
        let (take_record, record_note) = match take_record(&project, &take, args.record.as_deref())
        {
            Ok(found) => found,
            Err(message) => return util::refuse(message),
        };

        let request = PromoteClip {
            name: name.clone(),
            take_path: take,
            recipe: recipe.clone(),
            prompt: stated(args.prompt.as_deref()),
            tags: args.tags.clone().unwrap_or_default(),
            note: stated(args.note.as_deref()),
            events,
            created_by: Actor::parse(ACTOR),
            take_record,
            overwrite,
        };
        // The bake is synchronous CPU-and-disk work (edit application plus
        // glTF serialization); a runtime thread spent on it is a runtime
        // thread not answering anything else.
        let baking = tokio::task::spawn_blocking(move || {
            promote_clip(&project, &request).map_err(|e| e.to_string())
        });
        let promoted = match tokio::time::timeout(PROMOTE_TIMEOUT, baking).await {
            Ok(Ok(Ok(promoted))) => promoted,
            Ok(Ok(Err(message))) => {
                return util::refuse(format!(
                    "the promote of {name} was refused.\n{message}\n\nnothing was written."
                ));
            }
            Ok(Err(err)) => return util::refuse(format!("the bake task failed: {err}")),
            Err(_) => {
                return util::refuse(format!(
                    "the bake of {name} is still running after {} minutes, so this call gave \
                     up on it. it was not killed and may yet finish — check list_clips before \
                     trying again.",
                    util::minutes(PROMOTE_TIMEOUT)
                ));
            }
        };

        let mut text = format!(
            "{preamble}promoted {name} into {} — no review; it is in the library now.\n{}\n\
             {record_note}{}",
            promoted.rel_path,
            promoted.report,
            shipped_lines(&self.config.project, &promoted)
        );
        if let Some(was) = promoted.previous_recipe() {
            let _ = write!(
                text,
                "\n{SHIPPED_RECIPE_HEADING} (what {name} was baked with before this call):\n{}",
                was.lines()
            );
        } else if promoted.replaced.is_some() {
            let _ = writeln!(
                text,
                "\n{SHIPPED_RECIPE_HEADING}: the replaced clip recorded none, so every \
                 unstated knob was identity"
            );
        }
        let _ = write!(
            text,
            "\n{EFFECTIVE_RECIPE_HEADING} (every knob stated, nothing inherited from the \
             bake):\n{}",
            promoted
                .recipe()
                .map_or_else(|| recipe.lines(), ClipRecipe::lines)
        );
        // Authored events are not curation: a different take under the same
        // name is a different motion, and an event restated blindly would
        // sit at a moment that no longer exists. So they are not carried —
        // and the honest thing to do about what was dropped is to say so.
        if args.events.as_ref().is_none_or(Vec::is_empty)
            && let Some(replaced) = &promoted.replaced
            && let Some(events) = &replaced.events
        {
            let authored: Vec<&str> = events
                .iter()
                .filter(|e| !e.origin.is_contacts())
                .map(|e| e.name.as_str())
                .collect();
            if !authored.is_empty() {
                let _ = write!(
                    text,
                    "\nnote: the replaced clip carried {} authored event(s) ({}) and none were \
                     stated here, so they did not ship — restate them as events to keep them",
                    authored.len(),
                    authored.join(", ")
                );
            }
        }
        util::report(text)
    }

    /// Copy a sound into the library, now.
    #[tool(
        description = "Copy an audio file — sfx, music or voice — into the library with its \
                       record. THIS WRITES THE LIBRARY DIRECTLY — there is no review queue — so \
                       inspect_audio the file first: you cannot hear it, but the plot shows \
                       clipping, dead air and truncation. A name already used by ANY audio kind \
                       is refused (a game's audio map is by stem); an existing sound of this \
                       kind is refused unless overwrite:true; a silent or clipped file is \
                       refused unless allow_defective:true — fix the sound instead. The record \
                       generate_audio wrote beside the file is read automatically. Returns the \
                       sidecar path and the catalog line."
    )]
    pub(crate) async fn promote_audio(
        &self,
        Parameters(args): Parameters<PromoteAudioArgs>,
    ) -> CallToolResult {
        let project = self.config.project.clone();
        let Some(kind) = parse_audio_kind(&args.kind) else {
            return util::refuse(format!(
                "{:?} is not an audio kind — use sfx, music or voice (speech is voice). a clip \
                 goes through promote_clip; meshes have no door here.",
                args.kind
            ));
        };
        let name = match validate_name(&args.name) {
            Ok(name) => name,
            Err(err) => return util::refuse(err.to_string()),
        };
        let file = resolve_path(&project, &args.file);
        if !file.is_file() {
            return util::refuse(format!(
                "no audio file at {} — pass the path exactly as generate_audio reported it",
                file.display()
            ));
        }

        // A game's audio map is by stem: `hit` is one sound across every
        // audio directory, so a voice line called `hit` beside an sfx called
        // `hit` would make which of them plays a matter of load order.
        let catalog = Catalog::scan(&project);
        let elsewhere: Vec<&AssetRecord> = catalog
            .records()
            .iter()
            .filter(|r| r.kind.is_audio() && r.kind != kind && r.name == name)
            .collect();
        if !elsewhere.is_empty() {
            return util::refuse(format!(
                "{name} is already taken by another audio kind — a game's audio map is by \
                 stem, so pick another name. nothing was written.\nholding the stem:\n  {}",
                elsewhere
                    .iter()
                    .map(|r| format!("{}:{} ({})", r.kind.as_str(), r.name, r.rel_path))
                    .collect::<Vec<_>>()
                    .join("\n  ")
            ));
        }
        let overwrite = args.overwrite.unwrap_or(false);
        if let Some(existing) = catalog.resolve(&name, Some(kind))
            && !overwrite
        {
            return util::refuse(format!(
                "refused: promoting {name} would replace {}. pass overwrite:true if replacing \
                 it is the intent, or pick another name (list_audio shows what is taken). \
                 nothing was written.",
                existing.rel_path
            ));
        }

        let expected = match kind {
            Kind::Sfx => RecordKind::Sfx,
            Kind::Music => RecordKind::Music,
            Kind::Voice => RecordKind::Speech,
            // Unreachable: parse_audio_kind only returns audio kinds.
            // Spelled out so a fourth audio kind fails to compile here.
            Kind::Clip | Kind::Body | Kind::Model => {
                unreachable!("parse_audio_kind returns audio kinds only")
            }
        };
        let (record, record_note) =
            match audio_record(&project, &file, args.record.as_deref(), expected) {
                Ok(found) => found,
                Err(message) => return util::refuse(message),
            };

        let request = PromoteAudio {
            kind,
            name: name.clone(),
            file,
            record,
            prompt: stated(args.prompt.as_deref()),
            tags: args.tags.unwrap_or_default(),
            note: stated(args.note.as_deref()),
            created_by: Actor::parse(ACTOR),
            overwrite,
            allow_defective: args.allow_defective.unwrap_or(false),
        };
        // Decode-and-copy is blocking work; a long music track decodes for
        // a while.
        let copying = tokio::task::spawn_blocking(move || {
            promote_audio(&project, &request).map_err(|e| e.to_string())
        });
        let promoted = match tokio::time::timeout(PROMOTE_TIMEOUT, copying).await {
            Ok(Ok(Ok(promoted))) => promoted,
            Ok(Ok(Err(message))) => {
                return util::refuse(format!(
                    "the promote of {name} was refused.\n{message}\n\nnothing was written."
                ));
            }
            Ok(Err(err)) => return util::refuse(format!("the promote task failed: {err}")),
            Err(_) => {
                return util::refuse(format!(
                    "the promote of {name} is still running after {} minutes, so this call \
                     gave up on it — check list_audio before trying again.",
                    util::minutes(PROMOTE_TIMEOUT)
                ));
            }
        };
        let mut text = format!(
            "promoted {name} into {} — no review; it is in the library now.\n{}\n{record_note}{}",
            promoted.rel_path,
            promoted.report,
            shipped_lines(&self.config.project, &promoted)
        );
        if let Some(replaced) = &promoted.replaced {
            let _ = write!(
                text,
                "replaced the {} {} created {} by {}",
                replaced.kind, replaced.name, replaced.created, replaced.created_by
            );
        }
        util::report(text)
    }

    /// File a rigged body, now, behind the gates.
    #[tool(
        description = "Copy a rigged, exported .glb into the library as a body, with its \
                       record. THIS WRITES THE LIBRARY DIRECTLY — there is no review queue — \
                       and it runs three gates first: the export gate on the file (a \
                       self-contained container, one armature, every contract bone, at most \
                       four influences), `forge rig check` on the profile's reference clip \
                       (names, depths, rest rotations, the weights, stature, feet on the \
                       ground, the planted foot's own lowest vertex, and that all the driven \
                       bones actually bind), and a name that is already taken, which is \
                       refused unless you pass overwrite:true and then echoes the record it \
                       replaced. LOOK FIRST ANYWAY: render_model draws the body from every \
                       angle and render_clip_strip poses a clip on it — numbers say a body is \
                       wired, a picture says what it is. The sidecar records this body's OWN \
                       bone lengths and its motion_scale, re-derived from the .glb being \
                       filed, so what the record claims is re-derivable from the file it \
                       describes. Pass the rig and export records so the provenance is \
                       recorded rather than reconstructed."
    )]
    pub(crate) async fn promote_body(
        &self,
        Parameters(args): Parameters<PromoteBodyArgs>,
    ) -> CallToolResult {
        self.promote_mesh(Kind::Body, &args).await
    }

    /// File a static mesh, now.
    #[tool(
        description = "Copy a normalised .glb into the library as a model — a prop, a \
                       fixture, a held weapon — with its record. THIS WRITES THE LIBRARY \
                       DIRECTLY — there is no review queue — after the export gate on the \
                       file and the taken-name refusal, which is lifted only by \
                       overwrite:true and then echoes what it replaced. A model stands on no \
                       rig, so no rig check runs and none should: a prop that could claim the \
                       contract is a prop a game would try to animate. The mesh must already \
                       be normalised — metres, the origin where the profile says a thing of \
                       its kind rests, matte — which is what `forge gen prop` does; this door \
                       only files. Look at it with render_model first."
    )]
    pub(crate) async fn promote_model(
        &self,
        Parameters(args): Parameters<PromoteBodyArgs>,
    ) -> CallToolResult {
        self.promote_mesh(Kind::Model, &args).await
    }
}

/// Which records a mesh promote reads, and what each one is called when it
/// is the wrong kind.
const MESH_RECORDS: [(&str, RecordKind); 4] = [
    ("lift_record", RecordKind::Lift),
    ("rig_record", RecordKind::Rig),
    ("export_record", RecordKind::Export),
    ("prop_record", RecordKind::Prop),
];

impl ForgeServer {
    /// The body of both mesh doors: resolve, gate, file, report.
    ///
    /// One function for two kinds because everything except the rig check
    /// and the request type is the same question, and the place two copies
    /// would drift is exactly the taken-name refusal — the gate that costs
    /// somebody else's asset when it goes soft.
    async fn promote_mesh(&self, kind: Kind, args: &PromoteBodyArgs) -> CallToolResult {
        let project = self.config.project.clone();
        let name = match validate_name(&args.name) {
            Ok(name) => name,
            Err(err) => return util::refuse(err.to_string()),
        };
        let glb = resolve_path(&project, &args.glb);
        if !glb.is_file() {
            return util::refuse(format!(
                "no mesh at {} — pass the path the exporter reported. nothing was written.",
                glb.display()
            ));
        }

        let overwrite = args.overwrite.unwrap_or(false);
        let catalog = Catalog::scan(&project);
        if let Some(existing) = catalog.resolve(&name, Some(kind))
            && !overwrite
        {
            return util::refuse(format!(
                "refused: promoting {name} would replace {}, {} created {} by {}. pass \
                 overwrite: true if replacing it is the intent, or pick another name \
                 (list_models shows what is taken). nothing was written.",
                existing.rel_path,
                existing
                    .sidecar
                    .as_ref()
                    .and_then(|s| s.prompt.clone())
                    .unwrap_or_else(|| String::from("a mesh with no prompt recorded")),
                existing
                    .sidecar
                    .as_ref()
                    .map_or_else(|| String::from("?"), |s| s.created.clone()),
                existing
                    .sidecar
                    .as_ref()
                    .map_or_else(|| String::from("?"), |s| s.created_by.to_string()),
            ));
        }

        let mut records: Vec<Option<GeneratorRecord>> = Vec::new();
        for (flag, expected) in MESH_RECORDS {
            let stated_path = match flag {
                "lift_record" => args.lift_record.as_deref(),
                "rig_record" => args.rig_record.as_deref(),
                "export_record" => args.export_record.as_deref(),
                _ => args.prop_record.as_deref(),
            };
            match mesh_record(&project, stated_path, expected, flag) {
                Ok(record) => records.push(record),
                Err(refusal) => return util::refuse(refusal),
            }
        }
        let [lift, rig, export, prop] = records.try_into().unwrap_or_default();

        // The rig check is the gate that needs an engine, and this crate
        // links none: it shells out to the same binary that serves this
        // server, so the findings an agent reads are the findings
        // `forge rig check` prints.
        if kind == Kind::Body
            && let Some(refusal) = self.rig_check(&glb).await
        {
            return refusal;
        }

        let blend = stated(args.blend.as_deref()).map(|blend| resolve_path(&project, &blend));
        let request = MeshRequest {
            name: name.clone(),
            glb_path: glb,
            blend_path: blend,
            lift_record: lift,
            rig_record: rig,
            export_record: export,
            prop_record: prop,
            prompt: stated(args.prompt.as_deref()),
            tags: args.tags.clone().unwrap_or_default(),
            note: stated(args.note.as_deref()),
            created_by: Actor::parse(ACTOR),
            overwrite,
        };
        let filing = tokio::task::spawn_blocking(move || request.file(&project, kind));
        let promoted = match tokio::time::timeout(PROMOTE_TIMEOUT, filing).await {
            Ok(Ok(Ok(promoted))) => promoted,
            Ok(Ok(Err(message))) => {
                return util::refuse(format!(
                    "the promote of {name} was refused.\n{message}\n\nnothing was written."
                ));
            }
            Ok(Err(err)) => return util::refuse(format!("the promote task failed: {err}")),
            Err(_) => {
                return util::refuse(format!(
                    "the promote of {name} is still running after {} minutes, so this call \
                     gave up on it — check list_models before trying again.",
                    util::minutes(PROMOTE_TIMEOUT)
                ));
            }
        };
        let mut text = format!(
            "promoted {name} into {} — no review; it is in the library now.\n{}\n{}",
            promoted.rel_path,
            promoted.report,
            shipped_lines(&self.config.project, &promoted)
        );
        if let Some(body) = &promoted.record.body {
            let _ = writeln!(
                text,
                "skeleton: {} bones re-derived from the .glb, motion_scale {:.4} — a consumer \
                 multiplies the root translation track by that and nothing else",
                body.bones.len(),
                body.motion_scale
            );
        }
        if let Some(replaced) = &promoted.replaced {
            let _ = write!(
                text,
                "replaced the {} {} created {} by {}",
                replaced.kind, replaced.name, replaced.created, replaced.created_by
            );
        }
        util::report(text)
    }

    /// Run `forge rig check` on a body, and turn a failing report into a
    /// refusal — or `None` when it passed.
    async fn rig_check(&self, glb: &std::path::Path) -> Option<CallToolResult> {
        let mut command = tokio::process::Command::new(&self.config.renderer);
        command
            .arg("--project")
            .arg(&self.config.project.root)
            .arg("rig")
            .arg("check")
            .arg(glb);
        match util::run(&mut command, RIG_CHECK_TIMEOUT).await {
            util::Ran::Ok(_) => None,
            util::Ran::Failed(captured) => Some(util::refuse(format!(
                "refused: {} does not satisfy the rig contract, so no clip in the library \
                 would play on it. nothing was written.\n\n{}\n{}",
                glb.display(),
                captured.stdout.trim(),
                captured.stderr_tail()
            ))),
            util::Ran::Unlaunchable(err) => Some(util::refuse(format!(
                "the rig check could not be run ({}: {err}), and a body is not filed on an \
                 unrun gate. nothing was written.",
                self.config.renderer.display()
            ))),
            util::Ran::TimedOut => Some(util::refuse(format!(
                "the rig check on {} is still running after {} minutes, so this call gave up \
                 on it. nothing was written.",
                glb.display(),
                util::minutes(RIG_CHECK_TIMEOUT)
            ))),
        }
    }
}

/// One mesh promote's arguments, before they are split by kind.
struct MeshRequest {
    name: String,
    glb_path: PathBuf,
    blend_path: Option<PathBuf>,
    lift_record: Option<GeneratorRecord>,
    rig_record: Option<GeneratorRecord>,
    export_record: Option<GeneratorRecord>,
    prop_record: Option<GeneratorRecord>,
    prompt: Option<String>,
    tags: Vec<String>,
    note: Option<String>,
    created_by: Actor,
    overwrite: bool,
}

impl MeshRequest {
    /// Hand this to the library's own door for the kind — the same
    /// function `forge promote body` and `forge promote model` call, so
    /// what an agent files is byte-for-byte what a human would have.
    fn file(self, project: &Project, kind: Kind) -> Result<Promoted, String> {
        match kind {
            Kind::Body => promote_body(
                project,
                &PromoteBody {
                    name: self.name,
                    glb_path: self.glb_path,
                    blend_path: self.blend_path,
                    lift_record: self.lift_record,
                    rig_record: self.rig_record,
                    export_record: self.export_record,
                    prompt: self.prompt,
                    tags: self.tags,
                    note: self.note,
                    created_by: self.created_by,
                    overwrite: self.overwrite,
                },
            ),
            _ => promote_model(
                project,
                &PromoteModel {
                    name: self.name,
                    glb_path: self.glb_path,
                    blend_path: self.blend_path,
                    lift_record: self.lift_record,
                    prop_record: self.prop_record,
                    prompt: self.prompt,
                    tags: self.tags,
                    note: self.note,
                    created_by: self.created_by,
                    overwrite: self.overwrite,
                },
            ),
        }
        .map_err(|e| e.to_string())
    }
}

/// One generator record a mesh promote was handed, held to its kind.
///
/// A record of the wrong kind is refused rather than ignored: a lift record
/// passed as the rig's would put the lift's parameters in the sidecar under
/// a heading that says the rig ran, which is a lie the record shape exists
/// to make impossible.
fn mesh_record(
    project: &Project,
    stated_path: Option<&str>,
    expected: RecordKind,
    flag: &str,
) -> Result<Option<GeneratorRecord>, String> {
    let Some(stated_path) = stated(stated_path) else {
        return Ok(None);
    };
    let path = resolve_path(project, &stated_path);
    if !path.is_file() {
        return Err(format!(
            "no record at {} for {flag} — pass the path exactly as the generator reported it. \
             nothing was written.",
            path.display()
        ));
    }
    let record = GeneratorRecord::load(&path).map_err(|e| e.to_string())?;
    if record.kind != expected {
        return Err(format!(
            "{} describes a {} run, not {expected}, so it is not {flag}. nothing was written.",
            path.display(),
            record.kind
        ));
    }
    Ok(Some(record))
}

/// The shipped recipe (or identity) with the stated knobs on top, then the
/// loop flag, which says more than a blend can: `loop: true` with no blend
/// is a loop by name, `loop: false` turns an inherited one off.
///
/// # Errors
///
/// The message to refuse with when a stated value is unusable: an unknown
/// `in_place` or `y_mode` word, a malformed retime. Each names the valid
/// forms, because the caller can fix its own call next turn.
fn effective_recipe(
    base: Option<&ClipRecipe>,
    args: &PromoteClipArgs,
) -> Result<ClipRecipe, String> {
    let in_place = match args.in_place.as_deref() {
        None => None,
        Some(mode) => Some(
            InPlaceMode::parse(mode)
                .ok_or_else(|| format!("in_place must be off, strip or detrend, not {mode:?}"))?,
        ),
    };
    let y_mode = match args.y_mode.as_deref() {
        None => None,
        Some(mode) => Some(
            RootYMode::parse(mode)
                .ok_or_else(|| format!("y_mode must be off, strip or detrend, not {mode:?}"))?,
        ),
    };
    if let Some(spec) = args.retime.as_deref() {
        parse_retime(spec).map_err(|err| err.to_string())?;
    }
    let requested = PartialRecipe {
        trim_start_s: args.trim_start_s,
        trim_end_s: args.trim_end_s,
        in_place,
        y_mode,
        exaggerate: args.exaggerate,
        arm_bend_deg: args.arm_bend_deg,
        lean_deg: args.lean_deg,
        shoulder_back_deg: args.shoulder_back_deg,
        loop_blend_s: args.loop_blend_s,
        retime: args.retime.clone(),
        clip: None,
    };
    let identity = ClipRecipe::default();
    let mut recipe = overlay_recipe(base.unwrap_or(&identity), &requested);
    match args.looping {
        Some(true) => recipe.looping = true,
        Some(false) => {
            recipe.looping = false;
            recipe.loop_blend_s = 0.0;
        }
        None => {}
    }
    Ok(recipe)
}

/// Turn stated take-time events into authored events, refusing what a
/// bake would silently drop.
///
/// The built-clip times written here are provisional — mapped through the
/// recipe at the library's default fps — and the bake recomputes them
/// against the take's real clock. What is checked hard is what cannot be
/// fixed later: the name alphabet, the audio link shape, and whether the
/// moment survives the recipe's own trim at all.
fn proposed_events(
    stated: Option<&[ProposedEvent]>,
    recipe: &ClipRecipe,
) -> Result<Vec<AnimEvent>, String> {
    let Some(stated) = stated.filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    let edit = recipe
        .to_edit(DEFAULT_FPS)
        .map_err(|e| format!("the recipe does not apply: {e}"))?;
    let mut events = Vec::with_capacity(stated.len());
    for event in stated {
        if !valid_event_name(&event.name) {
            return Err(format!(
                "{:?} is not an event name — lower-case [a-z0-9_]+ only",
                event.name
            ));
        }
        if event.name.starts_with("footstep_") {
            return Err(format!(
                "{:?} is derived from the take's contact labels at bake time — do not state \
                 footsteps by hand",
                event.name
            ));
        }
        if !event.t_src.is_finite() || event.t_src < 0.0 {
            return Err(format!(
                "event {:?} at {}s: the time is seconds on the raw take and must be a \
                 non-negative number",
                event.name, event.t_src
            ));
        }
        let audio = match event.audio.as_deref() {
            None => None,
            Some(text) => Some(AudioRef::parse(text).ok_or_else(|| {
                format!("{text:?} is not an audio link — use kind:name, e.g. sfx:pistol_shot")
            })?),
        };
        let Some(t) = edit.map_time(event.t_src, DEFAULT_FPS) else {
            return Err(format!(
                "event {:?} at {}s falls before the recipe's kept window, so the bake would \
                 drop it — move the event or widen the trim",
                event.name, event.t_src
            ));
        };
        events.push(AnimEvent {
            t,
            t_src: Some(event.t_src),
            name: event.name.clone(),
            origin: EventOrigin::parse(ACTOR),
            audio,
        });
    }
    Ok(events)
}

/// What was found of a generator record, and the line that says so.
type FoundRecord = (Option<GeneratorRecord>, String);

/// The take's generator record: the one stated, else the `.take.json` the
/// sweep writes beside every take, else none — with a line saying which.
///
/// # Errors
///
/// A stated record that is missing, unreadable, or describes another kind
/// of run.
fn take_record(
    project: &Project,
    take: &Path,
    stated: Option<&str>,
) -> Result<FoundRecord, String> {
    let stem = take
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let beside = take.with_file_name(format!("{stem}.take.json"));
    load_record(project, stated, &beside, RecordKind::Take, "the take")
}

/// The sound's generator record: the one stated, else the `<stem>.json`
/// `generate_audio` writes beside it, else none.
///
/// # Errors
///
/// As [`take_record`].
fn audio_record(
    project: &Project,
    file: &Path,
    stated: Option<&str>,
    expected: RecordKind,
) -> Result<FoundRecord, String> {
    let beside = file.with_extension("json");
    load_record(project, stated, &beside, expected, "the sound")
}

/// Read a generator record from the stated path, or from where the
/// generator leaves one, and hold it to the kind this door expects: a lift
/// record handed as a take's would be filed as if it were.
fn load_record(
    project: &Project,
    stated: Option<&str>,
    beside: &Path,
    expected: RecordKind,
    what: &str,
) -> Result<FoundRecord, String> {
    let (path, how) = match stated.map(str::trim).filter(|s| !s.is_empty()) {
        Some(stated) => {
            let path = resolve_path(project, stated);
            if !path.is_file() {
                return Err(format!(
                    "no record at {} — pass the path exactly as the generator reported it, or \
                     omit it to use the one beside the file",
                    path.display()
                ));
            }
            (path, "as stated")
        }
        None if beside.is_file() => (beside.to_path_buf(), "found beside the file"),
        None => {
            return Ok((
                None,
                format!(
                    "record: none ({} is not there), so the provenance is the narrower claim; \
                     pass record to say how it was made\n",
                    beside.display()
                ),
            ));
        }
    };
    let record = GeneratorRecord::load(&path).map_err(|e| e.to_string())?;
    if record.kind != expected {
        return Err(format!(
            "{} describes a {} run, not {expected} — it is not {what}'s record",
            path.display(),
            record.kind
        ));
    }
    let fake = if record.fake {
        " — a --fake placeholder; nothing about it is a measurement"
    } else {
        ""
    };
    Ok((
        Some(record),
        format!("record: {} ({how}){fake}\n", path.display()),
    ))
}

/// `sfx`, `music`, `voice` — and `speech`, the word `generate_audio` uses
/// for the voice kind.
fn parse_audio_kind(text: &str) -> Option<Kind> {
    let text = text.trim().to_ascii_lowercase();
    if text == "speech" {
        return Some(Kind::Voice);
    }
    Kind::parse(&text).filter(|kind| kind.is_audio())
}

/// A stated path, read against the project root when it is relative: the
/// client launched this server from wherever it liked, and the one directory
/// both sides know is the project.
pub(crate) fn resolve_path(project: &Project, text: &str) -> PathBuf {
    let path = PathBuf::from(text.trim());
    if path.is_absolute() || path.exists() {
        path
    } else {
        project.root.join(path)
    }
}

/// An optional argument, with an empty or blank value read as "not stated".
pub(crate) fn stated(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

/// Where the record landed and the line `forge catalog` would print for it.
fn shipped_lines(project: &Project, promoted: &Promoted) -> String {
    let record = &promoted.record;
    let tags = record.tags.join(",");
    format!(
        "sidecar: {}\ncatalog: {:<8} {:<20} {:<14} {:<12} {}\n",
        project
            .rel_to_root(&promoted.sidecar)
            .unwrap_or_else(|| promoted.sidecar.display().to_string()),
        record.kind.as_str(),
        record.name,
        record.provenance.as_str(),
        util::first_line(&tags, 12),
        util::first_line(record.prompt.as_deref().unwrap_or("—"), 72),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The taken-name refusal on the mesh doors, which is the gate that
    /// costs somebody else's asset when it goes soft: a successful frame
    /// naming the record it would have replaced and the word that gets past
    /// it. The `overwrite` leg is exercised end to end in `mcp_session`,
    /// where a real rig check runs.
    #[tokio::test]
    async fn a_taken_mesh_name_is_refused_naming_what_it_would_replace() {
        let (dir, project) = crate::testing::library();
        let body = project.kind_dir(Kind::Body).join("mannequin.glb");
        let server = crate::testing::server(project);

        let taken = server
            .promote_body(Parameters(PromoteBodyArgs {
                name: String::from("mannequin"),
                glb: body.display().to_string(),
                ..PromoteBodyArgs::default()
            }))
            .await;
        let text = util::frame_text(&taken);
        assert!(
            text.contains("would replace bodies/mannequin.glb"),
            "{text}"
        );
        assert!(text.contains("overwrite: true"), "{text}");
        assert!(text.contains("the fixture mannequin"), "{text}");
        assert!(text.contains("nothing was written"), "{text}");

        // A model of a name no model holds gets past the collision gate and
        // onto the file, which is where a mesh that is not there stops.
        let missing = server
            .promote_model(Parameters(PromoteBodyArgs {
                name: String::from("barrel"),
                glb: dir.path().join("nowhere.glb").display().to_string(),
                ..PromoteBodyArgs::default()
            }))
            .await;
        let text = util::frame_text(&missing);
        assert!(text.contains("no mesh at"), "{text}");
        assert!(text.contains("nothing was written"), "{text}");
    }

    /// A record of the wrong kind is refused rather than filed under a
    /// heading that says a run happened which did not.
    #[tokio::test]
    async fn a_record_of_the_wrong_kind_is_refused_by_name() {
        let (dir, project) = crate::testing::library();
        let body = project.kind_dir(Kind::Body).join("mannequin.glb");
        let record = dir.path().join("wrong.json");
        std::fs::write(
            &record,
            br#"{"forge_record": 2, "kind": "lift", "tool": "trellis2",
                 "created": "2026-08-30", "created_by": "human",
                 "backend": {"name": null, "commit": null, "python": null, "torch": null,
                             "model": null, "model_revision": null},
                 "inputs": [], "params": {}, "measured": {}, "outputs": [], "fake": false}"#,
        )
        .expect("write the record");
        let server = crate::testing::server(project);

        let refused = server
            .promote_body(Parameters(PromoteBodyArgs {
                name: String::from("mannequin"),
                glb: body.display().to_string(),
                rig_record: Some(record.display().to_string()),
                overwrite: Some(true),
                ..PromoteBodyArgs::default()
            }))
            .await;
        let text = util::frame_text(&refused);
        assert!(text.contains("describes a lift run"), "{text}");
        assert!(text.contains("rig_record"), "{text}");
        assert!(text.contains("nothing was written"), "{text}");
    }

    #[test]
    fn the_overlay_keeps_unstated_knobs_and_the_loop_flag_outranks_it() {
        let shipped = ClipRecipe {
            trim_start_s: 0.25,
            trim_end_s: 1.3,
            in_place: InPlaceMode::Detrend,
            looping: true,
            loop_blend_s: 0.2,
            lean_deg: 4.0,
            ..ClipRecipe::default()
        };
        let args = PromoteClipArgs {
            lean_deg: Some(6.0),
            ..PromoteClipArgs::default()
        };
        let recipe = effective_recipe(Some(&shipped), &args).expect("recipe");
        assert!((recipe.trim_start_s - 0.25).abs() < f32::EPSILON);
        assert_eq!(recipe.in_place, InPlaceMode::Detrend);
        assert!((recipe.lean_deg - 6.0).abs() < f32::EPSILON);
        assert!(recipe.looping);

        let off = PromoteClipArgs {
            looping: Some(false),
            ..PromoteClipArgs::default()
        };
        let recipe = effective_recipe(Some(&shipped), &off).expect("recipe");
        assert!(!recipe.looping);
        assert!(recipe.loop_blend_s.abs() < f32::EPSILON);

        let on = PromoteClipArgs {
            looping: Some(true),
            ..PromoteClipArgs::default()
        };
        let recipe = effective_recipe(None, &on).expect("loop by name");
        assert!(recipe.looping);
        assert!(recipe.loop_blend_s.abs() < f32::EPSILON);

        let fresh = effective_recipe(None, &PromoteClipArgs::default()).expect("identity");
        assert_eq!(fresh, ClipRecipe::default());
    }

    #[test]
    fn a_bad_mode_or_retime_is_refused_with_the_valid_forms() {
        let bad_mode = PromoteClipArgs {
            in_place: Some(String::from("sideways")),
            ..PromoteClipArgs::default()
        };
        let message = effective_recipe(None, &bad_mode).expect_err("refuse");
        assert!(message.contains("off, strip or detrend"), "{message}");

        let bad_height = PromoteClipArgs {
            y_mode: Some(String::from("up")),
            ..PromoteClipArgs::default()
        };
        assert!(effective_recipe(None, &bad_height).is_err());

        let bad_retime = PromoteClipArgs {
            retime: Some(String::from("0.1:x")),
            ..PromoteClipArgs::default()
        };
        assert!(effective_recipe(None, &bad_retime).is_err());
    }

    #[test]
    fn events_are_checked_and_mapped_through_the_trim() {
        let recipe = ClipRecipe {
            trim_start_s: 0.5,
            ..ClipRecipe::default()
        };
        let fire = ProposedEvent {
            t_src: 1.0,
            name: String::from("fire"),
            audio: Some(String::from("sfx:pistol_shot")),
        };
        let events = proposed_events(Some(std::slice::from_ref(&fire)), &recipe).expect("ok");
        assert_eq!(events.len(), 1);
        assert!((events[0].t - 0.5).abs() < 1e-4, "{}", events[0].t);
        assert_eq!(events[0].t_src, Some(1.0));
        assert_eq!(events[0].origin, EventOrigin::parse(ACTOR));

        let early = ProposedEvent {
            t_src: 0.1,
            ..fire.clone()
        };
        let message = proposed_events(Some(&[early]), &recipe).expect_err("trimmed away");
        assert!(message.contains("kept window"), "{message}");

        let step = ProposedEvent {
            name: String::from("footstep_l"),
            ..fire.clone()
        };
        assert!(proposed_events(Some(&[step]), &recipe).is_err());
        let shouting = ProposedEvent {
            name: String::from("Fire!"),
            ..fire.clone()
        };
        assert!(proposed_events(Some(&[shouting]), &recipe).is_err());
        let bad_link = ProposedEvent {
            audio: Some(String::from("pistol_shot")),
            ..fire
        };
        assert!(proposed_events(Some(&[bad_link]), &recipe).is_err());
        assert!(proposed_events(None, &recipe).expect("none").is_empty());
    }

    #[test]
    fn speech_is_the_voice_kind_and_a_clip_is_not_audio() {
        assert_eq!(parse_audio_kind("speech"), Some(Kind::Voice));
        assert_eq!(parse_audio_kind(" SFX "), Some(Kind::Sfx));
        assert_eq!(parse_audio_kind("clip"), None);
        assert_eq!(parse_audio_kind("noise"), None);
    }

    #[test]
    fn a_relative_path_is_read_against_the_project() {
        let (_dir, project) = crate::testing::empty_project();
        let resolved = resolve_path(&project, "out/sweeps/x.npz");
        assert_eq!(resolved, project.root.join("out/sweeps/x.npz"));
        assert_eq!(
            resolve_path(&project, "/abs/x.npz"),
            PathBuf::from("/abs/x.npz")
        );
        assert_eq!(stated(Some("  ")), None);
        assert_eq!(stated(Some(" a ")).as_deref(), Some("a"));
    }

    #[test]
    fn a_record_of_the_wrong_kind_is_not_this_files_record() {
        let (_dir, project) = crate::testing::empty_project();
        let sfx = crate::testing::toolkit("crates/forge_library/tests/fixtures/python/sfx.json");
        let message = load_record(
            &project,
            Some(sfx.to_str().expect("utf-8")),
            Path::new("/nowhere.json"),
            RecordKind::Take,
            "the take",
        )
        .expect_err("wrong kind");
        assert!(message.contains("not take"), "{message}");

        let (absent, note) = load_record(
            &project,
            None,
            Path::new("/nowhere.take.json"),
            RecordKind::Take,
            "the take",
        )
        .expect("absent is fine");
        assert!(absent.is_none());
        assert!(note.contains("record: none"), "{note}");
    }
}

/// The `generate_clips → promote_clip` round trip, driven through the tool
/// handlers with no GPU and no backend.
///
/// This lives in the crate rather than under `tests/` because the handlers
/// are `pub(crate)` on a `pub(crate)` server — the crate's public surface
/// is `serve`, and every rmcp type stays inside — so an integration test
/// has no door to a handler. Were `ForgeServer` and `tools` made public
/// this module moves to `tests/round_trip.rs` unchanged.
#[cfg(test)]
mod round_trip {
    use forge_library::promote::{PromoteClip, promote_clip};
    use forge_library::schema::{Actor, ClipRecipe, InPlaceMode, Provenance};
    use forge_library::{Catalog, Kind};
    use rmcp::handler::server::wrapper::Parameters;

    use super::{
        EFFECTIVE_RECIPE_HEADING, PromoteAudioArgs, PromoteClipArgs, SHIPPED_RECIPE_HEADING,
    };
    use crate::testing::{empty_project, server, toolkit};
    use crate::tools::generate::GenerateClipsArgs;
    use crate::util::frame_text;

    /// The pinned roll take: the same file `forge_library`'s parity test
    /// bakes, so the arithmetic below is written against a known clip.
    fn take_fixture() -> std::path::PathBuf {
        toolkit("crates/forge_motion/tests/fixtures/blender/gen_roll.npz")
    }

    /// The roll's recipe as the tool states it…
    fn tool_args(name: &str, overwrite: bool) -> PromoteClipArgs {
        PromoteClipArgs {
            name: name.to_owned(),
            take: take_fixture().display().to_string(),
            prompt: Some(String::from("a forward roll")),
            tags: Some(vec![String::from("action")]),
            trim_start_s: Some(0.25),
            trim_end_s: Some(1.3),
            in_place: Some(String::from("detrend")),
            exaggerate: Some(1.15),
            lean_deg: Some(4.0),
            overwrite: Some(overwrite),
            ..PromoteClipArgs::default()
        }
    }

    /// …and the same recipe as the library takes it.
    fn library_request(name: &str) -> PromoteClip {
        PromoteClip {
            name: name.to_owned(),
            take_path: take_fixture(),
            recipe: ClipRecipe {
                trim_start_s: 0.25,
                trim_end_s: 1.3,
                in_place: InPlaceMode::Detrend,
                exaggerate: 1.15,
                lean_deg: 4.0,
                ..ClipRecipe::default()
            },
            prompt: Some(String::from("a forward roll")),
            tags: vec![String::from("action")],
            note: None,
            events: Vec::new(),
            created_by: Actor::parse(super::ACTOR),
            take_record: None,
            overwrite: false,
        }
    }

    #[tokio::test]
    async fn the_tool_ships_the_same_bytes_as_the_library_door() {
        let (_a, project_a) = empty_project();
        let (_b, project_b) = empty_project();
        let server = server(project_a.clone());

        let result = server
            .promote_clip(Parameters(tool_args("roll", false)))
            .await;
        let text = frame_text(&result);
        assert_ne!(result.is_error, Some(true), "{text}");
        assert!(text.contains("promoted roll into clips/roll.glb"), "{text}");
        assert!(text.contains("sidecar: assets/clips/roll.json"), "{text}");
        assert!(text.contains("catalog: clip     roll"), "{text}");
        assert!(text.contains(EFFECTIVE_RECIPE_HEADING), "{text}");
        assert!(
            !text.contains(SHIPPED_RECIPE_HEADING),
            "a first promote replaced nothing: {text}"
        );

        let by_library = promote_clip(&project_b, &library_request("roll")).expect("library");
        let via_tool = project_a.kind_dir(Kind::Clip).join("roll.glb");
        let tool_bytes = std::fs::read(&via_tool).expect("read the tool's clip");
        let library_bytes = std::fs::read(&by_library.asset).expect("read the library's clip");
        assert!(
            tool_bytes == library_bytes,
            "the tool and the library door baked different bytes"
        );

        let catalog = Catalog::scan(&project_a);
        let record = catalog
            .resolve("roll", Some(Kind::Clip))
            .and_then(|r| r.sidecar.clone())
            .expect("the sidecar parses");
        assert_eq!(record.created_by, Actor::parse(super::ACTOR));
        assert_eq!(record.provenance, Provenance::Reconstructed);
        assert_eq!(record.tags, vec![String::from("action")]);
        assert_eq!(record.prompt.as_deref(), Some("a forward roll"));
        assert!(
            project_a.manifest_path().is_file(),
            "the manifest was refreshed"
        );
    }

    #[tokio::test]
    async fn a_second_promote_refuses_by_name_and_overwrite_echoes_both_recipes() {
        let (_dir, project) = empty_project();
        let server = server(project.clone());
        let first = server
            .promote_clip(Parameters(tool_args("roll", false)))
            .await;
        assert_ne!(first.is_error, Some(true), "{}", frame_text(&first));

        let refused = server
            .promote_clip(Parameters(PromoteClipArgs {
                lean_deg: Some(6.0),
                ..tool_args("roll", false)
            }))
            .await;
        assert_eq!(refused.is_error, Some(true));
        let text = frame_text(&refused);
        assert!(
            text.contains("roll already exists as clips/roll.glb"),
            "{text}"
        );
        assert!(text.contains("overwrite:true"), "{text}");
        assert!(text.contains("nothing was written"), "{text}");
        assert!(text.contains(SHIPPED_RECIPE_HEADING), "{text}");
        assert!(text.contains(EFFECTIVE_RECIPE_HEADING), "{text}");
        let unchanged = Catalog::scan(&project)
            .resolve("roll", Some(Kind::Clip))
            .and_then(|r| r.sidecar.clone())
            .and_then(|s| s.recipe)
            .expect("recipe");
        assert!(
            (unchanged.lean_deg - 4.0).abs() < f32::EPSILON,
            "the refusal wrote nothing"
        );

        // Only the lean is stated: the trims and detrend come from the
        // shipped record, and both recipes come back in full.
        let overwritten = server
            .promote_clip(Parameters(PromoteClipArgs {
                name: String::from("roll"),
                take: take_fixture().display().to_string(),
                lean_deg: Some(6.0),
                overwrite: Some(true),
                ..PromoteClipArgs::default()
            }))
            .await;
        let text = frame_text(&overwritten);
        assert_ne!(overwritten.is_error, Some(true), "{text}");
        assert!(
            text.contains("read from its recipe, not defaulted"),
            "{text}"
        );
        assert!(text.contains(SHIPPED_RECIPE_HEADING), "{text}");
        assert!(text.contains(EFFECTIVE_RECIPE_HEADING), "{text}");
        assert!(text.contains("lean          4.00 deg"), "{text}");
        assert!(text.contains("lean          6.00 deg"), "{text}");
        let now = Catalog::scan(&project)
            .resolve("roll", Some(Kind::Clip))
            .and_then(|r| r.sidecar.clone())
            .and_then(|s| s.recipe)
            .expect("recipe");
        assert!((now.lean_deg - 6.0).abs() < f32::EPSILON);
        assert!(
            (now.trim_start_s - 0.25).abs() < f32::EPSILON,
            "the trim was inherited"
        );
        assert_eq!(
            now.in_place,
            InPlaceMode::Detrend,
            "the in-place mode was inherited"
        );
    }

    #[tokio::test]
    async fn a_missing_take_or_a_bad_name_is_refused_before_anything_is_written() {
        let (_dir, project) = empty_project();
        let server = server(project.clone());
        let missing = server
            .promote_clip(Parameters(PromoteClipArgs {
                take: String::from("out/sweeps/nowhere.npz"),
                ..tool_args("roll", false)
            }))
            .await;
        assert_eq!(missing.is_error, Some(true));
        assert!(
            frame_text(&missing).contains("no take at"),
            "{}",
            frame_text(&missing)
        );

        let bad = server
            .promote_clip(Parameters(tool_args("Gen Roll", false)))
            .await;
        assert_eq!(bad.is_error, Some(true));
        assert!(
            frame_text(&bad).contains("lower-case"),
            "{}",
            frame_text(&bad)
        );
        assert!(Catalog::scan(&project).is_empty(), "nothing was written");
    }

    #[tokio::test]
    async fn generate_clips_with_no_backend_refuses_and_names_doctor() {
        let (dir, mut project) = empty_project();
        // An empty backends directory: every backend is missing, and the
        // refusal is decided before anything is spawned — which is what
        // keeps this test off the GPU and away from the (nonexistent)
        // renderer. `forge.toml`'s `[backends] dir` is what the Python side
        // would read too, via FORGE_BACKENDS.
        let none = dir.path().join("no-backends");
        std::fs::create_dir_all(&none).expect("mkdir");
        project.backends_dir = Some(none);
        let server = server(project.clone());
        let result = server
            .generate_clips(Parameters(GenerateClipsArgs {
                prompt: String::from("A person waves hello."),
                ..GenerateClipsArgs::default()
            }))
            .await;
        assert_eq!(result.is_error, Some(true));
        let text = frame_text(&result);
        assert!(text.contains("doctor"), "{text}");
        assert!(text.contains("nothing was written"), "{text}");
        assert!(
            text.contains("ardy") || text.contains("toolkit"),
            "the refusal names what is missing: {text}"
        );
        assert!(
            !project
                .out_dir(forge_library::project::OutKind::Sweeps)
                .join("0-")
                .exists()
        );
        assert!(Catalog::scan(&project).is_empty());
    }

    #[tokio::test]
    async fn a_stem_taken_by_another_audio_kind_is_refused_with_the_holders() {
        let (dir, project) = empty_project();
        let server = server(project.clone());
        let wav = dir.path().join("bark.wav");
        write_sine_wav(&wav, 0.5);

        let first = server
            .promote_audio(Parameters(PromoteAudioArgs {
                kind: String::from("sfx"),
                name: String::from("bark"),
                file: wav.display().to_string(),
                prompt: Some(String::from("a dog barks once")),
                ..PromoteAudioArgs::default()
            }))
            .await;
        let text = frame_text(&first);
        assert_ne!(first.is_error, Some(true), "{text}");
        assert!(
            text.contains("promoted bark into audio/sfx/bark.wav"),
            "{text}"
        );
        assert!(
            text.contains("sidecar: assets/audio/sfx/bark.json"),
            "{text}"
        );
        assert!(text.contains("catalog: sfx      bark"), "{text}");
        assert!(text.contains("record: none"), "{text}");

        let as_voice = server
            .promote_audio(Parameters(PromoteAudioArgs {
                kind: String::from("speech"),
                name: String::from("bark"),
                file: wav.display().to_string(),
                ..PromoteAudioArgs::default()
            }))
            .await;
        assert_eq!(as_voice.is_error, Some(true));
        let text = frame_text(&as_voice);
        assert!(text.contains("another audio kind"), "{text}");
        assert!(text.contains("sfx:bark (audio/sfx/bark.wav)"), "{text}");

        let again = server
            .promote_audio(Parameters(PromoteAudioArgs {
                kind: String::from("sfx"),
                name: String::from("bark"),
                file: wav.display().to_string(),
                ..PromoteAudioArgs::default()
            }))
            .await;
        assert_eq!(again.is_error, Some(true));
        assert!(
            frame_text(&again).contains("overwrite:true"),
            "{}",
            frame_text(&again)
        );

        let replaced = server
            .promote_audio(Parameters(PromoteAudioArgs {
                kind: String::from("sfx"),
                name: String::from("bark"),
                file: wav.display().to_string(),
                overwrite: Some(true),
                ..PromoteAudioArgs::default()
            }))
            .await;
        let text = frame_text(&replaced);
        assert_ne!(replaced.is_error, Some(true), "{text}");
        assert!(text.contains("replaced the sfx bark"), "{text}");
    }

    /// A half-second 440 Hz sine, 16-bit mono — enough for the decoder and
    /// the duration measurement, and nothing the library has to fetch.
    fn write_sine_wav(path: &std::path::Path, seconds: f32) {
        let rate: u32 = 22_050;
        let frames = (seconds * rate as f32) as u32;
        let data_len = frames * 2;
        let mut bytes = Vec::with_capacity(44 + data_len as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            let t = i as f32 / rate as f32;
            let sample = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
            bytes.extend_from_slice(&((sample * f32::from(i16::MAX)) as i16).to_le_bytes());
        }
        std::fs::write(path, bytes).expect("write the wav");
    }
}
