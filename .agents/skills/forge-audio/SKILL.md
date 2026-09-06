---
name: forge-audio
description: Generate, review and ship game sound effects, music and spoken lines through local Forge backends. Use for game audio production; design a new character voice through forge-voice first.
---

# Make and ship audio

Generate into `out/`, inspect the waveform and spectrogram, listen when possible,
then promote the accepted file. Audio claims integrity and provenance, never
bit-exact reproduction. A passing audio gate catches broken audio; it cannot
establish that the words, performance or loop are right.

## Choose the execution path

| Command | Backend | Execution |
|---|---|---|
| `forge gen sfx` | `moss_sfx` | ComfyUI, MOSS-SoundEffect |
| `forge gen music` | `acestep` | ComfyUI, ACE-Step |
| `forge gen voice` | `moss_tts` | ComfyUI, MOSS-VoiceGenerator; see forge-voice |
| `forge gen speech` | `moss_speech` | Separate Python interpreter, Transformers 5.0.0 and torch 2.9.1+cu128 |

Speech was restored on 2026-09-05. The old Comfy speech graph fails under
Transformers 5.16.1 and can return silent audio. Do not shim it, bump the pack
speculatively, or downgrade the shared host. Speech now runs the existing
MOSS-TTS-Local-Transformer weights in a compatible isolated environment.
`--backend moss_tts` remains a speech alias for this isolated path.

Run `just doctor` first. A selected backend must read `ok`; `off` means the
project did not choose that kind. Choose voice in `[make]` and run `forge setup`
to install both voice design and speech. The isolated installer can adopt an
existing environment, checkout and model directory with `--adopt-env`,
`--adopt-checkout` and `--adopt-checkpoints`. It never changes ComfyUI's env.

Run `just gpu` before generation. One generator at a time, across all projects.
`forge gpu --free` releases the card; the MOSS Comfy pack may require the managed
service restart because `/free` cannot unload its models. The isolated speaker
releases its models on process exit. Its 14 GB descriptor is a budget, not a
measured peak. Never generate alongside a studio model on the real adapter.

## Generate

```
just sfx impact "Sharp metal impact, immediate attack, short tail" --seconds 1 --seed 101
just music combat "Driving industrial combat music" --duration 30 --seed 101 --gain-db -3
just speech warning "Hold the scrap line." --voice scavenger --seed 101
```

A speech voice is a designed name under `assets-src/voices/<name>/ref.wav`, or
a brought 5–15 second clip by path. WAV, MP3, FLAC and M4A are decoded through
ffmpeg before tokenization. A brought clip needs a source-ledger entry.
A designed voice's reference and `voice.json` are both hashed in the line record.

Speech supports batches with `--lines-file` containing `stem|text`, plus
`--out-dir`. Each line resets to the recorded seed. Sampling parameters are
explicit, including separate text and audio samplers. The record hashes the
actual model files and code, records runtime versions, and leaves unobserved
revisions null. A supplied reference transcript is recorded but is not used by
this checkpoint. Lines that reach the token limit without an end token are
refused; split long dialogue into shorter sentences.

MCP `generate_audio` accepts `kind: sfx | music | speech`. Supply `voice` for
real speech. Generation returns a job, not an asset: call `wait` with that job
until completion, then inspect its output and record. CLI and MCP share the
same GPU lease. A failed job is not authorization to promote its partial file.

Use `--created-by human` or `agent:<name>` for provenance. Outputs and JSON
records stay under `out/audio/`; a generator never writes the library.

## Review

Run `just audio <file>`, then inspect its rendered PNG. Check the attack,
silence, tail, clipping and spectrogram. Listen to speech for intelligibility
and complete word endings. Listen across a music loop seam; a 30-second file
is not evidence of a seamless 30-second loop.

Silence and runs of pinned full-scale samples fail generation before a success
record is written. Music is also decoded from its final OGG and checked: Vorbis
can overshoot when the pre-encoding WAV passed. `--gain-db` is an integer knob
applied upstream by the graph. Render again with lower gain if it clips.
Never repair, normalize or trim the generated file by hand.

Useful review cues: an effect onset after 50 ms can feel late; long lead or tail
silence deserves inspection; music ending more than 9 dB quieter than it starts
needs a loop review. These are cues, not automatic approval criteria.
A waveform cannot confirm the right spoken words. The user's hearing outranks
its numerical verdict.

## Ship and verify

```
forge promote audio voice out/audio/voice/warning.wav warning --record out/audio/voice/warning.json --created-by agent:codex
```

Use kind `sfx`, `music` or `voice`. Stems share one namespace across these kinds.
Promotion copies the judged bytes, writes the sidecar and refreshes the manifest.
A voice audition record is a source, not a spoken-line record, and cannot be
promoted as dialogue. Missing provenance stays unknown; do not invent it.

Run `just audio-list`, `just manifest-check`, `just verify` and `just audit`.
After any hand add/remove/rename in `assets/`, rebuild the manifest and check it.
Fix a shipped record through `forge promote ... --overwrite`, never by editing
it. Keep source clips and their source records with the library. Never commit
`out/`; commit only when asked.

## Failure means a next action

- Missing backend: use forge-setup; do not fall back silently to another model.
- Silent or clipped audio: render again upstream; no success record means no promotion.
- Speech environment version refusal: use the isolated installer, not a host patch.
- Wrong words or unfinished endings: reject the take, revise the line or seed, and listen again.
- GPU lease blocked: inspect the holder and release the managed backend; never run a second generator around the lock.
- Music fades or has a long silent tail: reject it for a loop brief even when the validity gate passes.

### Recorded music loops

Generate a longer source and explicitly select the period inside its active music:

```sh
forge gen music --prompt "Steady instrumental combat groove, 120 bpm" --duration 30 --bpm 120 --seed 101 --gain-db -6 --loop-start 2 --loop-duration 16 --loop-crossfade 0.5 --format wav --out out/audio/music/combat_loop.wav --record out/audio/music/combat_loop.json
```

All three loop knobs are required. `--duration` remains the generated source
length; `--loop-duration` is the output period. The original generated FLAC stays
beside the output as `<stem>.source.flac`, hashed as `loop_source`. Keep that file
with the record after promotion; the sidecar retains its source path and hash.
Choose a new output name for each trial.

`linear_wrap_pcm16_v1` rounds seconds to the nearest PCM frame (ties to even),
keeps exactly N selected frames, and blends the first C frames from the source
at S+N+i toward S+i with weight i/C. The source must extend through S+N+C.
Integer samples round ties to even. The period is unchanged; its first frame
continues the original last frame's source neighborhood. The final OGG is
decoded and checked for clipping, silence and the exact selected frame count.
Use WAV for exact-period loops: the local Vorbis decoder can return fewer frames
than the PCM supplied to the encoder. A changed period is refused with a WAV
rerun instruction; the encoded file is never padded or trimmed. Fake requests record
`applied: false`; they do not perform this transform.

MCP uses `loop_start`, `loop_duration`, `loop_crossfade`, `bpm`, `gain_db` and
`thinking` for music only; `seconds` is the generated source duration.
Listen across several repeats for rhythm, harmony and crossfade artifacts.
A clean waveform does not establish a musically seamless loop. Reject a faded
or silent selection and generate a new recorded trial with a different selection.
