//! Scrapline: a complete scrapyard arena survival game.
mod audio;
mod presentation;
mod runtime;
mod sim;
mod ui;

use bevy::prelude::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "SCRAPLINE // LAST SHIFT\n\nWASD / arrows move; mouse aims; hold left click to fire; Space dashes.\nEsc pauses; 1 / 2 / 3 install upgrades.\n\nOptions: --asset-root PATH, --autoplay, --frames N, --scenario NAME,\n         --screenshot PATH, --report PATH, --zoom WORLD_HEIGHT\nCapture scenarios: title, combat, upgrade, paused, victory, defeat, showcase, boss"
        );
        return;
    }
    let compiled_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bundled_root = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .filter(|dir| {
            dir.join("assets/audio/music/scrapyard_combat_loop.wav")
                .exists()
        });
    let root = args
        .iter()
        .position(|arg| arg == "--asset-root")
        .and_then(|index| args.get(index + 1))
        .map_or_else(
            || bundled_root.unwrap_or(compiled_root),
            std::path::PathBuf::from,
        );
    let root = match root.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            eprintln!(
                "Could not resolve asset directory {}: {error}",
                root.display()
            );
            std::process::exit(1);
        }
    };
    if !root
        .join("designs/scrapyard/batch-02/armed-scavenger-combat.glb")
        .exists()
    {
        eprintln!(
            "Game assets were not found at {}. Run from the source project or use --asset-root PATH.",
            root.display()
        );
        std::process::exit(1);
    }
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.015, 0.023, 0.032)))
        .insert_resource(sim::Sim::new(0x5C_AA_91))
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: root.to_string_lossy().into_owned(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "SCRAPLINE // LAST SHIFT".into(),
                        resolution: (1280, 720).into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins((
            runtime::RuntimePlugin,
            audio::GameAudioPlugin,
            presentation::PresentationPlugin,
            ui::GameUiPlugin,
        ))
        .run();
}
