//! A scrolling orbital causeway and runtime composition of the delivered characters.
use crate::{
    Options, Set,
    sim::{EnemyKind, FxKind, Game, Phase},
};
use bevy::{
    animation::AnimationTargetId, app::AnimationSystems, gltf::Gltf, post_process::bloom::Bloom,
    prelude::*, transform::TransformSystems, world_serialization::WorldInstanceReady,
};
use std::collections::HashMap;

pub struct ArtPlugin;
#[derive(Resource, Default)]
pub struct Ready(pub bool);
#[derive(Resource)]
struct Art {
    drone: Handle<bevy::world_serialization::WorldAsset>,
    reload_clip: Handle<AnimationClip>,
    cargo: Option<Handle<bevy::world_serialization::WorldAsset>>,
    station: Option<Handle<bevy::world_serialization::WorldAsset>>,
    cargo_size: Vec3,
    station_size: Vec3,
    cube: Handle<Mesh>,
    sphere: Handle<Mesh>,
    ring: Handle<Mesh>,
    metal: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
    white: Handle<StandardMaterial>,
    cyan: Handle<StandardMaterial>,
    orange: Handle<StandardMaterial>,
    red: Handle<StandardMaterial>,
    player: Handle<Gltf>,
    enemy: Handle<Gltf>,
    ground: serde_json::Value,
    manifest: serde_json::Value,
    attachment: serde_json::Value,
}
#[derive(Resource, Default)]
struct Instances {
    actors: HashMap<u64, Entity>,
    barriers: HashMap<u64, Entity>,
    pickups: HashMap<u64, Entity>,
    player: Option<Entity>,
    player_ready: bool,
}
#[derive(Component)]
struct Segment(f32);
#[derive(Component)]
struct Actor {
    player: bool,
}
#[derive(Component)]
struct Animated {
    root: Entity,
    player: bool,
    run: AnimationNodeIndex,
    aim: AnimationNodeIndex,
    shot: AnimationNodeIndex,
    last_shots: u32,
    reload: AnimationNodeIndex,
}
#[derive(Component)]
struct ActorVisual;
#[derive(Component)]
pub struct GameCamera;
#[derive(Component)]
struct EnemyBar(u64);
#[derive(Component)]
struct ReactorRing(f32);
fn read(p: std::path::PathBuf) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}
fn f(v: &serde_json::Value) -> f32 {
    v.as_f64().unwrap() as f32
}
fn v3(v: &serde_json::Value) -> Vec3 {
    Vec3::new(f(&v[0]), f(&v[1]), f(&v[2]))
}
fn q(v: &serde_json::Value) -> Quat {
    Quat::from_xyzw(f(&v[0]), f(&v[1]), f(&v[2]), f(&v[3]))
}
fn mat(m: &mut Assets<StandardMaterial>, color: Color, glow: f32) -> Handle<StandardMaterial> {
    m.add(StandardMaterial {
        base_color: color,
        metallic: 0.35,
        perceptual_roughness: 0.65,
        emissive: LinearRgba::from(color) * glow,
        ..default()
    })
}
fn child_box(
    c: &mut ChildSpawnerCommands,
    a: &Art,
    pos: Vec3,
    size: Vec3,
    m: &Handle<StandardMaterial>,
) {
    c.spawn((
        Mesh3d(a.cube.clone()),
        MeshMaterial3d(m.clone()),
        Transform::from_translation(pos).with_scale(size),
    ));
}
impl Plugin for ArtPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Ready>()
            .init_resource::<Instances>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (spawn_player, sync, camera, reactor_motion)
                    .chain()
                    .in_set(Set::Present),
            )
            .add_systems(
                PostUpdate,
                animate
                    .after(AnimationSystems)
                    .before(TransformSystems::Propagate),
            )
            .add_observer(actor_ready);
    }
}
fn setup(
    mut commands: Commands,
    server: Res<AssetServer>,
    options: Res<Options>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut game: ResMut<Game>,
) {
    let library = read(options.root.join("showcase/library.json"));
    let size = |name: &str, fallback: Vec3| {
        let entry = library["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["name"] == name);
        entry
            .map(|entry| v3(&entry["bounds_m"][1]) - v3(&entry["bounds_m"][0]))
            .unwrap_or(fallback)
    };
    let a = Art {
        drone: server.load(GltfAssetLabel::Scene(0).from_asset("showcase/models/relay_drone.glb")),
        reload_clip: server
            .load(GltfAssetLabel::Animation(0).from_asset("showcase/clips/relay_reload.glb")),
        cargo: options
            .root
            .join("showcase/models/relay_cargo.glb")
            .exists()
            .then(|| {
                server.load(GltfAssetLabel::Scene(0).from_asset("showcase/models/relay_cargo.glb"))
            }),
        station: options
            .root
            .join("showcase/models/relay_station.glb")
            .exists()
            .then(|| {
                server
                    .load(GltfAssetLabel::Scene(0).from_asset("showcase/models/relay_station.glb"))
            }),
        cargo_size: size("relay_cargo", Vec3::new(3., 0.76, 1.2)),
        station_size: size("relay_station", Vec3::new(2.4, 2.3, 1.2)),
        cube: meshes.add(Cuboid::default()),
        sphere: meshes.add(Sphere::new(1.).mesh().ico(2).unwrap()),
        ring: meshes.add(Torus::new(0.92, 1.0)),
        metal: mat(&mut materials, Color::srgb(0.36, 0.43, 0.5), 0.),
        dark: mat(&mut materials, Color::srgb(0.045, 0.065, 0.105), 0.),
        white: mat(&mut materials, Color::srgb(0.72, 0.78, 0.82), 0.),
        cyan: mat(&mut materials, Color::srgb(0.08, 0.72, 0.96), 4.),
        orange: mat(&mut materials, Color::srgb(1., 0.3, 0.065), 4.),
        red: mat(&mut materials, Color::srgb(0.9, 0.045, 0.02), 3.),
        player: server.load("scavenger.glb"),
        enemy: server.load("rusher.glb"),
        ground: read(options.root.join("grounding.json")),
        manifest: read(options.root.join("scavenger/library.json")),
        attachment: read(options.root.join("attachment.json")),
    };
    game.barrier_widths = [
        a.cargo_size.x * 0.76 / a.cargo_size.y,
        a.station_size.x * 2.3 / a.station_size.y,
    ];
    game.barrier_depths = [
        a.cargo_size.z * 0.76 / a.cargo_size.y,
        a.station_size.z * 2.3 / a.station_size.y,
    ];
    commands.spawn((
        Camera3d::default(),
        GameCamera,
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.014, 0.025, 0.055)),
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            fov: 66f32.to_radians(),
            far: 1200.,
            ..default()
        }),
        Bloom {
            intensity: 0.20,
            ..Bloom::NATURAL
        },
        DistanceFog {
            color: Color::srgb(0.032, 0.065, 0.125),
            directional_light_color: Color::srgb(0.35, 0.11, 0.035),
            directional_light_exponent: 24.,
            falloff: FogFalloff::Linear {
                start: 55.,
                end: 240.,
            },
        },
        Transform::from_xyz(0.65, 2.25, 3.8).looking_at(Vec3::new(0.65, 1.4, -25.), Vec3::Y),
    ));
    for i in 0..14 {
        commands
            .spawn((
                Segment(i as f32 * 20.),
                Transform::from_xyz(0., 0., -i as f32 * 20.),
                Visibility::default(),
            ))
            .with_children(|c| {
                child_box(
                    c,
                    &a,
                    Vec3::new(0., -0.26, 0.),
                    Vec3::new(10.8, 0.5, 19.85),
                    &a.metal,
                );
                for x in [-3.5, 0., 3.5] {
                    child_box(
                        c,
                        &a,
                        Vec3::new(x, 0.005, 0.),
                        Vec3::new(3.3, 0.012, 19.2),
                        &a.dark,
                    );
                    for z in [-8., -4., 0., 4., 8.] {
                        for side in [-1., 1.] {
                            child_box(
                                c,
                                &a,
                                Vec3::new(x + side * 1.4, 0.03, z + 0.4),
                                Vec3::new(0.07, 0.02, 0.65),
                                &a.white,
                            );
                        }
                        child_box(
                            c,
                            &a,
                            Vec3::new(x, 0.017, z),
                            Vec3::new(3.15, 0.015, 0.045),
                            &a.metal,
                        );
                    }
                }
                for side in [-1., 1.] {
                    if let Some(station) = &a.station {
                        c.spawn((
                            WorldAssetRoot(station.clone()),
                            Transform::from_xyz(side * 6.5, 0., -5.).with_rotation(
                                Quat::from_rotation_y(-side * std::f32::consts::FRAC_PI_2),
                            ),
                        ));
                    } else {
                        child_box(
                            c,
                            &a,
                            Vec3::new(side * 6.5, 1.6, -5.),
                            Vec3::new(0.8, 3.2, 1.2),
                            &a.metal,
                        );
                    }
                    child_box(
                        c,
                        &a,
                        Vec3::new(side * 5.2, 0.18, 0.),
                        Vec3::new(0.25, 0.36, 20.),
                        &a.white,
                    );
                    child_box(
                        c,
                        &a,
                        Vec3::new(side * 4.91, 0.04, 0.),
                        Vec3::new(0.045, 0.04, 18.),
                        &a.cyan,
                    );
                    if i % 2 == 0 {
                        c.spawn((
                            crate::lighting::Practical,
                            PointLight {
                                intensity: 22000.,
                                color: Color::srgb(1., 0.22, 0.055),
                                range: 10.,
                                radius: 0.45,
                                shadow_maps_enabled: false,
                                ..default()
                            },
                            Transform::from_xyz(side * 5.2, 1.5, 0.),
                        ));
                    }
                    for z in [-7., 0., 7.] {
                        child_box(
                            c,
                            &a,
                            Vec3::new(side * 5.4, 0.6, z),
                            Vec3::new(0.3, 1.2, 0.4),
                            &a.metal,
                        );
                        child_box(
                            c,
                            &a,
                            Vec3::new(side * 5.4, 1.25, z),
                            Vec3::new(0.35, 0.1, 0.55),
                            &a.orange,
                        );
                    }
                    if i % 4 == 0 {
                        child_box(
                            c,
                            &a,
                            Vec3::new(side * 6.1, 4., 0.),
                            Vec3::new(0.8, 8., 1.4),
                            &a.white,
                        );
                        child_box(
                            c,
                            &a,
                            Vec3::new(side * 5.65, 4., -0.1),
                            Vec3::new(0.04, 6.5, 0.4),
                            &a.cyan,
                        );
                        c.spawn((
                            Mesh3d(a.cube.clone()),
                            MeshMaterial3d(a.metal.clone()),
                            Transform::from_xyz(side * 4.4, 8.3, 0.)
                                .with_rotation(Quat::from_rotation_z(side * 0.6))
                                .with_scale(Vec3::new(0.9, 4.7, 1.1)),
                        ));
                    }
                }
                if i % 4 == 0 {
                    child_box(
                        c,
                        &a,
                        Vec3::new(0., 10.15, 0.),
                        Vec3::new(7., 0.7, 1.2),
                        &a.white,
                    );
                    child_box(
                        c,
                        &a,
                        Vec3::new(0., 9.76, -0.1),
                        Vec3::new(5.2, 0.04, 0.55),
                        &a.cyan,
                    );
                }
            });
    }
    // A purpose-built orbital accelerator gives the road a destination and scale.
    let alloy = materials.add(StandardMaterial {
        base_color: Color::srgb(0.055, 0.09, 0.14),
        metallic: 0.8,
        perceptual_roughness: 0.35,
        fog_enabled: false,
        ..default()
    });
    let energy = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.65, 0.95),
        emissive: LinearRgba::new(1., 24., 48., 1.),
        unlit: false,
        fog_enabled: false,
        ..default()
    });
    for layer in 0..3 {
        let radius = 42. + layer as f32 * 12.;
        commands
            .spawn((
                ReactorRing(if layer % 2 == 0 { 0.015 } else { -0.012 }),
                Transform::from_xyz(-16., 32., -195. - layer as f32 * 14.),
                Visibility::default(),
            ))
            .with_children(|c| {
                for i in 0..56 {
                    if (i + layer * 5) % 19 < 3 {
                        continue;
                    }
                    let angle = i as f32 * std::f32::consts::TAU / 56.;
                    let pos = Vec3::new(angle.cos() * radius, angle.sin() * radius, 0.);
                    c.spawn((
                        Mesh3d(a.cube.clone()),
                        MeshMaterial3d(alloy.clone()),
                        Transform::from_translation(pos)
                            .with_rotation(Quat::from_rotation_z(angle))
                            .with_scale(Vec3::new(3.8, radius * 0.095, 5.)),
                    ));
                    c.spawn((
                        Mesh3d(a.cube.clone()),
                        MeshMaterial3d(energy.clone()),
                        Transform::from_translation(pos + Vec3::Z * 2.6)
                            .with_rotation(Quat::from_rotation_z(angle))
                            .with_scale(Vec3::new(0.25, radius * 0.08, 0.1)),
                    ));
                    if i % 7 == 0 {
                        c.spawn((
                            Mesh3d(a.cube.clone()),
                            MeshMaterial3d(alloy.clone()),
                            Transform::from_translation(pos * 1.1)
                                .with_rotation(Quat::from_rotation_z(angle))
                                .with_scale(Vec3::new(13., 2., 8.)),
                        ));
                    }
                }
            });
    }
    // Narrow energy spine and asymmetric supporting towers, clear of the play lane.
    for side in [-1., 1.] {
        commands.spawn((
            Mesh3d(a.cube.clone()),
            MeshMaterial3d(alloy.clone()),
            Transform::from_xyz(-16. + side * 53., -9., -221.)
                .with_rotation(Quat::from_rotation_z(side * 0.18))
                .with_scale(Vec3::new(9., 90., 18.)),
        ));
        commands.spawn((
            Mesh3d(a.cube.clone()),
            MeshMaterial3d(energy.clone()),
            Transform::from_xyz(-16. + side * 53., 6., -210.).with_scale(Vec3::new(0.6, 52., 0.5)),
        ));
    }
    // The planet and orbital debris are distant set dressing, never collision surfaces.
    let planet = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::new(0.002, 0.006, 0.01, 1.),
        perceptual_roughness: 1.,
        fog_enabled: false,
        ..default()
    });
    let mut planet_mesh = Sphere::new(86.).mesh().uv(192, 128);
    if let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
        planet_mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    {
        let colors: Vec<[f32; 4]> = positions
            .iter()
            .map(|p| {
                let n = Vec3::from_array(*p).normalize();
                let latitude = n.y.asin();
                let longitude = n.z.atan2(n.x);
                let curl = (longitude * 7. + latitude * 14.).sin() * 0.035
                    + (longitude * 17. - latitude * 9.).sin() * 0.012;
                let band = ((latitude + curl) * 38.).sin() * 0.5 + 0.5;
                let cloud = ((latitude + curl * 0.6) * 97.).sin() * 0.5 + 0.5;
                let warm = (latitude * 8. + longitude.sin() * 0.2)
                    .cos()
                    .max(0.)
                    .powi(6);
                [
                    0.055 + band * 0.11 + cloud * 0.07 + warm * 0.28,
                    0.14 + band * 0.20 + cloud * 0.09 + warm * 0.15,
                    0.25 + band * 0.23 + cloud * 0.10,
                    1.,
                ]
            })
            .collect();
        planet_mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    }
    commands.spawn((
        Mesh3d(meshes.add(planet_mesh)),
        MeshMaterial3d(planet),
        Transform::from_xyz(120., 105., -340.),
    ));
    let ring = materials.add(StandardMaterial {
        base_color: Color::srgb(0.34, 0.54, 0.61),
        emissive: LinearRgba::new(0.12, 0.18, 0.22, 1.),
        unlit: true,
        fog_enabled: false,
        ..default()
    });
    for band in 0..7 {
        commands.spawn((
            Mesh3d(meshes.add(Torus::new(
                104. + band as f32 * 2.4,
                105. + band as f32 * 2.4,
            ))),
            MeshMaterial3d(ring.clone()),
            Transform::from_xyz(120., 105., -340.).with_rotation(Quat::from_euler(
                EulerRot::XYZ,
                0.55,
                0.,
                0.35,
            )),
        ));
    }
    let stars = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::WHITE * 2.,
        unlit: true,
        fog_enabled: false,
        ..default()
    });
    for i in 0..140 {
        let x = ((i * 379 % 997) as f32 / 997. - 0.5) * 1000.;
        let y = (i * 173 % 571) as f32 * 0.7 + 12.;
        commands.spawn((
            Mesh3d(a.sphere.clone()),
            MeshMaterial3d(stars.clone()),
            Transform::from_xyz(x, y, -450. - (i % 5) as f32 * 60.)
                .with_scale(Vec3::splat(0.17 + (i % 3) as f32 * 0.1)),
        ));
    }
    for i in 0..9 {
        let side = if i % 4 == 0 { -1. } else { 1. };
        let z = -45. - i as f32 * 24.;
        let position = Vec3::new(side * (18. + (i % 3) as f32 * 9.), -8., z);
        if let Some(station) = &a.station {
            commands.spawn((
                WorldAssetRoot(station.clone()),
                Transform::from_translation(position).with_scale(Vec3::splat(5. + (i % 3) as f32)),
            ));
        } else {
            commands.spawn((
                Mesh3d(a.cube.clone()),
                MeshMaterial3d(a.dark.clone()),
                Transform::from_translation(position).with_scale(Vec3::new(
                    7.,
                    35. + i as f32 * 3.,
                    9.,
                )),
            ));
        }
    }
    commands.insert_resource(a);
}
fn spawn_player(
    mut commands: Commands,
    a: Res<Art>,
    gltfs: Res<Assets<Gltf>>,
    server: Res<AssetServer>,
    mut instances: ResMut<Instances>,
    mut ready: ResMut<Ready>,
) {
    if instances.player.is_none()
        && server.is_loaded_with_dependencies(&a.player)
        && server.is_loaded_with_dependencies(&a.enemy)
        && server.is_loaded_with_dependencies(&a.drone)
        && server.is_loaded_with_dependencies(&a.reload_clip)
        && a.cargo
            .as_ref()
            .is_none_or(|h| server.is_loaded_with_dependencies(h))
        && a.station
            .as_ref()
            .is_none_or(|h| server.is_loaded_with_dependencies(h))
    {
        let scene = gltfs.get(&a.player).unwrap().scenes[0].clone();
        let root = commands
            .spawn((
                Actor { player: true },
                WorldAssetRoot(scene),
                Transform::default(),
                ActorVisual,
            ))
            .id();
        instances.player = Some(root);
    }
    ready.0 = instances.player_ready;
}
fn actor_ready(
    event: On<WorldInstanceReady>,
    mut commands: Commands,
    actors: Query<&Actor>,
    children: Query<&Children>,
    targets: Query<(&Name, &AnimationTargetId)>,
    mut players: Query<&mut AnimationPlayer>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    a: Res<Art>,
    server: Res<AssetServer>,
    mut instances: ResMut<Instances>,
) {
    let Ok(actor) = actors.get(event.entity) else {
        return;
    };
    let g = gltfs
        .get(if actor.player { &a.player } else { &a.enemy })
        .unwrap();
    let mut graph = AnimationGraph::new();
    let (run, aim, shot) = if actor.player {
        // Masks exclude lower bones from aim/fire and upper bones from locomotion.
        for child in children.iter_descendants(event.entity) {
            if let Ok((name, id)) = targets.get(child) {
                let lower = name.as_str() == "Hips"
                    || ["UpLeg", "Leg", "Foot", "Toe"]
                        .iter()
                        .any(|part| name.contains(part));
                graph.add_target_to_mask_group(*id, if lower { 0 } else { 1 });
            }
        }
        let root = graph.root;
        (
            graph.add_clip_with_mask(
                g.named_animations["scrapyard_rifle_run-loop"].clone(),
                2,
                1.,
                root,
            ),
            graph.add_clip_with_mask(
                g.named_animations["scrapyard_rifle_aim-loop"].clone(),
                1,
                1.,
                root,
            ),
            graph.add_clip_with_mask(
                g.named_animations["scrapyard_rifle_fire"].clone(),
                1,
                1.,
                root,
            ),
        )
    } else {
        let root = graph.root;
        let node = graph.add_clip(g.named_animations["release_run-loop"].clone(), 1., root);
        (node, node, node)
    };
    let reload = if actor.player {
        graph.add_clip_with_mask(a.reload_clip.clone(), 1, 0., graph.root)
    } else {
        run
    };
    let handle = graphs.add(graph);
    for child in children.iter_descendants(event.entity) {
        if let Ok(mut player) = players.get_mut(child) {
            player.play(run).repeat();
            if actor.player {
                player.play(aim).repeat();
            }
            commands.entity(child).insert((
                AnimationGraphHandle(handle.clone()),
                Animated {
                    root: event.entity,
                    player: actor.player,
                    run,
                    aim,
                    shot,
                    last_shots: 0,
                    reload,
                },
            ));
        }
        if actor.player
            && let Ok((name, _)) = targets.get(child)
        {
            let socket = a.manifest["rig"]["sockets"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["name"] == a.attachment["socket"])
                .unwrap();
            if name.as_str() == socket["bone"].as_str().unwrap() {
                let local = Transform::from_translation(v3(&socket["translation"]))
                    .with_rotation(q(&socket["rotation"]))
                    .mul_transform(
                        Transform::from_translation(v3(&a.attachment["translation"]))
                            .with_rotation(q(&a.attachment["rotation_xyzw"])),
                    );
                commands.entity(child).with_children(|c| {
                    c.spawn((
                        WorldAssetRoot(
                            server.load(
                                GltfAssetLabel::Scene(0)
                                    .from_asset("scavenger/models/scrapyard_rifle.glb"),
                            ),
                        ),
                        local,
                    ));
                });
            }
        }
    }
    if actor.player {
        instances.player_ready = true;
    }
}
fn sync(
    mut commands: Commands,
    g: Res<Game>,
    a: Res<Art>,
    gltfs: Res<Assets<Gltf>>,
    mut state: ResMut<Instances>,
    mut transforms: Query<&mut Transform, Without<EnemyBar>>,
    mut bars: Query<(&EnemyBar, &mut Visibility, &mut Transform)>,
) {
    for (bar, mut visible, mut transform) in &mut bars {
        if let Some(enemy) = g.enemies.iter().find(|e| e.id == bar.0) {
            transform.scale.x = 0.65 * (enemy.hp / enemy.max_hp).clamp(0., 1.);
        }
        *visible = if g.enemies.iter().any(|e| e.id == bar.0 && e.pos.z > -45.) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if let Some(root) = state.player
        && let Ok(mut t) = transforms.get_mut(root)
    {
        t.translation = Vec3::new(g.x, g.y, 0.);
        let yaw = g.aim.x.atan2(-g.aim.z);
        t.rotation = Quat::from_rotation_y(-yaw.clamp(-0.7, 0.7));
    }
    for e in &g.enemies {
        let entity = *state.actors.entry(e.id).or_insert_with(|| {
            let root = if e.kind == EnemyKind::Rusher {
                let scene = gltfs.get(&a.enemy).unwrap().scenes[0].clone();
                commands
                    .spawn((
                        Actor { player: false },
                        WorldAssetRoot(scene),
                        ActorVisual,
                        Transform::default(),
                    ))
                    .id()
            } else {
                let root = commands
                    .spawn((Transform::default(), Visibility::default()))
                    .id();
                commands.entity(root).with_children(|c| {
                    c.spawn((
                        WorldAssetRoot(a.drone.clone()),
                        // The generated body is pitched nose-down in its local frame.
                        // Level its visual mount; simulation and projectile origins stay upright.
                        Transform::from_xyz(0., 1.25, 0.)
                            .with_rotation(Quat::from_rotation_x(-25_f32.to_radians())),
                    ));
                });
                root
            };
            commands.entity(root).with_children(|c| {
                c.spawn((
                    EnemyBar(e.id),
                    Mesh3d(a.cube.clone()),
                    MeshMaterial3d(if e.kind == EnemyKind::Rusher {
                        a.orange.clone()
                    } else {
                        a.red.clone()
                    }),
                    Transform::from_xyz(0., 2.15, 0.).with_scale(Vec3::new(0.65, 0.045, 0.06)),
                ));
            });
            root
        });
        let rotation = match e.kind {
            EnemyKind::Rusher => {
                Quat::from_rotation_y(std::f32::consts::PI) * Quat::from_rotation_x(-e.flash * 0.8)
            }
            EnemyKind::Trooper => Quat::from_rotation_z(e.flash * 0.25),
        };
        commands
            .entity(entity)
            .insert(Transform::from_translation(e.pos).with_rotation(rotation));
    }
    let gone: Vec<_> = state
        .actors
        .keys()
        .filter(|id| !g.enemies.iter().any(|e| &e.id == *id))
        .copied()
        .collect();
    for id in gone {
        commands.entity(state.actors.remove(&id).unwrap()).despawn();
    }
    for b in &g.barriers {
        let root = *state.barriers.entry(b.id).or_insert_with(|| {
            let root = commands
                .spawn((Transform::default(), Visibility::default()))
                .id();
            commands.entity(root).with_children(|c| {
                let (model, size) = (&a.station, a.station_size);
                let height = 2.3;
                if let Some(model) = model {
                    c.spawn((
                        WorldAssetRoot(model.clone()),
                        Transform::from_scale(Vec3::splat(height / size.y)),
                    ));
                } else {
                    child_box(
                        c,
                        &a,
                        Vec3::Y * height * 0.5,
                        Vec3::new(b.width, height, 1.2),
                        &a.metal,
                    );
                    child_box(
                        c,
                        &a,
                        Vec3::new(0., height * 0.8, 0.61),
                        Vec3::new(b.width * 0.8, 0.08, 0.03),
                        &a.orange,
                    );
                }
                child_box(
                    c,
                    &a,
                    Vec3::new(0., 0.025, b.depth * 0.5 + 0.25),
                    Vec3::new(b.width + 0.3, 0.03, 0.12),
                    &a.red,
                );
            });
            root
        });
        commands
            .entity(root)
            .insert(Transform::from_xyz(b.x, 0., b.z));
    }
    let gone: Vec<_> = state
        .barriers
        .keys()
        .filter(|id| !g.barriers.iter().any(|b| &b.id == *id))
        .copied()
        .collect();
    for id in gone {
        commands
            .entity(state.barriers.remove(&id).unwrap())
            .despawn();
    }
    for p in &g.pickups {
        let root = *state.pickups.entry(p.id).or_insert_with(|| {
            let root = commands
                .spawn((Transform::default(), Visibility::default()))
                .id();
            commands.entity(root).with_children(|c| {
                if let Some(cargo) = &a.cargo {
                    c.spawn((
                        WorldAssetRoot(cargo.clone()),
                        Transform::from_xyz(0., -0.25, 0.).with_scale(Vec3::splat(0.65)),
                    ));
                } else {
                    child_box(c, &a, Vec3::ZERO, Vec3::splat(0.4), &a.cyan);
                }
                c.spawn((
                    Mesh3d(a.ring.clone()),
                    MeshMaterial3d(a.cyan.clone()),
                    Transform::from_xyz(0., -0.72, 0.).with_scale(Vec3::splat(0.65)),
                ));
                child_box(
                    c,
                    &a,
                    Vec3::new(0., 0.48, 0.),
                    Vec3::new(0.09, 0.32, 0.05),
                    &a.cyan,
                );
                child_box(
                    c,
                    &a,
                    Vec3::new(0., 0.48, 0.),
                    Vec3::new(0.32, 0.09, 0.05),
                    &a.cyan,
                );
            });
            root
        });
        commands
            .entity(root)
            .insert(Transform::from_translation(p.pos));
    }

    let gone: Vec<_> = state
        .pickups
        .keys()
        .filter(|id| !g.pickups.iter().any(|p| &p.id == *id))
        .copied()
        .collect();
    for id in gone {
        commands
            .entity(state.pickups.remove(&id).unwrap())
            .despawn();
    }
}

fn camera(
    g: Res<Game>,
    time: Res<Time>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<GameCamera>>,
    mut segments: Query<(&Segment, &mut Transform), Without<GameCamera>>,
) {
    for (segment, mut t) in &mut segments {
        t.translation.z = 20. - (segment.0 - g.distance).rem_euclid(280.);
    }
    for (mut t, mut projection) in &mut cameras {
        let aim = if g.phase == Phase::Title {
            Vec3::new(-0.07, 0.04, -1.).normalize()
        } else {
            g.aim
        };
        let mut pos = g.camera();
        pos.y += (g.time * 14.).sin() * 0.018;
        if g.hurt > 0. {
            pos.x += (g.time * 80.).sin() * g.hurt * 0.06;
        }
        let punch = g
            .effects
            .iter()
            .filter(|f| f.kind == FxKind::Shot)
            .map(|f| (1. - f.age / f.life).max(0.))
            .fold(0_f32, f32::max);
        pos.z += punch * 0.055 + g.impact * 0.13;
        pos.y += (g.time * 71.).sin() * g.impact * 0.045;
        pos.x += (g.time * 93.).cos() * g.impact * 0.045;
        let desired = Transform::from_translation(pos).looking_to(aim, Vec3::Y);
        let alpha = 1. - (-time.delta_secs() * 14.).exp();
        t.translation = desired.translation;
        t.rotation = desired.rotation;
        if let Projection::Perspective(p) = &mut *projection {
            let target = (if g.focus { 48f32 } else { 66f32 }
                + if g.overdrive > 0. { 4. } else { 0. }
                + g.nova_flash * 7.)
                .to_radians();
            p.fov += (target - p.fov) * alpha;
        }
    }
}
fn reactor_motion(g: Res<Game>, mut rings: Query<(&ReactorRing, &mut Transform)>) {
    for (ring, mut t) in &mut rings {
        t.rotation = Quat::from_rotation_z(g.time * ring.0);
    }
}
fn ground(a: &Art, name: &str, time: f32) -> f32 {
    let row = a.ground["clips"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["animation"] == name)
        .unwrap();
    let samples = row["samples"].as_array().unwrap();
    let i = samples.partition_point(|s| f(&s["t"]) < time);
    if i == 0 {
        return f(&samples[0]["vertical_offset_m"]);
    }
    if i == samples.len() {
        return f(&samples[i - 1]["vertical_offset_m"]);
    }
    let (x, y) = (&samples[i - 1], &samples[i]);
    let u = (time - f(&x["t"])) / (f(&y["t"]) - f(&x["t"]));
    f(&x["vertical_offset_m"]) + (f(&y["vertical_offset_m"]) - f(&x["vertical_offset_m"])) * u
}
fn animate(
    g: Res<Game>,
    a: Res<Art>,
    mut players: Query<(&mut AnimationPlayer, &mut Animated)>,
    mut visuals: Query<&mut Transform, With<ActorVisual>>,
) {
    for (mut player, mut anim) in &mut players {
        let speed = if g.phase == Phase::Playing {
            if anim.player {
                g.speed() / 2.5583997
            } else {
                1.
            }
        } else {
            0.
        };
        if let Some(run) = player.animation_mut(anim.run) {
            run.set_speed(speed);
        }
        if anim.player {
            if g.shots != anim.last_shots {
                player.play(anim.shot).replay();
                anim.last_shots = g.shots;
            }
            let shooting = player
                .animation(anim.shot)
                .is_some_and(|a| !a.is_finished());
            let reload_weight = if g.reload > 0. {
                ((1.3 - g.reload) / 0.10).min(g.reload / 0.10).clamp(0., 1.)
            } else {
                0.
            };
            player
                .play(anim.reload)
                .set_speed(0.)
                .set_seek_time((1.3 - g.reload).clamp(0., 1.299))
                .set_weight(reload_weight);
            if let Some(aim) = player.animation_mut(anim.aim) {
                aim.set_weight((if shooting { 0. } else { 1. }) * (1. - reload_weight));
                aim.set_speed(speed);
            }
            if let Some(shot) = player.animation_mut(anim.shot) {
                shot.set_weight((if shooting { 1. } else { 0. }) * (1. - reload_weight));
                shot.set_speed(if g.phase == Phase::Playing { 1. } else { 0. });
            }
        }
        let seek = player.animation(anim.run).map_or(0., |a| a.seek_time());
        if let Ok(mut t) = visuals.get_mut(anim.root) {
            t.translation.y = if anim.player { g.y } else { 0. }
                + ground(
                    &a,
                    if anim.player {
                        "scrapyard_rifle_run-loop"
                    } else {
                        "release_run-loop"
                    },
                    seek,
                );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Barrier, Bolt, Enemy, Fx, Pickup};
    #[test]
    fn dynamic_objects_have_world_positions_on_their_first_frame() {
        let mut app = App::new();
        let mut game = Game::new();
        game.enemies = vec![Enemy {
            id: 1,
            pos: Vec3::new(2., 0., -40.),
            hp: 100.,
            max_hp: 100.,
            kind: EnemyKind::Trooper,
            fire_in: 1.,
            flash: 0.,
        }];
        game.barriers = vec![Barrier {
            id: 2,
            x: -2.,
            z: -50.,
            width: 2.,
            depth: 1.2,
        }];
        game.bolts = vec![Bolt {
            id: 3,
            pos: Vec3::new(1., 1., -20.),
            velocity: Vec3::Z,
        }];
        game.pickups = vec![Pickup {
            id: 4,
            pos: Vec3::new(3., 1., -30.),
        }];
        game.effects = vec![Fx {
            id: 5,
            kind: FxKind::Hit,
            from: Vec3::new(2., 1., -40.),
            to: Vec3::ZERO,
            age: 0.,
            life: 0.2,
        }];
        let art = Art {
            drone: default(),
            reload_clip: default(),
            cargo: default(),
            station: default(),
            cargo_size: Vec3::ONE,
            station_size: Vec3::ONE,
            cube: default(),
            sphere: default(),
            ring: default(),
            metal: default(),
            dark: default(),
            white: default(),
            cyan: default(),
            orange: default(),
            red: default(),
            player: default(),
            enemy: default(),
            ground: serde_json::Value::Null,
            manifest: serde_json::Value::Null,
            attachment: serde_json::Value::Null,
        };
        app.insert_resource(game)
            .insert_resource(art)
            .init_resource::<Instances>()
            .init_resource::<Assets<Gltf>>()
            .add_systems(Update, sync);
        app.update();
        let world = app.world();
        let instances = world.resource::<Instances>();
        for (entity, expected) in [
            (instances.actors[&1], Vec3::new(2., 0., -40.)),
            (instances.barriers[&2], Vec3::new(-2., 0., -50.)),
            (instances.pickups[&4], Vec3::new(3., 1., -30.)),
        ] {
            assert_eq!(
                world.get::<Transform>(entity).unwrap().translation,
                expected
            );
        }
    }
}
