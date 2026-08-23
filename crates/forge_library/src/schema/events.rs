//! Gameplay metadata a clip's record carries: timeline events and the
//! root-motion track the bake would otherwise throw away.
//!
//! Both exist because the pipeline computes these facts and then discards
//! them: ARDY labels foot contacts as it generates, and the bake measures the
//! hips' travel right before `in_place` deletes it. A game needs exactly
//! those two things — when to play a footstep, and how fast to move a
//! character playing an in-place loop — and re-deriving them downstream means
//! guessing at what the pipeline already knew.
//!
//! # The two clocks
//!
//! An event has two homes in time. [`AnimEvent::t_src`] is **take seconds** —
//! where the moment lives on the raw `.npz`, which is the durable source a
//! recipe replays on top of. [`AnimEvent::t`] is **built-clip seconds** — the
//! same moment after the recipe's trim and retime, which is what a game
//! samples. `t_src` is the stored truth for authored events: it survives a
//! reviewer tuning the trim knobs, while `t` is recomputed at every bake
//! through [`forge_motion::Edit::map_time`] and verified against it. Derived
//! events (`origin: "contacts"`) are recomputed wholesale instead.
//!
//! # Rounding
//!
//! Every float in this module is rounded to three decimals on write — a
//! millisecond, a millimetre — because these records are git-tracked and
//! reviewed as diffs, and the noise digits of an `f32` would bury the change
//! that matters. Rounding lives in the serializers rather than in the
//! constructors so the byte contract holds no matter who built the value.

use serde::{Deserialize, Serialize};

use super::{Actor, Kind};

/// Whether a name works as an event name: lower-case letters, digits and
/// underscores, at least one of them.
///
/// The same alphabet as asset names, for the same reason: event names end up
/// as keys in game code and as labels on contact-sheet strips, and a name
/// that survives both is a small set.
#[must_use]
pub fn valid_event_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// One named instant on a clip's timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimEvent {
    /// When it happens on the **built clip**, seconds from frame 0. This is
    /// the value a game samples, and it is recomputed at every bake.
    #[serde(serialize_with = "ser3")]
    pub t: f32,
    /// When it happens on the **source take**, seconds — the stored truth for
    /// authored events. `None` for events that only exist on the built clip,
    /// which is a fact about how they were authored, not a gap to fill in.
    #[serde(serialize_with = "ser3_opt")]
    pub t_src: Option<f32>,
    /// Event name, `[a-z0-9_]+` — see [`valid_event_name`]. `footstep_l` and
    /// `footstep_r` are the two names contact derivation is allowed to use.
    pub name: String,
    /// Who or what placed the event.
    pub origin: EventOrigin,
    /// A sound to hang off the event, when one is linked.
    pub audio: Option<AudioRef>,
}

/// Who or what placed an event on the timeline.
///
/// Serialised as one string — `"contacts"`, `"human"`, `"agent:<name>"`,
/// `"unknown"` — the same shape as [`Actor`] plus one machine origin, and for
/// the same reason: it fits in a table cell and a `git diff`, and agent names
/// are free-form.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EventOrigin {
    /// Derived from ARDY's per-frame foot-contact labels at bake time.
    /// Recomputed on every bake, never carried forward.
    Contacts,
    /// Somebody placed it by hand.
    Human,
    /// An agent placed it, named however it named itself.
    Agent(String),
    /// The record predates anyone recording this.
    #[default]
    Unknown,
}

impl EventOrigin {
    /// Read an origin from the string form. Everything that is not
    /// `"contacts"` goes through [`Actor::parse`], so an unfamiliar name is
    /// kept as an agent name rather than laundered into unknown.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        if text.trim() == "contacts" {
            return Self::Contacts;
        }
        match Actor::parse(text) {
            Actor::Human => Self::Human,
            Actor::Agent(name) => Self::Agent(name),
            Actor::Unknown => Self::Unknown,
        }
    }

    /// The origin an actor's authored event carries.
    #[must_use]
    pub fn from_actor(actor: &Actor) -> Self {
        match actor {
            Actor::Human => Self::Human,
            Actor::Agent(name) => Self::Agent(name.clone()),
            Actor::Unknown => Self::Unknown,
        }
    }

    /// Whether the event was derived from contact labels, which pins its name
    /// to the footstep vocabulary.
    #[must_use]
    pub const fn is_contacts(&self) -> bool {
        matches!(self, Self::Contacts)
    }
}

impl std::fmt::Display for EventOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contacts => f.write_str("contacts"),
            Self::Human => f.write_str("human"),
            Self::Agent(name) => write!(f, "agent:{name}"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

impl Serialize for EventOrigin {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for EventOrigin {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Ok(Self::parse(&text))
    }
}

/// A link from an event to a shipped sound, serialised as `"sfx:pistol_shot"`
/// — the kind's lower-case name, a colon, the sound's stem.
///
/// One string rather than an object because that is the form agents already
/// pass names around in, and because a link is a reference, not a record:
/// everything true about the sound lives in the sound's own sidecar. The
/// manifest carries the same string; [`forge_manifest::AudioLink`] is its
/// consumer-side reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioRef {
    /// Which audio kind the target lives under.
    pub kind: Kind,
    /// The target's file stem.
    pub name: String,
}

impl AudioRef {
    /// Read a reference from the `kind:name` form. `None` for anything else,
    /// including a non-audio kind — a clip "linked" to another clip is not a
    /// sound cue, and accepting it would defer the confusion to a game.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (kind, name) = text.split_once(':')?;
        let kind = Kind::parse(kind)?;
        if !kind.is_audio() {
            return None;
        }
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        Some(Self {
            kind,
            name: name.to_owned(),
        })
    }
}

impl std::fmt::Display for AudioRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.kind, self.name)
    }
}

impl Serialize for AudioRef {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for AudioRef {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "{text:?} is not an audio reference — write kind:name \
                 with kind one of sfx, music, voice"
            ))
        })
    }
}

/// What the root actually did before `in_place` treated it — the measurement
/// the bake destroys and a character controller needs back.
///
/// Lives in [`super::Measured`] because it is an output of the bake, not an
/// instruction to it: the recipe's `in_place` says what to remove, and this
/// records what was removed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootMotion {
    /// Net XZ travel from the first built frame to the last, metres, as
    /// `[x, z]` in the built clip's ground plane — rig space, character
    /// facing −Z (`Edit::apply` turns the take before anything measures it).
    #[serde(serialize_with = "ser3_pair")]
    pub net_m: [f32; 2],
    /// Heading of that net travel in the ground plane, degrees. `None` when
    /// the clip does not travel enough for a heading to mean anything —
    /// never zero standing in for "no direction".
    #[serde(serialize_with = "ser3_opt")]
    pub direction_deg: Option<f32>,
    /// Fastest frame-to-frame root speed over the clip, metres per second,
    /// when it was measured.
    #[serde(serialize_with = "ser3_opt")]
    pub peak_speed_mps: Option<f32>,
    /// The pre-strip XZ root position per **built** frame, metres,
    /// millimetre-rounded, in the same rig-space plane as [`Self::net_m`].
    /// Same clock as the clip, so a game samples it by playback time with
    /// no further mapping — forward travel runs down −Z, like the facing.
    #[serde(serialize_with = "ser3_track")]
    pub track_xz_m: Vec<[f32; 2]>,
}

/// Three-decimal rounding, the write-side contract of this module.
fn round3(value: f32) -> f32 {
    let rounded = (f64::from(value) * 1000.0).round() / 1000.0;
    rounded as f32
}

// The next three take references because that is `serialize_with`'s contract,
// not because it is idiomatic for a Copy type.

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's serialize_with passes the field by reference"
)]
fn ser3<S: serde::Serializer>(value: &f32, serializer: S) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_f32(round3(*value))
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    clippy::ref_option,
    reason = "serde's serialize_with passes the field by reference"
)]
fn ser3_opt<S: serde::Serializer>(
    value: &Option<f32>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    match value {
        Some(v) => serializer.serialize_some(&round3(*v)),
        None => serializer.serialize_none(),
    }
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's serialize_with passes the field by reference"
)]
fn ser3_pair<S: serde::Serializer>(
    value: &[f32; 2],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.collect_seq(value.iter().map(|v| round3(*v)))
}

fn ser3_track<S: serde::Serializer>(
    value: &[[f32; 2]],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.collect_seq(value.iter().map(|p| [round3(p[0]), round3(p[1])]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_names_are_the_asset_name_alphabet() {
        assert!(valid_event_name("footstep_l"));
        assert!(valid_event_name("fire2"));
        for bad in ["", "Footstep", "foot step", "foot-step", "step!"] {
            assert!(!valid_event_name(bad), "{bad:?} should be refused");
        }
    }

    #[test]
    fn origins_round_trip_and_contacts_beats_the_agent_fallback() {
        for (text, origin) in [
            ("contacts", EventOrigin::Contacts),
            ("human", EventOrigin::Human),
            ("agent:claude", EventOrigin::Agent(String::from("claude"))),
            ("unknown", EventOrigin::Unknown),
            ("", EventOrigin::Unknown),
        ] {
            assert_eq!(EventOrigin::parse(text), origin, "{text:?}");
        }
        // Actor::parse would keep "contacts" as an agent name; the origin
        // parser must claim it first or every derived event mislabels itself.
        assert_eq!(EventOrigin::parse("contacts"), EventOrigin::Contacts);
        assert_eq!(
            EventOrigin::Agent(String::from("claude")).to_string(),
            "agent:claude"
        );
        assert_eq!(
            EventOrigin::from_actor(&Actor::Agent(String::from("x"))),
            EventOrigin::Agent(String::from("x"))
        );
    }

    #[test]
    fn an_audio_ref_is_one_string_both_ways() {
        let audio = AudioRef::parse("sfx:pistol_shot").expect("parse");
        assert_eq!(audio.kind, Kind::Sfx);
        assert_eq!(audio.name, "pistol_shot");
        assert_eq!(audio.to_string(), "sfx:pistol_shot");

        let json = serde_json::to_string(&audio).expect("serialise");
        assert_eq!(json, "\"sfx:pistol_shot\"");
        let back: AudioRef = serde_json::from_str(&json).expect("parse back");
        assert_eq!(back, audio);
        // And the manifest reads the same string.
        let link = forge_manifest::AudioLink::parse(&audio.to_string()).expect("manifest link");
        assert_eq!(link.kind, forge_manifest::AudioKind::Sfx);
    }

    #[test]
    fn a_reference_that_is_not_a_sound_is_refused_loudly() {
        assert_eq!(AudioRef::parse("clip:walk"), None);
        assert_eq!(AudioRef::parse("pistol_shot"), None);
        assert_eq!(AudioRef::parse("sfx:"), None);
        assert_eq!(AudioRef::parse("laser:zap"), None);
        let error = serde_json::from_str::<AudioRef>("\"clip:walk\"")
            .expect_err("must refuse a clip-to-clip link");
        assert!(error.to_string().contains("kind:name"), "{error}");
    }

    #[test]
    fn times_are_rounded_to_three_decimals_on_write_only() {
        let event = AnimEvent {
            t: 0.150_432_1,
            t_src: Some(4.100_987_6),
            name: String::from("footstep_l"),
            origin: EventOrigin::Contacts,
            audio: None,
        };
        // The in-memory value is untouched; the bytes are the contract.
        let json = serde_json::to_string(&event).expect("serialise");
        assert!(json.contains("\"t\":0.15,"), "{json}");
        assert!(json.contains("\"t_src\":4.101"), "{json}");
        assert!((event.t - 0.150_432_1).abs() < f32::EPSILON);
    }

    #[test]
    fn the_root_track_is_millimetre_rounded() {
        let motion = RootMotion {
            net_m: [2.611_111, -0.000_4],
            direction_deg: Some(87.654_32),
            peak_speed_mps: None,
            track_xz_m: vec![[0.0, 0.0], [0.123_456, 0.999_999_5]],
        };
        let json = serde_json::to_string(&motion).expect("serialise");
        assert!(json.contains("[2.611,-0.0]"), "{json}");
        assert!(json.contains("87.654"), "{json}");
        assert!(json.contains("\"peak_speed_mps\":null"), "{json}");
        assert!(json.contains("[0.123,1.0]"), "{json}");
    }

    #[test]
    fn rounding_is_idempotent_so_a_resave_is_no_diff() {
        let event = AnimEvent {
            t: 0.163_499_9,
            t_src: None,
            name: String::from("fire"),
            origin: EventOrigin::Agent(String::from("claude")),
            audio: AudioRef::parse("sfx:pistol_shot"),
        };
        let once = serde_json::to_string(&event).expect("serialise");
        let reloaded: AnimEvent = serde_json::from_str(&once).expect("parse");
        let twice = serde_json::to_string(&reloaded).expect("serialise again");
        assert_eq!(once, twice);
    }
}
