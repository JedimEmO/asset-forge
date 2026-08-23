# forge_manifest

The consumer contract of an [asset-forge](https://github.com/JedimEmO/asset-forge)
library: the typed, versioned manifest a game reads at `assets/library.json`.

- `Manifest` — the rig (profile, bone table, sockets, the exact `.glb` hash),
  bodies (rigged characters every clip plays on), models (props, with their
  bounds), clips (duration, loop flag, root-motion mode + pre-strip travel
  track, gameplay events) and audio, all denormalized from the library's
  sidecars so a game never parses one. Every file entry carries its `sha256`.
- `Manifest::from_slice` refuses a manifest newer than this build understands,
  naming both schema numbers; `Manifest::to_vec_pretty` is byte-deterministic
  so "regenerate and compare" (`forge manifest --check`) is a valid CI gate.
- `AudioLink` — the `kind:name` form an event uses to point at a shipped sound,
  with a parser and a resolver against the manifest's audio list.

```rust,no_run
use forge_manifest::Manifest;

let manifest = Manifest::from_slice(&std::fs::read("assets/library.json")?)?;
let walk = manifest.clip("walk").expect("the library ships a walk");
println!("{} — {:?}s, loop {}, root motion {:?}", walk.path, walk.duration_s, walk.looped, walk.root_motion.mode);
for socket in &manifest.rig.sockets {
    println!("{} rides {} at {:?}", socket.name, socket.bone, socket.translation);
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

The file is written by `forge manifest` in the toolkit and rebuilt from the
sidecars, so a game never reads a sidecar; `forge manifest --check` fails
when the committed file no longer matches the library.

Engine-free on purpose (serde + serde_json only). The manifest is equally
readable from any engine or build tool; nothing in it names Bevy, a level, or
a game.

Null means unknown throughout: a field the library never measured is `null`,
never a default pretending to be a measurement.
