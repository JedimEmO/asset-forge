---
name: forge-audio
description: Make and ship a sound — an effect through MOSS-SoundEffect, a track through ACE-Step, a spoken line through MOSS-TTS in a voice designed by forge-voice, all three inside the ComfyUI host — rendered to out/, judged from its plot and numbers, then filed with its record. Use when the user wants a sound effect, music or a voice line, or says a shipped sound is clipped, late, quiet or truncated.
---

# Audio: prompt → `out/audio/<kind>/` → plot → `assets/audio/<kind>/`

Three backends, one shape, and each step names its door: **`just sfx` /
`just music` / `just speech`** (the MCP door is `generate_audio`, which
returns a job you follow with `wait`) render a file and its record into
`out/audio/<kind>/`; **`just audio`** (`inspect_audio`) measures and draws
it; **`just promote-audio`** (`promote_audio`) files it. All three
generators run inside the ComfyUI host now — ACE-Step native, MOSS through
TTS-Audio-Suite — so none of them has a venv or a server of its own. Nothing a generator writes lands under `assets/`;
a sound is looked at before it ships. None of the three is bit-reproducible,
so a shipped sound claims integrity (sha256) and provenance, never
regeneration: the record says what was asked and what the backend used, and
the file is the bytes that were judged.

**Two things are true at this pin and neither is a setup problem** (both
measured 2026-08-30, `designs/hosting.md` § the first real run of the audio
path):

- **`just speech` cannot make a line.** MOSS-TTS 1.7B does not run under
  the host's transformers 5, and the pack turns that into *silence* rather
  than an error — the graph completes, and the audio gate now refuses the
  file and writes no record. `backends/moss_tts`'s `[[notices]]` names both
  API breaks and the pin that would lift them. `just voice` (the designer)
  is unaffected and works.
- **`just music` renders but does not promote.** ACE-Step 1.5 turbo comes
  off the host at exactly 0.0 dBFS whatever the content, with runs of 10 to
  186 pinned samples, and the clipping gate is right to refuse it. The fix
  is a stated gain knob in the graph, not a file edited by hand.

The timings below were measured on that run; the log lines are the ones
these modules print.

## Prerequisites (check, don't assume)

- `just doctor` — the row for the backend you need reads `ok`:

  | Recipe | Row | What each word means here |
  |---|---|---|
  | `just sfx` | `moss_sfx` `[comfy]` | `partial`: `MOSS-SoundEffect-v2.0` (10.46 GB) is not in the host's `models/TTS/moss_soundeffect_v2/` yet, and the row names it with its GB. The pack downloads it on the node's first run — **not** into the HF cache |
  | `just music` | `acestep` `[comfy]` | `partial`: the 10.03 GB all-in-one checkpoint is not in `models/checkpoints/`; `bash backends/acestep/install.sh` is the one thing that fetches it |
  | `just speech` | `moss_tts` `[comfy]` | `ok` here does **not** mean a line can be spoken — the row is node classes and weight files, and neither is what is broken (see above). The same row covers `just voice` (`forge-voice`); the three weights are 5.72 + 3.95 + 6.61 GB under `models/TTS/moss_tts/` |

  All three are `comfy` backends, so their rows are judged against the
  service: `missing` means **nothing is listening at `[hardware]
  comfy_url`** — `systemctl --user start forge-comfy` — and `broken` means
  the service answers as another commit than pinned, a node pack is off its
  pin, or a workflow names a class it does not have. None of those is fixed
  by downloading anything. `missing` on a first setup → `forge-setup`.

  A row that reads **`off — [make] music = false`** is not a problem: this
  project did not choose that kind. If the user wants it, that is a
  `[make]` line in `forge.toml` (or `init_project` with `adopt: true`),
  then `forge setup`.
- **The GPU is free enough.** `just gpu`. Measured 2026-08-30:
  MOSS-SoundEffect **10.0 GB**, ACE-Step **13.1 GB**, MOSS-TTS **7.1 GB**
  *on top of* the voice designer's **5.3 GB** — the pack unloads neither,
  so a speech after a voice is the pair. Each `backend.toml`'s `vram_gb`
  sits above its peak (11, 14, 13) and is a budget, never a measurement.
  None of them co-resides with the image model (23.3 GB) or an ARDY sweep
  (15.4 GB); a studio window on the real adapter holds the card too.
- **Give the card back afterwards.** `forge gpu --free` is the door, and
  for the MOSS pack it is not enough: `POST /free` does not unload what
  TTS-Audio-Suite loaded (9.1 GB stayed after an effect, 7.3 GB after a
  speech, 5.4 GB after a voice), so **`systemctl --user restart
  forge-comfy` is the lever** — 4.4 s, measured. Native ACE-Step needs
  nothing at all: its card comes back by itself. `forge gpu --free` says
  which of the two happened, and refuses to call the card back unless free
  VRAM reaches the card's idle floor.
- For speech: **a voice**. Designed, as a rule — `forge-voice` makes
  `assets-src/voices/<name>/ref.wav` from a description and a seed, with
  its record beside it, and `--voice <name>` finds it by name. A clip you
  brought instead (5–15 s of one person speaking cleanly,
  `.wav/.mp3/.flac/.m4a`) goes by path and needs a row in
  `assets-src/SOURCES.md` or `forge verify` fails it. Either way the same
  reference in gives the same voice out, and the record carries the clip's
  sha256 so "same" is checkable.

## Steps

### 1. Generate — one of three

```
just sfx    <name> "<prompt>"  [--seconds 3] [--seed N] [--steps 100] [--cfg 4] [--created-by agent:<you>]
just music  <name> "<prompt>"  [--duration 30] [--seed N] [--bpm N] [--keyscale "Am"] [--lyrics-file f] [--thinking]
just speech <name> "<text>"    --voice <voice-name> | --voice path/to/clip.wav [--language en] [--seed N]
```

`<name>` is the file stem, `[a-z0-9_]+`, and it is **one name across every
audio kind** (step 3 refuses a stem that exists in another kind). Outputs:
`out/audio/sfx/<name>.wav`, `out/audio/music/<name>.ogg`,
`out/audio/voice/<name>.wav`, each with `<name>.json` beside it — the
`forge_record` the promote reads. Pass `--created-by human` or
`agent:<name>`; without it the record says `unknown`, and that is what it
will say forever.

Every one of the three posts a graph to the host and fetches what came out:
ComfyUI v0.34.2 has no WAV save node, so each template ends in `SaveAudio`
(FLAC) and the Python half transcodes it to 16-bit PCM with **ffmpeg**
before anything measures it. A missing ffmpeg is exit 6, refused before the
card is leased.

| Recipe | Log line | Healthy | Not |
|---|---|---|---|
| sfx | `[sfx] 3 s, 100 steps, cfg 4, seed 1627167584: a heavy iron chain drags …` | the knobs, the seed and the prompt; the seed is fresh and random unless you said `--seed`, and either way it is in the record | exit 3 in ~100 ms: not installed → `forge-setup` |
| sfx | `[sfx] 30s on the host (the first render of a session compiles the DiT)` | only if `TORCHDYNAMO_DISABLE=1` were off — it is on in the unit, so nothing compiles and this line means the host is simply busy | a graph that never finishes: `journalctl --user -u forge-comfy -n 50` |
| sfx | `[sfx] OK out/audio/sfx/<name>.wav (seed N, peak -0.0 dBFS)` | **26.1 s** measured cold, 20.1 s with the model already loaded, 39.0 s over MCP behind another job | exit 5 `the effect is silent` / `is clipped`: the gate refused the render and wrote **no record** — re-render |
| music | `[music] 30 s at 96 bpm in Am, seed 1421`, then `[music] prompt b1f0… on http://127.0.0.1:8188` | the knobs and the prompt id the host gave; no server is started, because there is no server any more | — |
| music | `[music] OK out/audio/music/<name>.ogg (1.9 MB, 30.014668 s)` | **18.3 s** measured (16.4–18.3 over five renders); the seconds are the file's, not the request's | exit 5 `the track is clipped: N consecutive samples pinned at full scale` — expected at this pin, see above |
| speech | `[tts] reference assets-src/voices/warden/ref.wav uploaded as forge_voice_warden_ref.wav` | the clip travels as an **uploaded file**, `LoadAudio` reads it by that name and the host decodes it in its own venv (PyAV) — no path, no torchcodec, no codes handed over | — |
| speech | `[tts] seed 1357188160, en: Few come this deep. Fewer leave.` | the seed, the language and the line | — |
| speech | `[tts] OK …` | **58.1 s** first (weights + load), 22–28 s after | at this pin it does not get here: exit 5 `the line is silent: peak -120.0 dBFS`, and no record. That is the transformers-5 break, not your call |
| voice | `[voice] designing warden at seed 1357188160: gravel-voiced, sixties, unhurried, dry` | `forge-voice`'s door; the designer works at this pin | — |
| voice | `[voice] OK assets-src/voices/warden/ref.wav (seed N, peak -4.8 dBFS, 6.08 s)` | **24.1 s** measured | — |

**Prompt rules.**

- **Describe the sound, not the game event.** Material, action, space,
  tail: "heavy oak door slams shut in a stone hall, short reverb tail",
  not "door close sfx". The model has never seen your game.
- **`--seconds` is the whole file, tail included.** A tail that needs
  2 s inside a 1 s file is cut; ask for the length the decay needs and trim
  nothing by hand.
- **Music:** genre, mood, instrumentation; `--duration` 10–600 s;
  `--lyrics-file` for a vocal, omit for instrumental; `--thinking` uses the
  5 Hz planner — slower, better structure. Both of ACE-Step's seeds, the
  resolved key and tempo and both checkpoints go into the record.
- **Speech:** the line is the prompt; `[pause 1.5s]` in the text is an
  explicit pause; `--language` defaults `en`, `auto` lets the model infer,
  a name not on the model card's list passes through as a tag with a
  warning. MOSS-TTS's API takes no seed: `seed` stays `null` unless you
  pass `--seed`, which seeds torch's RNG and is recorded only then. The
  voice is not a prompt knob: a line that sounds like the wrong person is
  fixed in `forge-voice` (another seed, a rewritten description), never by
  re-wording the line.
- **One-shots never need a long lead-in**; 50 ms of silence before the
  first sound is a warning at step 2 and a late-feeling hit in a game.

**What the first call costs, per backend.** MOSS-SoundEffect: the weights
(10.46 GB) if absent, then a model load the host keeps for the session.
`torch.compile` of its DiT is **off**, and not as a preference:
`Environment=TORCHDYNAMO_DISABLE=1` is in `backends/comfy/forge-comfy.service`
because with the compile on every effect spent ~60 s compiling and then died
with `cudaMallocAsync does not yet support checkPoolLiveAllocations`
(2026-08-30). It is the host's knob now, not `backends/moss_sfx`'s `[env]`,
and **do not export `TORCHDYNAMO_DISABLE=0`** — the crash is what it
prevents. ACE-Step: the host loads the checkpoint on the first track and
gives the card back by itself. MOSS-TTS: 5.72 + 3.95 + 6.61 GB if absent,
then a load that stays until the unit is restarted.

### 2. Judge — `just audio out/audio/<kind>/<name>.<ext>`

Measures the file and draws it to `out/audio/<name>.png`. Exit 1 when it
is silent or clipped. The block, as captured on the first seed:

```
file:     out/audio/sfx/sword_whoosh.wav
format:   2.000s  48000 Hz  1 ch
level:    peak -0.0 dBFS   rms -22.5 dBFS   lufs -19.9   crest 22.5 dB
shape:    lead silence 169 ms   tail 1354 ms   dc -0.0000   full-scale 1 (run 1)
warning:  169 ms of silence before the first sound - a one-shot will feel late
plot:     out/audio/sword_whoosh.png (1400x690)
```

A healthy effect reads `verdict:  clean` where the `warning:` line is
(`peak -1.0 dBFS rms -20.1 lufs -22.4, verdict: clean` is one from the
sample library). Any `warning:` line replaces `verdict: clean`, and the
exit stays 0 unless it is one of the two defects:

| Warning | Threshold | Gate? |
|---|---|---|
| `file is silent` | nothing above 0.001 | **defect, exit 1** |
| `clipped: N consecutive samples pinned at full scale (M total)` | a **run** of ≥ 3 samples at ≥ 0.999 | **defect, exit 1** |
| `DC offset ±x` | \|dc\| > 0.01 | worth a look |
| `N ms of silence before the first sound` | lead > 50 ms | worth a look — a one-shot will feel late |
| `ends N dB quieter than it starts` | > 9 dB, files over 10 s | worth a look — jumps at a loop point |
| `crest factor N dB` | < 6 dB | worth a look — very compressed |
| `peaks at only N dBFS` | < −18 dBFS | worth a look — quiet beside a normalised library |

**Clipping is a run, not a count.** Peak-normalising to 0 dBFS puts a
sample at the rail by construction; `full-scale 1 (run 1)` is not a
defect and a count-based check once flagged five of eight shipped
effects. **Loudness is approximate** — K-weighting is a high-pass
stand-in, enough to compare sounds in one library and not enough to
certify a master.

**`Read out/audio/<name>.png`.** Waveform over spectrogram, the facts
line under the title and the warning in red. You cannot hear the file;
the plot shows what the numbers do not: flat tops (a clipped run), dead
air at the head, a tail cut mid-decay (the waveform ends at a wall, not
at zero), a hum as a horizontal line in the spectrogram, a click as a
vertical one. For the user's ears: `just play` opens the studio on the
audio library; a file under `out/` is not in the library yet, so judge it
from the plot or promote it first.

The 169 ms lead above went to 71 ms on the second seed with "sharp
transient at the very start" added to the prompt and `--seconds 1.5`
(the whoosh and its room needed no more): the re-render in the table
below is the fix, and it is cheap. The sample's line, for the speech shape:
`5.280s 48000 Hz 2 ch`, `peak -4.8 dBFS rms -18.7 dBFS lufs -19.8 crest
13.9 dB`, `lead silence 0 ms tail 35 ms`, `verdict: clean` — four phrases
with the pauses the punctuation asked for.

### 3. Ship — `just promote-audio <kind> <name> out/audio/<kind>/<name>.<ext>`

`<kind>` is `sfx`, `music` or `voice`. The record is found at the file's
stem + `.json`, which is where step 1 put it; `--record <json>` names
another; no record at all files the sound with `unknown` provenance and
says so on stderr (`no record at … — the sound will say unknown
provenance`). The door runs the same measurements as step 2 and refuses a
defective file (silent, or clipped hard enough to distort) with exit 2,
naming the defect and `--allow-defective`; that flag ships it anyway, and
the fix is upstream of the promote, not the flag. Other flags: `--prompt`,
`--tag` (repeatable), `--note`, `--created-by`, `--overwrite`.

The same door by hand, which is also what a `--note` or `--prompt` with
spaces needs — a recipe's `*flags` re-splits them:

```sh
forge promote audio sfx out/audio/sfx/<name>.wav <name> \
    --record out/audio/sfx/<name>.json --created-by agent:<you>
```

(`forge` is `target/debug/forge` under the toolkit.)

| Line | Meaning |
|---|---|
| `shipped sword_whoosh.wav: 1.50s, 48000 Hz, 1 channel(s)` | the bytes were copied as judged, the sidecar written after them |
| `manifest refreshed` then `-> audio/sfx/sword_whoosh.wav (recorded, moss_sound_effect, created 2026-08-23 by agent:e2e)` | `recorded` because a record came with it; the manifest is refreshed by the promote itself |
| `shipped warden_greeting.wav: 5.28s, 48000 Hz, 2 channel(s)` then `-> audio/voice/warden_greeting.wav (recorded, moss_tts, created 2026-08-23 by human)` | a voice line; its sidecar's `source` is the reference clip (path + sha256) and its generator block names the voice and, for a designed one, its `voice_record` |
| `forge: …/voice.json is a designed voice's record, not a spoken line's — lines are cloned from the voice with \`forge gen speech --voice <name>\` …` (exit 2) | a voice's own record was handed to the promote; the audition clip is a source, not a line |
| `forge: <name> already exists as …/assets/audio/sfx/<name>.wav; pass overwrite to replace it` (exit 2) | refused; `--overwrite` replaces the file, the sidecar and — across containers — removes the old file so one stem never has two |
| `<name> is already a <kind>: audio/<kind>/<name>.<ext> — a game's audio map is by stem, so pick another name` | refused, and `--overwrite` does not help: pick another name |
| `forge: out/audio/sfx/<name>.json describes a sfx run, not music — it is not the sound's record` (exit 2) | the `--record` is for another kind of file; checked before the stem |
| a refusal naming `file is silent` or `clipped: …` and `--allow-defective` (exit 2) | the door measured the file and it is defective; re-render (another seed, lower `--cfg`) rather than shipping it — `--allow-defective` is for the rare sound that is meant to be that way |

### 4. Verify

- `just audio-list` — every shipped sound on one line each:
  `? sfx/sword_whoosh.wav   1.50s  48000 Hz 1ch  peak   -0.0  lufs  -18.8`,
  then `1 file(s)   ! = defect, ? = worth a look`; exits 1 on any `!`. An
  empty library says `no audio under …/assets/audio — nothing to measure`
  and passes.
- `just manifest-check`, `just verify`, `just audit`. Those are the
  project's gates; `just ci` is the toolkit's own gate, run from the
  checkout — its dev recipes always act on the checkout, never on your
  project.

## Seen → consequence → fix

| Seen | Consequence | Fix |
|---|---|---|
| exit 3 in ~100 ms, `<backend> is not installed — generation through it is off` | nothing ran | `forge-setup` |
| the doctor row reads `off — [make] sfx = false` | nothing is wrong; this project did not choose that kind, so it was never probed | choose it in `[make]` (or `init_project` with `adopt: true`), then `forge setup` |
| a comfy row reads `missing` and the generate exits 3 | nothing is listening at `[hardware] comfy_url` | `systemctl --user start forge-comfy`, then `systemctl --user status forge-comfy` |
| `generate_audio` came back with a job id and no file | correct — a generate returns a job | `wait(job, max_s)`; the result names the file and its record |
| `FAIL model:… absent` in doctor, then a long first call | the weights are downloading (sfx ~11 GB, tts ~8 GB) | wait once; doctor reads `ok` after |
| CUDA out of memory | the card was held — by whatever the ComfyUI host last loaded, a studio window, or a generate you forgot | `just gpu`; `forge gpu --free`, and for the MOSS pack `systemctl --user restart forge-comfy`; never two generates at once |
| the job sits `blocked` with `blocked_by: comfy — …` | the release ladder could not prove the card came back and is **withholding** the lease | that is the safety net working: `systemctl --user restart forge-comfy`, then `forge gpu --free`, which clears the withholding only when free VRAM reaches the card's idle floor |
| `the line is silent: peak -120.0 dBFS`, exit 5, no record | MOSS-TTS 1.7B does not run under the host's transformers 5; the pack caught its own error and returned a silent tensor | nothing you can pass fixes it — `backends/moss_tts`'s notice names the pin that would. Say so; do not ship the file |
| `the track is clipped: N consecutive samples pinned at full scale`, exit 5 | ACE-Step turbo normalises to peak on the host side | expected at this pin: `music` renders and does not promote. The fix is a stated gain knob in the graph (`AudioAdjustVolume` between `VAEDecodeAudio` and `SaveAudio`), never a file edited by hand |
| `clipped: N consecutive samples …`, exit 1 | the render overshot | re-render: another `--seed`, a lower `--cfg`; never normalise the file by hand — the record would then describe a sound that is not the file |
| the tail ends at a wall in the plot | `--seconds`/`--duration` shorter than the decay | re-render longer |
| `N ms of silence before the first sound` on a one-shot | the hit will feel late in a game | re-render with another seed; a re-prompt that names the attack ("sharp transient") helps |
| a voice that does not sound like the reference | the reference is too short, too long, noisy or two people | 5–15 s, one clean speaker; the stderr length warning names it; a designed voice is rerolled in `forge-voice` |
| `--voice kessa: no designed voice at …/assets-src/voices/kessa/ref.wav` (exit 4) | a bare name that nothing designed | `forge-voice` (`just voice kessa "…"`), or a path to a clip you brought |
| `ffmpeg is not on PATH` (exit 6) | the host saves FLAC and every audio verb transcodes it here | install ffmpeg; it is refused before the card is leased, not after a render |
| `--language xx` warning | passed through as a tag, not rejected | fine if the model handles it; the record keeps what you said |
| `<name> is already a <kind>` | stems are one namespace across sfx/music/voice | another name |
| the user says it sounds wrong | the plot passed and the ear did not | back to step 1: the prompt or the seed; the knobs do not change what a model heard |

## Commit set

`assets/audio/<kind>/<name>.<ext>` + `<name>.json`; `assets/library.json`;
for a voice line, the voice it was cloned from —
`assets-src/voices/<name>/{ref.wav,voice.json}` when designed, the clip plus
its `SOURCES.md` row when brought and its licence allows it in the
repository (the record carries its sha256 either way). Never `out/`. Commit
only when the user asks.

## Known limits (say them, don't fight them)

- Nothing here is bit-reproducible; a re-render at the same seed is a
  sibling, not a copy. The record is honest about that by claiming the
  hash.
- MOSS-TTS ships as the one speech backend; `--backend` names it and
  naming OmniVoice exits 2 and says it is a v1.1 add.
- A designed voice speaks English or Chinese; a line in one of MOSS-TTS's
  other 29 languages needs a brought clip.
- The 8B MOSS-TTS Delay model OOMs on 24 GB, so `speech.api.json` states
  the **1.7B** — which is a different voice from the 4B the venv ran, and
  which does not run at all under the host's transformers 5. A voice
  re-cloned after that move does not match one cloned before it.
- **`just speech` makes no line at this pin and `just music` makes none
  that promotes.** Both are the host's, both are dated in
  `designs/hosting.md`, and neither is fixed by re-prompting. `sfx` and
  `voice` work.
- `just audio` cannot tell a good sound from a bad one — only a broken one.
  What the generators now refuse for themselves is the same three checks,
  run before the record is written: silence, a full-scale run, a render
  far shorter than the length asked for.
