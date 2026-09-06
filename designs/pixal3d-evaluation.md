# Pixal3D evaluation — 2026-09-06

The user requested a local Pixal3D comparison after rejecting the procedural
models in Relay Run. This is an isolated experiment, not a replacement of the
supported Forge backend or a completed release stage.

The checkout and environment are under
`/home/mmy/forge-demos/pixal3d-trial-20260906`. Pixal3D source is pinned to
`f7cf38429b0bd264f1995f0f8743a88b1c728b94`; its single-view weights are pinned to
`b0cb2e1b794cab9aa0ac38a95d794a4d9337437f`. The model metadata and installation
logs live alongside the trial. The isolated venv reads existing TRELLIS CUDA
libraries and installs additions locally. It does not upgrade the supported
TRELLIS environment.

`export_ablation.py` runs one TRELLIS sample and exports it with projection-back
0.9 and 0.0, holding the reference, sample, 6,000-vertex target and 1,024-pixel
texture fixed. Pixal3D uses 0.0 in its inference script. Both outputs and an
experimental JSON receipt are retained under `results/`.

`run_pixal.py` adapts the pinned Pixal3D inference script for a like-budget trial:
original drone reference, Forge's flat-background keyer, original authorized
DINOv3 checkpoint, pinned local NAF source, 1,024 generation resolution,
6,000-vertex export target, 1,024 texture and PNG embedding. The executed retry uses MoGe-2 camera estimation; the earlier proposed
manual FOV of 0.2 radians was not used. The keyer replaces
RMBG; it does not alter the original reference. No derived mesh is hand repaired.
Experimental receipts identify these adaptations rather than posing as Forge
records. The existing non-commercial nvdiffrast texture-baker limitation remains.

Multi-view inference uses separate weights and camera-to-world transforms with
horizontal FOV. Uncalibrated independent reference drawings do not establish a
valid multi-view test. A successful single-view run will not be reported as
multi-view qualification.

The interrupted earlier demo work remains in progress: audio launcher and corner
HUD changes are in source, and the generated-model integration requires a reviewed
library before packaging. The last complete playable package remains v04. Earlier
release acceptance, rejected lifts and evidence are preserved.

## Host reboot during evaluation

The host rebooted on 2026-09-06 at 10:54:55 CEST. The preceding journal ends
with repeated system-memory pressure notices; no CUDA OOM, NVIDIA Xid, kernel
OOM kill or machine-check error was established. The cause remains unresolved.
NATTEN was building with four workers alongside the TRELLIS budget diagnostic.
That concurrency is retired for this trial. No Pixal3D inference has completed.

Evidence and precise incomplete/completed status are in
`/home/mmy/forge-demos/pixal3d-trial-20260906/crash-20260906/summary.json`.
The interrupted jobs were left stopped. Subsequent retries serialize builds and inference, limit
build workers to one, enforce cgroup RAM limits with no swap allowance, and
record host RAM and GPU memory. Preserve interrupted attempts rather than
overwriting their logs. The completed projection comparison still showed
fractured surfaces at both settings; it did not establish a fix.

## Bounded retry

The user authorized a retry after the reboot. A 64 MiB cgroup test was killed
with `oom-kill` when it tried to allocate 128 MiB, confirming enforcement.
Pixal3D runs alone under `MemoryMax=36G`, `MemorySwapMax=0`, and a 40-minute
runtime ceiling. A monitor stops it below 10 GiB available host RAM or above
21,000 MiB total GPU usage. Torch allocations are separately capped at 75%
of GPU memory. These are limits, not measured peaks.

The source build was avoided: NAF's checked-in attention layer explicitly
supports the legacy NATTEN API. The official NATTEN 0.17.5 wheel for Torch 2.6,
CUDA 12.4 and Python 3.11 installed in the isolated venv. Its pretrained NAF
smoke test returned finite 1×1024×32×32 features on the GPU. This differs
from Pixal3D's recommended NATTEN 0.21.0 and must remain in the trial record.
The full trial uses MoGe-2 camera estimation, superseding the initial proposed
manual FOV assumption. Logs and memory telemetry use the new `pixal-retry1`
name and do not replace pre-crash evidence.


## Completed guarded run and visual rejection

`pixal-retry1` stopped cleanly at the Torch allocation cap during NAF texture
conditioning: it requested another 4 GiB with 14.74 GiB allocated. Physical
GPU memory still had headroom. This was a cap-induced CUDA OOM, not another
host crash and not proof of the earlier reboot's cause.

A fused NATTEN adapter failed a strict numerical comparison and was not used
for inference. `attention_chunked.py` instead streams independent value-channel
blocks through the legacy attention operation. The pretrained NAF smoke test
matches the original at atol 2e-5 / rtol 2e-4. TF32 is disabled. This is an
experimental local adapter, not qualification of upstream NATTEN 0.21.0.
Exact retry scripts are archived under `results/pixal-retry2-adapter/`.

`pixal-retry2` completed seed 42 at resolution 1024, exported the GLB and exited
0 with every memory guard unchanged. Recorded inference time: 187.14 seconds;
whole monitored process: 195.63 seconds. Torch peak allocation: 13,930,545,152
bytes (12.97 GiB). Two-second samples observed total GPU usage up to 16,896 MiB
(16.5 GiB), and at least 26.09 GiB available host RAM. Cgroup usage reached its
36 GiB limit, including cache; this does not measure unconstrained RAM demand.
The machine remained up. One successful run does not establish crash resolution.

The four-view culling-off render rejects this output for the demo: the rear
shell is torn open and surfaces are fragmented. The front is more coherent
than the TRELLIS projection-zero comparison, but neither is acceptable.
Pixal3D's export rotation also reverses Forge's front/back convention.
A separate topology diagnostic hit its 2 GiB RAM cap; no connectivity numbers
are claimed. Visual evidence is sufficient for rejection.

Evidence: `results/pixal-retry2/{experiment.json,review.json,pixal.glb,views.png}`,
`pixal-retry2.log`, `pixal-retry2-memory.jsonl`, and `pixal-retry2-exit.json`.
The GLB SHA-256 is
`ad8304ae6a3a97b2e12893b5ffcbaf91c2fbcd9911290934bbe2ffa76f213706`.
No model was promoted, no production backend was replaced, and multi-view
remains untested. The next useful diagnostic is a same-sample export-budget
comparison; a 6,000 target may contribute to defects and has not been isolated.


## Same-sample geometry-budget comparison

The user authorized this diagnostic. `pixal-budget1` completed under the same
memory guards, with one generation and three sequential exports from its exact
pre-export tensors. The reference, seed, camera estimate, texture size (1024),
remesh projection (0), rotation and other export settings stayed fixed. The
pre-export sample is preserved as `results/pixal-budget1/export-input.pt` for
future diagnostics without another inference. Its hash is in `review.json`;
the exact scripts are in `results/pixal-budget1-adapter/`.

| Export target | Actual triangles | Actual exported vertices | Visual result |
| --- | ---: | ---: | --- |
| 6,000 | 5,869 | 7,797 | Rear fragmentation; collapsed barrels and round details |
| 60,000 | 57,514 | 57,872 | Coherent rear structure, side details and barrels |
| 240,000 | 229,639 | 202,284 | Minor further improvement at contact-sheet scale |

These are measured output counts; the upstream parameter's documented vertex
wording does not guarantee the exported vertex count after UV seams.
The 60,000 candidate was inspected from four angles with culling both off and
on. The 6,000 and 240,000 versions were inspected with culling off. Increasing
the budget substantially resolves the apparent destruction. The earlier
low-budget rejection remains valid for that file, but cannot reject the
underlying generated shape. This does not prove that Pixal3D beats TRELLIS at
a higher budget; that comparison has not been performed.

Select the 60,000 export as the next visual candidate. It still needs prop
normalization for orientation and scale, honest provenance integration,
in-game review and performance checks before shipping. The fixed 1024 texture
remains somewhat soft. No model was promoted or production backend replaced.
Multi-view remains untested.

Whole monitored process: 188.26 seconds, exit 0, no monitor stop. Torch peak:
13,930,545,152 bytes. Two-second samples observed total GPU usage up to
17,491 MiB and available host RAM no lower than 24.83 GiB. The host remained
stable. This is a second completed bounded run, not proof of reboot root cause.
Evidence and output hashes live in `results/pixal-budget1/experiment.json` and
`review.json`; sheets are `views-6000.png`, `views-60000.png`,
`views-240000.png` and `views-60000-cull-on.png`.


## Drone integrated into Relay Run v05

The selected 60k-target sample is now normalized and promoted in the external
showcase project through the standard doors, with reconstructed provenance and
an explicit experimental note. No unsupported lift record was invented.
Its final model has 57,282 triangles; original experiment evidence and rejected
attempts remain unchanged. The new v05 package bundles the source experiment
receipt, selected raw export, adapter, normalization record and licence notice.

Twenty firing drones at 1440x900 on desktop Vulkan RTX 4090 averaged 3.114 ms
per frame (p95 3.840 ms), measured after warm-up with uncapped presentation.
The temporary Xvfb display averaged 19.409 ms for the same fixture, so that
measurement must not be treated as desktop mesh performance. Normal gameplay,
corner ammo HUD and default audio routing were also checked. Details and
reproduction are in `demos/relay-runner/VERIFICATION.md`. Cargo and station
models are still pending; this is a drone integration, not completion of the
full asset showcase or qualification of multi-view generation.


## Cargo and station asset pass

Selected pixal-station1 (seed42, estimated camera) and pixal-cargo3 (seed42,
manual FOV0.2 trial assumption) at60k targets/2048 textures. Cargo1/2 remain
rejected for detached fragments; changing seed alone did not resolve them.
The manual-camera trial has a coherent shell in four views. All runs and
pre-export samples remain in the external trial; no failed evidence was replaced.

Floor props required an upstream pitch correction: the CLI prop normalizer now
accepts and records --pitch-deg, rotating world+X after yaw before scale/place.
The selected props use yaw180,pitch-20, floor placement. Station is3.2m tall,
cargo0.76m. Their reconstructed sidecars honestly omit unsupported generator
blocks while packaged experimental receipts retain actual Pixal3D provenance.
No supported generator was silently replaced, and multi-view remains untested.

Both are integrated in Relay Run v08 with model-bound collision depth, thin
approach markings and soft front fill. Full scope and validation are in
`demos/relay-runner/VERIFICATION.md`; v08 evidence retains final screenshots,
frame reports, package verification and per-run sampled memory. Earlier release
acceptance evidence is unchanged.

## Relay v09 alignment follow-up — 2026-09-06

No generation was rerun. The selected cargo3 and station1 exports were
normalized again with recorded final headings +27.5 and -27 degrees after
yaw180/pitch-20. New `*-pixal60k-aligned.prop.json` records and four-view sheets
are under the showcase project's `out/props`; earlier normalization and trial
evidence remain. Both were visually reviewed and promoted with overwrite.
The v09 package includes the aligned records, meshes, review sheets and
normalizer source. Cargo is now a cyan-marked supply pickup; only station
props block the runner. Desktop captures are under
`out/relay-runner-20260906/v09`.
