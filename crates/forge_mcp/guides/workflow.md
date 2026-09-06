# Local Asset Forge workflow

Guide contract: v1. Use tools/list for this build's exact parameter schemas.
The server is bound to one game directory and never switches libraries implicitly.

## Start

Call init_project with the bound directory shown in the server instructions.
Choose the asset kinds and GPU tier. Fake tier produces branded placeholders
for workflow tests, not production assets. Continue on this connection after
initializing the bound directory. Creating another directory requires a separate
server with --project for that game.

Read licences before setup. Accept required IDs only with user authorization.
Use doctor to diagnose missing environments or weights before generation.

## Produce and review

References are external original PNGs. Import with import_reference and truthful
source information. Bring one complete subject in three-quarter view on a flat
backdrop without cropping, a floor or cast shadow; the long side must be at least
1024 pixels. Inspect the image first; import refusals explain what must change.

| Asset | Tools in order |
|---|---|
| Prop | import_reference, generate_mesh, render_model, prepare_prop, render_model, promote_model |
| Humanoid | import_reference, generate_mesh, render_model, prepare_body, skin_body, export_body, render_model, promote_body |
| Clip | generate_clips, review sweep, promote_clip with a complete recipe, render_clip_strip on the real body, retain or regenerate based on review |
| Audio | generate_audio, inspect_audio and listening where available, promote_audio |

After every tool returning a job, call wait before using its outputs. A running
reply is not completion: wait on the same ID again. Use status, list_runs and
cancel to inspect or stop jobs. Do not duplicate a slow request. One GPU is shared.

Review meshes from all supplied views and clips on the actual body, including
foot contact and attachment alignment. Audio plots expose silence, clipping and
abrupt tails, but do not establish that the result matches its brief.
Promotion writes the library immediately and requires overwrite for an existing
name. Retain candidate sources and recipes, including rejected results. Never
repair derived files or provenance records by hand to pass a check.

## Validate and deliver

- verify {} checks integrity, reference provenance and profile drift.
- audit {} checks reproduction by bytes and posed animation, plus body conformance.
  audit {"fit": true} enables the CLI's optional body-fit checks.
- manifest_check {} compares the manifest to a fresh projection without rewriting it.

These read-only tools return structured check, passed, exit_code, code and report
fields. Failures also set the MCP error-result flag. Read the report and correct
sources or use the appropriate production door. Do not blindly retry invalid
inputs. check_timeout and check_unavailable identify execution failures.
Passing checks establishes only the properties named, not artistic quality.

Use export_bundle to deliver a self-contained body with named animations. Keep
its record and notices. Artifact paths are local. Images normally arrive inline;
oversized images return a path for a client with filesystem access. Retrieval of
oversized images without filesystem access remains a documented limitation.

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


### Keyframe-guided motion through the CLI

When a bounded idle audition stays nearly static, repeated guard wording is
not a verified remedy at the installed ARDY Core pin. The CLI also supports
`forge gen motion keys --base <take.npz> --keys <source.keys.json> --prompt
<activity> --out-dir <new-directory> --samples 4 --seed <seed>`.
Read `forge gen motion keys --help` for the JSON format and coordinates.
This path is not exposed by the current `generate_clips` MCP tool.

The keys file is an authored source, never a patch to a generated take.
Its constraint groups are `Hips`, `LeftHand`, `RightHand`, `LeftFoot` and
`RightFoot`. `Head` and arbitrary skeleton bones cannot be constraint groups.
Invalid groups, duplicate groups, nonfinite vectors and zero aim directions
are refused before loading the backend. Preserve the keys and base take with
the output records, then review and bake through the ordinary clip doors.
A low-activity warning alone cannot decide whether a subtle idle reads well;
judge the actual body, foot contact, visible movement and loop boundary.
