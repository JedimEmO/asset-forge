//! Sparse orbital infrastructure and deterministic, model-backed shipping traffic.
//! Models are Y-up, ships point along local +Z; library bounds provide their scale.
use crate::{Options, Set, sim::Game};
use bevy::{
    prelude::*,
    world_serialization::{WorldAsset, WorldInstanceReady},
};
use std::collections::HashMap;

pub struct SceneryPlugin;
/// Every present scenery model and its recursive render dependencies are loaded.
#[derive(Resource, Default)]
pub struct SceneryReady(pub bool);
const LANDMARK_SLOTS: u32 = 12;
const SPACING: f32 = 36.;
const LOOP: f32 = LANDMARK_SLOTS as f32 * SPACING;

#[derive(Clone)]
struct Model {
    scene: Handle<WorldAsset>,
    center: Vec3,
    size: Vec3,
}
#[derive(Resource)]
struct SceneryAssets {
    landmarks: Vec<Option<Model>>,
    ships: Vec<Option<Model>>,
    foundation: Handle<Mesh>,
    alloy: Handle<StandardMaterial>,
}
#[derive(Component)]
struct Landmark {
    slot: u32,
    side: f32,
    generation: i32,
}
#[derive(Component)]
struct SkyShipModel(f32);
#[derive(Resource, Default)]
struct SkyMaterials(HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>);
#[derive(Component)]
struct Ship {
    lane: usize,
    member: usize,
}

impl Plugin for SceneryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SceneryReady>()
            .init_resource::<SkyMaterials>()
            .add_observer(ship_ready)
            .add_systems(Startup, setup)
            .add_systems(Update, check_ready.before(Set::Present))
            .add_systems(Update, (landmarks, traffic).in_set(Set::Present));
    }
}
// Space traffic is outside the causeway atmosphere. Clone only these instances'
// materials; combat interceptors keep the shared asset's original PBR response.
fn ship_ready(
    event: On<WorldInstanceReady>,
    sky: Query<&SkyShipModel>,
    children: Query<&Children>,
    mut meshes: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: ResMut<SkyMaterials>,
) {
    let Ok(sky_model) = sky.get(event.entity) else {
        return;
    };
    for entity in children.iter_descendants(event.entity) {
        let Ok(mut mesh) = meshes.get_mut(entity) else {
            continue;
        };
        let original = mesh.0.id();
        if let Some(handle) = cache.0.get(&original) {
            mesh.0 = handle.clone();
            continue;
        }
        let Some(mut material) = materials.get(&mesh.0).cloned() else {
            continue;
        };
        material.fog_enabled = false;
        material.emissive_texture = material.base_color_texture.clone();
        material.emissive = LinearRgba::new(sky_model.0, sky_model.0, sky_model.0, 1.);
        let handle = materials.add(material);
        cache.0.insert(original, handle.clone());
        mesh.0 = handle;
    }
}
fn check_ready(
    server: Res<AssetServer>,
    assets: Res<SceneryAssets>,
    mut ready: ResMut<SceneryReady>,
) {
    ready.0 = assets
        .landmarks
        .iter()
        .chain(&assets.ships)
        .flatten()
        .all(|model| server.is_loaded_with_dependencies(&model.scene));
}
fn load_model(
    name: &str,
    options: &Options,
    server: &AssetServer,
    library: &serde_json::Value,
) -> Option<Model> {
    let path = format!("showcase/models/{name}.glb");
    if !crate::metadata::exists(&options.root, &path) {
        return None;
    }
    let row = library["models"]
        .as_array()?
        .iter()
        .find(|row| row["name"] == name)?;
    let bounds = &row["bounds_m"];
    let vector = |v: &serde_json::Value| -> Option<Vec3> {
        Some(Vec3::new(
            v[0].as_f64()? as f32,
            v[1].as_f64()? as f32,
            v[2].as_f64()? as f32,
        ))
    };
    let lo = vector(&bounds[0])?;
    let hi = vector(&bounds[1])?;
    let size = hi - lo;
    if !size.is_finite() || size.min_element() <= 0. {
        return None;
    }
    Some(Model {
        scene: server.load(GltfAssetLabel::Scene(0).from_asset(path)),
        center: (lo + hi) * 0.5,
        size,
    })
}
fn setup(
    mut commands: Commands,
    options: Res<Options>,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let library = crate::metadata::read(&options.root, "showcase/library.json");
    let assets = SceneryAssets {
        foundation: meshes.add(Cuboid::default()),
        alloy: materials.add(StandardMaterial {
            base_color: Color::srgb(0.16, 0.22, 0.29),
            metallic: 0.65,
            perceptual_roughness: 0.55,
            ..default()
        }),
        landmarks: [
            "relay_radar",
            "relay_reactor",
            "relay_station",
            "relay_cargo",
        ]
        .into_iter()
        .map(|name| load_model(name, &options, &server, &library))
        .collect(),
        ships: ["relay_interceptor", "relay_freighter", "relay_cruiser"]
            .into_iter()
            .map(|name| load_model(name, &options, &server, &library))
            .collect(),
    };
    if assets.landmarks.iter().any(Option::is_some) {
        for slot in 0..LANDMARK_SLOTS {
            for side in [-1., 1.] {
                commands.spawn((
                    Landmark {
                        slot,
                        side,
                        generation: i32::MIN,
                    },
                    Transform::from_xyz(side * 14., 0., -400.),
                    Visibility::Hidden,
                ));
            }
        }
    }
    // Two small three-ship formations, a cargo hauler and a slow capital ship.
    for lane in 0..4 {
        let class = match lane {
            0 | 2 => 0,
            1 => 1,
            _ => 2,
        };
        let Some(model) = assets.ships.get(class).and_then(Option::as_ref) else {
            continue;
        };
        let count = if class == 0 { 3 } else { 1 };
        let length = [7.5, 18., 42.][class];
        let scale = length / model.size.z;
        for member in 0..count {
            commands
                .spawn((
                    Ship { lane, member },
                    ship_transform(lane, member, 0.),
                    Visibility::default(),
                ))
                .with_children(|c| {
                    c.spawn((
                        SkyShipModel(if class == 0 { 0.6 } else { 2.4 }),
                        WorldAssetRoot(model.scene.clone()),
                        Transform::from_translation(-model.center * scale)
                            .with_scale(Vec3::splat(scale)),
                    ));
                });
        }
    }
    commands.insert_resource(assets);
}

/// A 432 m rolling window changes its arrangement only after passing the camera.
fn landmark_position(slot: u32, distance: f32) -> (i32, f32) {
    let absolute = slot as f32 * SPACING;
    let generation = ((distance - absolute - 30.) / LOOP).ceil() as i32;
    (generation, distance - absolute - generation as f32 * LOOP)
}
fn mix(mut n: u32) -> u32 {
    n ^= n >> 16;
    n = n.wrapping_mul(0x7feb352d);
    n ^= n >> 15;
    n = n.wrapping_mul(0x846ca68b);
    n ^ (n >> 16)
}
fn landmarks(
    mut commands: Commands,
    g: Res<Game>,
    assets: Res<SceneryAssets>,
    mut query: Query<(Entity, &mut Landmark, &mut Transform, &mut Visibility)>,
) {
    if assets.landmarks.is_empty() {
        return;
    }
    for (entity, mut landmark, mut transform, mut visibility) in &mut query {
        let (generation, z) = landmark_position(landmark.slot, g.distance);
        transform.translation.z = z;
        if landmark.generation == generation {
            continue;
        }
        landmark.generation = generation;
        let seed = mix(landmark
            .slot
            .wrapping_add((generation as u32).wrapping_mul(LANDMARK_SLOTS))
            .wrapping_mul(2)
            .wrapping_add(u32::from(landmark.side > 0.)));
        commands.entity(entity).despawn_children();
        // Empty bays are deliberate: both open space and silhouettes need room.
        // Guarantee two asymmetric landmark beats per stretch without filling
        // every bay: radar at 72m left, reactor at 180m right.
        let featured = if landmark.slot == 2 && landmark.side < 0. {
            Some(0)
        } else if landmark.slot == 5 && landmark.side > 0. {
            Some(1)
        } else {
            None
        };
        if featured.is_none() && seed % 7 < 3 {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Inherited;
        let index = featured.unwrap_or((seed / 7) as usize % assets.landmarks.len());
        let Some(model) = &assets.landmarks[index] else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let height = match index {
            0 => 7.5,
            1 => 11.,
            2 => 3.5,
            _ => 1.5,
        };
        let scale = height / model.size.y;
        // Use the horizontal bounding radius, so every yaw remains clear of the road.
        let radius = model.size.x.hypot(model.size.z) * scale * 0.5;
        transform.translation.x = landmark.side * (7.5 + radius + (seed % 3) as f32 * 2.);
        transform.rotation = Quat::from_rotation_y(if landmark.side > 0. {
            -std::f32::consts::FRAC_PI_2
        } else {
            std::f32::consts::FRAC_PI_2
        });
        // The machinery stands on a side bay joined to the causeway edge.
        // Cancel the landmark yaw for the deck so its bridge always runs across X.
        let outer = transform.translation.x.abs() + radius + 0.8;
        let deck_width = outer - 5.25;
        let deck_center = landmark.side * (outer + 5.25) * 0.5;
        let inverse = transform.rotation.inverse();
        let deck_local = inverse * Vec3::new(deck_center - transform.translation.x, -0.32, 0.);
        let deck_depth = model.size.x * scale + 2.;
        commands.entity(entity).with_children(|c| {
            c.spawn((
                Mesh3d(assets.foundation.clone()),
                MeshMaterial3d(assets.alloy.clone()),
                Transform::from_translation(deck_local)
                    .with_rotation(inverse)
                    .with_scale(Vec3::new(deck_width, 0.64, deck_depth)),
            ));
            c.spawn((
                WorldAssetRoot(model.scene.clone()),
                Transform::from_xyz(
                    -model.center.x * scale,
                    (model.size.y * 0.5 - model.center.y) * scale,
                    -model.center.z * scale,
                )
                .with_scale(Vec3::splat(scale)),
            ));
        });
    }
}

fn ship_transform(lane: usize, member: usize, time: f32) -> Transform {
    // Horizontal lanes cross the sky above firing lines. Recycling happens >250 m
    // sideways, outside the widest gameplay view, even for the capital ship.
    let (speed, y, z, phase, direction) = match lane {
        0 => (34., 14., -95., 0.42, 1.),
        1 => (12., 24., -145., 0.32, -1.),
        2 => (47., 20., -125., 0.03, -1.),
        _ => (5.5, 36., -180., 0.51, 1.),
    };
    let x = ((time * speed + phase * 640.).rem_euclid(640.) - 320.) * direction;
    let formation = if member == 0 {
        Vec3::ZERO
    } else {
        Vec3::new(-direction * 11., -1.5, if member == 1 { -8. } else { 8. })
    };
    Transform::from_translation(Vec3::new(x, y, z) + formation).with_rotation(
        Quat::from_rotation_y(direction * std::f32::consts::FRAC_PI_2),
    )
}
fn traffic(g: Res<Game>, mut query: Query<(&Ship, &mut Transform)>) {
    for (ship, mut transform) in &mut query {
        *transform = ship_transform(ship.lane, ship.member, g.time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn landmarks_recycle_only_behind_camera_into_distant_fog() {
        let (_, before) = landmark_position(0, 29.9);
        let (generation, after) = landmark_position(0, 30.1);
        assert!(before > 29.);
        assert_eq!(generation, 1);
        assert!(after < -400.);
        for distance in [0., 300., 1000., 10000.] {
            for slot in 0..LANDMARK_SLOTS {
                let (_, z) = landmark_position(slot, distance);
                assert!((-402.1..=30.1).contains(&z));
            }
        }
    }
    #[test]
    fn traffic_stays_above_combat_and_wraps_outside_view() {
        for lane in 0..4 {
            for frame in 0..3600 {
                let a = ship_transform(lane, 0, frame as f32 / 30.).translation;
                let b = ship_transform(lane, 0, (frame + 1) as f32 / 30.).translation;
                assert!(a.y >= 14. && a.z <= -95.);
                if a.distance(b) > 5. {
                    assert!(a.x.abs() > 310. && b.x.abs() > 310.);
                }
            }
        }
    }
}
