//! Input, fixed-step integration, persistence, and reproducible runtime captures.
use crate::{
    sim::{Input, Phase, Sim},
    ui::{UiAction, UiActions, UiMeta},
};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use serde::{Deserialize, Serialize};

pub(crate) struct RuntimePlugin;
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum GameSet {
    Input,
    Simulate,
    Present,
}
#[derive(Resource, Default)]
pub(crate) struct Controls(pub Input);
#[derive(Resource, Default)]
struct FireGate(bool);
#[derive(Resource, Default)]
struct RunClock {
    accumulator: f32,
    finished: bool,
}
#[derive(Resource)]
struct Harness {
    frame: u32,
    limit: u32,
    autoplay: bool,
    isolated: bool,
    capture: Option<String>,
    report: Option<String>,
    wall_start: std::time::Instant,
}
#[derive(Serialize, Deserialize)]
struct Record {
    best_score: u32,
    best_wave: u32,
    runs: u32,
    sound_enabled: bool,
}
impl Default for Record {
    fn default() -> Self {
        Self {
            best_score: 0,
            best_wave: 0,
            runs: 0,
            sound_enabled: true,
        }
    }
}
fn record_path() -> std::path::PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map_or_else(
            || {
                std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                    .join(".local/share")
            },
            std::path::PathBuf::from,
        )
        .join("scrapline/record.json")
}
impl Plugin for RuntimePlugin {
    fn build(&self, app: &mut App) {
        let args: Vec<String> = std::env::args().collect();
        let value = |name: &str| {
            args.iter()
                .position(|a| a == name)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let run_harness = Harness {
            frame: 0,
            limit: value("--frames").and_then(|v| v.parse().ok()).unwrap_or(0),
            autoplay: args.iter().any(|a| a == "--autoplay"),
            isolated: args.iter().any(|a| a == "--autoplay" || a == "--scenario"),
            capture: value("--screenshot"),
            report: value("--report"),
            wall_start: std::time::Instant::now(),
        };
        if let Some(scenario) = value("--scenario") {
            let mut sim = app.world_mut().resource_mut::<Sim>();
            sim.start_run();
            match scenario.as_str() {
                "title" => sim.return_to_title(),
                "upgrade" => {
                    sim.phase = Phase::Upgrade;
                    sim.level = 7;
                    sim.choices = vec![
                        crate::sim::UpgradeKind::Multishot,
                        crate::sim::UpgradeKind::Shield,
                        crate::sim::UpgradeKind::BlastRounds,
                    ];
                }
                "paused" => {
                    sim.phase = Phase::Paused;
                }
                "victory" | "defeat" => {
                    sim.phase = if scenario == "victory" {
                        Phase::Victory
                    } else {
                        Phase::Defeat
                    };
                    sim.wave = if scenario == "victory" { 10 } else { 6 };
                    sim.kills = if scenario == "victory" { 1010 } else { 360 };
                    sim.score = sim.kills * 50;
                    sim.level = 18;
                    sim.run_time = 712.;
                }
                "combat" => {}
                "showcase" => sim.prepare_showcase(false),
                "boss" => sim.prepare_showcase(true),
                _ => panic!("Unknown capture scenario: {scenario}"),
            }
        }
        let saved = std::fs::read(record_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Record>(&bytes).ok())
            .unwrap_or_default();
        app.insert_resource(UiMeta {
            best_score: saved.best_score,
            best_wave: saved.best_wave,
            runs: saved.runs,
            sound_enabled: saved.sound_enabled && !run_harness.isolated,
        })
        .insert_resource(run_harness)
        .init_resource::<Controls>()
        .init_resource::<FireGate>()
        .init_resource::<RunClock>()
        .configure_sets(
            Update,
            (GameSet::Input, GameSet::Simulate, GameSet::Present).chain(),
        )
        .add_systems(Update, (input, actions).chain().in_set(GameSet::Input))
        .add_systems(Update, simulate.in_set(GameSet::Simulate))
        .add_systems(Last, harness);
    }
}
fn input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    sim: Res<Sim>,
    harness: Res<Harness>,
    mut controls: ResMut<Controls>,
    mut actions: ResMut<UiActions>,
    mut had_focus: Local<bool>,
    mut fire_gate: ResMut<FireGate>,
) {
    if let Ok(window) = windows.single() {
        if *had_focus && !window.focused && sim.phase == Phase::Playing && !harness.autoplay {
            actions.0.push(UiAction::Pause);
        }
        *had_focus = window.focused;
    }
    if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::KeyP) {
        actions.0.push(UiAction::Pause);
    }
    if keys.just_pressed(KeyCode::KeyM) {
        actions.0.push(UiAction::ToggleSound);
    }
    if keys.just_pressed(KeyCode::Enter) {
        actions.0.push(match sim.phase {
            Phase::Title => UiAction::Start,
            Phase::Paused => UiAction::Resume,
            Phase::Victory | Phase::Defeat => UiAction::Restart,
            _ => UiAction::Upgrade(99),
        });
    }
    if keys.just_pressed(KeyCode::KeyR)
        && matches!(sim.phase, Phase::Paused | Phase::Victory | Phase::Defeat)
    {
        actions.0.push(UiAction::Restart);
    }
    for (i, key) in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3]
        .iter()
        .enumerate()
    {
        if keys.just_pressed(*key) && sim.phase == Phase::Upgrade {
            actions.0.push(UiAction::Upgrade(i));
        }
    }
    let held = |a, b| {
        if keys.pressed(a) || keys.pressed(b) {
            1.0
        } else {
            0.0
        }
    };
    let movement = Vec2::new(
        held(KeyCode::KeyD, KeyCode::ArrowRight) - held(KeyCode::KeyA, KeyCode::ArrowLeft),
        held(KeyCode::KeyS, KeyCode::ArrowDown) - held(KeyCode::KeyW, KeyCode::ArrowUp),
    );
    let mut aim = sim.player.aim;
    if let (Ok(window), Ok((camera, transform))) = (windows.single(), cameras.single())
        && let Some(cursor) = window.cursor_position()
        && let Ok(ray) = camera.viewport_to_world(transform, cursor)
        && let Some(distance) = ray.intersect_plane(Vec3::Y * 1.35, InfinitePlane3d::new(Vec3::Y))
    {
        let point = ray.get_point(distance);
        aim = (Vec2::new(point.x, point.z) - sim.player.pos).normalize_or_zero();
    }
    if !mouse.pressed(MouseButton::Left) {
        fire_gate.0 = false;
    }
    controls.0 = Input {
        movement,
        aim,
        fire: sim.phase == Phase::Playing && mouse.pressed(MouseButton::Left) && !fire_gate.0,
        dash: sim.phase == Phase::Playing && (keys.just_pressed(KeyCode::Space) || controls.0.dash),
    };
    if harness.autoplay {
        if sim.phase == Phase::Title {
            actions.0.push(UiAction::Start);
        }
        if sim.phase == Phase::Upgrade {
            actions.0.push(UiAction::Upgrade(
                (sim.level as usize) % sim.choices.len().max(1),
            ));
        }
        let nearest = sim.enemies.iter().min_by(|a, b| {
            a.pos
                .distance_squared(sim.player.pos)
                .total_cmp(&b.pos.distance_squared(sim.player.pos))
        });
        let target = nearest.map_or(Vec2::X, |e| e.pos - sim.player.pos);
        let orbit = Vec2::new((sim.run_time * 0.23).cos(), (sim.run_time * 0.23).sin()) * 12.;
        let evade = if target.length() < 6. {
            -target.normalize_or_zero() * 1.8
        } else {
            Vec2::ZERO
        };
        controls.0 = Input {
            movement: ((orbit - sim.player.pos).normalize_or_zero() + evade).normalize_or_zero(),
            aim: target.normalize_or_zero(),
            fire: true,
            dash: target.length() < 3.5,
        };
    }
}
fn actions(
    mut actions: ResMut<UiActions>,
    mut sim: ResMut<Sim>,
    mut meta: ResMut<UiMeta>,
    mut clock: ResMut<RunClock>,
    harness: Res<Harness>,
    mut fire_gate: ResMut<FireGate>,
    mut controls: ResMut<Controls>,
) {
    for action in actions.0.drain(..) {
        let previous_phase = sim.phase;
        match action {
            UiAction::Start | UiAction::Restart => {
                if !harness.autoplay {
                    let seed = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(42, |d| d.as_nanos() as u64);
                    *sim = Sim::new(seed);
                }
                sim.start_run();
                fire_gate.0 = true;
                controls.0.fire = false;
                controls.0.dash = false;
                clock.finished = false;
                clock.accumulator = 0.;
            }
            UiAction::Pause => sim.toggle_pause(),
            UiAction::Resume => {
                if sim.phase == Phase::Paused {
                    sim.toggle_pause();
                }
            }
            UiAction::Menu => sim.return_to_title(),
            UiAction::Upgrade(i) => {
                if sim.phase == Phase::Upgrade {
                    sim.choose_upgrade(i);
                    fire_gate.0 = true;
                    controls.0.fire = false;
                }
            }
            UiAction::ToggleSound => meta.sound_enabled = !meta.sound_enabled,
        }
        if previous_phase != Phase::Playing && sim.phase == Phase::Playing {
            fire_gate.0 = true;
            controls.0.fire = false;
        }
    }
}
fn simulate(
    time: Res<Time>,
    mut controls: ResMut<Controls>,
    mut sim: ResMut<Sim>,
    mut clock: ResMut<RunClock>,
    mut meta: ResMut<UiMeta>,
    harness: Res<Harness>,
) {
    if sim.phase == Phase::Playing {
        clock.accumulator += if harness.autoplay && harness.limit > 0 {
            1. / 60.
        } else {
            time.delta_secs().min(0.1)
        };
        let mut input = controls.0;
        while clock.accumulator >= 1. / 60. && sim.phase == Phase::Playing {
            sim.step(1. / 60., input);
            input.dash = false;
            controls.0.dash = false;
            clock.accumulator -= 1. / 60.;
        }
    } else {
        controls.0.dash = false;
        clock.accumulator = 0.;
    }
    if matches!(sim.phase, Phase::Victory | Phase::Defeat) && !clock.finished {
        clock.finished = true;
        meta.runs += 1;
        meta.best_score = meta.best_score.max(sim.score);
        meta.best_wave = meta.best_wave.max(sim.wave);
    }
    if meta.is_changed() && !harness.isolated {
        let record = Record {
            best_score: meta.best_score,
            best_wave: meta.best_wave,
            runs: meta.runs,
            sound_enabled: meta.sound_enabled,
        };
        let path = record_path();
        if let Some(parent) = path.parent() {
            let result = std::fs::create_dir_all(parent).and_then(|()| {
                std::fs::write(
                    path.with_extension("tmp"),
                    serde_json::to_vec_pretty(&record).unwrap_or_default(),
                )
                .and_then(|()| std::fs::rename(path.with_extension("tmp"), &path))
            });
            if let Err(error) = result {
                warn!("Could not save run record: {error}");
            }
        }
    }
}
fn harness(
    mut commands: Commands,
    mut harness: ResMut<Harness>,
    sim: Res<Sim>,
    mut exit: MessageWriter<AppExit>,
) {
    harness.frame += 1;
    if harness.limit == 0 {
        return;
    }
    if harness.frame == harness.limit {
        if let Some(path) = &harness.capture {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path.clone()));
        }
        if let Some(path) = &harness.report {
            let report = serde_json::json!({"frame":harness.frame,"phase":format!("{:?}",sim.phase),"wave":sim.wave,"hp":sim.player.hp,"kills":sim.kills,"score":sim.score,"level":sim.level,"enemies":sim.enemies.len(),"bullets":sim.bullets.len(),"pickups":sim.pickups.len(),"run_time":sim.run_time,"player_pos":[sim.player.pos.x,sim.player.pos.y],"aim":[sim.player.aim.x,sim.player.aim.y],"shield":sim.player.shield,"build":crate::sim::UpgradeKind::ALL.iter().filter(|k|sim.upgrade_rank(**k)>0).map(|k|serde_json::json!({"upgrade":k.title(),"rank":sim.upgrade_rank(*k)})).collect::<Vec<_>>(),"wall_seconds":harness.wall_start.elapsed().as_secs_f64(),"mean_fps":f64::from(harness.frame)/harness.wall_start.elapsed().as_secs_f64()});
            if let Err(error) =
                std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap_or_default())
            {
                error!("Failed to write runtime report {path}: {error}");
                exit.write(AppExit::error());
            }
        }
    }
    if harness.frame >= harness.limit + 30 {
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dash_survives_a_render_frame_without_simulation_tick() {
        let mut app = App::new();
        let mut sim = Sim::new(7);
        sim.start_run();
        app.insert_resource(sim)
            .insert_resource(Time::<()>::default())
            .insert_resource(Controls(Input {
                dash: true,
                aim: Vec2::Y,
                ..default()
            }))
            .init_resource::<RunClock>()
            .init_resource::<UiMeta>()
            .insert_resource(Harness {
                frame: 0,
                limit: 0,
                autoplay: false,
                isolated: true,
                capture: None,
                report: None,
                wall_start: std::time::Instant::now(),
            })
            .add_systems(Update, simulate);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f64(1.0 / 100.0));
        app.update();
        assert!(app.world().resource::<Controls>().0.dash);
        assert!(app.world().resource::<Sim>().player.dashing.abs() < f32::EPSILON);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f64(1.0 / 100.0));
        app.update();
        assert!(!app.world().resource::<Controls>().0.dash);
        assert!(app.world().resource::<Sim>().player.dashing > 0.);
    }
}
