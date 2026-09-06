# External consumer contract

A character GLB carries its skin and named animations. A game also needs events,
props, sound files and the notices explaining where those assets came from.
We deliver those together without changing the accepted library.

`release/consumer/stage.py` is a small supported delivery recipe for the reviewed
rusher and armed scavenger fixtures. The recipe ships in the staged installation.
Its asset selection is deliberately specific; its coordinate and ownership rules
are engine-neutral. The Bevy fixture has its own Cargo workspace and no Forge dependency.

## Reproduce the delivery

From a toolkit checkout, build the CLI and stage a development installation:

```sh
cargo build --locked -p forge
python3 release/package.py --binary target/debug/forge --output /absolute/new/install
python3 /absolute/new/install/release/consumer/stage.py \
  --install /absolute/new/install \
  --acceptance /absolute/acceptance-project \
  --scavenger /absolute/asset-forge-checkout \
  --out /absolute/new/consumer
```

Both output directories must be new. `--acceptance` is the preserved external
acceptance project described in `release-handoff.md`; verify the archive hash
before restoring it if the live project is gone. Nothing here calls a generator.
The packager can use a release binary when installation qualification reaches stage 4.

Run these from an unrelated directory, such as your home directory:

```sh
python3 /absolute/new/consumer/verify.py /absolute/new/consumer
cargo build --locked --manifest-path /absolute/new/consumer/Cargo.toml
/absolute/new/consumer/target/debug/forge-consumer-fixture /absolute/new/consumer
```

`CARGO_TARGET_DIR` can share an existing build cache; use that directory's binary
when set. The executable takes an absolute delivery directory, so it does not
find assets through its working directory or the toolkit checkout.
Python staging needs NumPy and SciPy; the consumer itself only needs its delivered files.

For software-rendered review on this Linux machine:

```sh
python3 /absolute/new/consumer/run_review.py \
  --binary /absolute/new/consumer/target/debug/forge-consumer-fixture \
  --delivery /absolute/new/consumer --display :187 \
  --icd /usr/share/vulkan/icd.d/lvp_icd.json
```

Choose an unused display. The wrapper records its own Xvfb process and stops it
in `finally`, unsets Wayland, and launches the consumer from the home directory.
This uses no generator and leaves the real adapter available.

The executable stops after its checks and four captures. Keep `runtime.json`,
`review-6.png`, `review-12.png`, `review-30.png`, `review-90.png` and the launch log together.
It refuses a prior `runtime.json`; stage a new delivery for another evidence run.
A render still needs visual review. Its exit status cannot judge asset quality.

## Files and provenance

The two original `assets/library.json` manifests live under separate namespaces.
Their complete libraries accompany them, so every manifest path still resolves.
`rusher.glb` and `scavenger.glb` are fresh `forge bundle` exports, each with its
unchanged writer-produced bundle record. Bind the animation names in that record,
then join events to the original manifest through the recorded library clip name.

`bundle` does not export a game library. This recipe separately copies prop and
audio files with the manifests and original sidecars. We retain `assets-src`,
source ledgers, references and authored recipes under `provenance/<namespace>`.
The receipt hashes every delivered input. Runtime code reads manifests;
sidecars and sources travel for inspection, not as a second runtime schema.

Code notices, asset notices, source ledgers and generator fields retain their
original scope. In particular, the nvdiffrast non-commercial notice stays with
the meshes. Copying an asset into a game directory does not change its provenance
or broaden its integrity claim into reproduction. This is local test delivery,
not a new grant over the sample assets.

## Coordinates and ownership

| Quantity | Contract |
| --- | --- |
| Length and time | glTF metres and seconds; transforms use right-handed axes, +Y up |
| Character heading | Baked motion faces −Z; the profile rest pose faces +Z. Do not add a compensating half turn to the hierarchy |
| Rotation | Quaternion arrays are `[x, y, z, w]`; compose parent × child |
| Model pivot | Preserve the authored origin and normalized GLB transforms. Floor props use their measured minimum Y; centered pickups require a consumer placement offset |
| Grip frame | Socket prop convention is grip origin, long axis +Y, front −Z; the rifle correction is separately authored |
| Socket | Bone world transform × manifest socket local transform × authored attachment transform × prop node transform |
| Bundle motion scale | Forge has already multiplied the Hips translation curve by the body's scale; do not scale the character node or the curve again |
| Controller speed | Pre-strip manifest speed × body `motion_scale` × playback speed; apply controller heading to local −Z travel |
| Root travel | `strip` delegates XZ displacement to the controller; `detrend` retains within-cycle surges; `off` retains travel and must not receive duplicate controller displacement |
| Grounding | Apply the sampled positive lift to the visual root after animation, leaving the controller and collision position unchanged |
| Events | Manifest seconds on the clip clock; loops repeat events, one-shots stop. A null audio link leaves the sound choice to the game |

The fixture advances each named animation independently. The run controllers move
for half a second at their measured speed, then hold for the review captures.
The manifest's loop flag controls repetition. Native Bevy animation events verify
that the event times reach the runtime, including the rusher's time-zero footstep.
Audio files load through Bevy's decoder; this is delivery evidence, not listening acceptance.

Both main fixture bodies have scale 1.0. A separate unchanged `scrapyard_rusher`
export exercises its non-identity scale numerically against the source channels.
The verifier checks that only the Hips translation changes, exactly once.

## Grounding and attachments

`grounding.py` generalizes the existing batch measurement by accepting a staged
installation and measuring each animation inside its delivered bundle.
This matters for fitted bodies: raw clip translations have not received bundle scale.
It samples baked keys and 60 Hz, selecting vertices with at least 0.5 combined
foot/toe weight. The correction is `max(0, -minimum_foot_y)` in bundle space.

Interpolate adjacent offsets using the animation's seek time. This fixture plays
one clip per body instance; transition grounding and terrain IK remain consumer work.
A weighted blend of separate offsets is only an approximation for blended skeletal
poses and needs its own contact review. The track does not pull airborne feet down,
and sampled contact does not prove continuous contact between samples.
The runtime independently skins foot vertices using Bevy joint world transforms
and inverse bind matrices, checking flat-floor penetration within 2 mm.

The scavenger uses `hand_l` and the unchanged batch-02 rifle correction.
The consumer attaches the separate rifle at runtime and checks its world transform
against the animated hand throughout the run. That quaternion belongs to these
left-handed aim/run/fire poses. It is not a universal socket correction.
The rusher's accepted lowered-hand guard has no established rifle pose.
Support-hand IK and finger closure remain outside this fixture.

## External atlas input

VFX remains an external texture pack, outside Forge's library schema.
The existing `scrapyard-vfx-1` descriptor is the concrete fixture for this contract;
no raster generation is added. A delivery must state the following:

| Field | Meaning |
| --- | --- |
| `path`, `sha256`, `size_px` | Original image bytes, integrity and pixel dimensions |
| `grid`, `frames`, `order` | Equal rectangular cells; positive columns/rows; frame count within capacity; row-major from top left |
| `fps`, `loop` | Positive frames per second; frame `floor(t * fps)`; one-shots disappear at `frames / fps`, loops wrap |
| `pivot_normalized_top_left` | Per-cell origin in [0,1]², X right and Y down. Quad coordinates subtract the pivot, then invert image Y into local up |
| `alpha`, `alpha_discard_below` | Straight alpha and an explicit material discard threshold; preserve original bytes |
| `filter` | Linear sampling with clamped cell UVs; disable mipmaps unless packing supplies extruded gutters |
| `billboard`, orientation | Explicit camera-facing choice; muzzle image +X follows projected barrel direction |
| source and notices | Origin statement, source ledger or prompt record, plus applicable notices |

The delivered batch uses four frames per 2×2 image. Its threshold of 4/255 and
its authored pivots belong to those images. They are not defaults for arbitrary art.
The verifier checks the descriptor and hashes; VFX playback is not a stage-1
Bevy rendering claim. A game adds effects on its own event and spatial logic.
