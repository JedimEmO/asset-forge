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

Engine-free on purpose (serde + serde_json only). The manifest is equally
readable from any engine or build tool; nothing in it names Bevy, a level, or
a game.

Null means unknown throughout: a field the library never measured is `null`,
never a default pretending to be a measurement.
