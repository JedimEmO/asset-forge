//! Dramatic key/fill separation and real light from emissive fixtures and combat.
use crate::{
    Set,
    sim::{FxKind, Game},
};
use bevy::{light::CascadeShadowConfigBuilder, prelude::*};
pub struct LightingPlugin;
#[derive(Component)]
enum LightRole {
    Key,
    Fill,
    Rim,
}
#[derive(Component)]
pub struct Practical;
#[derive(Component)]
struct CombatLight(bool);
#[derive(Resource)]
pub struct LightingState(pub bool);
impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(LightingState(true))
            .add_systems(Startup, setup)
            .add_systems(Update, update.after(Set::Present));
    }
}
fn setup(mut c: Commands, options: Res<crate::Options>, mut state: ResMut<LightingState>) {
    state.0 = !options.flat_light;
    c.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.35, 0.52, 1.),
        brightness: 160.,
        ..default()
    });
    c.spawn((
        LightRole::Key,
        DirectionalLight {
            illuminance: 12500.,
            color: Color::srgb(1., 0.68, 0.4),
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            maximum_distance: 130.,
            first_cascade_far_bound: 16.,
            ..default()
        }
        .build(),
        Transform::from_xyz(-18., 8., -16.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    c.spawn((
        LightRole::Fill,
        DirectionalLight {
            illuminance: 2200.,
            color: Color::srgb(0.65, 0.78, 1.),
            ..default()
        },
        Transform::from_xyz(8., 12., 18.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    c.spawn((
        LightRole::Rim,
        DirectionalLight {
            illuminance: 2300.,
            color: Color::srgb(0.12, 0.58, 1.),
            ..default()
        },
        Transform::from_xyz(14., 5., -20.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    for blast in [false, true] {
        c.spawn((
            CombatLight(blast),
            PointLight {
                intensity: 0.,
                range: if blast { 18. } else { 5. },
                radius: 0.4,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::default(),
        ));
    }
}
fn update(
    keys: Res<ButtonInput<KeyCode>>,
    options: Res<crate::Options>,
    g: Res<Game>,
    mut state: ResMut<LightingState>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut lights: Query<(&mut DirectionalLight, &mut Transform, &LightRole)>,
    mut points: Query<(&mut PointLight, &mut Transform, &CombatLight), Without<DirectionalLight>>,
    mut fixtures: Query<&mut PointLight, (With<Practical>, Without<CombatLight>)>,
) {
    if options.frames == 0 && keys.just_pressed(KeyCode::F9) {
        state.0 = !state.0;
    }
    ambient.brightness = if state.0 { 160. } else { 650. };
    ambient.color = if state.0 {
        Color::srgb(0.35, 0.52, 1.)
    } else {
        Color::srgb(0.48, 0.62, 1.)
    };
    for (mut l, mut t, role) in &mut lights {
        if matches!(role, LightRole::Key) {
            l.illuminance = if state.0 { 12500. } else { 9500. };
            l.color = if state.0 {
                Color::srgb(1., 0.68, 0.4)
            } else {
                Color::srgb(1., 0.84, 0.68)
            };
            *t = Transform::from_translation(if state.0 {
                Vec3::new(-18., 8., -16.)
            } else {
                Vec3::new(-10., 18., -25.)
            })
            .looking_at(Vec3::ZERO, Vec3::Y);
        } else if matches!(role, LightRole::Fill) {
            l.illuminance = if state.0 { 2200. } else { 3500. };
            l.color = if state.0 {
                Color::srgb(0.65, 0.78, 1.)
            } else {
                Color::srgb(0.84, 0.91, 1.)
            };
        } else {
            l.illuminance = if state.0 { 2300. } else { 0. };
        }
    }
    for mut l in &mut fixtures {
        l.intensity = if state.0 { 22000. } else { 0. };
    }
    for (mut l, mut t, kind) in &mut points {
        let fx = g
            .effects
            .iter()
            .filter(|f| {
                if kind.0 {
                    matches!(f.kind, FxKind::Detonate | FxKind::Nova | FxKind::Kill)
                } else {
                    f.kind == FxKind::Shot
                }
            })
            .min_by(|a, b| a.age.total_cmp(&b.age));
        if let Some(f) = fx {
            t.translation = f.from + Vec3::Y * 0.3;
            l.color = if kind.0 {
                Color::srgb(0.18, 0.65, 1.)
            } else {
                Color::srgb(0.35, 0.8, 1.)
            };
            l.intensity = if state.0 {
                (1. - f.age / if kind.0 { 0.3 } else { 0.075 })
                    .max(0.)
                    .powi(2)
                    * if kind.0 { 320000. } else { 12000. }
            } else {
                0.
            };
        } else {
            l.intensity = 0.;
        }
    }
}
