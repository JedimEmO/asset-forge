//! Bounded event audio. Sound never consumes or changes simulation/UI actions.
use bevy::{audio::Volume, prelude::*};

use crate::{
    runtime::GameSet,
    sim::{EffectKind, Phase, Sim},
    ui::UiMeta,
};

pub(crate) struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MixState>()
            .add_systems(Startup, load_audio)
            .add_systems(Update, mix_audio.in_set(GameSet::Present));
    }
}

#[derive(Resource)]
struct Sounds {
    effects: [Handle<AudioSource>; 7],
}

#[derive(Component)]
struct CombatMusic;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
enum Cue {
    Shot,
    Hit,
    Explosion,
    Pickup,
    Dash,
    Upgrade,
    Ui,
}

impl Cue {
    fn settings(self) -> (f32, f64, f64, usize) {
        // Gain, lifetime (including load grace), minimum interval, voice cap.
        match self {
            Self::Shot => (0.24, 0.34, 0.075, 3),
            Self::Hit => (0.22, 0.32, 0.08, 2),
            Self::Explosion => (0.30, 1.00, 0.22, 2),
            Self::Pickup => (0.40, 0.52, 0.15, 1),
            Self::Dash => (0.36, 0.38, 0.10, 1),
            Self::Upgrade => (0.48, 1.10, 0.70, 1),
            Self::Ui => (0.36, 0.185, 0.10, 1),
        }
    }
}

#[derive(Component)]
struct EffectVoice {
    cue: Cue,
    expires_at: f64,
}

#[derive(Resource)]
struct MixState {
    phase: Option<Phase>,
    run_time: f32,
    last_effect: u64,
    last_xp: f32,
    overdrive: f32,
    magnet: f32,
    shield: f32,
    next_allowed: [f64; 7],
    music_gain: f32,
}

impl Default for MixState {
    fn default() -> Self {
        Self {
            phase: None,
            run_time: 0.,
            last_effect: 0,
            last_xp: 0.,
            overdrive: 0.,
            magnet: 0.,
            shield: 0.,
            next_allowed: [0.; 7],
            music_gain: 0.,
        }
    }
}

fn load_audio(mut commands: Commands, server: Res<AssetServer>) {
    commands.insert_resource(Sounds {
        effects: [
            "shot",
            "hit",
            "explosion",
            "pickup",
            "dash",
            "upgrade",
            "ui",
        ]
        .map(|name| server.load(format!("assets/audio/sfx/scrapyard_{name}.wav"))),
    });
    commands.spawn((
        Name::new("Combat music"),
        CombatMusic,
        AudioPlayer::new(server.load("assets/audio/music/scrapyard_combat_loop.wav")),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(0.)),
    ));
}

fn mix_audio(
    mut commands: Commands,
    sounds: Res<Sounds>,
    sim: Res<Sim>,
    meta: Res<UiMeta>,
    time: Res<Time<Real>>,
    mut state: ResMut<MixState>,
    voices: Query<(Entity, &EffectVoice)>,
    mut music: Query<(&mut PlaybackSettings, Option<&mut AudioSink>), With<CombatMusic>>,
) {
    let now = time.elapsed_secs_f64();
    let restarting = sim.run_time < state.run_time
        || (state.phase == Some(Phase::Title) && sim.phase == Phase::Playing);
    if restarting {
        state.last_effect = 0;
        state.next_allowed = [0.; 7];
    }
    let stop_effects = !meta.sound_enabled || matches!(sim.phase, Phase::Title | Phase::Paused);
    let mut counts = [0usize; 7];
    let mut active = 0;
    for (entity, voice) in &voices {
        if stop_effects || restarting || voice.expires_at <= now {
            commands.entity(entity).despawn();
        } else {
            active += 1;
            counts[voice.cue as usize] += 1;
        }
    }

    let target = if !meta.sound_enabled {
        0.
    } else if sim.phase == Phase::Playing {
        0.25
    } else {
        0.08
    };
    // Muting is immediate; scene changes crossfade the volume over ~0.2 s.
    state.music_gain = if meta.sound_enabled {
        state.music_gain + (target - state.music_gain) * (time.delta_secs() * 5.).min(1.)
    } else {
        0.
    };
    for (mut settings, sink) in &mut music {
        settings.volume = Volume::Linear(state.music_gain);
        if let Some(mut sink) = sink {
            sink.set_volume(settings.volume);
        }
    }

    let mut requested = [false; 7];
    if state.phase.is_some_and(|previous| previous != sim.phase) {
        requested[Cue::Ui as usize] = true;
        if sim.phase == Phase::Victory {
            requested[Cue::Upgrade as usize] = true;
        }
    }
    for effect in &sim.effects {
        if effect.id <= state.last_effect {
            continue;
        }
        let cue = match effect.kind {
            EffectKind::Muzzle => Some(Cue::Shot),
            EffectKind::Hit => Some(Cue::Hit),
            EffectKind::Explosion => Some(Cue::Explosion),
            EffectKind::Dash => Some(Cue::Dash),
            EffectKind::Heal => Some(Cue::Pickup),
            EffectKind::LevelUp => Some(Cue::Upgrade),
            EffectKind::Spawn => None,
        };
        if let Some(cue) = cue {
            requested[cue as usize] = true;
        }
    }
    // XP and timed powerups have no visual effect event; observe their increase.
    if !restarting
        && (sim.xp > state.last_xp
            || sim.player.overdrive > state.overdrive + 0.2
            || sim.player.magnet > state.magnet + 0.2
            || sim.player.shield > state.shield + 1.)
    {
        requested[Cue::Pickup as usize] = true;
    }
    // Always advance the watermark while muted: enabling sound cannot replay a backlog.
    state.last_effect = sim
        .effects
        .iter()
        .map(|effect| effect.id)
        .max()
        .unwrap_or(state.last_effect)
        .max(state.last_effect);
    state.run_time = sim.run_time;
    state.phase = Some(sim.phase);
    state.last_xp = sim.xp;
    state.overdrive = sim.player.overdrive;
    state.magnet = sim.player.magnet;
    state.shield = sim.player.shield;
    if !meta.sound_enabled {
        return;
    }
    let mut spawned = 0;
    for cue in [
        Cue::Upgrade,
        Cue::Dash,
        Cue::Ui,
        Cue::Shot,
        Cue::Explosion,
        Cue::Pickup,
        Cue::Hit,
    ] {
        let index = cue as usize;
        let (gain, duration, interval, cap) = cue.settings();
        if !requested[index]
            || now < state.next_allowed[index]
            || active >= 8
            || counts[index] >= cap
            || spawned >= 3
            || (matches!(sim.phase, Phase::Paused | Phase::Title) && cue != Cue::Ui)
        {
            continue;
        }
        commands.spawn((
            EffectVoice {
                cue,
                expires_at: now + duration,
            },
            AudioPlayer::new(sounds.effects[index].clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(gain)),
        ));
        state.next_allowed[index] = now + interval;
        active += 1;
        spawned += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Effect;

    fn app() -> App {
        let mut app = App::new();
        let mut sim = Sim::new(42);
        sim.phase = Phase::Playing;
        app.add_plugins(MinimalPlugins)
            .insert_resource(sim)
            .insert_resource(UiMeta {
                sound_enabled: true,
                ..default()
            })
            .insert_resource(Sounds {
                effects: std::array::from_fn(|_| Handle::default()),
            })
            .init_resource::<MixState>()
            .add_systems(Update, mix_audio);
        app
    }

    fn effect(id: u64, kind: EffectKind) -> Effect {
        Effect {
            id,
            kind,
            pos: Vec2::ZERO,
            age: 0.,
            duration: 1.,
            radius: 1.,
        }
    }

    fn voices(app: &mut App) -> usize {
        app.world_mut()
            .query::<&EffectVoice>()
            .iter(app.world())
            .count()
    }

    #[test]
    fn horde_events_are_coalesced_and_never_replayed() {
        let mut app = app();
        app.world_mut().resource_mut::<Sim>().effects = (1..=100)
            .map(|id| {
                effect(
                    id,
                    if id % 2 == 0 {
                        EffectKind::Hit
                    } else {
                        EffectKind::Explosion
                    },
                )
            })
            .collect();
        app.update();
        assert_eq!(voices(&mut app), 2);
        app.update();
        assert_eq!(voices(&mut app), 2);
        // The highest-id short-lived effect can disappear before older effects.
        app.world_mut().resource_mut::<Sim>().effects.truncate(1);
        app.update();
        assert_eq!(app.world().resource::<MixState>().last_effect, 100);
    }

    #[test]
    fn mute_stops_active_voices_and_does_not_queue_events() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<Sim>()
            .effects
            .push(effect(1, EffectKind::Muzzle));
        app.update();
        assert_eq!(voices(&mut app), 1);
        app.world_mut().resource_mut::<UiMeta>().sound_enabled = false;
        app.world_mut()
            .resource_mut::<Sim>()
            .effects
            .push(effect(2, EffectKind::Explosion));
        app.update();
        assert_eq!(voices(&mut app), 0);
        app.world_mut().resource_mut::<UiMeta>().sound_enabled = true;
        app.update();
        assert_eq!(voices(&mut app), 0);
    }
}
