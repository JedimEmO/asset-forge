# Sources

What the sample library was made from, and on what terms. "Can we ship
this?" has to be answerable from this file alone, without re-deriving
anything or going back to the network.

## Reference images (`refs/`)

Every reference image under `refs/` has a row here — where it came from, on
what terms, and what was made from it. A reference PNG claims integrity (its
sha256, in the `<name>.ref.json` beside it) and this row, never
regeneration: the image is an input to the toolkit, not an output of it, and
no model here will paint it twice. The row is where its origin and its
licence live, and a PNG without one is a file nobody can account for — which
is why `forge verify` fails on it.

**The door writes the row; nobody types one.** `forge ref import <png>
--name <n> --kind character|prop --source "<where it came from>"` (MCP
`import_reference`, `just ref-import`) is the one way a PNG gets under
`refs/`: it holds the picture to the format, keys it with `mesh.py`'s own
keyer, runs the pre-checks that would otherwise cost a lift, stores the
**original bytes** — never the keyed image — and writes both
`<name>.ref.json` and this row. Re-import with `--overwrite` to correct a
row's Origin; the door restates Origin and the date and leaves the **For**
cell standing, because that sentence is somebody's account of what was made
from the picture and not the door's to invent. The first three rows below
were written that way on 2026-08-30, restating what the file already said,
and `ember_knight` came through the door for real on 2026-08-31; the
`<name>.lift.json` beside each still carries the lift.

| File | Origin | For | Date |
|---|---|---|---|
| `characters/vex_runner.png` | xAI grok (cloud image model), image_edit chained from a style board that does not ship here, then two self-edits (one arm per side in T-pose; taller, six heads, shoulders higher); painted 2026-08-18 | `assets/bodies/vex_runner.glb` — lifted at seed 7 after seed 42 left the rear skull absent | 2026-08-30 |
| `props/sword.png` | xAI grok, image_edit chained from the same style board; painted and lifted 2026-08-22 | `assets/models/sword.glb` — a held weapon, grip at the origin (`hand_r` socket) | 2026-08-30 |
| `props/barrel.png` | xAI grok, image_edit chained from the same style board; painted 2026-08-18 | `assets/models/barrel.glb` — a floor prop, floor at the origin | 2026-08-30 |
| `characters/ember_knight.png` | xAI grok (cloud image model), drawn 2026-08-30 under xAI's consumer terms (user owns the output) for the Phase 2 fitted-skeleton runs; third version, re-proportioned to fingertip span equal to height and seven heads after two squat drafts the fit gate refused | `assets/bodies/ember_knight.glb` — lifted at seed 7 (the first lift that needed no re-roll), skinned by SkinTokens and carrying its own fitted bone lengths | 2026-08-31 |
| `characters/scrapyard_scavenger.png` | OpenAI built-in image_gen; generated 2026-09-04; prompt in designs/scrapyard/prompts-v1.json | a character reference, imported by `forge ref import` | 2026-09-04 |
| `characters/scrapyard_rusher.png` | OpenAI built-in image_gen; revised solid head 2026-09-04; prompt in designs/scrapyard/rusher-reference-v2-prompt.txt | a character reference, imported by `forge ref import` | 2026-09-04 |
| `props/scrapyard_rifle.png` | OpenAI built-in image_gen; generated 2026-09-04; prompt in designs/scrapyard/prompts-v1.json | a prop reference, imported by `forge ref import` | 2026-09-04 |
| `props/scrapyard_repair.png` | OpenAI built-in image_gen, generated 2026-09-04 for the scrapyard combat batch | a prop reference, imported by `forge ref import` | 2026-09-04 |
| `props/scrapyard_overdrive.png` | OpenAI built-in image_gen, generated 2026-09-04 for the scrapyard combat batch | a prop reference, imported by `forge ref import` | 2026-09-04 |
| `props/scrapyard_magnet.png` | OpenAI built-in image_gen, generated 2026-09-04 for the scrapyard combat batch | a prop reference, imported by `forge ref import` | 2026-09-04 |

**Licence posture.** All four were generated with the grok CLI ("Grok
Build", xAI) in the repository this one was distilled from. xAI's consumer
Terms of Service (<https://x.ai/legal/terms-of-service>) state that the user
retains ownership of inputs and owns the output, free to use including
commercially; xAI asks (not requires) attribution per its brand guidelines
and keeps a broad licence back to user content for its own purposes. One
honesty note, recorded so nobody re-checks it: x.ai serves 403 to plain
fetchers, so that wording was confirmed in 2026-08 through search excerpts
of the then-current terms rather than a direct page retrieval. If a
storefront demands chapter and verse, open the page in a browser and paste
the clause here.

Images with no human authorship are, per current US Copyright Office
guidance, likely not copyrightable by anyone. That cuts both ways: weak
exclusivity over these exact pixels, and no third party with a claim
against shipping them. For source references that trade is fine; nothing
derived from them is encumbered by the images.

The full prompts are in each `<name>.lift.json` (`inputs[0].prompt`),
verbatim from the repository this one was distilled from — examples of
prompt shape (subject, framing, background rules), not style guidance for
this toolkit; a record is never edited to tidy them. The edit chain that
produced each PNG — the style board, the intermediate edits — is not
reproducible and does not ship; that is why the lift record claims the
PNG's hash and nothing upstream of it.

**What nvdiffrast means for the shipped meshes, plainly.** The textures on
all four lifted samples (`vex_runner`, `ember_knight`, `sword`, `barrel`)
were baked through nvdiffrast 0.4.0, which ships under the NVIDIA Source Code License
— non-commercial use only. So: the geometry's provenance is clean
(TRELLIS.2 is MIT, code and weights), but **the sample textures are not
licensed for commercial use or commercial redistribution**. They ship for
demonstration, so the tools have something to show on a fresh clone. A
commercial project does not reuse these samples; it lifts its own
references once a replacement baker lands, or ships its own textures.
Every lift record names the baker (`texture_baker: "nvdiffrast (NVIDIA
Source Code License, non-commercial)"`), `assets/LICENSE.md` says the same
per sample kind, and `backends/README.md` has the component table.

## Clips (`takes/`, `assets/clips/`)

The six clips — `walk`, `idle`, `roll`, `jump`, `pistol_shoot`, `death` —
are baked from ARDY takes (NVIDIA; code Apache-2.0, weights under the
NVIDIA Open Model License) generated in the previous repository between
2026-07-30 and 2026-08-02, at ARDY commit `693f74d`. The raw takes ship
under `takes/<name>.npz`, untrimmed, so every clip rebuilds from its own
record (`forge audit`, byte for byte).

Their provenance is **reconstructed**, on purpose. No take record survives
from those sweeps: the sweep tool of the time wrote none, and the sidecars
that did exist once carried `seed 0, duration 4.0` because those were a
writer's argparse defaults, not what ran. So the generator block says `null`
for seed, cfg, duration and sample, and the note on each clip names the
sweep file the take came from, which is the one honest fact about the
sweep — read its `s0` as a filename, not a seed. The recipes (trims,
in-place mode, loop blend, retime) are recorded in full because they were
re-applied here through `forge promote clip`; `auto_trim` is `null` because
the door has no flag for it — the old loop and action searches found the
trims, and the trims are what the record states. This is the shape a
migrated clip is allowed to have: a migration nulls what it cannot know and
never launders a guess up to `recorded`.

## Audio (`assets/audio/`)

The three sounds were rendered in this repository on 2026-08-23 through
`just sfx` and `just music`, so each carries a **recorded** generator block
with its seed, steps, cfg and model. The previous repository's sounds had
`unknown` provenance and were not brought over.

| Name | Kind | Backend | Seed | Note |
|---|---|---|---|---|
| `footsteps_stone` | sfx | MOSS-SoundEffect-v2 (Apache-2.0), 2 s, 100 steps, cfg 4 | 7 | four boot steps at a walking pace; the model renders a sequence however the prompt is worded — a "single footstep" prompt gave six quiet taps peaking at -19.5 dBFS |
| `door_metal` | sfx | MOSS-SoundEffect-v2, 2 s, 100 steps, cfg 4 | 101 | first seed tried |
| `ambient_crypt` | music | ACE-Step 1.5 turbo (MIT), 30 s, `acestep-5Hz-lm-0.6B` planner; B minor, 80 bpm as the model resolved them | 7, plus the LM seed the record carries | not a seamless loop: every 15 s render (four seeds, two wordings) spent its last third near silence; at 30 s the model fills the length and ends with about two seconds of tail |

`tavern` (music) was rendered on 2026-08-31 through the ComfyUI host's
ACE-Step 1.5 turbo graph at `--gain-db -3`, the first track to ship through
that graph's `AudioAdjustVolume` node: the same prompt and seed without it
came off the host pinned at 0.0 dBFS with 84 consecutive clipped samples, and
with it the file peaks at -2.6 dBFS with none. It is not a loop; its last
2.7 s are a decaying tail. `door_slam` and `chain_drag` (sfx) are this
repository's own MOSS-SoundEffect renders and carry recorded generator blocks;
neither has a row above because a sound's record *is* its account, and this
table exists for the pictures, whose origin lives nowhere else.

Neither generator is bit-reproducible, so the shipped file is the exact
file that was auditioned (`just audio`, verdict `clean`) and its record is
the account of the run that made it; a re-render at the same seed gives the
same measurements and different bytes.

## Voices (`voices/`) and voice lines (`assets/audio/voice/`)

A voice here is designed, not brought. `voices/crypt_warden/ref.wav` was
spoken by MOSS-VoiceGenerator (OpenMOSS-Team, 1.7B, Apache-2.0) on
2026-08-23 from the description *"Deep, slow, weathered male voice,
English, low pitch, unhurried, grave and calm, the keeper of an old tomb"*
at seed 7, reading the toolkit's default audition sentence; `voice.json`
beside it is the run's record (the description, the line, the seed, the
four sampling knobs, the clip's sha256), and the provenance is
**recorded**. Seeds 11 and 1234 were auditioned alongside it; 1234 cut its
last word (`tail 0 ms`) and 7 had the cleanest plot (lead 1 ms, tail
91 ms, verdict clean). Nobody's voice was recorded to make it, which is
the point: a reference clip the project does not own is a line it cannot
ship. A designed voice is accounted for by its `voice.json` and needs no
row here; a voice that *is* brought has no record and gets one — a
`voices/` table with origin, terms and what was made from it — and `forge
verify` refuses a `voices/<name>/ref.*` with neither.

The one line, `warden_greeting` ("Few come this deep. Fewer leave. State
your business, and mind the dust."), was cloned from that clip by
MOSS-TTS-Local-Transformer-v1.5 (Apache-2.0) the same day through
`just speech … --voice crypt_warden`; its record carries the clip and the
voice record as hashed inputs, and its sidecar names the clip as its
`source`, so the line's provenance reaches the description and the seed.
MOSS-TTS takes no seed, so the line's `seed` is `null` and the shipped
file is the exact render that was judged (`just audio`: 5.28 s, verdict
clean).
