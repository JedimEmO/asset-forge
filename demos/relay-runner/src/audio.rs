use crate::voice_fx::{ProcessedVoice, VoiceProcessing};
use crate::{
    Set,
    sim::{FxKind, Game, Phase},
};
use bevy::{
    audio::{AddAudioSource, Volume},
    prelude::*,
};
use std::sync::atomic::Ordering;
pub struct AudioPlugin;
#[derive(Resource, Default)]
pub struct AnnouncerStats {
    pub starts: u32,
    pub multikills: u32,
    pub reloads: u32,
}
#[derive(Resource)]
struct Sounds {
    shot: Handle<AudioSource>,
    hit: Handle<AudioSource>,
    kill: Handle<AudioSource>,
    nova: Handle<AudioSource>,
    pickup: Handle<AudioSource>,
    free_fire: Handle<AudioSource>,
    multikill: Handle<AudioSource>,
    reload: Handle<AudioSource>,
    blast: Handle<AudioSource>,
}
#[derive(Resource, Default)]
struct Mixer {
    last: u64,
    last_time: f32,
    announced: u32,
    duck_until: f32,
    multi_seen: u32,
    pending_free: bool,
    pending_multi: bool,
    last_reload: f32,
}
#[derive(Component)]
struct Music;
#[derive(Component)]
struct Voice(f32);
#[derive(Component)]
struct ReloadVoice;
impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<ProcessedVoice>()
            .init_resource::<VoiceProcessing>()
            .init_resource::<Mixer>()
            .init_resource::<AnnouncerStats>()
            .add_systems(Startup, setup)
            .add_systems(Update, mix.in_set(Set::Present));
    }
}
fn setup(mut c: Commands, server: Res<AssetServer>) {
    c.insert_resource(Sounds {
        shot: server.load("rusher/audio/sfx/release_rifle_shot.wav"),
        hit: server.load("scavenger/audio/sfx/scrapyard_hit.wav"),
        kill: server.load("scavenger/audio/sfx/scrapyard_explosion.wav"),
        nova: server.load("scavenger/audio/sfx/scrapyard_dash.wav"),
        pickup: server.load("scavenger/audio/sfx/scrapyard_pickup.wav"),
        free_fire: server.load("showcase/audio/voice/relay_free_fire.wav"),
        multikill: server.load("showcase/audio/voice/relay_multikill.wav"),
        reload: server.load("showcase/audio/sfx/relay_reload.wav"),
        blast: server.load("showcase/audio/sfx/relay_plasma.wav"),
    });
    c.spawn((
        Music,
        AudioPlayer::new(server.load("rusher/audio/music/release_combat_loop.wav")),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(0.)),
    ));
}
fn mix(
    mut c: Commands,
    g: Res<Game>,
    time: Res<Time>,
    sounds: Res<Sounds>,
    mut mixer: ResMut<Mixer>,
    mut stats: ResMut<AnnouncerStats>,
    mut music: Query<&mut AudioSink, With<Music>>,
    voices: Query<(Entity, &Voice)>,
    reloads: Query<Entity, With<ReloadVoice>>,
    sources: Res<Assets<AudioSource>>,
    mut processed: ResMut<Assets<ProcessedVoice>>,
    processing: Res<VoiceProcessing>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if keys.just_pressed(KeyCode::F7) {
        processing.0.fetch_xor(true, Ordering::Relaxed);
    }
    if g.time < mixer.last_time {
        mixer.last = 0;
        mixer.announced = 0;
        mixer.duck_until = 0.;
        mixer.multi_seen = 0;
        mixer.pending_multi = false;
        mixer.pending_free = false;
        mixer.last_reload = 0.;
    }
    if g.overdrive_activations > mixer.announced {
        mixer.announced = g.overdrive_activations;
        mixer.pending_free = g.sound && g.phase == Phase::Playing;
    }
    if g.multikills > mixer.multi_seen {
        mixer.multi_seen = g.multikills;
        mixer.pending_multi = g.sound && g.phase == Phase::Playing;
    }
    if !g.sound || g.phase != Phase::Playing {
        mixer.pending_free = false;
        mixer.pending_multi = false;
        mixer.duck_until = 0.;
    }
    if time.elapsed_secs() >= mixer.duck_until
        && (mixer.pending_multi || mixer.pending_free)
        && sources.contains(if mixer.pending_multi {
            &sounds.multikill
        } else {
            &sounds.free_fire
        })
    {
        let multi = mixer.pending_multi;
        let duration = if multi { 3.6 } else { 6.0 };
        let source = if multi {
            mixer.pending_multi = false;
            stats.multikills += 1;
            sounds.multikill.clone()
        } else {
            mixer.pending_free = false;
            stats.starts += 1;
            sounds.free_fire.clone()
        };
        mixer.duck_until = time.elapsed_secs() + duration;
        c.spawn((
            AudioPlayer(processed.add(ProcessedVoice {
                source: sources.get(&source).unwrap().clone(),
                enabled: processing.0.clone(),
            })),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(1.0)),
            Voice(mixer.duck_until),
        ));
    }
    if g.reload > 0. && mixer.last_reload == 0. && g.sound && g.phase == Phase::Playing {
        stats.reloads += 1;
        c.spawn((
            AudioPlayer::new(sounds.reload.clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.75)),
            Voice(time.elapsed_secs() + 1.4),
            ReloadVoice,
        ));
    }
    if g.reload == 0. || !g.sound || g.phase != Phase::Playing {
        for entity in &reloads {
            c.entity(entity).despawn();
        }
    }
    mixer.last_reload = g.reload;
    for mut sink in &mut music {
        sink.set_volume(Volume::Linear(if g.sound && g.phase == Phase::Playing {
            if time.elapsed_secs() < mixer.duck_until {
                0.055
            } else if g.overdrive > 0. {
                0.23
            } else if g.wave_banner > 0. {
                0.19
            } else {
                0.14
            }
        } else {
            0.
        }));
    }
    for (entity, voice) in &voices {
        if time.elapsed_secs() > voice.0 || !g.sound || g.phase != Phase::Playing {
            c.entity(entity).despawn();
        }
    }
    mixer.last_time = g.time;
    let mut count = 0;
    for effect in &g.effects {
        if effect.id <= mixer.last {
            continue;
        }
        mixer.last = effect.id;
        if !g.sound || g.phase != Phase::Playing || count >= 4 || voices.iter().count() > 14 {
            continue;
        }
        let (source, gain) = match effect.kind {
            FxKind::Shot => (&sounds.shot, 0.26),
            FxKind::Hit => (&sounds.hit, 0.38),
            FxKind::Kill => (&sounds.kill, 0.60),
            FxKind::Nova => (&sounds.nova, 0.65),
            FxKind::Dash => (&sounds.nova, 0.35),
            FxKind::Pickup => (&sounds.pickup, 0.35),
            FxKind::Hurt => (&sounds.hit, 0.3),
            FxKind::Detonate => (&sounds.blast, 0.85),
            FxKind::Launch => (&sounds.nova, 0.55),
            FxKind::Energy => (&sounds.pickup, 0.3),
        };
        c.spawn((
            AudioPlayer::new(source.clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(
                gain * if time.elapsed_secs() < mixer.duck_until {
                    0.45
                } else {
                    1.
                },
            )),
            Voice(time.elapsed_secs() + 2.2),
        ));
        count += 1;
    }
}
