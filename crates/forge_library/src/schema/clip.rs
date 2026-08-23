//! The clip edit recipe: what turns a raw take into a shipped clip.

use forge_motion::{Edit, InPlace, YMode};
use serde::{Deserialize, Serialize};

use crate::{LibraryError, Result};

/// ARDY takes are 20 fps. Only a fallback: the real value comes from the take
/// or from [`super::Measured::fps`], and this is what a record that lost it
/// gets so a trim in seconds still converts to a plausible frame count.
pub const DEFAULT_FPS: f32 = 20.0;

/// How net root travel is removed, as one value.
///
/// Not a bool plus a mode string, and the bool alone is not enough: on a roll
/// the difference between [`Self::Strip`] and [`Self::Detrend`] is 417 mm of
/// hip travel. One enum, three words.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InPlaceMode {
    /// Keep the take's travel exactly as generated.
    #[default]
    Off,
    /// Pin the hips' X/Z. Right for locomotion loops the game drives.
    Strip,
    /// Remove only the linear drift, keeping within-clip surges. Right for
    /// travelling one-shots — rolls, dodges — where the body genuinely lunges.
    Detrend,
}

impl InPlaceMode {
    /// The lower-case name used in records and on the command line.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Strip => "strip",
            Self::Detrend => "detrend",
        }
    }

    /// Parse the name back. `None` for anything else, so a caller can refuse
    /// with the three valid words rather than silently choosing one.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "off" | "" => Some(Self::Off),
            "strip" => Some(Self::Strip),
            "detrend" => Some(Self::Detrend),
            _ => None,
        }
    }

    /// The manifest's vocabulary for the same three words.
    #[must_use]
    pub const fn root_motion_mode(self) -> forge_manifest::RootMotionMode {
        match self {
            Self::Off => forge_manifest::RootMotionMode::Off,
            Self::Strip => forge_manifest::RootMotionMode::Strip,
            Self::Detrend => forge_manifest::RootMotionMode::Detrend,
        }
    }
}

impl From<InPlace> for InPlaceMode {
    fn from(value: InPlace) -> Self {
        match value {
            InPlace::Off => Self::Off,
            InPlace::Strip => Self::Strip,
            InPlace::Detrend => Self::Detrend,
        }
    }
}

impl From<InPlaceMode> for InPlace {
    fn from(value: InPlaceMode) -> Self {
        match value {
            InPlaceMode::Off => Self::Off,
            InPlaceMode::Strip => Self::Strip,
            InPlaceMode::Detrend => Self::Detrend,
        }
    }
}

/// How net root **height** is removed, as one value.
///
/// Independent of [`InPlaceMode`], which only ever treats X/Z: the recentre
/// every clip gets leaves Y alone because it is real height above the floor,
/// and this is the only knob that may move it. Jump, land and vault takes are
/// why it exists — the game moves the capsule, so the clip must not carry the
/// lift as well.
///
/// Absent from a record means [`Self::Off`], which is what every recipe
/// written before the knob existed says, and what keeps them rebuilding
/// byte-identically.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RootYMode {
    /// Keep the take's height exactly as generated.
    #[default]
    Off,
    /// Pin root Y to frame 0's value, flattening dips as well as travel.
    Strip,
    /// Remove only the linear drift from first frame to last, keeping surges
    /// like a landing crouch.
    Detrend,
}

impl RootYMode {
    /// The lower-case name used in records and on the command line.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Strip => "strip",
            Self::Detrend => "detrend",
        }
    }

    /// Parse the name back. `None` for anything else, so a caller can refuse
    /// with the three valid words rather than silently choosing one.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "off" | "" => Some(Self::Off),
            "strip" => Some(Self::Strip),
            "detrend" => Some(Self::Detrend),
            _ => None,
        }
    }
}

impl From<YMode> for RootYMode {
    fn from(value: YMode) -> Self {
        match value {
            YMode::Off => Self::Off,
            YMode::Strip => Self::Strip,
            YMode::Detrend => Self::Detrend,
        }
    }
}

impl From<RootYMode> for YMode {
    fn from(value: RootYMode) -> Self {
        match value {
            RootYMode::Off => Self::Off,
            RootYMode::Strip => Self::Strip,
            RootYMode::Detrend => Self::Detrend,
        }
    }
}

/// Which automatic trim search chose the window this recipe records.
///
/// Provenance rather than instruction: the recipe stores the trims the search
/// *resolved to*, so re-baking must not run it again — the bake applies
/// `trim_start_s`/`trim_end_s` and nothing re-runs the search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutoTrim {
    /// Cut the cleanest gait cycle out of a locomotion take.
    Loop,
    /// Cut the active span out of a one-shot.
    Action,
}

/// A full edit recipe, in the units the command line uses.
///
/// This mirrors [`forge_motion::Edit`], which is the thing that actually
/// applies the maths, with two differences that are both deliberate: trims are
/// in **seconds** here and frames there, because seconds is what the sidecar
/// stores and what a human types; and `retime` is the `src:dst,…` spec
/// string, because that is what the sidecar stores and what the CLI parses.
///
/// # Why every field defaults to identity
///
/// The struct is `#[serde(default)]` with a hand-written [`Default`] whose
/// values are the identity edit — `exaggerate: 1.0`, everything else zero or
/// off. That is not a convenience. The first generation of sidecars simply
/// did not have the five style keys, and the code that read them made up the
/// same identity values in an ad-hoc `unwrap_or` per field. Writing the
/// contract down as `Default` makes it one statement that a test can check,
/// instead of five literals scattered through a parser that nothing checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipRecipe {
    /// Seconds cut from the start. Takes tend to open with a settle.
    pub trim_start_s: f32,
    /// Seconds cut from the end. Takes tend to close with a drift.
    pub trim_end_s: f32,
    /// Which auto-trim search produced those trims, if any.
    pub auto_trim: Option<AutoTrim>,
    /// Root travel treatment. X/Z only.
    pub in_place: InPlaceMode,
    /// Root height treatment. Absent means [`RootYMode::Off`], so every
    /// recipe written before this key existed still rebuilds unchanged.
    pub y_mode: RootYMode,
    /// Whether the tail is blended back to frame 0 and the clip named as a
    /// loop. Kept separate from `loop_blend_s` because it also drives the
    /// `-loop` suffix on the clip name inside the `.glb`.
    #[serde(rename = "loop")]
    pub looping: bool,
    /// Seconds of tail blended toward frame 0. Capped at half the clip by the
    /// baker, so a 0.4 s blend on a 16-frame stride would rewrite half of it.
    pub loop_blend_s: f32,
    /// Scale on each arm joint's deviation from its clip-mean pose. `1.0` is
    /// unchanged. Legs are excluded so foot contacts survive.
    pub exaggerate: f32,
    /// Constant forward bend on the forearms, degrees — runner arms.
    pub arm_bend_deg: f32,
    /// Forward lean, degrees, split down three spine joints.
    pub lean_deg: f32,
    /// Upper-arm pull-back, degrees, so the hands ride beside the hips.
    pub shoulder_back_deg: f32,
    /// Piecewise time remap as `src:dst,src:dst,…`, or `None` for no remap.
    ///
    /// Stored as the spec string rather than parsed pairs so that what the
    /// record holds is exactly what the CLI takes. This field was once
    /// write-only: the sidecar recorded it and nothing could read it back, so
    /// a re-bake of a retimed clip silently dropped the retime.
    pub retime: Option<String>,
    /// Name of the animation inside the built `.glb`.
    ///
    /// Part of the recipe because an engine binds clips by name: re-baking a
    /// clip under a different internal name leaves every animation player
    /// looking it up finding nothing, and finding nothing is exactly what an
    /// engine reports as silence rather than as an error.
    pub clip: Option<String>,
}

impl Default for ClipRecipe {
    fn default() -> Self {
        Self {
            trim_start_s: 0.0,
            trim_end_s: 0.0,
            auto_trim: None,
            in_place: InPlaceMode::Off,
            y_mode: RootYMode::Off,
            looping: false,
            loop_blend_s: 0.0,
            exaggerate: 1.0,
            arm_bend_deg: 0.0,
            lean_deg: 0.0,
            shoulder_back_deg: 0.0,
            retime: None,
            clip: None,
        }
    }
}

impl ClipRecipe {
    /// The recipe as [`forge_motion`] applies it, with trims converted to
    /// frames.
    ///
    /// `fps` comes from the take or from the sidecar's measured block; a
    /// non-positive value falls back to [`DEFAULT_FPS`] rather than producing
    /// a clip trimmed to nothing.
    ///
    /// # Errors
    ///
    /// Fails only when `retime` does not parse. That is the check a promote
    /// runs before it writes anything: a recipe that cannot become an `Edit`
    /// is not bakeable, and it is far better to find that out before the
    /// take is copied than half way through a bake.
    pub fn to_edit(&self, fps: f32) -> Result<Edit> {
        let fps = if fps > 0.0 { fps } else { DEFAULT_FPS };
        let frames = |seconds: f32| (seconds * fps).round().max(0.0) as usize;
        Ok(Edit {
            trim_start: frames(self.trim_start_s),
            trim_end: frames(self.trim_end_s),
            retime: match self.retime.as_deref() {
                None => Vec::new(),
                Some(spec) => parse_retime(spec)?,
            },
            in_place: self.in_place.into(),
            y_mode: self.y_mode.into(),
            arm_bend_deg: self.arm_bend_deg,
            lean_deg: self.lean_deg,
            shoulder_back_deg: self.shoulder_back_deg,
            exaggerate: self.exaggerate,
            loop_blend_s: self.loop_blend_s,
        })
    }

    /// The recipe an [`Edit`] describes, with trims converted back to seconds.
    ///
    /// The fields an `Edit` has no opinion about — which auto-trim found the
    /// window, what the clip is called inside the `.glb` — come back empty.
    /// Use [`Self::with_edit`] to change the knobs on a recipe that already
    /// exists, which is what the studio's take preview does.
    #[must_use]
    pub fn from_edit(edit: &Edit, fps: f32) -> Self {
        let fps = if fps > 0.0 { fps } else { DEFAULT_FPS };
        Self {
            trim_start_s: edit.trim_start as f32 / fps,
            trim_end_s: edit.trim_end as f32 / fps,
            auto_trim: None,
            in_place: edit.in_place.into(),
            y_mode: edit.y_mode.into(),
            // A blend of zero seconds is not a loop; recording `loop: true`
            // beside `loop_blend_s: 0.0` would put a `-loop` suffix on a clip
            // that does not close.
            looping: edit.loop_blend_s > 0.0,
            loop_blend_s: edit.loop_blend_s,
            exaggerate: edit.exaggerate,
            arm_bend_deg: edit.arm_bend_deg,
            lean_deg: edit.lean_deg,
            shoulder_back_deg: edit.shoulder_back_deg,
            retime: (!edit.retime.is_empty()).then(|| format_retime(&edit.retime)),
            clip: None,
        }
    }

    /// This recipe with the knob values replaced by `edit`, keeping everything
    /// an `Edit` cannot express.
    #[must_use]
    pub fn with_edit(&self, edit: &Edit, fps: f32) -> Self {
        Self {
            auto_trim: self.auto_trim,
            clip: self.clip.clone(),
            ..Self::from_edit(edit, fps)
        }
    }

    /// The recipe, one knob per line, in the units the arguments use.
    ///
    /// Echoed on every promote so the caller can see what it is about to bake
    /// rather than assume: with an existing name, half of these values may
    /// have come from the clip being replaced.
    #[must_use]
    pub fn lines(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "  trim          {:.3}s off the start, {:.3}s off the end",
            self.trim_start_s, self.trim_end_s
        );
        let _ = writeln!(out, "  in_place      {}", self.in_place.as_str());
        let _ = writeln!(out, "  y_mode        {}", self.y_mode.as_str());
        let _ = writeln!(
            out,
            "  loop          {}",
            if self.looping {
                format!("yes, {:.3}s blend", self.loop_blend_s)
            } else {
                String::from("no")
            }
        );
        let _ = writeln!(out, "  exaggerate    {:.3}", self.exaggerate);
        let _ = writeln!(out, "  arm_bend      {:.2} deg", self.arm_bend_deg);
        let _ = writeln!(out, "  lean          {:.2} deg", self.lean_deg);
        let _ = writeln!(out, "  shoulder_back {:.2} deg", self.shoulder_back_deg);
        if let Some(retime) = &self.retime {
            let _ = writeln!(out, "  retime        {retime}");
        }
        if let Some(clip) = &self.clip {
            let _ = writeln!(out, "  clip name     {clip}");
        }
        out
    }
}

/// The knobs a caller stated, and nothing else.
///
/// A promote over an existing name starts from the shipped clip's recorded
/// recipe and replaces only the knobs the caller actually said — see
/// [`overlay_recipe`]. Every field is an [`Option`] so that "not stated" is
/// distinguishable from "stated as zero": a stated `loop_blend_s: 0.0` turns
/// an inherited loop *off*, and an unstated one leaves it alone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PartialRecipe {
    /// Seconds trimmed from the start.
    pub trim_start_s: Option<f32>,
    /// Seconds trimmed from the end.
    pub trim_end_s: Option<f32>,
    /// Root travel treatment.
    pub in_place: Option<InPlaceMode>,
    /// Root height treatment.
    pub y_mode: Option<RootYMode>,
    /// Scale on arm-swing deviation from the clip mean.
    pub exaggerate: Option<f32>,
    /// Elbow bend in degrees.
    pub arm_bend_deg: Option<f32>,
    /// Forward lean in degrees.
    pub lean_deg: Option<f32>,
    /// Upper-arm pull-back in degrees.
    pub shoulder_back_deg: Option<f32>,
    /// Seconds of tail blended back to frame 0. Stating any value above zero
    /// makes the clip a loop; stating zero makes it not one.
    pub loop_blend_s: Option<f32>,
    /// Piecewise time remap as `src:dst,…`. An empty string removes an
    /// inherited one. Validated when the recipe becomes an `Edit` — see
    /// [`parse_retime`] to refuse early with the part named.
    pub retime: Option<String>,
    /// The animation's name inside the `.glb`.
    pub clip: Option<String>,
}

/// The shipped recipe with only the stated knobs replaced.
///
/// # The trim-wipe fix
///
/// When a promote's target name already exists, the old generator resolved
/// every argument the caller did not state by inheriting it from the sidecar
/// of the clip it was about to overwrite — an argv missing so much as
/// `--no-loop` produced a plausible clip built to somebody else's recipe. The
/// rule now is the opposite and explicit: the shipped recipe is the starting
/// point, the caller's stated knobs land on top of it, and the **whole**
/// effective recipe reaches the bake and comes back in the response, so the
/// caller sees what it baked rather than assuming.
#[must_use]
pub fn overlay_recipe(shipped: &ClipRecipe, requested: &PartialRecipe) -> ClipRecipe {
    let mut recipe = shipped.clone();
    if let Some(value) = requested.trim_start_s {
        recipe.trim_start_s = value;
    }
    if let Some(value) = requested.trim_end_s {
        recipe.trim_end_s = value;
    }
    if let Some(value) = requested.exaggerate {
        recipe.exaggerate = value;
    }
    if let Some(value) = requested.arm_bend_deg {
        recipe.arm_bend_deg = value;
    }
    if let Some(value) = requested.lean_deg {
        recipe.lean_deg = value;
    }
    if let Some(value) = requested.shoulder_back_deg {
        recipe.shoulder_back_deg = value;
    }
    if let Some(value) = requested.loop_blend_s {
        // A loop blend does nothing without the loop flag, so a stated blend
        // is what says "close this into a loop" — and a stated zero says the
        // opposite, which is how an inherited loop gets turned off.
        recipe.loop_blend_s = value;
        recipe.looping = value > 0.0;
    }
    if let Some(mode) = requested.in_place {
        recipe.in_place = mode;
    }
    if let Some(mode) = requested.y_mode {
        recipe.y_mode = mode;
    }
    if let Some(spec) = requested.retime.as_deref() {
        let spec = spec.trim();
        recipe.retime = (!spec.is_empty()).then(|| spec.to_owned());
    }
    if let Some(clip) = &requested.clip {
        recipe.clip = Some(clip.clone());
    }
    recipe
}

/// Parse a `src:dst,src:dst,…` retime spec into keypoints, sorted.
///
/// Pinned against the retired Python `parse_retime`, including the sort:
/// that Python sorted the pairs before interpolating, so the sort is part of
/// the format and not an implementation detail — a spec written out of order
/// has always meant the same thing.
///
/// # Errors
///
/// Fails on a part with no colon or an unparseable number, naming the part —
/// a retime is typed by hand and a silent partial parse would produce a clip
/// with plausible-looking wrong timing.
pub fn parse_retime(spec: &str) -> Result<Vec<(f32, f32)>> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Ok(Vec::new());
    }
    let mut pairs = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (source, destination) = part.split_once(':').ok_or_else(|| LibraryError::Retime {
            spec: spec.to_owned(),
            detail: format!("{part:?} is not src:dst"),
        })?;
        let parse = |text: &str, which: &str| {
            text.trim()
                .parse::<f32>()
                .map_err(|_| LibraryError::Retime {
                    spec: spec.to_owned(),
                    detail: format!("{which} of {part:?} is not a number"),
                })
        };
        pairs.push((
            parse(source, "the source time")?,
            parse(destination, "the output time")?,
        ));
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    Ok(pairs)
}

/// Write keypoints back as a spec string, so a recipe round-trips.
#[must_use]
pub fn format_retime(pairs: &[(f32, f32)]) -> String {
    pairs
        .iter()
        .map(|(source, destination)| format!("{source}:{destination}"))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_recipe_is_the_identity_edit() {
        let edit = ClipRecipe::default().to_edit(20.0).expect("to_edit");
        assert!(edit.is_identity(), "{edit:?}");
    }

    #[test]
    fn a_missing_field_is_identity_not_zero() {
        // A recipe with none of the style keys; a zero `exaggerate` would
        // flatten every arm to its clip-mean pose.
        let recipe: ClipRecipe = serde_json::from_str("{}").expect("empty object");
        assert_eq!(recipe, ClipRecipe::default());
        assert!((recipe.exaggerate - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn y_mode_round_trips_through_json_and_reaches_the_edit() {
        let recipe = ClipRecipe {
            y_mode: RootYMode::Detrend,
            ..ClipRecipe::default()
        };
        let json = serde_json::to_string(&recipe).expect("serialize");
        assert!(json.contains("\"y_mode\":\"detrend\""), "{json}");
        let back: ClipRecipe = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, recipe);
        assert_eq!(back.to_edit(20.0).expect("to_edit").y_mode, YMode::Detrend);
        assert_eq!(
            ClipRecipe::from_edit(&back.to_edit(20.0).expect("to_edit"), 20.0).y_mode,
            RootYMode::Detrend
        );
    }

    #[test]
    fn a_recipe_without_y_mode_is_off_and_still_the_identity_edit() {
        let legacy = r#"{"trim_start_s":0.25,"trim_end_s":1.3,"in_place":"detrend"}"#;
        let recipe: ClipRecipe = serde_json::from_str(legacy).expect("legacy recipe");
        assert_eq!(recipe.y_mode, RootYMode::Off);
        assert_eq!(
            recipe.to_edit(20.0).expect("to_edit").y_mode,
            YMode::Off,
            "the absent key must not reach the bake as anything else"
        );
    }

    #[test]
    fn mode_names_match_their_serde_spelling() {
        for mode in [RootYMode::Off, RootYMode::Strip, RootYMode::Detrend] {
            let json = serde_json::to_string(&mode).expect("serialize");
            assert_eq!(json, format!("\"{}\"", mode.as_str()));
            assert_eq!(RootYMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(RootYMode::parse("flatten"), None);
        for mode in [InPlaceMode::Off, InPlaceMode::Strip, InPlaceMode::Detrend] {
            let json = serde_json::to_string(&mode).expect("serialize");
            assert_eq!(json, format!("\"{}\"", mode.as_str()));
            assert_eq!(InPlaceMode::parse(mode.as_str()), Some(mode));
            assert_eq!(mode.root_motion_mode().as_str(), mode.as_str());
        }
    }

    #[test]
    fn retime_parses_sorted_and_round_trips() {
        let pairs = parse_retime("1.5:0.72,0.3:0,0.6:0.12").expect("parse");
        assert_eq!(pairs, vec![(0.3, 0.0), (0.6, 0.12), (1.5, 0.72)]);
        assert_eq!(format_retime(&pairs), "0.3:0,0.6:0.12,1.5:0.72");
        assert_eq!(
            parse_retime(&format_retime(&pairs)).expect("reparse"),
            pairs
        );
    }

    #[test]
    fn a_malformed_retime_names_the_part() {
        let error = parse_retime("0.3:0,nonsense").expect_err("should refuse");
        assert!(format!("{error}").contains("nonsense"), "{error}");
    }

    #[test]
    fn trims_survive_the_frame_conversion() {
        let recipe = ClipRecipe {
            trim_start_s: 0.25,
            trim_end_s: 1.3,
            ..ClipRecipe::default()
        };
        let edit = recipe.to_edit(20.0).expect("to_edit");
        // The roll fixture's own numbers.
        assert_eq!(edit.trim_start, 5);
        assert_eq!(edit.trim_end, 26);
        let back = ClipRecipe::from_edit(&edit, 20.0);
        assert!((back.trim_start_s - 0.25).abs() < 1e-6, "{back:?}");
        assert!((back.trim_end_s - 1.3).abs() < 1e-6, "{back:?}");
    }

    #[test]
    fn with_edit_keeps_what_an_edit_cannot_say() {
        let recipe = ClipRecipe {
            auto_trim: Some(AutoTrim::Action),
            clip: Some(String::from("Roll")),
            ..ClipRecipe::default()
        };
        let mut edit = recipe.to_edit(20.0).expect("to_edit");
        edit.lean_deg = 8.0;
        let tuned = recipe.with_edit(&edit, 20.0);
        assert_eq!(tuned.auto_trim, Some(AutoTrim::Action));
        assert_eq!(tuned.clip.as_deref(), Some("Roll"));
        assert!((tuned.lean_deg - 8.0).abs() < f32::EPSILON);
    }

    #[test]
    fn overlay_replaces_only_what_was_stated() {
        let shipped = ClipRecipe {
            trim_start_s: 0.25,
            trim_end_s: 1.3,
            in_place: InPlaceMode::Detrend,
            looping: true,
            loop_blend_s: 0.15,
            lean_deg: 4.0,
            retime: Some(String::from("0.5:0.4")),
            clip: Some(String::from("Roll")),
            ..ClipRecipe::default()
        };
        // Nothing stated: the shipped recipe comes back whole.
        assert_eq!(overlay_recipe(&shipped, &PartialRecipe::default()), shipped);

        let tuned = overlay_recipe(
            &shipped,
            &PartialRecipe {
                trim_end_s: Some(1.0),
                loop_blend_s: Some(0.0),
                retime: Some(String::from("  ")),
                ..PartialRecipe::default()
            },
        );
        assert!(
            (tuned.trim_start_s - 0.25).abs() < f32::EPSILON,
            "inherited"
        );
        assert!((tuned.trim_end_s - 1.0).abs() < f32::EPSILON, "stated");
        assert!(!tuned.looping, "a stated zero blend turns the loop off");
        assert_eq!(
            tuned.retime, None,
            "an empty retime removes the inherited one"
        );
        assert_eq!(tuned.in_place, InPlaceMode::Detrend, "inherited");
        assert_eq!(tuned.clip.as_deref(), Some("Roll"), "inherited");
        assert!((tuned.lean_deg - 4.0).abs() < f32::EPSILON, "inherited");

        let looped = overlay_recipe(
            &ClipRecipe::default(),
            &PartialRecipe {
                loop_blend_s: Some(0.2),
                in_place: Some(InPlaceMode::Strip),
                ..PartialRecipe::default()
            },
        );
        assert!(looped.looping);
        assert_eq!(looped.in_place, InPlaceMode::Strip);
    }
}
