# SCRAPLINE // LAST SHIFT

The scrapyard's machines have turned on the night shift.
Keep moving, collect their scrap, and build a weapon that can clear the yard.

Scrapline is a single-player, angled top-down arena shooter built in Bevy 0.19.
A shift runs for ten waves, ending with the Foreman and the remaining machines.

## Play

From the repository root:

```sh
cargo run -p scrapyard_arena --bin scrapline
```

Press **Enter** or click **Start shift**.
The game reads its character packages and audio directly from this checkout.
You can also run `just play-scrapline`.

`just package-scrapline` creates `out/scrapline-linux/` with a standalone
Linux executable, its runtime assets, a player guide, and file hashes.
Run `./PLAY.sh` from that folder; it can be moved outside the checkout.
An explicit `--asset-root PATH` overrides automatic asset discovery.

The workspace uses Rust 1.96 or newer.
Running the game requires a desktop window and a compatible graphics driver.
The UI scales with the window, with layouts designed for 1280×720 and 1920×1080.

| Control | Action |
| --- | --- |
| WASD or arrow keys | Move |
| Mouse | Aim |
| Hold left mouse button | Fire |
| Space | Dash in your movement direction, or aim direction while stationary |
| 1, 2, 3 or click a card | Choose an upgrade |
| Esc or P | Pause or resume |
| Enter | Start, resume, or retry after a finished shift |
| R | Restart from pause or the end screen |
| M | Toggle sound |

The pause menu also returns to the title screen and shows your installed upgrades.
Closing the window exits the game.

## Survive the shift

You have unlimited ammunition. Keep an escape route open and use your dash
when machines crowd you or a warning points across your path.
Rushers chase you, shooters cover lanes, bombers close the distance,
and tanks force you to move around them.

Destroying machines drops scrap that fills your XP bar.
Each level offers three upgrades, and clearing a wave grants another choice.
The game pauses while you choose, so there is time to read each effect.
Wave clears also repair some damage and collect the remaining XP scrap.

Fourteen upgrade types change damage, firing speed, projectiles, mobility,
pickup reach, critical hits, armor, shielding, and recovery.
You can invest in repeated ranks or combine effects into a different build:

| Build idea | Upgrades that work together |
| --- | --- |
| Clear dense crowds | Split chamber, Tungsten core, Volatile rounds |
| Stay ahead of the rush | Servo boots, Phase capacitor, Hair trigger |
| Recover between hits | Salvaged armor, Barrier cell, Repair nanites |

Overclock trades maximum health for damage and firing speed.
The rank and effect are shown before you install an upgrade;
check the pause screen to see the build you have assembled.

Glowing pickups provide repairs, shields, temporary Overdrive, and a scrap magnet.
Active Overdrive and magnet timers appear above the dash indicator.
Upgrades last for the current shift; a new shift starts with a fresh build.

Your best score, furthest completed-run wave, completed shifts, and sound preference
are saved to `$XDG_DATA_HOME/scrapline/record.json`,
or `~/.local/share/scrapline/record.json` when `XDG_DATA_HOME` is unset.
An unfinished shift is not saved for later resumption.

See [the verification report](VERIFICATION.md) for completed playthroughs, native-input checks, screenshots and package validation.

## Check the game

We keep the arena rules separate from rendering so that gameplay tests need no window.
The UI tests exercise actual Bevy entities and actions, including upgrade selection
and keeping the HUD tree intact while health changes.

```sh
cargo test -p scrapyard_arena --bin scrapline
cargo check -p scrapyard_arena
```

The game suite covers collision order, enemy warnings, input buffering, bounded
entity counts, upgrades, pause/reset, end states, audio voice limits, VFX playback,
and UI interactions. Five complete combat-bot runs exercise the ordinary game
rules without health injection and finish in approximately twelve minutes.

```sh
cargo test -p scrapyard_arena --bin scrapline bot_balance_run -- --nocapture
xvfb-run -a python3 crates/scrapyard_arena/tools/verify_runtime.py
```

The second command requires Xvfb and Python Xlib. It drives real keyboard and
mouse events against the game window, then checks the captured state.

For a repeatable rendered combat check, run this from the repository root:

```sh
mkdir -p out/scrapline
cargo run -p scrapyard_arena --bin scrapline -- \
  --autoplay --frames 1800 \
  --screenshot out/scrapline/combat.png \
  --report out/scrapline/combat.json
```

The automated player moves, aims, fires, dashes, and chooses upgrades.
With a frame limit, simulation advances at 60 fixed steps per second;
the capture writes a screenshot and a JSON state summary, then exits.
Autoplay and capture scenarios are silent and do not update your saved record.
Use `--scenario title` and a short frame limit to capture the title screen.
Other menu captures are `upgrade`, `paused`, `victory`, and `defeat`.
`showcase` and `boss` are explicitly labeled authored inspection scenes,
with temporary invulnerability for visual review. They are not normal-play
validation. Starting a new shift clears every inspection override.
Use `--zoom 6` for close asset inspection; ordinary play uses a height of 22 world units.

## Assets and source

The game uses the project's generated armed scavenger and rusher combat packages.
Additional machines, arena structures, projectiles, and pickup visuals are built
from runtime meshes to keep each silhouette readable in a crowd.

The [combat asset guide](../../designs/scrapyard/batch-02/README.md)
documents character animation, rifle attachment, grounding data, VFX, and audio.
The [first production report](../../designs/scrapyard/production-status.md)
records the original mesh and rig validation.
The [source ledger](../../assets-src/SOURCES.md) and adjacent asset sidecars
retain the generation and integrity records.

The generated character textures retain their baker's non-commercial notice.
Those asset terms remain separate from the Rust source license;
this checkout is not a cleared commercial distribution package.
