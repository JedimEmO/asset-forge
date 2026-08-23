# forge_library

What an [asset-forge](https://github.com/JedimEmO/asset-forge) library knows
about itself: the sidecar beside every shipped file, the catalog scanned from
them, the project (`forge.toml`), the four doors that write the library,
the manifest projection a game reads, and the checks. No engine dependency:
the studio, the MCP server and the audit all ask this crate the same
questions and get one answer.

```rust,no_run
use std::path::Path;
use forge_library::{Catalog, Kind, Project, Query};

let project = Project::discover(Path::new("."))?;       // walks up to forge.toml
let catalog = Catalog::scan(&project);                    // milliseconds; never persisted
for clip in catalog.find(&Query::of_kind(Kind::Clip)) {
    println!("{}  {:?}", clip.name, clip.prompt());
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## What is here

| Module | What it does |
|---|---|
| `schema` | sidecar schema 1: `Sidecar`, `Kind` (clip, body, model, sfx, music, voice), `Provenance`, `Generator` tagged by tool, `ClipRecipe` with every knob written, `PartialRecipe` + `overlay_recipe`, events |
| `generator_record` | the `forge_record: 1` files Python writes beside every generator output, read and projected into the sidecar's generator block |
| `project` | `Project::discover` / `Project::init`; the kind → directory convention; the rig profile named in `forge.toml` |
| `catalog` | `Catalog::scan` — every sidecar under `assets/`, with the derived numbers a browser row shows |
| `promote` | the four doors — `promote_clip` (native bake through `forge_motion`), `promote_body`, `promote_model`, `promote_audio` — payload first, record last, both atomic; an existing name refused unless `overwrite` |
| `manifest` | project the catalog into `assets/library.json` (`forge_manifest` types, byte-deterministic), and hold the committed file to a rebuild |
| `verify` | the engine-free checks: hashes, self-contained `.glb`s, stature and feet, events, the reference ledger, rig-profile drift |
| `audit` | the clip claims that need no engine: footsteps re-derive, root tracks rebuild, every clip rebuilds byte for byte |
| `rebake`, `migrate` | every clip from its own record; every sidecar to the current schema, nulling what was only ever a default |
| `backends`, `metrics_cache`, `hash`, `report`, `clock` | backend discovery for doctor, cached audio measurements, `sha256:`, the `FAIL` / `WARN` / `note` report, an injectable clock for tests |

## The three invariants

- **Sidecars are the source of truth.** The catalog is derived by scanning
  and never written back, so it cannot go stale.
- **Every flag is explicit.** A bake is a pure function of the caller's
  whole `ClipRecipe`, identity values included; nothing is inherited from
  the clip being replaced. `Promoted::replaced` hands the old record back
  so a door can echo it.
- **Payload first, record last.** The file is written before its sidecar
  and both land by atomic rename, so a scan never sees half an asset.

And the rule under the schema: **`null` means unknown, and a default is
never written as if it were a measurement.** `designs/records.md` in the
toolkit has every field.

## Tests

`cargo test -p forge_library` runs in seconds and links no engine: a
temporary project built from the toolkit's rig profile, the fixture
mannequin, fixture takes, and a byte-equality test between a record written
by `python/forge_gen/records.py` and one re-emitted here.
