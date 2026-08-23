# Records

What a file in this repository is allowed to say about itself, and who
writes it. Two schemas with a clean boundary: **Python writes generator
records** beside everything a generator produces, **Rust writes library
sidecars** beside everything a promote ships. The manifest a game reads is a
projection of the sidecars and claims nothing of its own.

One rule under both: **`null` means unknown, and a default is never written
as if it were a measurement.** The first records this toolkit descends from
were whatever the generator dumped; thirty shipped clips said `seed 0,
duration 4.0` because those were the writer's argparse defaults, and every
one had been promoted out of a sweep with other values. A writer that cannot
say "I do not know" will lie instead, so every generator field is optional
and a missing value is written as `null`, not omitted.

## The generator record (`forge_record: 1`)

Written by `python/forge_gen/records.py` (stdlib, so five interpreters and
Blender's can all import it), read by
`crates/forge_library/src/generator_record.rs`. Key order is the Rust field
order and the free-form objects are written with sorted keys, so a record
re-saved by either side is the same bytes; a fixture test holds the two
writers to byte equality. One record per run, beside the output, named by
convention: `<name>.lift.json` beside the reference PNG, `<name>.rig.json`
beside the `.blend`, `<name>.export.json` beside the exported `.glb`,
`<name>.prop.json` beside the normalized prop, `<take>.take.json` beside the
`.npz`, `<stem>.json` beside a sound.

| Field | Type | What it says |
|---|---|---|
| `forge_record` | `1` | the schema; a reader refuses any other number before parsing a field |
| `kind` | `lift \| prop \| rig \| export \| take \| sfx \| music \| speech` | what kind of run |
| `tool` | string | `trellis2`, `blender`, `ardy`, `moss_sound_effect`, `ace_step`, `moss_tts` — the name the sidecar's generator block will carry |
| `created` | `YYYY-MM-DD` | the day the run finished |
| `created_by` | `human \| agent:<name> \| unknown` | who asked; `forge gen` writes `unknown` unless `--created-by` is among the command's flags |
| `backend` | `{name, commit, python, torch, model, model_revision}` | which backend ran, pinned; every key present, `null` where unread |
| `inputs[]` | `{role, path, sha256, source, prompt}` | what the run was handed: `image`, `mesh`, `blend`, `reference` by role, hashed as read; a prompt is an input with no path |
| `params` | object | every knob, stated, `null` where the generator was not told; a lift carries `texture_baker` |
| `outputs[]` | `{path, sha256, bytes}` | what it produced, hashed after the file was final |
| `measured` | object | what the run measured of its own output (vertices, triangles, the fit numbers, a frame count) |
| `fake` | bool | a `--fake` placeholder: the output passes the same validators and nothing else about it is true |
| `note` | string or `null` | anything the next reader should know |

Paths inside a record are relative to the project root when the file sits
under it, absolute otherwise — a record that said `../../tmp/x.glb` would be
relative to wherever its reader stood.

**The lift record lives beside the PNG**, as `<name>.lift.json`, because
the PNG is the durable input and the `out/lifts/<name>.glb` it produced is
scratch. `forge promote body` and `forge promote model` read it from there
(`--lift-record`), together with the rig or prop record and the export
record, and fold all of them into one sidecar. A lift record's `params`
always names the texture baker —
`"texture_baker": "nvdiffrast (NVIDIA Source Code License, non-commercial)"`
— because that is a licence fact travelling with the asset, not a detail.

## The library sidecar (`schema: 1`)

Written by `forge promote` (`crates/forge_library/src/promote.rs`), typed in
`crates/forge_library/src/schema/`, one `<name>.json` beside each shipped
file under `assets/`. The catalog is rebuilt by scanning them in
milliseconds and is never persisted, so it cannot go stale; the manifest is
projected from them by `forge manifest` and held to a rebuild by `forge
manifest --check`.

| Field | Type | What it says |
|---|---|---|
| `schema` | `1` | refuse-newer: a build meeting a higher number refuses before it can rewrite the record without the fields it did not understand |
| `kind` | `clip \| body \| model \| sfx \| music \| voice` | six kinds, not three: music, sfx and voice have different generators and different review questions, and a body records a rig-contract claim a model must never be able to make |
| `name` | string | the file stem beside it |
| `prompt` | string or `null` | the motion description, the sound, the spoken line; never invented, never an empty string standing in for unknown |
| `tags` | `[string]` | curation; survive a re-promote unless restated |
| `created`, `created_by` | date, `human \| agent:<name> \| unknown` | who shipped it, when |
| `provenance` | `recorded \| reconstructed \| unknown` | how much of the record to believe (below) |
| `rig` | profile name or `null` | what a body is skinned to, a clip baked against; `null` on models and sounds |
| `generator` | tagged by `tool` | `ardy {seed, duration_s, cfg, sample, sweep_take…}`; `trellis2 {resolution, seed, decimation_target_vertices, texture_size, remesh, texture_baker, image, image_sha256, lift_sha256, post {tool, script, version}}`; `moss_sound_effect`, `ace_step`, `moss_tts` with their own knobs — one struct per tool so a music record cannot claim an `arm_bend_deg` |
| `source` | `{path, sha256, skeleton}` | the durable input: the `.npz` take a clip is editable from, the `.blend` a body was exported from, the voice reference a line was cloned from — hashed, so drift is visible |
| `recipe` | `ClipRecipe` or `null` | clips only: trims, in-place and height modes, loop and blend, the four style knobs, retime, the clip's name inside the glb — every field written, identity values included |
| `measured` | `{frames, fps, duration_s, avg_speed_mps, root_motion, mesh {vertices, triangles, bones_skinned, lowest_y, bounds}}` | facts read from the built file, never asked of the generator |
| `events` | `[event]`, `[]` or `null` | `null` means nothing ever examined the clip; `[]` means something looked and found none — the two empties are different facts |
| `content_hash` | `sha256:…` | required: integrity is the one claim every kind can make |
| `note` | string or `null` | |

### Provenance

| Marker | Meaning |
|---|---|
| `recorded` | written from a generator record as the asset was made; the numbers are the ones used |
| `reconstructed` | rebuilt after the fact — a take promoted without its `.take.json`, a body whose lift record is gone; trust the recipe, distrust anything a writer could have defaulted |
| `unknown` | a file with a hash and nothing else |

A marker only ever moves down. A migration nulls what it cannot know and
never launders a guess up; a rebake carries the marker it found.

### Integrity versus reproduction, per kind

| Kind | Claims | Checked by |
|---|---|---|
| clip | **reproduction**: a pure function of take, recipe and rig profile | `forge audit` — every clip rebuilt from its record and compared by bytes, then bound to the fixture mannequin beside the shipped file and sampled at every keyed frame, each bone within 1 mm in world space and 1e-3 per quaternion component |
| body | **integrity** of the shipped `.glb` (`content_hash`), of the `.blend` it came from (`source.sha256`), the Blender version in the post step, plus the contract | `forge verify` (hashes, self-contained container, stature band, feet) and `forge audit` / `forge rig check` (the contract, the reference clip binding) |
| model | **integrity** of the `.glb` and of the `.blend` when one is named; bounds in metres | `forge verify` |
| sfx, music, voice | **integrity** of the file plus the seed, the model and the reference clip's hash where they exist — the file that was auditioned, not the file the prompt would make again | `forge verify`; `forge audio list` for silence and clipping |
| reference PNG | **integrity** (sha256 in the lift record) and a row in `assets-src/SOURCES.md` — never regeneration | `forge verify`: a PNG without a row fails |

Neither TRELLIS.2, Blender's glTF exporter, MOSS nor ACE-Step is
bit-reproducible, so `forge rebake` skips bodies and models by name and says
so, and a sound is never rebuilt. Do not promise the wider claim for the
narrower kind.

### Nothing is inherited

A baked clip receives its **whole** recipe, every knob stated, and nothing
in the bake reads the sweep or the clip it is about to replace. An earlier
baker resolved an unstated trim by inheriting it from the sidecar of the
clip being overwritten, and a new take silently shipped with the old clip's
cuts.

At the door, `forge promote clip` makes the resolution visible instead of
silent. A new name starts from the identity recipe. When the name already
exists, the shipped clip's recorded recipe is the starting point and the
flags stated on the command line land on top of it; the whole effective
recipe is printed before anything is baked:

```
walk already exists as clips/walk.glb — pass --overwrite if replacing it is the intent.
the recipe it would have baked with:
  trim          0.200s off the start, 0.000s off the end
  in_place      strip
  y_mode        off
  loop          yes, 0.200s blend
  exaggerate    1.000
  arm_bend      0.00 deg
  lean          0.00 deg
  shoulder_back 0.00 deg
```

With `--overwrite` the bake runs on that echoed recipe and the record it
replaced is printed beside the one that shipped, `was` against `now`.
Authored events are not carried — a different take under the same name is a
different motion — and the door says which ones it dropped. Tags survive,
because curation is not provenance; the prompt comes from the take's own
record, not from the clip replaced, for the same reason events do not.

### Schema numbers

A record format bumps when it must be able to **say something new**, never
to say less: an older build that drops a field it does not know unauthors
it on its next rewrite, which is why both readers refuse a newer number
before parsing. A consumer format (`library.json`) bumps so an old reader
says "you are behind" rather than "corrupt". Adding a socket, a tag or a
note does not bump anything; renaming a bone bumps the rig profile's
`version` and expects every clip to need rebaking.

## What `forge verify` checks

`crates/forge_library/src/verify.rs` — the engine-free half, the one that
also runs in CI and in the MCP server. In order of how much a failure would
mean:

1. every sidecar parses at schema 1 and its `content_hash` matches the file
   beside it; the sidecar's `name` is its stem;
2. every clip's take exists where `source.path` says and still hashes to
   `source.sha256`; the clip stands on the project's rig;
3. every body and model `.glb` is a self-contained container — one JSON
   chunk, one BIN chunk, no `uri` under `buffers` or `images` — and a body
   stands inside the contract's stature band with its feet within
   `foot_tolerance_m` of the ground; a named `.blend` still exists (drift is
   a warning, absence a failure);
4. every event names a usable thing at a time on the clip, every
   contacts-derived event speaks the footstep vocabulary, every authored
   time agrees with its take time through the recipe to 25 ms, and every
   audio link resolves to a shipped sound;
5. every reference PNG under `assets-src/refs/` has a row in `SOURCES.md`;
6. the rig profile has not drifted: `rig.glb` hashing differently from the
   contract's `sources` is a failure, `rig.blend` a warning, and the bones
   re-derived from `rig.glb` still match `contract.json` position for
   position.

Lines print as `FAIL <subject>  <detail>` or `WARN …` / `note …`; the exit
is 1 on any `FAIL`. `forge audit` runs this first, then the four clip claims
(footsteps re-derive, root tracks rebuild, bytes rebuild, poses rebuild) and
the body contract.
