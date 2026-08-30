# Handover — the ComfyUI host half of Phase 1 B

**Written by a duplicate instance of implementer B**, resumed outside the
workflow by a coordinator reply. The workflow's own implementer B owns every
file in this worktree and wrote none of this; it should fold what is below
into its work and into its `patch_notes`, and the merge agent should treat
this file as part of B's return. Nothing else in the tree was touched by the
duplicate — this file is the whole of it.

Everything here is a measurement taken on the real host on **2026-08-30**,
not a reading of upstream documentation. Where a number came from the card or
the service it says so.

---

## 1. TTS-Audio-Suite is installed on the host, and it cost nothing

| what | value |
|---|---|
| repo | `https://github.com/diodiogod/TTS-Audio-Suite` |
| pin | `fab00263fbdcdaddd4c721d1b560e1a08b6025ea` (v5.8.7, 2026-08-28) |
| licence | MIT |
| clone | `$PREFIX/data/custom_nodes/TTS-Audio-Suite`, 185 MB |
| pips installed | **none** |
| unit | `systemctl --user restart forge-comfy.service`, `GET /system_stats` answered 200 after the restart |
| card at the time | 1015 MiB used of 24564 — the idle CUDA context, nothing else; no generate was running and none was started |

**The venv is unchanged, proved after the restart** (`$PREFIX/venv/bin/python`):

```
torch 2.13.0+cu130
torchaudio 2.11.0+cu130
numpy 2.5.2
transformers 5.16.1
```

This matters more than it looks. The pack's `requirements.txt` asks for
`torch>=2.0.0`, `torchaudio>=2.0.0` and — the dangerous one —
`numpy>=1.26.4,<2.3.0`, which would have *downgraded* the host's numpy 2.5.2
under every image template. **No pip was run at all**, because it turned out
none was needed: the pack registers each node behind its own `try/except`, so
a pack whose optional engines cannot import still contributes the classes
whose imports succeed. All 58 of its nodes registered on the first restart
with the venv exactly as it was.

The Manager's own answer confirms it. `GET /v2/snapshot/get_current`
(**not** `/api/manager/snapshot/get_current`, which is 404 on this build)
now returns:

```json
"git_custom_nodes": {
  "https://github.com/city96/ComfyUI-GGUF.git": {"hash": "6ea2651e7df66d7585f6ffee804b20e92fb38b8a", "disabled": false},
  "https://github.com/diodiogod/TTS-Audio-Suite.git": {"hash": "fab00263fbdcdaddd4c721d1b560e1a08b6025ea\n", "disabled": false}
}
```

Two things to carry into `backends/comfy/snapshot.json`, which must be
**re-fetched and never hand-edited**: `install.sh` already prints the live
snapshot through `json.load` → `json.dump(indent=2)` + a trailing newline, so
that transform is the established one and reproduces the committed file's
shape; and **the Manager writes the pack's hash with a trailing `\n` inside
the JSON string**. That newline is the Manager's own answer and belongs in
the committed copy as fetched. Anyone who tidies it by hand has hand-edited a
snapshot, which this repository already files beside a hand-repaired
`.blend`.

The four places the pack must live, for the pack rule: `[[comfy.packs]]` in
`backends/comfy/backend.toml`, `backends/comfy/install.sh` (cloned at the pin
**before** the unit starts — packs are scanned once at startup),
`backends/comfy/snapshot.json` re-fetched as above, and `designs/hosting.md`'s
pins row. `probe.py` should hold the clone to its pin the way it already
holds ComfyUI-GGUF's.

One more fact for `install.sh`: the Manager's config at
`$PREFIX/data/user/__manager/config.ini` reads `allow_pip_install = False`
and `always_lazy_install = False`, which is why adding a clone to
`custom_nodes/` and restarting could not run the pack's `install.py` behind
our back. If that setting ever flips, this install stops being free.

---

## 2. The class names, captured from the live `/object_info`

Never from memory, per `CLAUDE.md` and `serve.md` §4. `GET /object_info` went
from **645 classes to 703** across the restart; these are the 58 new ones,
verbatim as the service spells them, with the display name it gives each:

```
ASRPunctuationTruecaseNode        📝 ASR Punctuation / Truecase
CharacterVoicesNode               🎭 Character Voices
ChatterBoxAudioAnalyzer           🌊 Audio Wave Analyzer
ChatterBoxAudioAnalyzerOptions    🔧 Audio Analyzer Options
ChatterBoxEngineNode              ⚙️ ChatterBox TTS Engine
ChatterBoxF5TTSEditOptions        🔧 F5-TTS Edit Options
ChatterBoxF5TTSEditVoice          👄 F5-TTS Speech Editor
ChatterBoxOfficial23LangEngineNode ⚙️ ChatterBox Official 23-Lang Engine
ChatterBoxVoiceCapture            🎙️ Voice Capture
CosyVoiceEngineNode               ⚙️ CosyVoice3 Engine
DotsTTSEngineNode                 ⚙️ Dots TTS Engine
DramaBoxDatasetPrepNode           📦 DramaBox Dataset Prep
DramaBoxDatasetRowsNode           🧾 DramaBox Dataset Rows
DramaBoxEngineNode                ⚙️ DramaBox Engine
DramaBoxTrainingConfigNode        🎛️ DramaBox Training Config
EchoTTSEngineNode                 ⚙️ Echo-TTS Engine
F5TTSEngineNode                   ⚙️ F5 TTS Engine
FishAudioS2EngineNode             ⚙️ Fish Audio S2 Pro Engine
GraniteASREngineNode              ⚙️ Granite ASR Engine
HiggsAudioEngineNode              ⚙️ Higgs Audio 2 Engine
HiggsAudioV3EngineNode            ⚙️ Higgs Audio v3 Engine
IndexTTSEmotionOptionsNode        🌈 IndexTTS-2 Emotion Vectors
IndexTTSEngineNode                ⚙️ IndexTTS 2 / 2.5 Engine
LoadRVCModelNode                  🎭 Load RVC Character Model
MergeAudioNode                    🥪 Merge Audio
MossClipStagingNode               🎞️ Training Clip Staging
MossDatasetPrepNode               📦 MOSS Dataset Prep
MossDatasetRowsNode               🧾 MOSS Dataset Rows
MossSoundEffectV2EngineNode       ⚙️ MOSS SoundEffect v2 Engine
MossTTSEngineNode                 ⚙️ MOSS-TTS Engine
MossTrainingConfigNode            🎛️ MOSS Training Config
MouthMovementAnalyzer             🗣️ Silent Speech Analyzer
OmniVoiceEngineNode               ⚙️ OmniVoice Engine
OmniVoiceInstructionBuilderNode   📐 Visual Tag Builder
PhonemeTextNormalizer             📝 Phoneme Text Normalizer
Qwen3TTSEngineNode                ⚙️ Qwen3-TTS Engine
QwenEmotionNode                   🌈 IndexTTS-2 Text Emotion
RVCDatasetPrepNode                📦 RVC Dataset Prep
RVCEngineNode                     ⚙️ RVC Engine
RVCPitchOptionsNode               🔧 RVC Pitch Extraction Options
RVCTrainingConfigNode             🎛️ RVC Training Config
RefreshVoiceCacheNode             ♻️ Refresh Voice Cache
SRTAdvancedOptionsNode            🔧 SRT Advanced Options
SaveCharacterVoiceNode            💾 Save Character Voice
StepAudioEditXAudioEditorNode     🎨 Step Audio EditX - Audio Editor
StepAudioEditXEngineNode          ⚙️ Step Audio EditX Engine
StringMultilineTagEditor          🏷️ Multiline TTS Tag Editor
TextToSRTBuilderNode              📺 Text to SRT Builder
UnifiedASRTranscribeNode          ✏️ ASR Transcribe
UnifiedModelTrainingNode          🎓 Model Training
UnifiedSoundEffectsNode           🌩️ Sound Effects
UnifiedTTSSRTNode                 📺 TTS SRT
UnifiedTTSTextNode                🎤 TTS Text
UnifiedVoiceChangerNode           🔄 Voice Changer
UnifiedVoiceDesignerNode          🎨 Voice Designer
VibeVoiceEngineNode               ⚙️ VibeVoice Engine
VisemeDetectionOptionsNode        🔧 Viseme Mouth Shape Options
VoiceFixerNode                    🤐 Voice Fixer
```

### The ones the three templates need

The pack's shape is **engine node → unified node**: a node that configures a
model, wired into a node that does the work. So the `[comfy] nodes` lists are:

| template | graph | `[comfy] nodes` |
|---|---|---|
| `moss_sfx/workflows/sfx.api.json` | `MossSoundEffectV2EngineNode` → `UnifiedSoundEffectsNode` → `SaveAudio` | `["MossSoundEffectV2EngineNode", "UnifiedSoundEffectsNode", "SaveAudio"]` |
| `moss_tts/workflows/speech.api.json` | `MossTTSEngineNode` → `UnifiedTTSTextNode` → `SaveAudio`, with `LoadAudio` → `CharacterVoicesNode` → `opt_narrator` for the cloned reference | `["MossTTSEngineNode", "UnifiedTTSTextNode", "CharacterVoicesNode", "LoadAudio", "SaveAudio"]` |
| `moss_tts/workflows/voice.api.json` | `MossTTSEngineNode` (`model_variant = "Voice Design 1.7B"`) → `UnifiedVoiceDesignerNode` → `SaveAudio` | `["MossTTSEngineNode", "UnifiedVoiceDesignerNode", "SaveAudio"]` |

ACE-Step needs no pack: **ACE-Step 1.5 is native to the pinned host** and its
classes were in the 645 before the pack arrived —
`CheckpointLoaderSimple`, `TextEncodeAceStepAudio1.5`,
`EmptyAceStep1.5LatentAudio`, `ModelSamplingAuraFlow`, `ConditioningZeroOut`,
`KSampler`, `VAEDecodeAudio`, `SaveAudio`.

ComfyUI's own bundled template `audio_ace_step_1_5_checkpoint.json` (in
`comfyui_workflow_templates_json`, at the host's pin) is the graph to cut
`music.api.json` from rather than inventing one; it wants one checkpoint,
`ace_step_1.5_turbo_aio.safetensors` from
`Comfy-Org/ace_step_1.5_ComfyUI_files` into `models/checkpoints/`, and it
wires `TextEncodeAceStepAudio1.5` (tags, lyrics, seed, bpm, duration,
timesignature, language, keyscale — which is `music.py`'s `request_payload`
almost field for field) and `EmptyAceStep1.5LatentAudio` (seconds) into a
`KSampler` at `euler`/`simple` behind `ModelSamplingAuraFlow`, with the
negative side a `ConditioningZeroOut` of the positive.

### The knobs, from the live schemas

Worth writing into the templates deliberately rather than taking the default:

- `UnifiedSoundEffectsNode.enable_audio_cache` and
  `UnifiedTTSTextNode.enable_audio_cache` both default to **true**. Set both
  **false**. The unit runs `--cache-none`, but this is the *pack's own*
  in-memory cache and it would hand back an earlier render as a re-roll —
  "a record that lies", and the exact thing `cached` is meant to be observed
  for rather than inferred.
- `UnifiedSoundEffectsNode` takes `description`, `duration_seconds`, `seed`;
  `MossSoundEffectV2EngineNode` takes `inference_steps` (default 100),
  `cfg_scale` (4.0), `sigma_shift` (5.0) and `negative_prompt`. That is
  `sfx.py`'s `--seconds/--seed/--steps/--cfg` with nothing left over, so the
  record's `params` keys survive the move unchanged.
- `UnifiedVoiceDesignerNode` takes `reference_text`, `voice_instruction` and
  `seed` — the description-and-seed pair `forge gen voice` already records.
- `CharacterVoicesNode` has an `opt_audio_input` of type `AUDIO` and outputs
  `NARRATOR_VOICE`, which is what `UnifiedTTSTextNode.opt_narrator` wants.

---

## 3. Three findings the design has to absorb

### a. There is no unload node in this pack — `unload_node` is `null` for all three

`serve.md` §4 and §6 name `unload_node = "TTSAudioSuiteUnload"` and say it is
set "because a wrapper pack loads outside ComfyUI's memory manager". **No such
class exists at this pin.** The full list of what the pack registers is in §2
above, and there is no unload, free or release node anywhere in it.

So `unload_node = null` for `acestep`, `moss_sfx` and `moss_tts` alike, and
`POST /free` plus the unit restart — `forge_serve::card`'s ladder, §5 — is the
*only* lever the card has. That makes the ladder's step 4 and step 5 load
bearing rather than a safety net: on the image models `/free` gave the card
back on both spike runs, but whether it does for a wrapper pack that loads
its own models is **unmeasured**, and it is the first thing the real run
should record.

### b. ComfyUI cannot write a WAV, so every audio verb now needs ffmpeg

Read off the node schemas on the running host: `SaveAudio` writes **FLAC**,
and `SaveAudioAdvanced`'s format is a dynamic combo offering exactly
`flac`, `mp3` and `opus` — there is no WAV option in the pinned build
(`comfy_extras/nodes_audio.py`, v0.34.2).

Every audio record in this library measures a PCM WAV, `measure_wav` reads one
with the stdlib `wave` module, and `--out` refuses a name that is not `.wav`.
So each comfy audio template must end in `SaveAudio`, and the Python half must
`GET /view` the FLAC and transcode it to PCM16 WAV before `measure_wav` and
`add_output` ever see a file. FLAC is lossless, so nothing is lost — but three
consequences follow and none of them is cosmetic:

1. **`sfx`, `speech` and `voice` gain an ffmpeg dependency they did not have.**
   They used to write the WAV with `soundfile` inside their own venv. A
   missing ffmpeg is exit **6** (`missing_tool`), and it has to be refused
   *before* the card is taken, not after a four-minute render.
   `music.py` already carries `ffmpeg_bin` and `transcode_ogg` and is the
   place to lift the helper from.
2. Doctor's comfy row should say ffmpeg is required for the audio kinds, since
   it is now the difference between a template that runs and a file nobody can
   measure.
3. The record is unchanged in shape: the FLAC is an intermediate under the job's
   scratch, never an output, and `outputs` still hashes the WAV that shipped.

### c. The sound-effect engine `torch.compile`s the DiT, and the unit does not stop it

`MossSoundEffectV2EngineNode`'s own tooltip, quoted from `/object_info`:

> The DiT uses torch.compile automatically. Its first generation can spend
> several minutes compiling. Compatible compiled artifacts are cached and
> reused across ComfyUI sessions.

This is the same behaviour `backends/moss_sfx/backend.toml` has carried
`TORCHDYNAMO_DISABLE = "1"` for since 2026-08, with `hosting.md`'s reason —
one-shot generation never pays back a multi-minute compile. **That `[env]`
value dies with the venv**: under the comfy executor the backend has no
environment, and `forge-comfy.service` does not set it.

The choice is now the host's, and it is a real choice rather than a
carry-over, because the pack claims the compiled artifact is cached *across
sessions* — which the old venv could not do, and which is what made the
compile never pay back. Either:

- set `Environment=TORCHDYNAMO_DISABLE=1` in `forge-comfy.service` and keep
  the old behaviour, at the cost of every sound effect being slower forever;
  or
- leave it on, and record on the first real run how long the compile takes,
  whether the inductor cache survives a unit restart, and where it is written.

Whichever is chosen, it belongs in the unit file and in `hosting.md`, because
it is now a property of the host and not of a backend.

### d. And one open question the real run must still close

`decisions.md`, 2026-08-23: the cloner cannot open a reference by path in this
env (torchaudio → torchcodec), so `speech`'s inner half reads the clip with
`soundfile` and hands over codes. Under the pack that inner half is gone.

The structural answer the schemas give is that **the reference travels as an
uploaded file**: `POST /upload/image` puts the WAV in ComfyUI's input
directory (that route takes any file, not only images), `LoadAudio` reads it,
and `CharacterVoicesNode.opt_audio_input` takes the `AUDIO` tensor through to
`UnifiedTTSTextNode.opt_narrator`. Nothing is handed a path.

That is still a hypothesis. What makes it *plausible* here and not merely
hopeful is that the decode now happens in ComfyUI's venv, which carries
`av 18.1.0` — PyAV, not torchcodec — so the library collision that broke the
old path may simply not be on this road. **It is not measured.** One real
`speech` through the host, before `moss_tts`'s venv is deleted, with the
outcome dated under ComfyUI in `hosting.md`; and nothing is deleted until a
real sound has come out of the new path on the card.

---

## 4. What was not done

No GPU work. No generate was run, no MOSS weights were downloaded, no first
render was triggered — the card was left as it was found. No template file was
written, no `backend.toml` was edited, no Python or Rust was touched. The four
files under change in this worktree are the live implementer B's and were read
only to be sure this handover would not collide with them.
