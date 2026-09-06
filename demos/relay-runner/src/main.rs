mod art;
mod audio;
mod lighting;
mod post;
mod sim;
mod ui;
mod vfx;
mod voice_fx;
use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
    time::TimeUpdateStrategy,
    window::{CursorGrabMode, CursorOptions, PresentMode},
};
use sim::{Game, Input, Phase};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Resource)]
pub struct Options {
    pub root: PathBuf,
    pub autoplay: bool,
    pub frames: u32,
    pub capture: Option<PathBuf>,
    pub report: Option<PathBuf>,
    pub scenario: String,
    pub quiet: bool,
    pub benchmark: bool,
    pub post: bool,
    pub flat_light: bool,
}
#[derive(Resource, Default)]
struct Runtime {
    yaw: f32,
    pitch: f32,
    warm: u32,
    frames: u32,
    captured: bool,
    requested: bool,
    focus_seen: bool,
    save_done: bool,
    last_frame: Option<Instant>,
    frame_ms: Vec<f64>,
}
#[derive(Resource, Default)]
pub struct Controls(pub Input);
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum Set {
    Input,
    Sim,
    Present,
    Ui,
}
fn saved_path() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        })
        .join("relay-runner/best.json")
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value = |name: &str| args.windows(2).find(|p| p[0] == name).map(|p| p[1].clone());
    if args.iter().any(|a| a == "--help") {
        println!(
            "Relay Run\nA/D strafe | mouse aim | LMB fire | RMB focus | Space jump | Shift dodge | Q shockwave | E / MMB plasma blast | R reload | Esc pause | M mute\n--assets DIR --autoplay --frames N --screenshot FILE --report FILE --scenario title|combat|paused|dead|crowded|assets|pickup|burst|reload|blast --quiet --benchmark --no-post | F6 toggle post effects"
        );
        return;
    }
    let root = value("--assets")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .join("assets")
        })
        .canonicalize()
        .expect("assets directory; run the packaged PLAY.sh or pass --assets");
    assert!(
        root.join("scavenger.glb").exists() && root.join("rusher.glb").exists(),
        "Expected stage-1 delivered character bundles"
    );
    let options = Options {
        root: root.clone(),
        autoplay: args.iter().any(|a| a == "--autoplay"),
        frames: value("--frames").and_then(|v| v.parse().ok()).unwrap_or(0),
        capture: value("--screenshot").map(PathBuf::from),
        report: value("--report").map(PathBuf::from),
        scenario: value("--scenario").unwrap_or_else(|| "combat".into()),
        quiet: args.iter().any(|a| a == "--quiet"),
        benchmark: args.iter().any(|a| a == "--benchmark"),
        post: !args.iter().any(|a| a == "--no-post"),
        flat_light: args.iter().any(|a| a == "--flat-light"),
    };
    let capture_run = options.frames > 0;
    assert!(
        !options.benchmark || capture_run,
        "--benchmark requires bounded --frames"
    );
    let benchmark = options.benchmark;
    let mut game = Game::new();
    if let Ok(bytes) = std::fs::read(saved_path())
        && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes)
    {
        game.best = v["best"].as_u64().unwrap_or(0) as u32;
    }
    game.sound = !options.quiet;
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: root.to_string_lossy().into_owned(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "RELAY RUN".into(),
                    resolution: (1440, 900).into(),
                    present_mode: if benchmark {
                        PresentMode::AutoNoVsync
                    } else {
                        PresentMode::AutoVsync
                    },
                    ..default()
                }),
                ..default()
            })
            .set(bevy::render::RenderPlugin {
                synchronous_pipeline_compilation: capture_run,
                ..default()
            }),
    );
    if capture_run {
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1. / 60.,
        )));
    }
    app.insert_resource(options)
        .insert_resource(game)
        .init_resource::<Runtime>()
        .init_resource::<Controls>()
        .insert_resource(ClearColor(Color::srgb(0.016, 0.025, 0.063)))
        .configure_sets(
            Update,
            (Set::Input, Set::Sim, Set::Present, Set::Ui).chain(),
        )
        .add_plugins((
            art::ArtPlugin,
            post::PostPlugin,
            lighting::LightingPlugin,
            ui::UiPlugin,
            audio::AudioPlugin,
            vfx::VfxPlugin,
        ))
        .add_systems(Update, inputs.in_set(Set::Input))
        .add_systems(Update, simulate.in_set(Set::Sim))
        .add_systems(Last, harness)
        .run();
}
fn inputs(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut cursors: Query<&mut CursorOptions>,
    windows: Query<&Window>,
    mut game: ResMut<Game>,
    mut controls: ResMut<Controls>,
    mut runtime: ResMut<Runtime>,
    options: Res<Options>,
    ready: Res<art::Ready>,
) {
    // Bounded evidence must not inherit mouse/keyboard activity from the desktop.
    if options.frames > 0 {
        controls.0 = if options.scenario == "focus" && game.phase == Phase::Playing {
            Input {
                focus: true,
                aim: (Vec3::new(0., 1.25, -24.) - game.camera()).normalize(),
                ..default()
            }
        } else if options.autoplay && game.phase == Phase::Playing {
            scripted_controls(&game)
        } else {
            Input {
                aim: Vec3::NEG_Z,
                ..default()
            }
        };
        return;
    }
    if keys.just_pressed(KeyCode::F8) && options.report.is_some() {
        println!(
            "INPUT_SNAPSHOT {}",
            serde_json::json!({"phase":format!("{:?}",game.phase),"x":game.x,"y":game.y,"distance":game.distance,"charge":game.charge,"ammo":game.ammo,"energy":game.energy,"energy_collected":game.energy_collected,"blasts_fired":game.blasts_fired,"detonations":game.detonations,"multikills":game.multikills,"multikill_size":game.multikill_size,"shots":game.shots,"aim":game.aim.to_array(),"dash_cd":game.dash_cd})
        );
    }
    if game.phase == Phase::Playing && game.time == 0. && !options.autoplay {
        runtime.yaw = 0.;
        runtime.pitch = -0.04;
    }
    if keys.just_pressed(KeyCode::KeyM) {
        game.sound = !game.sound;
    }
    if keys.just_pressed(KeyCode::Escape) {
        game.phase = match game.phase {
            Phase::Playing => Phase::Paused,
            Phase::Paused => Phase::Playing,
            p => p,
        };
    }
    if keys.just_pressed(KeyCode::Enter) && ready.0 {
        match game.phase {
            Phase::Title | Phase::Dead => {
                game.start();
                runtime.yaw = 0.;
                runtime.pitch = -0.04;
            }
            Phase::Paused => game.phase = Phase::Playing,
            _ => {}
        }
    }
    if !options.autoplay
        && options.frames == 0
        && let Ok(window) = windows.single()
    {
        if runtime.focus_seen && !window.focused && game.phase == Phase::Playing {
            game.phase = Phase::Paused;
        }
        runtime.focus_seen = window.focused;
    }
    for mut cursor in &mut cursors {
        let grab = game.phase == Phase::Playing && !options.autoplay && options.frames == 0;
        cursor.visible = !grab;
        cursor.grab_mode = if grab {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
    }
    if game.phase != Phase::Playing {
        controls.0 = Input::default();
        return;
    }
    runtime.yaw = (runtime.yaw + motion.delta.x * 0.0021).clamp(-0.72, 0.72);
    runtime.pitch = (runtime.pitch - motion.delta.y * 0.0019).clamp(-0.38, 0.32);
    let aim = Vec3::new(
        runtime.yaw.sin() * runtime.pitch.cos(),
        runtime.pitch.sin(),
        -runtime.yaw.cos() * runtime.pitch.cos(),
    );
    controls.0 = Input {
        strafe: if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
            1.
        } else {
            0.
        } - if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
            1.
        } else {
            0.
        },
        fire: mouse.pressed(MouseButton::Left),
        focus: mouse.pressed(MouseButton::Right),
        jump: keys.just_pressed(KeyCode::Space),
        dash: keys.just_pressed(KeyCode::ShiftLeft),
        nova: keys.just_pressed(KeyCode::KeyQ),
        secondary: keys.just_pressed(KeyCode::KeyE) || mouse.just_pressed(MouseButton::Middle),
        reload: keys.just_pressed(KeyCode::KeyR),
        aim,
    };
    if options.autoplay {
        controls.0 = scripted_controls(&game);
    }
}
fn scripted_controls(game: &Game) -> Input {
    let target = game
        .enemies
        .iter()
        .filter(|e| e.pos.z < -3.)
        .min_by(|a, b| b.pos.z.total_cmp(&a.pos.z))
        .map(|e| e.pos + Vec3::Y * 1.25);
    Input {
        aim: target
            .map(|p| (p - game.camera()).normalize())
            .unwrap_or(Vec3::NEG_Z),
        fire: target.is_some(),
        strafe: (game.time * 0.55).sin() * 0.35,
        jump: false,
        secondary: game.energy >= 100.
            && game.enemies.iter().filter(|e| e.pos.z > -28.).count() >= 3,
        nova: game.enemies.iter().filter(|e| e.pos.z > -17.).count() >= 3,
        ..default()
    }
}

fn simulate(
    time: Res<Time>,
    options: Res<Options>,
    ready: Res<art::Ready>,
    controls: Res<Controls>,
    mut game: ResMut<Game>,
    mut runtime: ResMut<Runtime>,
) {
    if !ready.0 {
        return;
    }
    runtime.warm += 1;
    if options.frames > 0 && runtime.warm == 45 {
        match options.scenario.as_str() {
            "title" => {}
            "burst" => game.burst_fixture(),
            "blast" => game.blast_fixture(),
            "reload" => {
                game.start();
                game.enemies.clear();
                game.ammo = 3;
                game.reload = 1.3;
            }
            "pickup" => {
                game.start();
                game.enemies.clear();
                game.shield = 40.;
                game.charge = 10.;
                game.ammo = 0;
                game.reload = 1.;
                game.pickups = vec![sim::Pickup {
                    id: 20000,
                    pos: Vec3::new(0., 0.8, -0.2),
                }];
            }
            "paused" => {
                game.start();
                game.phase = Phase::Paused;
            }
            "dead" => {
                game.start();
                game.phase = Phase::Dead;
                game.distance = 1248.;
                game.kills = 36;
                game.score = 4900;
            }
            _ => game.start(),
        }
    }
    if runtime.warm < 45 && options.frames > 0 {
        return;
    }
    if options.frames > 0 && options.scenario == "crowded" {
        game.crowded_fixture();
    }
    if options.frames > 0 && matches!(options.scenario.as_str(), "assets" | "focus") {
        game.asset_fixture();
    }
    game.tick(time.delta_secs().min(1. / 30.), controls.0);
    if game.phase == Phase::Dead && !runtime.save_done && options.frames == 0 {
        let path = saved_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, format!("{{\"best\":{}}}", game.best));
        runtime.save_done = true;
    }
    if game.phase == Phase::Playing {
        runtime.save_done = false;
    }
}
fn harness(
    mut commands: Commands,
    options: Res<Options>,
    game: Res<Game>,
    particles: Res<vfx::VfxStats>,
    mut runtime: ResMut<Runtime>,
    ready: Res<art::Ready>,
    mut exit: MessageWriter<AppExit>,
    mut total: Local<u32>,
    players: Query<&AnimationPlayer>,
    announcer: Res<audio::AnnouncerStats>,
    light_state: Res<lighting::LightingState>,
    lenses: Query<(
        &bevy::post_process::dof::DepthOfField,
        &bevy::post_process::motion_blur::MotionBlur,
    )>,
) {
    if options.frames == 0 {
        return;
    }
    *total += 1;
    assert!(*total < options.frames + 2400, "assets failed to load");
    if !ready.0 || runtime.warm < 45 {
        return;
    }
    runtime.frames += 1;
    let now = Instant::now();
    if let Some(previous) = runtime.last_frame
        && runtime.frames > 180
        && !runtime.requested
    {
        runtime
            .frame_ms
            .push(now.duration_since(previous).as_secs_f64() * 1000.);
    }
    runtime.last_frame = Some(now);
    if runtime.frames >= options.frames && !runtime.requested {
        if let Some(path) = &options.capture {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path.clone()))
                .observe(|_: On<ScreenshotCaptured>, mut r: ResMut<Runtime>| {
                    r.captured = true;
                });
        } else {
            runtime.captured = true;
        }
        runtime.requested = true;
    }
    if runtime.requested && runtime.captured {
        let timing = frame_summary(&runtime.frame_ms);
        let lens = lenses.iter().next().map(|(d,m)|serde_json::json!({"focal_distance":d.focal_distance,"aperture":d.aperture_f_stops,"blur_cap_px":d.max_circle_of_confusion_diameter,"shutter":m.shutter_angle,"motion_samples":m.samples}));
        let report = serde_json::json!({"scenario":options.scenario,"dramatic_lighting":light_state.0,"lens":lens,"barrier_widths":game.barrier_widths,"barrier_depths":game.barrier_depths,"benchmark_uncapped":options.benchmark,"post_processing":options.post,"frame_timing":timing,"resolution":[1440,900],"phase":format!("{:?}",game.phase),"distance_m":game.distance,"kills":game.kills,"wave":game.wave,"combo":game.combo,"overdrive_seconds":game.overdrive,"overdrive_activations":game.overdrive_activations,"announcer_starts":announcer.starts,"particle_emitters":particles.emitters,"peak_particle_emitters":particles.peak_emitters,"particle_bursts":particles.bursts,"dropped_particle_emitters":particles.dropped,"energy":game.energy,"energy_collected":game.energy_collected,"blasts_fired":game.blasts_fired,"detonations":game.detonations,"multikills":game.multikills,"multikill_size":game.multikill_size,"multikill_announcer_starts":announcer.multikills,"reload_audio_starts":announcer.reloads,"shots":game.shots,"hits":game.hits,"supplies_collected":game.supplies_collected,"ammo":game.ammo,"reload_seconds":game.reload,"score":game.score,"shield":game.shield,"health":game.health,"enemies":game.enemies.len(),"animation_players":players.iter().count(),"frames":runtime.frames,"assets_ready":ready.0});
        if let Some(path) = &options.report {
            std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
        println!("{report}");
        exit.write(AppExit::Success);
    }
}

fn frame_summary(samples: &[f64]) -> serde_json::Value {
    if samples.is_empty() {
        return serde_json::Value::Null;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let percentile = |p: f64| sorted[((sorted.len() - 1) as f64 * p).round() as usize];
    serde_json::json!({"measurement":"wall-clock frame intervals; excludes first 180 simulation frames and screenshot readback; not GPU timestamp timing", "samples":samples.len(),"mean_ms":samples.iter().sum::<f64>() / samples.len() as f64,"p50_ms":percentile(0.5),"p95_ms":percentile(0.95),"p99_ms":percentile(0.99),"max_ms":sorted.last()})
}
