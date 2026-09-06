# Scrapyard combat batch

This batch extends the accepted scavenger, rusher and rifle for an angled
top-down wave-defense game. The game owns movement, aiming direction, damage,
pickup behavior and effect spawning.

## Character packages

- [Armed scavenger](armed-scavenger-combat.glb): character, attached rifle, three named animations.
- [Scavenger without weapon](scavenger-combat.glb): the same three clips for a consumer that attaches equipment itself.
- [Rusher](rusher-combat.glb): rush and melee attack.
- Individual clips and their source recipes live in `assets/clips/scrapyard_*.json`.
  Original selected ARDY takes are retained under `assets-src/takes/`.

| Animation | Duration | Runtime use |
| --- | --- | --- |
| scrapyard_rifle_aim-loop | 3.00 s | Quiet aiming hold; deliberately low activity |
| scrapyard_rifle_run-loop | 0.80 s | In-place run; measured source speed 2.56 m/s |
| scrapyard_rifle_fire | 0.55 s | Single heavy shot; fire event at 0.08 s |
| scrapyard_rusher_rush-loop | 1.25 s | In-place rush; source speed 2.61 m/s before body motion scale |
| scrapyard_rusher_attack | 0.85 s | Melee strike; attack_contact event at 0.403 s |

The companion [grounding.json](grounding.json) measures foot penetration on each
fitted body at baked keys and 60 Hz. Apply its interpolated vertical offset to
the visual root, blended with the same animation weights. It only lifts feet
that would penetrate; airborne poses are not pulled down. The scavenger run
needs roughly 6.7 cm at its deepest sample, and rusher rush roughly 3.3 cm.
This corrects flat-floor placement; terrain foot IK remains consumer work.
Rebuild it with `python3 designs/scrapyard/tools/grounding_track.py`.
The plain GLB bundles do not apply this separate runtime track themselves.

Blend aim/run in the consumer and trigger the shot as a one-shot. For simultaneous
movement and fire, mask the shot to the upper body; these files contain full-body
tracks, not an additive pose layer. A starting blend time is 0.10 s; tune in play.
The rusher bundle applies its recorded motion scale of 0.9167. Root X/Z travel
is stripped from locomotion; the game must move its controller.

The generated aim and fire motions are quiet in the raw motion metrics. Aim's
STATIC warning is intentional for a holding pose; fire is judged from its
retimed body strip and authored event, not presented as a clean raw metric row.
The new shot has visible kick and recovery but is a heavy-shot animation rather
than a sustained automatic-fire loop.

## Rifle attachment

These generated player poses are left-handed: `hand_l` is the trigger grip,
with the right hand forward. [rifle-attachment.json](rifle-attachment.json)
records the consumer-local translation and quaternion. The rifle's own accepted
authoring frame is unchanged.

The correction aligns the barrel horizontally from the left hand toward the
right hand at the first frame of the aim clip, with the rifle's sights upward.
It was reviewed from front, side and three-quarter views and across run and fire.
It is specific to this pose set; do not use it as a universal socket correction.
The generated mitt hands have no animated finger closure. Exact support-hand
contact during transitions remains a runtime IK polish task.

Rebuild the armed preview from unchanged inputs at repository root:

```sh
python3 designs/scrapyard/tools/attachment_preview.py \
  --body designs/scrapyard/batch-02/scavenger-combat.glb \
  --prop assets/models/scrapyard_rifle.glb \
  --config designs/scrapyard/batch-02/rifle-attachment.json \
  --out designs/scrapyard/batch-02/armed-scavenger-combat.glb
```

The adjacent attachment record hashes the inputs and output. This is a consumer
assembly, not a replacement body in Forge's library.

## Combat effects

[vfx/vfx.json](vfx/vfx.json) describes three original transparent 2×2 atlases:
muzzle flash (30 fps, 0.133 s), metal impact (20 fps, 0.20 s), and pickup burst
(12 fps, 0.333 s). Each plays once in row-major order and disappears after frame 4.

Use the recorded normalized pivot, straight-alpha blending and a camera-facing
quad. For the muzzle flash, rotate its rightward image axis along the projected
barrel direction and spawn it at the barrel tip on the fire event. Starting quad
widths are 0.55 m for muzzle flash, 0.45 m for impact, and 0.8 m for pickup burst;
these are authored starting values, not measured gameplay tuning.

The original alpha is preserved. Discard alpha below 4/255 in the material to
remove very faint generator speckles. The visible frame bounds were checked at
that threshold and do not touch cell boundaries. Disable mipmaps on these raw
atlases, or extrude gutters when packing them into an engine texture atlas.

Forge currently registers models, bodies, clips and audio. The VFX are delivered
as a separate texture pack with its own hash manifest; they are not silently
added as an unsupported kind to `assets/library.json`.

## Pickup designs

Three original references are retained here: repair (ivory/teal cross), overdrive
(yellow lightning cell) and magnet (blue horseshoe). Their shapes and top-facing
features distinguish them independently of color.

The accepted magnet is `assets/models/scrapyard_magnet.glb`: 5,801 triangles,
0.35 m longest extent, centered origin for a rotating pickup. The game can add
its own halo and bobbing transform.

Repair and overdrive remain reference designs only. The initial 3,000-vertex
lifts had gaps and faceting; repair still showed broken surfaces at the standard
6,000-vertex target with seed 7. Overdrive seed 7 failed in the backend before
export completed. No rejected pickup is in the library. Use clean runtime
geometry for these two pickup types instead of the draft meshes.

The TRELLIS environment also intermittently crashed during Python imports.
`PYTHONMALLOC=debug PYTHONFAULTHANDLER=1` allowed a repair run to finish, but it
is a diagnostic workaround, not an established fix: the overdrive retry later
failed with a Python type error during export. No backend packages were rebuilt
or safety gates relaxed for this batch.

## Review evidence and provenance

[Previews](previews/) contain the actual-body animation strips, including the
rifle attachment. [image-prompts.json](image-prompts.json) contains the exact
built-in image-generation prompts. Motion prompt text is retained in the take
records; the aim/rush/attack prompt set is also in [motion-prompts.tsv](motion-prompts.tsv).

All images were generated with OpenAI's built-in image_gen tool. Meshes use
TRELLIS.2, existing character rigs use SkinTokens, and motion takes use ARDY.
The texture baker's non-commercial notice remains in each lift record.
Bodies, meshes and raster images claim integrity; clips also claim exact rebuilds.

No commits were made. Runtime gameplay and performance evidence belongs in the
Bevy game's report; the checks here cover the asset packages themselves.


Run `python3 designs/scrapyard/tools/verify_batch.py` from the repository root to check the standalone package hashes, frame bounds and grounding inputs.

## Combat audio

Seven original short effects and one original music loop are accepted in the
Forge library. They were authored with analytic oscillators, seeded noise and
explicit envelopes, with no sampled or model-generated audio inputs. Retained
sources are `assets-src/audio/scrapyard_synth.py` and
`assets-src/audio/scrapyard_music.py`. Run either script from the repository root
and promote its outputs through Forge; never change the accepted WAV files by hand.

| Event | File under `assets/audio/` | Duration | Suggested initial gain |
| --- | --- | --- | --- |
| Rifle shot | `sfx/scrapyard_shot.wav` | 0.240 s | 0.30 |
| Armor hit | `sfx/scrapyard_hit.wav` | 0.220 s | 0.30 |
| Robot destruction | `sfx/scrapyard_explosion.wav` | 0.900 s | 0.40 |
| Pickup | `sfx/scrapyard_pickup.wav` | 0.420 s | 0.50 |
| Dash | `sfx/scrapyard_dash.wav` | 0.280 s | 0.40 |
| Upgrade chosen | `sfx/scrapyard_upgrade.wav` | 1.000 s | 0.60 |
| UI confirmation | `sfx/scrapyard_ui.wav` | 0.085 s | 0.40 |
| Combat music | `music/scrapyard_combat_loop.wav` | 32.000 s | 0.40 |

Gains are authored starting values; balance them in the game. Throttle armor-hit
voices and cap concurrent explosions. Effects are mono 48 kHz PCM; music is stereo
48 kHz PCM, 16 bars at 120 BPM in E minor. Repeat the music file directly: circular
event mixing retains note and delay tails through the loop boundary.

All eight audio files measure clean: no clipped runs, negligible DC, and zero-ms
lead silence. Effect peaks range from -2.5 to -9.1 dBFS; the music peaks at -6.6
dBFS. Its loop-boundary step is below 0.0001 full scale in either channel. The
waveforms/spectrograms were inspected; subjective listening has not been claimed.

[Audio source manifest](audio-source-manifest.json) and
[music source manifest](music-source-manifest.json) contain source/output hashes.
These are source manifests, not fabricated Forge generator records. Forge's
sidecars intentionally say unknown backend provenance because these authoring
scripts are outside its registered generation backends. Integrity hashes are
verified normally, and the original source is available for exact local rebuilds.

MOSS trials were withheld for delayed attacks, abrupt tails and excessive DC;
the 64-second ACE-Step trial faded to over four seconds of silence and was not
accepted as a looping track. Their untouched outputs remain under `out/`. The
prompts in [audio-prompts.txt](audio-prompts.txt) document the rejected trials,
not the accepted sounds.

Validation after promotion: Forge verify **83 checks passed**, manifest current,
12/12 clips rebuild byte-for-byte, and 4/4 bodies conform. The new WAVs also rebuild
from their source scripts to the accepted hashes.
