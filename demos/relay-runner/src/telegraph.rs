//! A committed sniper sight line exposes the safe dodge window before the burst.
use crate::{
    Set,
    sim::{EnemyKind, Game, Phase},
};
use bevy::{
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
};

pub struct TelegraphPlugin;
#[derive(Resource)]
struct TelegraphAssets {
    beam: Handle<Mesh>,
    ring: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}
#[derive(Component)]
struct Telegraph {
    enemy: u64,
    endpoint: bool,
}
impl Plugin for TelegraphPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(Update, update.in_set(Set::Present));
    }
}
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(TelegraphAssets {
        beam: meshes.add(Cuboid::default()),
        ring: meshes.add(Torus::new(0.88, 1.)),
        material: materials.add(StandardMaterial {
            base_color: Color::srgba(0.8, 0.2, 1., 0.7),
            emissive: LinearRgba::new(3.5, 0.15, 6., 1.),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        }),
    });
}
fn beam_transform(from: Vec3, to: Vec3, width: f32) -> Transform {
    let delta = to - from;
    Transform::from_translation((from + to) * 0.5)
        .with_rotation(Quat::from_rotation_arc(
            Vec3::Z,
            delta.normalize_or(Vec3::Z),
        ))
        .with_scale(Vec3::new(width, width, delta.length().max(0.001)))
}
fn update(
    mut commands: Commands,
    game: Res<Game>,
    assets: Res<TelegraphAssets>,
    mut query: Query<(Entity, &Telegraph, &mut Transform)>,
) {
    let active = |id| {
        game.enemies.iter().find(|enemy| {
            enemy.id == id
                && enemy.kind == EnemyKind::Sniper
                && enemy.pos.z > -46.
                && enemy.pos.z < -4.
                && enemy.fire_in <= 0.65
                && enemy.burst_left == 0
                && matches!(game.phase, Phase::Playing | Phase::Paused)
        })
    };
    for (entity, visual, _) in &query {
        if active(visual.enemy).is_none() {
            commands.entity(entity).despawn();
        }
    }
    for enemy in game
        .enemies
        .iter()
        .filter(|enemy| active(enemy.id).is_some())
    {
        let urgency = (1. - enemy.fire_in / 0.65).clamp(0., 1.);
        let beam = beam_transform(
            enemy.pos + Vec3::new(0., 1.4, 0.4),
            enemy.aim_lock,
            0.012 + urgency * 0.014,
        );
        // The ring marks the committed lane on the deck, rather than chasing the player.
        let ring = Transform::from_xyz(enemy.aim_lock.x, 0.055, enemy.aim_lock.z)
            .with_scale(Vec3::splat(0.7 - urgency * 0.28));
        for endpoint in [false, true] {
            let transform = if endpoint { ring } else { beam };
            if let Some((_, _, mut existing)) = query
                .iter_mut()
                .find(|(_, visual, _)| visual.enemy == enemy.id && visual.endpoint == endpoint)
            {
                *existing = transform;
            } else {
                commands.spawn((
                    Telegraph {
                        enemy: enemy.id,
                        endpoint,
                    },
                    Mesh3d(if endpoint {
                        assets.ring.clone()
                    } else {
                        assets.beam.clone()
                    }),
                    MeshMaterial3d(assets.material.clone()),
                    transform,
                    NotShadowCaster,
                    NotShadowReceiver,
                ));
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn beam_geometry_reaches_both_locked_endpoints_on_first_transform() {
        let from = Vec3::new(-3., 2.4, -44.);
        let to = Vec3::new(4., 0.85, 1.);
        let transform = beam_transform(from, to, 0.02);
        assert!(transform.transform_point(-Vec3::Z * 0.5).distance(from) < 0.0001);
        assert!(transform.transform_point(Vec3::Z * 0.5).distance(to) < 0.0001);
        let coincident = beam_transform(from, from, 0.02);
        assert!(coincident.rotation.is_finite());
    }
}
