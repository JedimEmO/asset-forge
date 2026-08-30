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
the file is the bytes that were judged. The sfx, inspect and promote lines
quoted below were captured on a real run (2026-08-23, a sword whoosh, two
seeds); the music lines on the sample library's render the same day; the
speech lines on the sample's `warden_greeting`, cloned from the designed
`crypt_warden` voice the same evening.

## Prerequisites (check, don't assume)

- `just doctor` — the row for the backend you need reads `ok`:

  | Recipe | Row | What each word means here |
  |---|---|---|
  | `just sfx` | `moss_sfx` `[comfy]` | `partial`: the weight (`MOSS-SoundEffect-v2.0`, ~11 GB) is not in the host's model folder yet, and the row names it with its GB |
  | `just music` | `acestep` `[comfy]` | `partial`: the ~7.5 GB checkpoint set is not on disk |
  | `just speech` | `moss_tts` `[comfy]` | `partial`: `MOSS-TTS-Local-Transformer-v1.5` (~8 GB) is absent; the same row covers `just voice` (`forge-voice`) and its ~4 GB `MOSS-VoiceGenerator` |

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
- **The GPU is free enough.** `just gpu`. MOSS-SoundEffect ~6–8 GB,
  MOSS-TTS (the 4B) ~12 GB, ACE-Step ~8 GB — all three budgets, not
  measurements. What stays on the card is **the ComfyUI unit**: ~0.4 GB of
  CUDA context while it is up, plus whatever workflow last loaded, until
  its unload node, `POST /free`, or `systemctl --user stop forge-comfy`.
  None of them co-resides with the image model (23.3 GB measured) or an
  ARDY sweep (15.4 GB measured); a studio window on the real adapter holds
  the card too.
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

| Recipe | Log line | Healthy | Not |
|---|---|---|---|
| sfx | `[sfx] loading OpenMOSS-Team/MOSS-SoundEffect-v2.0` | most of a call is this load (the backend's own `Loading DiT from …`, `DiT loaded: missing=0, unexpected=0`, `Pipeline assembled on cuda` follow); ~100 s more the first time while the weights arrive | exit 3 in ~100 ms: not installed → `forge-setup` |
| sfx | `[sfx] 2 s, 100 steps, cfg 4, seed 7: a heavy steel sword swung fast …` | the knobs, the seed and the prompt; the seed is fresh and random (`seed 457087253`) unless you said `--seed`; either way it is in the record. `--seconds 1.5` is accepted and prints `1.5 s` | — |
| sfx | `[sfx] OK …/out/audio/sfx/<name>.wav (seed 7)` then `record   …/<name>.json`, `output   …/<name>.wav`, `model    OpenMOSS-Team/MOSS-SoundEffect-v2.0`, `seed     7`, `elapsed  38.2 s` | ~30–40 s a call with the weights cached | — |
| music | `[music] ACE-Step server not running — starting it` | first call of the session; the model load is minutes; the server's log is `~/.local/state/asset-forge/acestep-server.log` | the start never answers `/health` within the timeout → read that log |
| music | `[music] server up after 6s (pid N)` | 6 s is a warm server; minutes is a cold one | — |
| music | `[music] OK … (1.9 MB, 10.0 s)` | the seconds are what `--duration` asked for; the record carries what the server measured | seconds far from `--duration`: look at the plot's tail before shipping it |
| speech | `[tts] loading OpenMOSS-Team/MOSS-TTS-Local-Transformer-v1.5 on cuda (sdpa)` after transformers' own `Loading weights: 100%\|…\| 438/438` and `Falling back to SDPA because flash_attention_2 is unavailable` | the 4B Local-Transformer; most of a call (34 s all in, warm) | an OOM here means the 8B was named through `MOSS_TTS_MODEL`: it does not fit 24 GB with the tokenizer resident |
| speech | `[tts] reference …/assets-src/voices/crypt_warden/ref.wav: 7.28 s at 24000 Hz -> 91 codes` | the clip read and tokenized here (soundfile in, the processor's own tokenizer; 12.5 codes a second); `--voice crypt_warden` resolved to that path | stderr `… is N s; 5–15 s clones best` before it: the reference is outside the band; it still runs, and it clones worse |
| speech | `[tts] English, cloning crypt_warden: Few come this deep. Fewer leave. …` | the language, the voice, the line | `inferred language` with `--language auto`; a tag you typed that is not on the card's list is passed through after a warning |
| speech | `[tts] OK …/out/audio/voice/warden_greeting.wav` then `record …/warden_greeting.json`, `output …/warden_greeting.wav`, `model OpenMOSS-Team/MOSS-TTS-Local-Transformer-v1.5`, `voice crypt_warden`, `elapsed 33.9 s` | the line is 48 kHz stereo, the model's output rate; the record carries the reference and, for a designed voice, its `voice.json`, both hashed | `Could not load libtorchcodec` (exit 5): the reference went to the processor as a path — it must not; see `designs/hosting.md` |

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

**What the first call costs, per backend.** MOSS-SoundEffect: the
weights (~11 GB) if absent, then the model load every call;
`torch.compile` of the DiT is **off** by default (`TORCHDYNAMO_DISABLE=1`
in `backends/moss_sfx/backend.toml`) because it runs for minutes and dies
with the process — export `TORCHDYNAMO_DISABLE=0` only for a long
`--batch-file` session. ACE-Step: the host loads the model on the first
track of a session and keeps it until the workflow's unload node or
`POST /free`; `systemctl --user stop forge-comfy` gives the whole card back
at the end of a session. MOSS-TTS: the weights (~8 GB) if absent, then the
4B load every call.

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
| CUDA out of memory | the card was held — by whatever the ComfyUI host last loaded, a studio window, or a generate you forgot | `just gpu`; `systemctl --user stop forge-comfy`; never two generates at once |
| `the ACE-Step server did not answer /health within N s — see …/acestep-server.log` | the server failed to load | read `~/.local/state/asset-forge/acestep-server.log`; the ready timeout is `[server] ready_timeout_s` in `backends/acestep/backend.toml`, and `--timeout S` is the wait for a track, not for the start |
| `clipped: N consecutive samples …`, exit 1 | the render overshot | re-render: another `--seed`, a lower `--cfg`; never normalise the file by hand — the record would then describe a sound that is not the file |
| the tail ends at a wall in the plot | `--seconds`/`--duration` shorter than the decay | re-render longer |
| `N ms of silence before the first sound` on a one-shot | the hit will feel late in a game | re-render with another seed; a re-prompt that names the attack ("sharp transient") helps |
| a voice that does not sound like the reference | the reference is too short, too long, noisy or two people | 5–15 s, one clean speaker; the stderr length warning names it; a designed voice is rerolled in `forge-voice` |
| `--voice kessa: no designed voice at …/assets-src/voices/kessa/ref.wav` (exit 4) | a bare name that nothing designed | `forge-voice` (`just voice kessa "…"`), or a path to a clip you brought |
| `Could not load libtorchcodec` (exit 5) after the 4B loaded | the reference reached the processor as a path; torchaudio's loader cannot open files in this env | the inner half reads the clip with soundfile and tokenizes it itself — this is a regression in `speech.py`, not a setup step |
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
- The 8B MOSS-TTS Delay model OOMs on 24 GB; the 4B Local-Transformer is
  what runs.
- The ACE-Step checkout is expected to be dirty (the soundfile patch);
  doctor notes it and it is not a defect.
- `just audio` cannot tell a good sound from a bad one — only a broken one.
