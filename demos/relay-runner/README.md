# Relay Run

A third-person combat runner on a collapsing orbital causeway.
Forward travel is automatic. Strafe freely, aim through the crosshair,
and survive increasingly dense patrols and barriers.

Run `./PLAY.sh`, then click **Start run** or press Enter.

| Control | Action |
| --- | --- |
| A / D or left / right | Strafe |
| Mouse | Aim |
| Left mouse | Fire |
| Right mouse | Focus aim and slow your sprint |
| Space | Jump |
| Left Shift | Side dodge with brief damage immunity |
| Q | Shockwave, when charged |
| E / middle mouse | Fire an aimed plasma blast |
| R | Reload |
| Escape | Pause / resume and release the mouse |
| M | Toggle sound |
| Enter | Start, restart or resume |

Shields regenerate after 3.5 seconds without damage. Health does not.
Cyan-marked supply crates refill the magazine, restore shields and ability charge.
Tall station units block gunfire; move around them. Headshots deal extra damage.
Intensity rises every 12 seconds and reaches its cap after one minute. Run again to beat your score.
Best score is stored in `~/.local/share/relay-runner/best.json`
(or under `XDG_DATA_HOME`). Automated runs never change the saved score.

## Build and package

The demo has an independent Cargo workspace and uses the reviewed stage-1 delivery.
It does not alter Forge's accepted assets, records or release evidence.

```sh
cargo build --locked --manifest-path demos/relay-runner/Cargo.toml
python3 demos/relay-runner/package.py \
  --delivery /absolute/stage1/consumer-final \
  --binary demos/relay-runner/target/debug/relay-runner \
  --showcase /absolute/relay-assets-v01 \
  --pixal-trial /absolute/pixal3d-trial-20260906 \
  --output /absolute/new/relay-runner
```

A shared `CARGO_TARGET_DIR` can reuse the toolkit's compiled Bevy dependencies.
Packaging also copies the system's Lato fonts with their notice.
The output includes original asset manifests, provenance and notices;
its own `game-delivery.json` hashes the complete payload.

For a bounded rendered test:

```sh
./relay-runner --autoplay --quiet --frames 900 \
  --screenshot /absolute/combat.png --report /absolute/combat.json
```

`--scenario title`, `--scenario paused` and `--scenario dead` capture menu states.
Without `--frames`, the game runs interactively. Use `--assets` when running an
unpackaged development binary. The packaged executable locates its own assets.

This is an original prototype inspired by space-opera shooters, with a procedural
station and previously accepted Forge characters, rifle and audio. Mesh texture
baking carries the retained nvdiffrast non-commercial notice. See `CONTRACT.md`,
`SCRAPYARD-NOTICES.md`, source ledgers and original generator records for scope.
The run/aim/fire mask is consumer animation composition; terrain and support-hand
IK are not implemented. The drone, cargo and station use reviewed experimental Pixal3D exports normalized through Forge; see
`EXPERIMENTAL-ASSETS.md` for its provenance and remaining scope.


For the bounded twenty-drone stress scenario, use `--quiet
--scenario crowded --benchmark --frames 1200 --report /absolute/crowded.json`.
The report measures wall-clock frame intervals after 180 warm-up frames,
excluding screenshot readback. Simulation uses fixed steps, while measurement
uses the real clock. Benchmark mode requests uncapped presentation; these are
whole-frame intervals, not GPU timestamp measurements. The fixture holds twenty
firing drones in place and restores health so the workload lasts the entire run.

Use `--scenario assets --frames 240 --screenshot /absolute/assets.png` for a bounded view of the supply pickup and tall cover at gameplay distance.

Combat introduces weaving interceptors, committed sniper bursts and heavy
gunships alongside the original drones and rushers in the opening 15 seconds.
Snipers show a violet targeting line: strafe after it locks to evade the burst.
Heavies fire a three-lane spread and survive one plasma blast, leaving a rifle
finish. Waves arrive every 5.8 seconds initially, tightening to four seconds;
every fifth wave gives a 7.5-second supply recovery. Run speed rises from
8.8 to 13 metres per second over the first minute. Drones glow before firing. Precision hits
interrupt rusher acceleration; kills return 12 shockwave charge. Chain four
kills within six seconds of each other to trigger six seconds of Overdrive:
a refilled magazine, faster fire and no ammo consumption. Taking damage
breaks the chain. Supplies remain available between encounters.

The orbital accelerator, moving energy rings and banded gas giant are
procedural environment art; the reviewed generated models remain intact.
Use `--scenario burst --frames 18` for the repeatable four-kill effect fixture.

F6 toggles the cinematic post-processing pass for comparison. It adds a cool
color grade, gentle vignette and reactive bloom; shockwaves briefly distort
the lens, while damage and Overdrive add subtle color separation near the
edges. The center aiming ray and HUD remain clear. `--no-post` starts with
this pass disabled (the previous build's bloom remains). No film grain is applied. The later lens pass below adds motion blur.

Reload now plays a generated upper-body clip while the legs keep running.
Its 1.3-second playback follows the gameplay reload timer, including supply
and Overdrive cancellation. The motion uses recorded sparse hand constraints;
individual fingers and a detached magazine are not animated.

A designed announcer calls FREE FIRE when Overdrive activates, with music and
combat effects ducked underneath it. Extensions of an active Overdrive do
not repeat the announcement. The HUD says UNLIMITED AMMO / HOLD FIRE.

Plasma starts charged. Its projectile detonates on an enemy, station cover or
the floor, or after a short fuse. The seven-metre AOE deals 160 damage and
clears nearby hostile bolts. Every eliminated enemy drops a violet energy
shard worth 25 charge; get close to pull it in. Four shards refill one blast.
This charge is separate from Q: time, kills and supply crates do not refill
plasma directly. A blast killing two or more hostiles earns a MULTIKILL
banner, 75 bonus points per victim and a matching announcer call. If the same
blast activates Free Fire, the calls play in sequence.

The reload has a generated 1.3-second mechanical sound, cancelled when the
reload is cancelled. Plasma has its own generated detonation sound, a particle comet trail,
a fading shock front, camera kick and reactive lens pulse.
The accepted Free Fire recording is preserved. Both announcer lines use in-game
streaming DSP: a 45 Hz high-pass, +6 dB bass shelf at 180 Hz, darkened treble
above 2.6 kHz, and a damped stereo chamber with 18 ms predelay. Dry diction stays
centered; the tail spreads across the stereo field. Press F7 to compare with
the original dry recordings, even while a line is playing. No processed WAV is
shipped. The runtime DSP source is retained with the asset provenance.
Use `--scenario blast` for a repeatable projectile-to-four-kill fixture.

Combat VFX use Hanabi 0.19 GPU particle simulation: textured ion muzzle flashes,
rifle tracers, stretched sparks, hot embers, smoke, pickup motes, phase wakes
and continuous hostile/projectile trails. Explosion layers have independent
lifetimes, drag, gravity, HDR color curves and soft opacity masks. The masks
are procedural consumer artwork, defined in `src/vfx.rs`. World scrolling is
applied to particle positions, so detached particles stay with the world.
Effects pause with gameplay and clear on restart. A 240-emitter ceiling bounds
GPU allocations; reports include current/peak emitters and dropped requests.
The old opaque rings, effect spheres and box-debris bursts are removed.

The lighting pass uses a grazing warm key, a cool rim and a restrained fill,
with tighter shadow cascades. Rail fixtures illuminate the road, while gunfire
and detonations briefly light nearby surfaces. F9 compares the lighting with
the earlier key/fill setup; `--flat-light` starts in that comparison mode.

Bokeh depth of field holds a broad gameplay focus, then smoothly tracks targets
near the aiming ray during RMB focus. Foreground blur is capped at 2.5 pixels
normally and 5 pixels during focus. Motion-vector blur uses a short shutter
while running, rises briefly during a dodge, and drops to almost zero during
RMB focus. It stops on menus/pause. F6 / `--no-post` disables the lens effects
alongside the existing grade. HUD text remains crisp; transparent particles
retain their authored trails because they do not write motion vectors.
Use `--scenario focus` for the repeatable aimed-focus fixture.

For the accepted v14 checkpoint, asset locations and next gameplay pass, see
[HANDOFF.md](HANDOFF.md).

The v15 scenery pass uses generated radar and reactor installations on attached
service decks, asymmetric arrangements and open bays. Interceptor formations,
industrial freighters and capital cruisers cross overhead on separate flight
paths. Background traffic is ambience; it does not block gunfire or damage the
player. The small weaving enemy shares the interceptor hull; heavies use their
own generated gunship model. Original v14 assets and package remain intact.

Sky traffic uses separate material instances with local texture fill and no
causeway fog, so hull detail stays readable against space. Combat models keep
the original material response. Radar and reactor bays appear early in each
stretch, with varied empty bays and supporting machinery between them.

Combat feedback: floating damage numbers show rifle, plasma and shockwave damage.
Gold CRIT labels distinguish critical rifle hits. Numbers drift and fade over
0.85 seconds, freeze on pause and clear on restart. Enemy surfaces receive a
small texture-preserving warm fill, with compact amber locators above targets.
