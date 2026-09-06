# Experimental generated props

The drone comes from the isolated Pixal3D `pixal-budget1` experiment, using the
60,000 export target. The original generated reference, reference ledger and
integrity records are under provenance/showcase/assets-src. Pixal3D's pinned
source and model revisions, adapter changes and sample settings are recorded
in provenance/pixal3d/experiment.json. The selected raw export and normalization
record are included there; no hand repair was performed.

Forge normalized it to 1.5 m longest extent, centered origin and front +Z with
a 180-degree yaw. The resulting library mesh has 57,282 triangles. Its sidecar
honestly says reconstructed with no generator block: Pixal3D is experimental
and does not have a supported Forge lift-record schema. This does not claim
TRELLIS generation or reproducibility of the delivered model.

Pixal3D code and weights use MIT licensing; its code notice is included.
Texture baking used nvdiffrast 0.4.0, whose non-commercial restriction remains.
Generation used the recorded NAF attention-streaming adapter and existing DINOv3
and MoGe-2 components; these models and generator runtimes are not distributed
in this game. Full local experiment evidence remains in
/home/mmy/forge-demos/pixal3d-trial-20260906.

The cargo and station are also reviewed Pixal3D assets, with 2048 textures and
60,000 geometry targets. Cargo uses pixal-cargo3 (seed42, manual FOV0.2 radians,
an explicit trial assumption); station uses pixal-station1 (seed42, MoGe camera
estimate). Both use yaw180 and pitch-20 degrees through the prop normalizer,
then a final up-axis heading of +27.5 degrees for cargo and -27 degrees for
station, before floor placement. These are visually selected alignment settings,
not measured authored poses. Cargo is 0.76m tall (59,753 triangles); station is 3.2m
(56,362 triangles), scaled uniformly to 2.3m when used as a tall obstacle.
Cargo is rendered at 65% scale as an ammo, shield and ability-charge pickup,
with a cyan ground ring and plus marker. It no longer blocks movement or shots.
Their full selected records and source exports are in the cargo/station
subdirectories of provenance/pixal3d. The two rejected cargo attempts remain
in the local trial archive. All three sidecars make integrity claims only.

The player, rusher and rifle are earlier reviewed Forge assets. Road panels,
structural beams, lights, warning markings and sky remain procedural.


The game applies a visually tuned -25 degree local X rotation at the drone's
visual mount to level the source model's nose-down pose. This is consumer
placement in `art.rs`, not a measured source orientation or a mesh repair.
Drone hit feedback uses a small roll; the rusher retains its separate lean.

## v15 asset/gameplay pass

Six new references were created with the built-in OpenAI image generator and
imported, unchanged, through Forge's reference door into the external showcase:
`relay_interceptor`, `relay_freighter`, `relay_cruiser`, `relay_radar`,
`relay_reactor`, and `relay_heavy`. Their original PNGs, integrity records and
source ledger accompany delivery. The cruiser reference was reframed after the
first image failed the two-percent margin gate; the refused image remains in
local image-generation history.

The same isolated Pixal3D adapter and guarded serial workflow are used. Each
candidate is judged from four raw views with culling off and on, then normalized
through Forge and reviewed again. Runtime ships point +Z with Y up; landscape
machinery sits on a floor origin. No derived mesh is hand repaired. The existing
experimental provenance and nvdiffrast non-commercial scope continue to apply.

The package retains `provenance/performance-assets/v15-assets/`: experimental
receipts, adapter hashes/source, memory and exit logs, raw exports, normalization
records and review sheets. The full pre-export tensors remain in the local trial
for future upstream export diagnostics. The scenery and sniper telegraph modules
are retained alongside the earlier runtime effect sources.
