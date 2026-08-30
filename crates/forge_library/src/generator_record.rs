//! The record a generator writes: `forge_record: 2`, read here, written by
//! Python.
//!
//! Two schemas with a clean boundary — **Python writes generator records,
//! Rust writes library sidecars.** A generator record is the account of one
//! run: which backend at which commit, what it was handed (hashed), every
//! knob it was given (`null` where it was not), what it produced (hashed),
//! and whether it was a `--fake` placeholder. A promote reads one of these to
//! fill a sidecar's `generator` block and set its provenance to `recorded`;
//! without one the sidecar says `reconstructed` with nulls, and nothing here
//! ever invents a value a record did not carry.
//!
//! The field order of the structs below is the field order `python/forge_gen/
//! records.py` writes, so a record re-serialised by this reader is the same
//! bytes — the property the byte-equality fixture test pins once the Python
//! writer lands (P2). One rule the Python side carries for that to hold: the
//! free-form `params` and `measured` objects are written with their keys
//! **sorted**, because this reader holds them in a sorted map and would
//! re-emit them that way. A lift record lives beside the PNG it was made from
//! as `<name>.lift.json`; [`GeneratorRecord::lift_beside`] finds it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::schema::{AceStepParams, ArdyParams, LiftParams, SoundEffectParams, SpeechParams};
use crate::{LibraryError, Result, read_bytes};

/// The generator record schema this build **writes**.
pub const RECORD_SCHEMA: u64 = 2;

/// The oldest schema this build reads.
///
/// **Both readers accept 1 and 2, and only 2 is ever written.** Nothing
/// under `assets/` or `assets-src/` was rewritten when the schema went to 2:
/// adding four nulls to a shipped sidecar is churn with no new fact in it,
/// and the library sidecar stays `schema: 1`. A v1 generator record simply
/// has no `executor`, `comfyui_commit`, `workflow_sha256` or `packs` — which
/// is what `null` already means everywhere else in this type.
pub const RECORD_SCHEMA_MIN: u64 = 1;

/// What kind of run a record describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordKind {
    /// A reference import: a drawn PNG in, the same bytes under
    /// `assets-src/refs/` out. Nothing generated it; it was brought.
    Ref,
    /// A TRELLIS.2 lift: PNG in, raw textured mesh out.
    Lift,
    /// A prop normalize: raw lift in, metres-and-matte prop out.
    Prop,
    /// A prepare: raw lift in, normalised mesh plus a bare skeleton out,
    /// which `forge gen skin` then hashes as its `mesh` input.
    Prepare,
    /// A skin: prepared glb in, rigged `.blend` on a skeleton fitted to
    /// this body out.
    Rig,
    /// A body export: rigged `.blend` in, self-contained `.glb` out.
    Export,
    /// An ARDY take: prompt in, `.npz` out.
    Take,
    /// A MOSS sound effect.
    Sfx,
    /// An ACE-Step track.
    Music,
    /// A MOSS TTS line.
    Speech,
    /// A MOSS `VoiceGenerator` design: a description in, the audition clip
    /// that every line of that character is then cloned from out.
    Voice,
}

impl RecordKind {
    /// The lower-case name used in records.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ref => "ref",
            Self::Lift => "lift",
            Self::Prop => "prop",
            Self::Prepare => "prepare",
            Self::Rig => "rig",
            Self::Export => "export",
            Self::Take => "take",
            Self::Sfx => "sfx",
            Self::Music => "music",
            Self::Speech => "speech",
            Self::Voice => "voice",
        }
    }
}

impl std::fmt::Display for RecordKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which backend ran, pinned.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordBackend {
    /// The backend directory's name: `trellis2`, `ardy`, `acestep`,
    /// `moss_sfx`, `moss_tts`.
    pub name: Option<String>,
    /// The upstream checkout's commit.
    pub commit: Option<String>,
    /// The interpreter version that ran.
    pub python: Option<String>,
    /// The torch version in the environment.
    pub torch: Option<String>,
    /// The model or checkpoint identifier.
    pub model: Option<String>,
    /// The model revision, when the hub names one.
    pub model_revision: Option<String>,
    /// How the run was executed: `env` (the backend's own interpreter) or
    /// `comfy` (a graph on the `ComfyUI` host). Written for **every**
    /// `forge_record: 2` record, `env` ones included — a record that says
    /// nothing about its executor is one nobody can group later — and
    /// `None` on a v1 record, which predates the question.
    #[serde(default)]
    pub executor: Option<String>,
    /// The host's `ComfyUI` commit, for a `comfy` run.
    #[serde(default)]
    pub comfyui_commit: Option<String>,
    /// `sha256:…` of the **tracked template file** the graph was loaded
    /// from — never of the patched graph. A reader can go and find a
    /// tracked file; nobody can check a hash of bytes that were never
    /// written down. What was patched into it lives in `params`.
    #[serde(default)]
    pub workflow_sha256: Option<String>,
    /// The custom node packs the host carried, `{repo: commit}`; an empty
    /// map for native nodes, `None` for an `env` run. A sorted map, so
    /// re-serialising a record gives the bytes Python wrote.
    #[serde(default)]
    pub packs: Option<std::collections::BTreeMap<String, String>>,
}

/// One thing the run was handed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordInput {
    /// What the input was for: `image`, `mesh`, `blend`, `prompt`,
    /// `reference`, `voice_record`.
    pub role: String,
    /// The file, when the input was one, relative to where the run was
    /// started from.
    pub path: Option<String>,
    /// `sha256:…` of that file.
    pub sha256: Option<String>,
    /// Where the file came from, for the reader.
    pub source: Option<String>,
    /// The text, when the input was a prompt.
    pub prompt: Option<String>,
}

/// One thing the run produced.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordOutput {
    /// The file, relative to where the run was started from.
    pub path: String,
    /// `sha256:…` of it.
    pub sha256: Option<String>,
    /// Its size.
    pub bytes: Option<u64>,
}

/// The account of one generator run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratorRecord {
    /// Always [`RECORD_SCHEMA`].
    pub forge_record: u64,
    /// What kind of run.
    pub kind: RecordKind,
    /// The tool, as the sidecar's generator block names it: `trellis2`,
    /// `ardy`, `moss_sound_effect`, `ace_step`, `moss_tts`, `blender`.
    pub tool: String,
    /// When, `YYYY-MM-DD`.
    pub created: String,
    /// Who asked for the run: `human`, `agent:<name>`, `unknown`.
    pub created_by: String,
    /// Which backend ran.
    #[serde(default)]
    pub backend: RecordBackend,
    /// What it was handed.
    #[serde(default)]
    pub inputs: Vec<RecordInput>,
    /// Every knob, stated; `null` where unknown. The shape depends on the
    /// tool, which is why it stays a [`serde_json::Value`] here and is
    /// projected into a typed block by the `*_params` readers below.
    #[serde(default)]
    pub params: serde_json::Value,
    /// What it produced.
    #[serde(default)]
    pub outputs: Vec<RecordOutput>,
    /// What the run measured of its own output, when it did.
    #[serde(default)]
    pub measured: serde_json::Value,
    /// Whether this was a `--fake` placeholder run: the outputs pass the same
    /// validators as real ones, and nothing else about them is true.
    #[serde(default)]
    pub fake: bool,
    /// Anything the run wanted the next reader to know.
    #[serde(default)]
    pub note: Option<String>,
}

impl GeneratorRecord {
    /// Read a record from JSON bytes, refusing a schema this build does not
    /// know. The schema check runs before the typed parse so the error says
    /// "you are behind" rather than "missing field".
    ///
    /// [`RECORD_SCHEMA_MIN`] through [`RECORD_SCHEMA`] are accepted; only
    /// [`RECORD_SCHEMA`] is written. A v1 record keeps its `forge_record: 1`
    /// through a round trip, so reading one does not quietly promote it.
    ///
    /// # Errors
    ///
    /// The bytes are not JSON, declare another `forge_record`, or do not
    /// match these types.
    pub fn from_slice(bytes: &[u8], path: &Path) -> Result<Self> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| LibraryError::json(path, e))?;
        match value
            .get("forge_record")
            .and_then(serde_json::Value::as_u64)
        {
            Some(schema) if (RECORD_SCHEMA_MIN..=RECORD_SCHEMA).contains(&schema) => {
                serde_json::from_value(value).map_err(|e| LibraryError::json(path, e))
            }
            Some(other) => Err(LibraryError::UnsupportedSchema {
                path: path.to_path_buf(),
                schema: other,
            }),
            None => Err(LibraryError::rejected(format!(
                "{} is not a generator record: no forge_record field",
                path.display()
            ))),
        }
    }

    /// Read a record file.
    ///
    /// # Errors
    ///
    /// As [`Self::from_slice`], plus the file being unreadable.
    pub fn load(path: &Path) -> Result<Self> {
        Self::from_slice(&read_bytes(path)?, path)
    }

    /// The lift record beside a reference image: `<stem>.lift.json` in the
    /// PNG's directory.
    #[must_use]
    pub fn lift_record_path(image: &Path) -> PathBuf {
        let stem = image
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        image.with_file_name(format!("{stem}.lift.json"))
    }

    /// Read the lift record beside a reference image, or `None` when there
    /// is none — a PNG nobody has lifted yet is a normal state.
    ///
    /// # Errors
    ///
    /// Fails only when the record exists and cannot be read.
    pub fn lift_beside(image: &Path) -> Result<Option<Self>> {
        let path = Self::lift_record_path(image);
        if path.is_file() {
            Self::load(&path).map(Some)
        } else {
            Ok(None)
        }
    }

    /// The record as bytes, in the Python writer's field order: two-space
    /// indent, trailing newline.
    ///
    /// # Errors
    ///
    /// Fails only when `serde_json` refuses to serialise.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut json = serde_json::to_vec_pretty(self)
            .map_err(|e| LibraryError::json("generator record", e))?;
        json.push(b'\n');
        Ok(json)
    }

    /// The first input with this role.
    #[must_use]
    pub fn input(&self, role: &str) -> Option<&RecordInput> {
        self.inputs.iter().find(|i| i.role == role)
    }

    /// The prompt the run was given, when any input carried one.
    #[must_use]
    pub fn prompt(&self) -> Option<&str> {
        self.inputs.iter().find_map(|i| i.prompt.as_deref())
    }

    /// The first output.
    #[must_use]
    pub fn output(&self) -> Option<&RecordOutput> {
        self.outputs.first()
    }

    /// A string knob.
    #[must_use]
    pub fn param_str(&self, key: &str) -> Option<String> {
        self.params
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    }

    /// An integer knob.
    #[must_use]
    pub fn param_i64(&self, key: &str) -> Option<i64> {
        self.params.get(key).and_then(serde_json::Value::as_i64)
    }

    /// A small unsigned knob.
    #[must_use]
    pub fn param_u32(&self, key: &str) -> Option<u32> {
        self.params
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
    }

    /// A float knob.
    #[must_use]
    pub fn param_f32(&self, key: &str) -> Option<f32> {
        self.params
            .get(key)
            .and_then(serde_json::Value::as_f64)
            .map(|v| v as f32)
    }

    /// A boolean knob.
    #[must_use]
    pub fn param_bool(&self, key: &str) -> Option<bool> {
        self.params.get(key).and_then(serde_json::Value::as_bool)
    }

    /// A knob that is a seed: ACE-Step writes its pair as a string, every
    /// other backend an integer; either comes back as text.
    #[must_use]
    pub fn param_seed_text(&self, key: &str) -> Option<String> {
        match self.params.get(key)? {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }

    /// The lift block a sidecar carries, projected from a `lift` record.
    /// Everything the record did not state stays `null`; the Blender step
    /// ([`LiftParams::post`]) is not here — the promote fills it from the
    /// rig, export or prop record it was also handed.
    #[must_use]
    pub fn lift_params(&self) -> LiftParams {
        let image = self.input("image");
        LiftParams {
            model: self
                .param_str("model")
                .or_else(|| self.backend.model.clone()),
            trellis_commit: self
                .param_str("trellis_commit")
                .or_else(|| self.backend.commit.clone()),
            resolution: self.param_u32("resolution"),
            pipeline_type: self.param_str("pipeline_type"),
            seed: self.param_i64("seed"),
            decimation_target_vertices: self.param_u32("decimation_target_vertices"),
            texture_size: self.param_u32("texture_size"),
            remesh: self.param_bool("remesh"),
            texture_baker: self.param_str("texture_baker"),
            image: image.and_then(|i| i.path.clone()),
            image_sha256: image.and_then(|i| i.sha256.clone()),
            lift_sha256: self.output().and_then(|o| o.sha256.clone()),
            post: None,
        }
    }

    /// The ARDY block, projected from a `take` record.
    #[must_use]
    pub fn ardy_params(&self) -> ArdyParams {
        ArdyParams {
            repo: self.param_str("repo"),
            commit: self
                .param_str("commit")
                .or_else(|| self.backend.commit.clone()),
            model: self
                .param_str("model")
                .or_else(|| self.backend.model.clone()),
            seed: self.param_i64("seed"),
            duration_s: self.param_f32("duration_s"),
            cfg: self.param_f32("cfg"),
            sample: self.param_u32("sample"),
            sweep_take: self.output().and_then(|o| {
                Path::new(&o.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            }),
        }
    }

    /// The MOSS `SoundEffect` block, projected from an `sfx` record.
    #[must_use]
    pub fn sound_effect_params(&self) -> SoundEffectParams {
        SoundEffectParams {
            model: self
                .param_str("model")
                .or_else(|| self.backend.model.clone()),
            seed: self.param_i64("seed"),
            duration_s: self.param_f32("duration_s"),
            steps: self.param_u32("steps"),
            cfg: self.param_f32("cfg"),
        }
    }

    /// The ACE-Step block, projected from a `music` record.
    #[must_use]
    pub fn ace_step_params(&self) -> AceStepParams {
        AceStepParams {
            lm_model: self.param_str("lm_model"),
            dit_model: self
                .param_str("dit_model")
                .or_else(|| self.backend.model.clone()),
            seed: self.param_seed_text("seed"),
            bpm: self.param_u32("bpm"),
            keyscale: self.param_str("keyscale"),
            timesignature: self.param_str("timesignature"),
            genres: self.param_str("genres"),
            lyrics: self.param_str("lyrics"),
            duration_s: self.param_f32("duration_s"),
        }
    }

    /// The MOSS TTS block, projected from a `speech` record.
    #[must_use]
    pub fn speech_params(&self) -> SpeechParams {
        SpeechParams {
            model: self
                .param_str("model")
                .or_else(|| self.backend.model.clone()),
            seed: self.param_i64("seed"),
            voice: self.param_str("voice"),
            reference: self
                .param_str("reference")
                .or_else(|| self.input("reference").and_then(|i| i.path.clone())),
            voice_record: self
                .param_str("voice_record")
                .or_else(|| self.input("voice_record").and_then(|i| i.path.clone())),
            language: self.param_str("language"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lift record in the Python writer's field order. The byte-equality
    /// claim — the Rust reader re-serialises what the Python wrote, exactly —
    /// is pinned here against this transcription and again in P2 against a
    /// capture from `records.py` itself.
    const LIFT: &str = r#"{
  "forge_record": 2,
  "kind": "lift",
  "tool": "trellis2",
  "created": "2026-08-23",
  "created_by": "human",
  "backend": {
    "name": "trellis2",
    "commit": "75fbf0183001ed9876c8dbb35de6b68552ee08bd",
    "python": "3.11.9",
    "torch": "2.6.0+cu124",
    "model": "microsoft/TRELLIS.2-4B",
    "model_revision": null,
    "executor": "env",
    "comfyui_commit": null,
    "workflow_sha256": null,
    "packs": null
  },
  "inputs": [
    {
      "role": "image",
      "path": "assets-src/refs/props/barrel.png",
      "sha256": "sha256:cbaf",
      "source": "the user",
      "prompt": null
    }
  ],
  "params": {
    "decimation_target_vertices": 6000,
    "pipeline_type": "1024_cascade",
    "remesh": true,
    "resolution": 1024,
    "seed": 42,
    "texture_baker": "nvdiffrast (non-commercial)",
    "texture_size": 1024
  },
  "outputs": [
    {
      "path": "out/lifts/barrel.glb",
      "sha256": "sha256:5833",
      "bytes": 1234
    }
  ],
  "measured": {},
  "fake": false,
  "note": null
}
"#;

    #[test]
    fn a_lift_record_reads_projects_and_round_trips_byte_for_byte() {
        let record = GeneratorRecord::from_slice(LIFT.as_bytes(), Path::new("barrel.lift.json"))
            .expect("reads");
        assert_eq!(record.kind, RecordKind::Lift);
        assert!(!record.fake);
        let params = record.lift_params();
        assert_eq!(params.model.as_deref(), Some("microsoft/TRELLIS.2-4B"));
        assert_eq!(params.seed, Some(42));
        assert_eq!(params.resolution, Some(1024));
        assert_eq!(params.remesh, Some(true));
        assert_eq!(
            params.texture_baker.as_deref(),
            Some("nvdiffrast (non-commercial)")
        );
        assert_eq!(
            params.image.as_deref(),
            Some("assets-src/refs/props/barrel.png")
        );
        assert_eq!(params.image_sha256.as_deref(), Some("sha256:cbaf"));
        assert_eq!(params.lift_sha256.as_deref(), Some("sha256:5833"));
        assert_eq!(params.post, None);
        let bytes = record.to_bytes().expect("serialise");
        assert_eq!(
            String::from_utf8(bytes).expect("utf8"),
            LIFT,
            "field order and formatting are the byte contract with the Python writer"
        );
    }

    /// The same lift as `forge_record: 1`: no `executor`, no
    /// `comfyui_commit`, no `workflow_sha256`, no `packs`. One of these is
    /// under `assets/` beside every shipped body, model and sound, and none
    /// of them was rewritten when the schema went to 2.
    const LIFT_V1: &str = r#"{
  "forge_record": 1,
  "kind": "lift",
  "tool": "trellis2",
  "created": "2026-08-23",
  "created_by": "human",
  "backend": {
    "name": "trellis2",
    "commit": "75fbf0183001ed9876c8dbb35de6b68552ee08bd",
    "python": "3.11.9",
    "torch": "2.6.0+cu124",
    "model": "microsoft/TRELLIS.2-4B",
    "model_revision": null
  },
  "inputs": [],
  "params": {"seed": 42},
  "outputs": [{"path": "out/lifts/barrel.glb", "sha256": "sha256:5833", "bytes": 1234}],
  "measured": {},
  "fake": false,
  "note": null
}
"#;

    #[test]
    fn both_schemas_are_read_and_only_the_newest_is_written() {
        // A v1 record reads, projects, and keeps its own schema number: a
        // reader that promoted it would be claiming the four keys were
        // absent on purpose rather than absent because nobody asked yet.
        let old = GeneratorRecord::from_slice(LIFT_V1.as_bytes(), Path::new("v1.json"))
            .expect("v1 still reads");
        assert_eq!(old.forge_record, 1);
        assert_eq!(old.backend.executor, None, "null means unknown");
        assert_eq!(old.backend.packs, None);
        assert_eq!(old.lift_params().seed, Some(42));

        let new =
            GeneratorRecord::from_slice(LIFT.as_bytes(), Path::new("v2.json")).expect("v2 reads");
        assert_eq!(new.forge_record, RECORD_SCHEMA);
        assert_eq!(new.backend.executor.as_deref(), Some("env"));
        assert_eq!(
            new.backend.workflow_sha256, None,
            "an env run genuinely has no workflow"
        );
    }

    #[test]
    fn a_newer_or_absent_schema_is_refused() {
        let newer = LIFT.replacen("\"forge_record\": 2", "\"forge_record\": 3", 1);
        let error =
            GeneratorRecord::from_slice(newer.as_bytes(), Path::new("x.json")).expect_err("refuse");
        assert!(error.to_string().contains("schema 3"), "{error}");
        let error = GeneratorRecord::from_slice(b"{\"kind\": \"lift\"}", Path::new("x.json"))
            .expect_err("refuse");
        assert!(error.to_string().contains("forge_record"), "{error}");
    }

    #[test]
    fn a_comfy_record_carries_its_host_its_template_and_its_packs() {
        let text = r#"{"forge_record": 2, "kind": "sfx", "tool": "moss_sound_effect",
            "created": "2026-08-30", "created_by": "agent:claude",
            "backend": {"name": "moss_sfx", "commit": null, "executor": "comfy",
                        "comfyui_commit": "169fcf35a2fc163fec31338b816503ddac0d3fcf",
                        "workflow_sha256": "sha256:9f1c",
                        "packs": {"https://github.com/b": "2b", "https://github.com/a": "1a"}},
            "inputs": [{"role": "prompt", "prompt": "a heavy iron door"}],
            "params": {"workflow": "sfx.api.json", "seed": 815273},
            "outputs": [{"path": "out/audio/sfx/door.wav"}]}"#;
        let record =
            GeneratorRecord::from_slice(text.as_bytes(), Path::new("s.json")).expect("reads");
        assert_eq!(record.backend.executor.as_deref(), Some("comfy"));
        assert_eq!(
            record.backend.commit, None,
            "a comfy backend has no checkout of its own"
        );
        let packs = record.backend.packs.as_ref().expect("packs");
        assert_eq!(
            packs.keys().map(String::as_str).collect::<Vec<_>>(),
            ["https://github.com/a", "https://github.com/b"],
            "a free-form map is sorted, like params"
        );
        assert_eq!(
            record.param_str("workflow").as_deref(),
            Some("sfx.api.json"),
            "the patch is knobs, and knobs live in params"
        );
    }

    #[test]
    fn the_lift_record_sits_beside_its_image() {
        assert_eq!(
            GeneratorRecord::lift_record_path(Path::new("refs/props/barrel.png")),
            PathBuf::from("refs/props/barrel.lift.json")
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let png = dir.path().join("barrel.png");
        assert_eq!(GeneratorRecord::lift_beside(&png).expect("none"), None);
        std::fs::write(dir.path().join("barrel.lift.json"), LIFT).expect("write");
        assert!(GeneratorRecord::lift_beside(&png).expect("some").is_some());
    }

    #[test]
    fn audio_and_take_projections_fall_back_to_the_backend_block() {
        let text = r#"{"forge_record": 1, "kind": "music", "tool": "ace_step",
            "created": "2026-08-23", "created_by": "agent:claude",
            "backend": {"model": "acestep-v15-turbo", "commit": "82252c2"},
            "inputs": [{"role": "prompt", "prompt": "dark ambient"}],
            "params": {"seed": "1,2", "bpm": 96, "lm_model": "acestep-5Hz-lm-0.6B"},
            "outputs": [{"path": "out/audio/theme.wav"}]}"#;
        let record =
            GeneratorRecord::from_slice(text.as_bytes(), Path::new("m.json")).expect("reads");
        let music = record.ace_step_params();
        assert_eq!(music.dit_model.as_deref(), Some("acestep-v15-turbo"));
        assert_eq!(music.seed.as_deref(), Some("1,2"));
        assert_eq!(music.bpm, Some(96));
        assert_eq!(record.prompt(), Some("dark ambient"));

        let text = r#"{"forge_record": 1, "kind": "take", "tool": "ardy",
            "created": "2026-08-23", "created_by": "human",
            "backend": {"model": "core", "commit": "693f74d"},
            "params": {"seed": 7, "duration_s": 4.0, "cfg": 2.5, "sample": 1},
            "outputs": [{"path": "out/sweeps/walk/walk__d4_c2_s7_1.npz"}]}"#;
        let record =
            GeneratorRecord::from_slice(text.as_bytes(), Path::new("t.json")).expect("reads");
        let ardy = record.ardy_params();
        assert_eq!(ardy.seed, Some(7));
        assert_eq!(ardy.commit.as_deref(), Some("693f74d"));
        assert_eq!(ardy.sweep_take.as_deref(), Some("walk__d4_c2_s7_1.npz"));
        assert_eq!(ardy.repo, None, "never invented");
    }
}
