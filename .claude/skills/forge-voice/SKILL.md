---
name: forge-voice
description: Design a character's voice from a description — MOSS-VoiceGenerator speaks one audition line into assets-src/voices/<name>/ref.wav with its record, judged from its plot and rerolled by seed until it is the character; every spoken line is then cloned from it by name through forge-audio's speech step. Use when the user wants a new voice for a character, says a project has no reference clip to clone, or wants a voice regenerated (another seed, a rewritten description).
---

# Voice: description → `assets-src/voices/<name>/{ref.wav,voice.json}` → lines by name

A project never brings a voice. `just voice <name> "<description>"` has
MOSS-VoiceGenerator (1.7B, Apache-2.0, the same `moss_tts` env as the
cloner) speak one audition sentence in a timbre designed from the words,
and what it speaks becomes a **source**: `ref.wav` beside `voice.json`, a
generator record of kind `voice` naming the description, the line, the
seed and every sampling knob. Every line of that character is then
`just speech <stem> "<text>" --voice <name>` — forge-audio's step 1 — and
the line's record carries the clip's hash and the voice record's, so a
shipped line chains back to the description and the seed. The voice is
designed once; two lines a month apart are the same person.

Every command and log line below was captured on a real run (2026-08-23,
`crypt_warden`, three seeds, one line shipped into the sample library).

## Over MCP

There is **no `design_voice` tool yet** (Phase 4 in `designs/forge2.md`):
designing a voice is `just voice` / `forge gen voice` at a terminal. Once a
voice exists under `assets-src/voices/<name>/`, `generate_audio {kind:
speech, voice: <name>}` clones it over MCP — but read `forge-audio`'s
speech step first: MOSS-TTS makes no line at the current TTS-Audio-Suite
pin, and the door refuses the silence rather than filing it.

## Prerequisites (check, don't assume)

- `just doctor` — `moss_tts` reads `ok`. `partial` with
  `model:OpenMOSS-Team/MOSS-VoiceGenerator absent` means the ~4 GB weights
  are not cached: the first `just voice` downloads them, or `just setup
  moss_tts` does. `missing` → `forge-setup`.
- **The GPU is free enough.** `just gpu`. The design peaks at ~12 GB (the
  generation loop, not the 1.7B's weights), the cloner at ~12 GB; neither
  co-resides with a sweep (15.4 GB measured) or the image model (23.3 GB).
  Both run inside the ComfyUI host now, so what holds the card afterwards is
  the host: `forge gpu --free`, or `systemctl --user stop forge-comfy`.
  Never two generates at once.
- A name: `[a-z0-9_]+`, the character's, because it becomes a directory and
  the `--voice` argument of every line.

## Steps

### 1. Design — `just voice <name> "<description>" [--seed N] [--line "<sentence>"] [--created-by human|agent:<you>]`

```
just voice crypt_warden "Deep, slow, weathered male voice, English, low pitch, unhurried, grave and calm, the keeper of an old tomb" --seed 7 --created-by human
```

Writes `assets-src/voices/<name>/ref.wav` and `voice.json`. A free-text
`describe` goes through a `just` positional, so apostrophes and commas are
fine; the flags after it are `*flags` and re-split quoted values, so a
multi-word `--line` goes to the binary directly:
`target/debug/forge gen voice <name> --describe "…" --line "…"`.

| Log line | Healthy | Not |
|---|---|---|
| `[voice] loading OpenMOSS-Team/MOSS-VoiceGenerator on cuda (sdpa)` | most of a call; ~80 s the first time after a download (a cold 4 GB file), ~20 s warm | exit 3 in ~100 ms: not installed → `forge-setup` |
| `Generating bs1 ...:   3%\|▎  \| 108/4096 [00:03<01:40, 39.63it/s]` | the model's own progress, ~40 tokens/s; the bar stops when the line ends, well short of its 4096 ceiling | a bar that runs on for minutes: the model did not stop — reroll the seed |
| `[voice] crypt_warden seed 7: Deep, slow, weathered male voice, …` then `[voice] line: The river runs past the old mill at dawn, …` | the seed (fresh and random unless `--seed`; either way in the record) and the exact words spoken | — |
| `[voice] OK …/assets-src/voices/crypt_warden/ref.wav (7.28 s at 24000 Hz)` then `record …/voice.json`, `output …/ref.wav`, `model OpenMOSS-Team/MOSS-VoiceGenerator`, `seed 7`, `voice crypt_warden`, `elapsed 22.2 s` | 5–15 s, the band the cloner wants; 24 kHz mono is the model's rate | stderr `the audition is N s; 5–15 s clones best` → another seed, or a longer/shorter `--line` |
| `forge-gen: input_rejected: …/ref.wav exists — a designed voice is a source, and replacing it changes every line cloned from it afterwards; pass --overwrite …` (exit 4) | refused: the voice is a source | `--overwrite` only when the user wants the character re-voiced, and then every shipped line of it is re-rendered (`forge verify` warns on each until it is) |

**The seed is the voice.** A description is a region of timbres, not a
point; the same words at another seed are another person. Reroll with
`--seed N` until it *is* the character. Never edit the wav — a hand-edited
clip no longer hashes to its record and `forge verify` fails it by name.

**Rerolling without burning the name.** `--out-dir out/voices/seed<N>`
designs into scratch (`out/voices/seed11/crypt_warden/ref.wav`), so three
seeds can sit side by side; the one kept is re-run into the project with
that `--seed` and judged again. On this card the same seed gave the same
bytes twice (sha256 equal) — convenient, not promised: the record claims the
hash, not regeneration.

**How to write a description.** The model card's register, English or
Chinese only: *"Hearty, jovial tavern owner's voice, loud and welcoming with
a slightly gruff, friendly tone in American English, radiating warmth and
hospitality."* Name, in roughly this order: **gender, age, pitch, pace,
accent, texture, mood**, then who they are. One sentence, adjectives before
the noun; the persona at the end is what the model leans on for delivery.
Say "English" when the line is English — it decides the accent's base. Do
not describe the game event or the scene; the model has never seen your
game. Do not ask for an emotion the audition line cannot carry: a neutral
sentence read "furious" comes back strained. The default line is neutral on
purpose, and `--line` is for a character whose register the default cannot
show (a shout, a whisper, a brogue).

### 2. Judge — `just audio assets-src/voices/<name>/ref.wav`, then Read the plot

```
file:     assets-src/voices/crypt_warden/ref.wav
format:   7.280s  24000 Hz  1 ch
level:    peak -3.3 dBFS   rms -16.7 dBFS   lufs -17.1   crest 13.4 dB
shape:    lead silence 1 ms   tail 91 ms   dc -0.0001   full-scale 0 (run 0)
verdict:  clean
plot:     out/audio/ref.png (1400x690)
```

Every voice's clip is `ref.wav`, so `just audio` writes every plot to
`out/audio/ref.png`; when comparing seeds, name the plot yourself:
`target/debug/forge audio inspect out/voices/seed11/crypt_warden/ref.wav
--out out/audio/crypt_warden_s11.png`. **Read the PNG.** You cannot hear
it; the plot shows what to refuse:

| In the plot / the block | Means | Do |
|---|---|---|
| `verdict: clean`, lead < 100 ms, tail > 0 ms, the waveform ending at zero | a clip the cloner can use | keep, or reroll for character |
| `tail 0 ms`, the waveform ending at a wall | the last word is cut (seed 1234 did this: 6.64 s, `tail 0 ms`) | reroll — the cloner learns the cut too |
| `clipped: N consecutive samples …`, exit 1 | overshoot | reroll; never normalise the file |
| `N ms of silence before the first sound` | a late start the cloner copies into every line | reroll |
| a 1–1.5 s gap mid-sentence | the comma, read slowly — a slow speaker's pause, not a defect | fine for a "slow, unhurried" voice; for a brisk one, reroll or remove the comma with `--line` |
| dense low harmonics in the spectrogram | a low voice, as described | the description is being followed |

The three seeds on the sample: 7 → 7.28 s, lead 1 ms, tail 91 ms, clean;
11 → 8.08 s, lead 10 ms, tail 101 ms, clean; 1234 → 6.64 s, **tail 0 ms**,
cut. 7 was kept for the cleanest plot. They sound the same to you; the plot
is what you have, and the user's ear is what decides: `just play` opens the
studio on the audio library for a shipped line, and a file under
`assets-src/` or `out/` is heard by any player the user has.

### 3. Lines — forge-audio's speech step, by name

```
just speech warden_greeting "Few come this deep. Fewer leave. State your business, and mind the dust." --voice crypt_warden --created-by human
just audio out/audio/voice/warden_greeting.wav
just promote-audio voice warden_greeting out/audio/voice/warden_greeting.wav --created-by human --tag sample
```

`--voice crypt_warden` resolves to `assets-src/voices/crypt_warden/ref.wav`
and records it (`inputs[role=reference]`, hashed) together with
`voice.json` (`inputs[role=voice_record]`, hashed); the promote writes the
clip as the line's `source` and `voice_record` into the sidecar's
generator block. The log lines, the plot and the refusals are in
`forge-audio`, step 1's speech rows.

### 4. Verify

- `just verify` — every `assets-src/voices/<name>/ref.*` has a `voice.json`
  beside it whose kind is `voice` and whose output hash is the clip, **or**
  a row in `assets-src/SOURCES.md` (a brought clip); neither:
  `FAIL crypt_warden  assets-src/voices/crypt_warden/ref.wav has no
  voice.json beside it and no row in SOURCES.md — a designed voice keeps its
  record (…), a brought clip needs a ledger row …`. A shipped line whose
  voice moved on is a warning naming the line; whose voice is gone, a
  failure.
- `just catalog --kind voice` — the line with `recorded` provenance.
- `just audio-list`, `just manifest-check`, `just verify`. Those are the
  project's gates; `just ci` is the toolkit's own gate, run from the
  checkout — its dev recipes always act on the checkout, never on your
  project.

## Seen → consequence → fix

| Seen | Consequence | Fix |
|---|---|---|
| exit 3 in ~100 ms, `moss_tts is not installed` | nothing ran | `forge-setup` |
| `FAIL model:OpenMOSS-Team/MOSS-VoiceGenerator absent` in doctor | the first design downloads ~4 GB, then ~80 s to load it cold | wait once |
| `voice name 'Crypt Warden' is not [a-z0-9_]+` (exit 4) | the name keys a directory and a `--voice` | lower-case, underscores |
| `…/ref.wav exists — a designed voice is a source …` (exit 4) | refused | another name; `--overwrite` only to re-voice the character, then re-render its lines |
| the audition is 4 s, or 17 s | the cloner warns or refuses (under 3 s, over 30 s) | another seed; a longer or shorter `--line` |
| `tail 0 ms` and a wall in the plot | the line was cut mid-word | reroll |
| CUDA out of memory | the card was held — the ComfyUI host still holding the last model, a studio window, a generate you forgot | `just gpu`; `forge gpu --free` or `systemctl --user stop forge-comfy`; never two at once |
| `Could not load libtorchcodec` on the *speech* step | the reference went to the processor as a path (torchaudio → torchcodec → ffmpeg collides with glib here) | it does not: the inner half reads the clip with soundfile and tokenizes it itself; seeing this means that code regressed |
| `forge verify`: `… is not the clip its voice.json describes — a designed voice is never edited` | someone edited or replaced `ref.wav` by hand | re-design with `--overwrite` so the record and the clip agree |
| the user says it does not sound like the character | the plot passed and the ear did not | the description or the seed; the knobs (`--temperature`, `--top-p`, `--top-k`, `--rep-penalty`) are the card's defaults and move prosody, not identity |
| a voice for a language other than English or Chinese | the model speaks those two | MOSS-TTS clones in 31 languages from a clip you bring — `forge-audio`, with a `SOURCES.md` row |

## Commit set

`assets-src/voices/<name>/ref.wav` + `voice.json` (the source and its
record, both, never one); then the line's `assets/audio/voice/<stem>.wav` +
`.json` and `assets/library.json` as forge-audio says. A brought clip
instead gets a `voices/` row in `assets-src/SOURCES.md`. Never `out/`.
Commit only when the user asks.

## Known limits (say them, don't fight them)

- **Chinese and English only.** The description and the line; other
  languages come out accented at best.
- **A description is not a seed — the seed is.** The record claims the
  clip's hash; regeneration at the same seed is a convenience that held on
  this card and is not a promise across cards or model revisions.
- **One audition is not a range.** A voice designed on a calm sentence may
  not carry a shout; the cloner follows the clip's register. Judge the
  first emotional line before writing twenty.
- **The plot cannot hear.** It finds a cut tail, a late start, clipping —
  not a voice that is the wrong person.
- Apache-2.0 (model and weights); the output is the project's.
