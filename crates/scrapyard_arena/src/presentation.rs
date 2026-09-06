//! Arena art, generated characters, animation, and combat feedback.
use crate::{
    runtime::{Controls, GameSet},
    sim::{ARENA_HALF, EffectKind, EnemyKind, Phase, PickupKind, Sim},
};
use bevy::{
    camera::ScalingMode, image::ImageSampler, math::Affine2, post_process::bloom::Bloom,
    prelude::*, render::render_resource::TextureFormat, world_serialization::WorldInstanceReady,
};
use std::collections::{HashMap, HashSet};

pub(crate) struct PresentationPlugin;
#[derive(Component)]
struct GameCamera;
#[derive(Component)]
struct Actor;
#[derive(Component)]
struct Animated {
    id: u64,
    player: bool,
    graph: Handle<AnimationGraph>,
    nodes: Vec<AnimationNodeIndex>,
}
#[derive(Component)]
struct ActorAnimation {
    id: u64,
    visual: Entity,
    player: bool,
    nodes: Vec<AnimationNodeIndex>,
    current: usize,
}
#[derive(Resource)]
struct Art {
    cube: Handle<Mesh>,
    sphere: Handle<Mesh>,
    ring: Handle<Mesh>,
    quad: Handle<Mesh>,
    vfx: Vec<Vec<Handle<StandardMaterial>>>,
    vfx_specs: Vec<VfxSpec>,
    vfx_images: Vec<Handle<Image>>,
    dark: Handle<StandardMaterial>,
    steel: Handle<StandardMaterial>,
    rust: Handle<StandardMaterial>,
    teal: Handle<StandardMaterial>,
    amber: Handle<StandardMaterial>,
    red: Handle<StandardMaterial>,
    violet: Handle<StandardMaterial>,
    white: Handle<StandardMaterial>,
    player_scene: Handle<WorldAsset>,
    enemy_scene: Handle<WorldAsset>,
    magnet_scene: Handle<WorldAsset>,
    player_graph: Handle<AnimationGraph>,
    enemy_graph: Handle<AnimationGraph>,
    player_nodes: Vec<AnimationNodeIndex>,
    enemy_nodes: Vec<AnimationNodeIndex>,
}
#[derive(Resource, Default)]
struct Visuals {
    actors: HashMap<u64, Entity>,
    bullets: HashMap<u64, Entity>,
    pickups: HashMap<u64, Entity>,
    effects: HashMap<u64, Entity>,
}
#[derive(Component)]
struct Transient;
#[derive(Component)]
struct PickupModel {
    id: u64,
}
#[derive(serde::Deserialize)]
struct VfxPack {
    effects: Vec<VfxSpec>,
}
#[derive(serde::Deserialize)]
struct VfxSpec {
    path: String,
    frames: usize,
    fps: f32,
    pivot_normalized_top_left: [f32; 2],
    alpha_discard_below: f32,
}

impl Plugin for PresentationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Visuals>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    prepare_vfx_images,
                    sync,
                    spin_pickup_models,
                    animate,
                    camera,
                )
                    .chain()
                    .in_set(GameSet::Present),
            );
    }
}
fn material(
    materials: &mut Assets<StandardMaterial>,
    color: Color,
    glow: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        emissive: LinearRgba::from(color) * glow,
        metallic: if glow > 0. { 0.25 } else { 0.7 },
        perceptual_roughness: 0.64,
        ..default()
    })
}
fn box_at(
    commands: &mut Commands,
    art: &Art,
    pos: Vec3,
    size: Vec3,
    mat: &Handle<StandardMaterial>,
) {
    commands.spawn((
        Mesh3d(art.cube.clone()),
        MeshMaterial3d(mat.clone()),
        Transform::from_translation(pos).with_scale(size),
    ));
}
fn setup(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let args: Vec<String> = std::env::args().collect();
    let viewport_height = args
        .windows(2)
        .find(|pair| pair[0] == "--zoom")
        .and_then(|pair| pair[1].parse::<f32>().ok())
        .filter(|zoom| zoom.is_finite())
        .unwrap_or(22.)
        .clamp(4., 40.);
    let player_path = "designs/scrapyard/batch-02/armed-scavenger-combat.glb";
    let enemy_path = "designs/scrapyard/batch-02/rusher-combat.glb";
    let (pg, pn) = AnimationGraph::from_clips(
        (0..3).map(|i| server.load(GltfAssetLabel::Animation(i).from_asset(player_path))),
    );
    let (eg, en) = AnimationGraph::from_clips(
        (0..2).map(|i| server.load(GltfAssetLabel::Animation(i).from_asset(enemy_path))),
    );
    let vfx_pack: VfxPack = serde_json::from_str(include_str!(
        "../../../designs/scrapyard/batch-02/vfx/vfx.json"
    ))
    .expect("validated VFX metadata");
    let vfx_images: Vec<Handle<Image>> = vfx_pack
        .effects
        .iter()
        .map(|spec| server.load(format!("designs/scrapyard/batch-02/vfx/{}", spec.path)))
        .collect();
    let vfx = vfx_pack
        .effects
        .iter()
        .zip(&vfx_images)
        .map(|(spec, image)| {
            (0..spec.frames)
                .map(|frame| {
                    materials.add(StandardMaterial {
                        base_color_texture: Some(image.clone()),
                        unlit: true,
                        alpha_mode: AlphaMode::Blend,
                        cull_mode: None,
                        uv_transform: Affine2::from_scale_angle_translation(
                            Vec2::splat(0.5),
                            0.,
                            Vec2::new((frame % 2) as f32 * 0.5, (frame / 2) as f32 * 0.5),
                        ),
                        ..default()
                    })
                })
                .collect()
        })
        .collect();
    let art = Art {
        cube: meshes.add(Cuboid::default()),
        sphere: meshes.add(
            Sphere::new(1.)
                .mesh()
                .ico(2)
                .expect("two subdivisions form a valid icosphere"),
        ),
        ring: meshes.add(Torus::new(0.88, 1.0)),
        quad: meshes.add(Rectangle::new(1., 1.)),
        vfx,
        vfx_specs: vfx_pack.effects,
        vfx_images,
        dark: material(&mut materials, Color::srgb(0.055, 0.075, 0.083), 0.),
        steel: material(&mut materials, Color::srgb(0.095, 0.15, 0.20), 0.),
        rust: material(&mut materials, Color::srgb(0.37, 0.17, 0.075), 0.),
        teal: material(&mut materials, Color::srgb(0.13, 0.93, 0.77), 2.),
        amber: material(&mut materials, Color::srgb(1., 0.55, 0.09), 2.),
        red: material(&mut materials, Color::srgb(1., 0.095, 0.045), 2.),
        violet: material(&mut materials, Color::srgb(0.62, 0.28, 1.), 2.),
        white: material(&mut materials, Color::srgb(0.8, 0.94, 1.), 3.),
        magnet_scene: server
            .load(GltfAssetLabel::Scene(0).from_asset("assets/models/scrapyard_magnet.glb")),
        player_scene: server.load(GltfAssetLabel::Scene(0).from_asset(player_path)),
        enemy_scene: server.load(GltfAssetLabel::Scene(0).from_asset(enemy_path)),
        player_graph: graphs.add(pg),
        enemy_graph: graphs.add(eg),
        player_nodes: pn,
        enemy_nodes: en,
    };
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.53, 0.69, 0.78),
        brightness: 500.,
        ..default()
    });
    commands.spawn((
        GameCamera,
        Camera3d::default(),
        Bloom {
            intensity: 0.1,
            ..default()
        },
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical { viewport_height },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0., 28., 23.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 6500.,
            shadow_maps_enabled: true,
            color: Color::srgb(0.91, 0.94, 1.),
            ..default()
        },
        Transform::from_xyz(-15., 30., -12.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    box_at(
        &mut commands,
        &art,
        Vec3::new(0., -0.3, 0.),
        Vec3::new(70., 0.5, 70.),
        &art.dark,
    );
    // Modular steel plates and thin seams keep combat space visually quiet.
    for x in -5..6 {
        for z in -5..6 {
            let pos = Vec3::new(x as f32 * 4., -0.055, z as f32 * 4.);
            box_at(
                &mut commands,
                &art,
                pos,
                Vec3::new(3.94, 0.1, 3.94),
                &art.steel,
            );
        }
    }
    for i in -10..11 {
        let p = i as f32 * 2.;
        for s in [-1., 1.] {
            box_at(
                &mut commands,
                &art,
                Vec3::new(p, 0.015, s * 21.8),
                Vec3::new(0.75, 0.035, 0.15),
                &art.amber,
            );
            box_at(
                &mut commands,
                &art,
                Vec3::new(s * 21.8, 0.015, p),
                Vec3::new(0.15, 0.035, 0.75),
                &art.amber,
            );
        }
    }
    for z in [-12., 0., 12.] {
        box_at(
            &mut commands,
            &art,
            Vec3::new(0., 0.008, z),
            Vec3::new(28., 0.012, 0.055),
            &art.rust,
        );
        for x in [-12., 12.] {
            box_at(
                &mut commands,
                &art,
                Vec3::new(x, 0.014, z),
                Vec3::new(2.4, 0.015, 1.2),
                &art.dark,
            );
            for i in -4..5 {
                box_at(
                    &mut commands,
                    &art,
                    Vec3::new(x + i as f32 * 0.24, 0.025, z),
                    Vec3::new(0.075, 0.015, 1.0),
                    &art.steel,
                );
            }
        }
    }
    for radius in [3.8, 4.0] {
        commands.spawn((
            Mesh3d(art.ring.clone()),
            MeshMaterial3d(art.rust.clone()),
            Transform::from_xyz(0., 0.012, 0.).with_scale(Vec3::new(radius, 0.006, radius)),
        ));
    }
    // Low barricades surround the playable floor; higher scrap remains outside.
    for s in [-1., 1.] {
        for i in -5..6 {
            let p = i as f32 * 4.4;
            box_at(
                &mut commands,
                &art,
                Vec3::new(p, 0.45, s * 23.),
                Vec3::new(4.1, 0.9, 1.0),
                &art.dark,
            );
            box_at(
                &mut commands,
                &art,
                Vec3::new(s * 23., 0.45, p),
                Vec3::new(1., 0.9, 4.1),
                &art.dark,
            );
            box_at(
                &mut commands,
                &art,
                Vec3::new(p, 0.91, s * 23.),
                Vec3::new(3.4, 0.08, 0.3),
                &art.rust,
            );
            for j in 0_i32..3 {
                let a = (i * 13 + j * 7).unsigned_abs() as f32;
                let pos = Vec3::new(
                    p + (j as f32 - 1.) * 0.8,
                    0.8 + j as f32 * 0.45,
                    s * (26. + (a % 4.)),
                );
                box_at(
                    &mut commands,
                    &art,
                    pos,
                    Vec3::new(2.8, 1.3, 2.0),
                    if j % 2 == 0 { &art.rust } else { &art.dark },
                );
            }
        }
    }
    // Container stacks and machinery frame every edge of the yard.
    for side in [-1., 1.] {
        for i in -5..6 {
            let z = i as f32 * 5.;
            let height = 1.0 + ((i * 7_i32).unsigned_abs() % 4) as f32 * 0.4;
            box_at(
                &mut commands,
                &art,
                Vec3::new(side * 26., height * 0.5, z),
                Vec3::new(3.4, height, 4.4),
                if i % 2 == 0 { &art.rust } else { &art.steel },
            );
            for rib in -3..4 {
                box_at(
                    &mut commands,
                    &art,
                    Vec3::new(side * 24.27, height * 0.5, z + rib as f32 * 0.55),
                    Vec3::new(0.08, height * 0.9, 0.08),
                    &art.dark,
                );
            }
        }
    }
    // Floor markings stay flush: no invisible obstacles or noisy rubble underfoot.
    for x in [-18., 18.] {
        for z in [-16., -8., 0., 8., 16.] {
            box_at(
                &mut commands,
                &art,
                Vec3::new(x, 0.02, z),
                Vec3::new(1.8, 0.02, 0.12),
                &art.rust,
            );
            box_at(
                &mut commands,
                &art,
                Vec3::new(x - 0.84, 0.02, z + 0.7),
                Vec3::new(0.12, 0.02, 1.5),
                &art.rust,
            );
            box_at(
                &mut commands,
                &art,
                Vec3::new(x + 0.84, 0.02, z + 0.7),
                Vec3::new(0.12, 0.02, 1.5),
                &art.rust,
            );
        }
    }
    for x in -5..6 {
        for z in -5..6 {
            for side in [-1., 1.] {
                box_at(
                    &mut commands,
                    &art,
                    Vec3::new(x as f32 * 4. + side * 1.8, 0.008, z as f32 * 4. + 1.8),
                    Vec3::new(0.12, 0.012, 0.12),
                    &art.dark,
                );
            }
        }
    }
    for (x, z) in [(-20., -20.), (20., -20.), (-20., 20.), (20., 20.)] {
        box_at(
            &mut commands,
            &art,
            Vec3::new(x, 1.5, z),
            Vec3::new(0.28, 3., 0.28),
            &art.dark,
        );
        box_at(
            &mut commands,
            &art,
            Vec3::new(x, 3., z),
            Vec3::new(1.1, 0.15, 0.35),
            &art.teal,
        );
    }
    commands.insert_resource(
        serde_json::from_str::<Grounding>(include_str!(
            "../../../designs/scrapyard/batch-02/grounding.json"
        ))
        .expect("validated grounding data"),
    );
    commands.insert_resource(art);
}
fn spawn_actor(commands: &mut Commands, art: &Art, id: u64, kind: Option<EnemyKind>) -> Entity {
    let player = kind.is_none();
    let entity = commands
        .spawn((Actor, Transform::default(), Visibility::default()))
        .id();
    commands.entity(entity).with_children(|p| {
        if player || kind == Some(EnemyKind::Rusher) {
            p.spawn((
                WorldAssetRoot(if player {
                    art.player_scene.clone()
                } else {
                    art.enemy_scene.clone()
                }),
                Transform::default(),
                Animated {
                    id,
                    player,
                    graph: if player {
                        art.player_graph.clone()
                    } else {
                        art.enemy_graph.clone()
                    },
                    nodes: if player {
                        art.player_nodes.clone()
                    } else {
                        art.enemy_nodes.clone()
                    },
                },
            ))
            .observe(animation_ready);
        } else if let Some((scale, color)) = match kind {
            Some(EnemyKind::Shooter) => Some((Vec3::new(0.8, 0.7, 1.0), &art.amber)),
            Some(EnemyKind::Tank) => Some((Vec3::new(1.5, 1.3, 1.2), &art.red)),
            Some(EnemyKind::Bomber) => Some((Vec3::splat(0.8), &art.violet)),
            Some(EnemyKind::Boss) => Some((Vec3::new(2.8, 2.8, 2.2), &art.red)),
            None | Some(EnemyKind::Rusher) => None,
        } {
            let y = scale.y * 0.55;
            p.spawn((
                Mesh3d(if kind == Some(EnemyKind::Bomber) {
                    art.sphere.clone()
                } else {
                    art.cube.clone()
                }),
                MeshMaterial3d(art.dark.clone()),
                Transform::from_xyz(0., y, 0.).with_scale(scale),
            ));
            p.spawn((
                Mesh3d(art.cube.clone()),
                MeshMaterial3d(color.clone()),
                Transform::from_xyz(0., y + scale.y * 0.3, scale.z * 0.52).with_scale(Vec3::new(
                    scale.x * 0.65,
                    0.14,
                    0.12,
                )),
            ));
            for side in [-1., 1.] {
                p.spawn((
                    Mesh3d(art.cube.clone()),
                    MeshMaterial3d(art.rust.clone()),
                    Transform::from_xyz(side * scale.x * 0.65, 0.35, 0.).with_scale(Vec3::new(
                        0.35,
                        0.65,
                        scale.z * 1.3,
                    )),
                ));
            }
            if kind == Some(EnemyKind::Shooter) || kind == Some(EnemyKind::Boss) {
                p.spawn((
                    Mesh3d(art.cube.clone()),
                    MeshMaterial3d(art.steel.clone()),
                    Transform::from_xyz(0., y, scale.z * 0.8)
                        .with_scale(Vec3::new(0.23, 0.23, scale.z)),
                ));
            }
            if let Some(kind) = kind {
                mechanical_details(p, art, kind, scale);
            }
        }
        let radius = match kind {
            None => 0.62,
            Some(EnemyKind::Boss) => 2.,
            Some(EnemyKind::Tank) => 1.,
            Some(EnemyKind::Rusher | EnemyKind::Shooter | EnemyKind::Bomber) => 0.65,
        };
        p.spawn((
            Mesh3d(art.ring.clone()),
            MeshMaterial3d(if player {
                art.teal.clone()
            } else {
                art.red.clone()
            }),
            Transform::from_xyz(0., 0.035, 0.).with_scale(Vec3::new(radius, 0.07, radius)),
        ));
    });
    entity
}
fn animation_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    animated: Query<&Animated>,
    mut players: Query<&mut AnimationPlayer>,
) {
    if let Ok(anim) = animated.get(ready.entity) {
        for child in children.iter_descendants(ready.entity) {
            if let Ok(mut player) = players.get_mut(child) {
                player.play(anim.nodes[0]).repeat();
                commands.entity(child).insert((
                    AnimationGraphHandle(anim.graph.clone()),
                    ActorAnimation {
                        id: anim.id,
                        visual: ready.entity,
                        player: anim.player,
                        nodes: anim.nodes.clone(),
                        current: 0,
                    },
                ));
            }
        }
    }
}
fn flat(pos: Vec2, y: f32) -> Vec3 {
    Vec3::new(pos.x, y, pos.y)
}
fn sync(
    mut commands: Commands,
    sim: Res<Sim>,
    art: Res<Art>,
    mut visuals: ResMut<Visuals>,
    mut transforms: Query<&mut Transform>,
    mut material_handles: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut gizmos: Gizmos,
    cameras: Query<&GlobalTransform, With<GameCamera>>,
) {
    let camera_rotation = cameras
        .single()
        .map_or(Quat::IDENTITY, GlobalTransform::rotation);
    let player_id = 0;
    let mut live = HashSet::from([player_id]);
    let p = *visuals
        .actors
        .entry(player_id)
        .or_insert_with(|| spawn_actor(&mut commands, &art, player_id, None));
    if let Ok(mut transform) = transforms.get_mut(p) {
        transform.translation = flat(sim.player.pos, 0.);
        transform.rotation = Quat::from_rotation_y(sim.player.aim.x.atan2(sim.player.aim.y));
    }
    for enemy in &sim.enemies {
        live.insert(enemy.id);
        let entity = *visuals
            .actors
            .entry(enemy.id)
            .or_insert_with(|| spawn_actor(&mut commands, &art, enemy.id, Some(enemy.kind)));
        if let Ok(mut transform) = transforms.get_mut(entity) {
            transform.translation = flat(enemy.pos, 0.);
            transform.rotation = Quat::from_rotation_y(enemy.aim.x.atan2(enemy.aim.y));
        }
        let tint = if enemy.telegraph > 0. {
            Color::srgb(1., 0.45, 0.08)
        } else {
            Color::srgba(1., 0.15, 0.08, 0.5)
        };
        if enemy.telegraph > 0. {
            gizmos.circle(
                Isometry3d::new(
                    flat(enemy.pos, 0.08),
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                ),
                enemy.radius + 0.4,
                tint,
            );
            gizmos.line(
                flat(enemy.pos, 0.12),
                flat(enemy.pos + enemy.aim * 5., 0.12),
                tint,
            );
        }
        if enemy.hp < enemy.max_hp {
            let a = flat(enemy.pos + Vec2::new(-enemy.radius, 0.), 2.4);
            gizmos.line(
                a,
                a + Vec3::X * (2. * enemy.radius),
                Color::srgb(0.2, 0.12, 0.1),
            );
            gizmos.line(
                a + Vec3::Y * 0.01,
                a + Vec3::Y * 0.01 + Vec3::X * (2. * enemy.radius * enemy.hp / enemy.max_hp),
                tint,
            );
        }
    }
    visuals.actors.retain(|id, entity| {
        if live.contains(id) {
            true
        } else {
            commands.entity(*entity).despawn();
            false
        }
    });
    live.clear();
    for bullet in &sim.bullets {
        live.insert(bullet.id);
        let entity = *visuals.bullets.entry(bullet.id).or_insert_with(|| {
            commands
                .spawn((
                    Transient,
                    Mesh3d(art.cube.clone()),
                    MeshMaterial3d(if bullet.hostile {
                        art.red.clone()
                    } else {
                        art.white.clone()
                    }),
                    Transform::default(),
                ))
                .id()
        });
        if let Ok(mut transform) = transforms.get_mut(entity) {
            *transform = Transform::from_translation(flat(
                bullet.pos,
                if bullet.hostile { 0.85 } else { 1.35 },
            ))
            .with_rotation(Quat::from_rotation_y(bullet.dir.x.atan2(bullet.dir.y)))
            .with_scale(Vec3::new(
                bullet.radius * 1.2,
                bullet.radius * 1.2,
                if bullet.hostile { 0.4 } else { 0.65 },
            ));
        }
    }
    visuals.bullets.retain(|id, e| {
        if live.contains(id) {
            true
        } else {
            commands.entity(*e).despawn();
            false
        }
    });
    live.clear();
    for pickup in &sim.pickups {
        live.insert(pickup.id);
        let entity = *visuals
            .pickups
            .entry(pickup.id)
            .or_insert_with(|| match pickup.kind {
                PickupKind::Magnet => commands
                    .spawn((Transient, Transform::default(), Visibility::default()))
                    .with_children(|parent| {
                        parent.spawn((
                            PickupModel { id: pickup.id },
                            WorldAssetRoot(art.magnet_scene.clone()),
                            Transform::from_scale(Vec3::splat(2.5)),
                        ));
                    })
                    .id(),
                PickupKind::Xp => commands
                    .spawn((
                        Transient,
                        Mesh3d(art.cube.clone()),
                        MeshMaterial3d(art.teal.clone()),
                        Transform::default(),
                    ))
                    .id(),
                PickupKind::Health | PickupKind::Overdrive | PickupKind::Shield => commands
                    .spawn((Transient, Transform::default(), Visibility::default()))
                    .with_children(|parent| pickup_details(parent, &art, pickup.kind))
                    .id(),
            });
        if let Ok(mut transform) = transforms.get_mut(entity) {
            *transform = if pickup.kind == PickupKind::Magnet {
                Transform::from_translation(flat(pickup.pos, 0.68 + (pickup.age * 3.).sin() * 0.1))
            } else {
                Transform::from_translation(flat(
                    pickup.pos,
                    (if pickup.kind == PickupKind::Xp {
                        0.3
                    } else {
                        0.52
                    }) + (pickup.age * 3.).sin() * 0.1,
                ))
                .with_rotation(Quat::from_euler(EulerRot::XYZ, 0.4, pickup.age * 1.8, 0.4))
                .with_scale(Vec3::splat(if pickup.kind == PickupKind::Xp {
                    0.22
                } else {
                    0.38
                }))
            };
        }
        if pickup.kind != PickupKind::Xp {
            gizmos.circle(
                Isometry3d::new(
                    flat(pickup.pos, 0.07),
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                ),
                0.65,
                match pickup.kind {
                    PickupKind::Health => Color::WHITE,
                    PickupKind::Overdrive => Color::srgb(1., 0.6, 0.1),
                    PickupKind::Magnet => Color::srgb(0.7, 0.3, 1.),
                    _ => Color::srgb(0.2, 1., 0.8),
                },
            );
        }
    }
    visuals.pickups.retain(|id, e| {
        if live.contains(id) {
            true
        } else {
            commands.entity(*e).despawn();
            false
        }
    });
    live.clear();
    for effect in &sim.effects {
        let atlas = match effect.kind {
            EffectKind::Muzzle => Some(0),
            EffectKind::Hit => Some(1),
            EffectKind::Heal | EffectKind::LevelUp => Some(2),
            _ => None,
        };
        let atlas = if let Some(index) = atlas {
            let Some(frame) = atlas_frame(&art.vfx_specs[index], effect.age) else {
                continue;
            };
            Some((index, frame))
        } else {
            None
        };
        live.insert(effect.id);
        let progress = (effect.age / effect.duration).clamp(0., 1.);
        let radius = effect.radius * (0.3 + progress * 0.7);
        let transform = if let Some((index, _)) = atlas {
            let spec = &art.vfx_specs[index];
            let width = match effect.kind {
                EffectKind::Muzzle => 1.4,
                EffectKind::Hit => 1.1,
                _ => 2.4,
            };
            billboard_transform(
                flat(
                    effect.pos,
                    if effect.kind == EffectKind::Muzzle {
                        1.35
                    } else {
                        0.95
                    },
                ),
                camera_rotation,
                if effect.kind == EffectKind::Muzzle {
                    Some(flat(sim.player.aim, 0.))
                } else {
                    None
                },
                spec.pivot_normalized_top_left,
                width,
            )
        } else {
            Transform::from_translation(flat(effect.pos, 0.1)).with_scale(Vec3::new(
                radius,
                0.12 * (1. - progress),
                radius,
            ))
        };
        let mat = if let Some((index, frame)) = atlas {
            art.vfx[index][frame].clone()
        } else {
            match effect.kind {
                EffectKind::Dash => art.teal.clone(),
                EffectKind::Spawn => art.red.clone(),
                _ => art.amber.clone(),
            }
        };
        let entity = *visuals.effects.entry(effect.id).or_insert_with(|| {
            commands
                .spawn((
                    Transient,
                    Mesh3d(if atlas.is_some() {
                        art.quad.clone()
                    } else {
                        art.ring.clone()
                    }),
                    MeshMaterial3d(mat.clone()),
                    transform,
                ))
                .id()
        });
        if let Ok(mut material) = material_handles.get_mut(entity) {
            material.0 = mat;
        }
        if let Ok(mut t) = transforms.get_mut(entity) {
            *t = transform;
        }
    }
    visuals.effects.retain(|id, e| {
        if live.contains(id) {
            true
        } else {
            commands.entity(*e).despawn();
            false
        }
    });
    if sim.phase == Phase::Playing {
        let start = sim.player.pos + sim.player.aim * 0.9;
        gizmos.line(
            flat(start, 0.08),
            flat(start + sim.player.aim * 2., 0.08),
            Color::srgba(0.25, 0.9, 0.8, 0.35),
        );
    }
}
#[derive(Resource, serde::Deserialize)]
struct Grounding {
    clips: Vec<GroundClip>,
}
#[derive(serde::Deserialize)]
struct GroundClip {
    animation: String,
    samples: Vec<GroundSample>,
}
#[derive(serde::Deserialize)]
struct GroundSample {
    t: f32,
    vertical_offset_m: f32,
}
impl Grounding {
    fn offset(&self, name: &str, time: f32) -> f32 {
        let Some(clip) = self.clips.iter().find(|c| c.animation == name) else {
            return 0.;
        };
        let (Some(first), Some(last)) = (clip.samples.first(), clip.samples.last()) else {
            return 0.;
        };
        let i = clip.samples.partition_point(|s| s.t < time);
        if i == 0 {
            return first.vertical_offset_m;
        }
        if i >= clip.samples.len() {
            return last.vertical_offset_m;
        }
        let a = &clip.samples[i - 1];
        let b = &clip.samples[i];
        a.vertical_offset_m
            + (b.vertical_offset_m - a.vertical_offset_m) * ((time - a.t) / (b.t - a.t).max(0.0001))
    }
}
fn animate(
    sim: Res<Sim>,
    controls: Res<Controls>,
    ground: Res<Grounding>,
    mut players: Query<(&mut AnimationPlayer, &mut ActorAnimation)>,
    mut visuals: Query<&mut Transform, With<Animated>>,
) {
    for (mut player, mut state) in &mut players {
        let enemy = sim.enemies.iter().find(|e| e.id == state.id);
        let moving = controls.0.movement.length_squared() > 0.1;
        let desired = if state.player {
            usize::from(moving && sim.phase == Phase::Playing)
        } else {
            usize::from(enemy.is_some_and(|e| e.telegraph > 0.))
        };
        if state.current != desired {
            player.stop_all();
            let animation = player.play(state.nodes[desired]);
            if state.player || desired == 0 {
                animation.repeat();
            }
            state.current = desired;
        }
        let speed = if sim.phase == Phase::Playing {
            if state.player && desired == 1 {
                sim.player.stats.speed / 2.56
            } else if !state.player && desired == 0 {
                1.5
            } else {
                1.
            }
        } else {
            0.
        };
        let mut seek = 0.;
        for (_, animation) in player.playing_animations_mut() {
            animation.set_speed(speed);
            seek = animation.seek_time();
        }
        let name = match (state.player, desired) {
            (true, 1) => "scrapyard_rifle_run-loop",
            (true, _) => "scrapyard_rifle_aim-loop",
            (false, 1) => "scrapyard_rusher_attack",
            _ => "scrapyard_rusher_rush-loop",
        };
        if let Ok(mut transform) = visuals.get_mut(state.visual) {
            transform.translation.y = ground.offset(name, seek);
            let recoil = if state.player {
                sim.effects
                    .iter()
                    .filter(|e| e.kind == EffectKind::Muzzle)
                    .map(|e| (1. - e.age / e.duration).max(0.))
                    .fold(0., f32::max)
            } else {
                0.
            };
            // First-frame rifle barrel has canonical yaw PI - 0.21954.
            // The player offset aligns the actual barrel to the controller aim.
            let yaw = std::f32::consts::PI + if state.player { 0.21954 } else { 0. };
            transform.rotation =
                Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-recoil * 0.045);
        }
    }
}
fn camera(sim: Res<Sim>, time: Res<Time>, mut cameras: Query<&mut Transform, With<GameCamera>>) {
    let focus = if sim.phase == Phase::Title {
        Vec2::new(0., -2.)
    } else {
        sim.player.pos * 0.72
    };
    let focus = focus.clamp(Vec2::splat(-ARENA_HALF + 7.), Vec2::splat(ARENA_HALF - 7.));
    for mut camera in &mut cameras {
        let target = flat(focus, 0.) + Vec3::new(0., 28., 23.);
        let alpha = 1. - (-time.delta_secs() * 7.).exp();
        camera.translation = camera.translation.lerp(target, alpha);
    }
}

// This is a consumer decode transform. Original PNG bytes and source hashes stay unchanged.
fn cutoff_alpha(pixels: &mut [u8], threshold: u8) {
    for pixel in pixels.chunks_exact_mut(4) {
        if pixel[3] < threshold {
            pixel[3] = 0;
        }
    }
}
fn prepare_vfx_images(
    art: Res<Art>,
    mut images: ResMut<Assets<Image>>,
    mut processed: Local<HashSet<AssetId<Image>>>,
) {
    for (handle, spec) in art.vfx_images.iter().zip(&art.vfx_specs) {
        if processed.contains(&handle.id()) {
            continue;
        }
        let Some(mut image) = images.get_mut(handle) else {
            continue;
        };
        if !matches!(
            image.texture_descriptor.format,
            TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb
        ) {
            warn!(
                "Unexpected VFX format for {}; alpha cutoff requires RGBA8",
                spec.path
            );
            processed.insert(handle.id());
            continue;
        }
        let Some(pixels) = image.data.as_mut() else {
            continue;
        };
        cutoff_alpha(pixels, (spec.alpha_discard_below * 255.).round() as u8);
        image.sampler = ImageSampler::linear(); // clamp-to-edge; the source PNG loader supplies one mip.
        processed.insert(handle.id());
    }
}
fn atlas_frame(spec: &VfxSpec, age: f32) -> Option<usize> {
    let frame = (age.max(0.) * spec.fps) as usize;
    (frame < spec.frames).then_some(frame)
}
fn billboard_transform(
    anchor: Vec3,
    camera_rotation: Quat,
    direction: Option<Vec3>,
    pivot: [f32; 2],
    width: f32,
) -> Transform {
    let angle = direction.map_or(0., |direction| {
        let projected = camera_rotation.inverse() * direction;
        projected.y.atan2(projected.x)
    });
    let rotation = camera_rotation * Quat::from_rotation_z(angle);
    let local_pivot = Vec3::new(pivot[0] - 0.5, 0.5 - pivot[1], 0.);
    Transform::from_translation(anchor - rotation * (local_pivot * width))
        .with_rotation(rotation)
        .with_scale(Vec3::splat(width))
}
fn spin_pickup_models(sim: Res<Sim>, mut models: Query<(&PickupModel, &mut Transform)>) {
    for (model, mut transform) in &mut models {
        if let Some(pickup) = sim.pickups.iter().find(|pickup| pickup.id == model.id) {
            transform.rotation = Quat::from_rotation_y(pickup.age * 1.8);
        }
    }
}

fn visual_part(
    parent: &mut ChildSpawnerCommands<'_>,
    mesh: &Handle<Mesh>,
    material: &Handle<StandardMaterial>,
    position: Vec3,
    scale: Vec3,
) {
    parent.spawn((
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(position).with_scale(scale),
    ));
}

fn pickup_details(parent: &mut ChildSpawnerCommands<'_>, art: &Art, kind: PickupKind) {
    match kind {
        PickupKind::Health => {
            // A broad white plus remains legible from the arena camera.
            visual_part(
                parent,
                &art.cube,
                &art.white,
                Vec3::ZERO,
                Vec3::new(2., 0.48, 0.62),
            );
            visual_part(
                parent,
                &art.cube,
                &art.white,
                Vec3::ZERO,
                Vec3::new(0.62, 0.48, 2.),
            );
            visual_part(
                parent,
                &art.cube,
                &art.teal,
                Vec3::new(0., 0.27, 0.),
                Vec3::new(0.32, 0.06, 0.32),
            );
        }
        PickupKind::Overdrive => {
            visual_part(
                parent,
                &art.cube,
                &art.dark,
                Vec3::ZERO,
                Vec3::new(0.92, 1.25, 0.8),
            );
            for side in [-1., 1.] {
                visual_part(
                    parent,
                    &art.cube,
                    &art.amber,
                    Vec3::new(0., side * 0.72, 0.),
                    Vec3::new(1.5, 0.28, 1.1),
                );
                visual_part(
                    parent,
                    &art.cube,
                    &art.amber,
                    Vec3::new(side * 0.56, 0., 0.),
                    Vec3::new(0.16, 1.2, 0.85),
                );
            }
            // Offset bars form a bold stepped lightning mark on the front.
            for (x, y) in [(-0.16, 0.29), (0., 0.), (0.16, -0.29)] {
                visual_part(
                    parent,
                    &art.cube,
                    &art.white,
                    Vec3::new(x, y, 0.43),
                    Vec3::new(0.48, 0.2, 0.08),
                );
            }
        }
        PickupKind::Shield => {
            visual_part(
                parent,
                &art.ring,
                &art.teal,
                Vec3::ZERO,
                Vec3::new(1., 0.55, 1.),
            );
            visual_part(
                parent,
                &art.sphere,
                &art.steel,
                Vec3::ZERO,
                Vec3::new(0.7, 0.18, 0.7),
            );
            visual_part(
                parent,
                &art.cube,
                &art.teal,
                Vec3::new(0., 0.2, 0.),
                Vec3::new(0.16, 0.08, 0.75),
            );
        }
        PickupKind::Xp | PickupKind::Magnet => {}
    }
}

fn mechanical_details(
    parent: &mut ChildSpawnerCommands<'_>,
    art: &Art,
    kind: EnemyKind,
    scale: Vec3,
) {
    let height = scale.y * 0.55;
    match kind {
        EnemyKind::Shooter => {
            visual_part(
                parent,
                &art.cube,
                &art.steel,
                Vec3::new(0., height + 0.42, -0.15),
                Vec3::new(0.95, 0.18, 0.72),
            );
            for side in [-1., 1.] {
                visual_part(
                    parent,
                    &art.cube,
                    &art.rust,
                    Vec3::new(side * 0.57, height + 0.16, 0.12),
                    Vec3::new(0.24, 0.32, 0.55),
                );
                visual_part(
                    parent,
                    &art.cube,
                    &art.dark,
                    Vec3::new(side * 0.22, height + 0.53, -0.14),
                    Vec3::new(0.09, 0.04, 0.5),
                );
            }
            visual_part(
                parent,
                &art.cube,
                &art.amber,
                Vec3::new(0., height, 1.08),
                Vec3::new(0.29, 0.29, 0.1),
            );
        }
        EnemyKind::Tank => {
            // Wide shoulder armor and a front ram emphasize its heavy silhouette.
            for side in [-1., 1.] {
                visual_part(
                    parent,
                    &art.cube,
                    &art.steel,
                    Vec3::new(side * 0.79, height + 0.35, 0.),
                    Vec3::new(0.4, 0.46, 1.25),
                );
            }
            visual_part(
                parent,
                &art.cube,
                &art.rust,
                Vec3::new(0., 0.32, 0.82),
                Vec3::new(1.8, 0.38, 0.25),
            );
            for x in [-0.35, 0., 0.35] {
                visual_part(
                    parent,
                    &art.cube,
                    &art.dark,
                    Vec3::new(x, height + 0.67, -0.1),
                    Vec3::new(0.12, 0.09, 0.72),
                );
            }
            visual_part(
                parent,
                &art.sphere,
                &art.red,
                Vec3::new(0., height + 0.66, 0.38),
                Vec3::splat(0.18),
            );
        }
        EnemyKind::Bomber => {
            // Crossed structural bands frame a bright exposed volatile core.
            visual_part(
                parent,
                &art.cube,
                &art.steel,
                Vec3::new(0., height, 0.),
                Vec3::new(1.68, 0.16, 0.24),
            );
            visual_part(
                parent,
                &art.cube,
                &art.steel,
                Vec3::new(0., height, 0.),
                Vec3::new(0.24, 0.16, 1.68),
            );
            visual_part(
                parent,
                &art.sphere,
                &art.violet,
                Vec3::new(0., height + 0.7, 0.),
                Vec3::splat(0.26),
            );
            for side in [-1., 1.] {
                visual_part(
                    parent,
                    &art.cube,
                    &art.amber,
                    Vec3::new(side * 0.57, height + 0.4, 0.),
                    Vec3::new(0.12, 0.42, 0.12),
                );
            }
        }
        EnemyKind::Boss => {
            // Foreman's broad shoulder blocks, twin cannons and furnace identify it at a glance.
            for side in [-1., 1.] {
                visual_part(
                    parent,
                    &art.cube,
                    &art.steel,
                    Vec3::new(side * 1.5, height + 0.7, 0.),
                    Vec3::new(0.55, 0.8, 1.9),
                );
                visual_part(
                    parent,
                    &art.cube,
                    &art.rust,
                    Vec3::new(side * 1.05, height + 0.15, 1.4),
                    Vec3::new(0.42, 0.42, 1.25),
                );
                visual_part(
                    parent,
                    &art.cube,
                    &art.amber,
                    Vec3::new(side * 1.05, height + 0.15, 2.06),
                    Vec3::new(0.46, 0.46, 0.12),
                );
            }
            visual_part(
                parent,
                &art.sphere,
                &art.amber,
                Vec3::new(0., height + 0.48, 1.18),
                Vec3::new(0.48, 0.48, 0.2),
            );
            for x in [-0.65, 0., 0.65] {
                visual_part(
                    parent,
                    &art.cube,
                    &art.steel,
                    Vec3::new(x, height + 1.48, -0.2),
                    Vec3::new(0.25, 0.16, 1.25),
                );
            }
        }
        EnemyKind::Rusher => {}
    }
}

#[cfg(test)]
mod vfx_tests {
    use super::*;
    #[test]
    fn cutoff_preserves_soft_glow_and_the_threshold() {
        let mut rgba = [12, 34, 56, 3, 12, 34, 56, 4, 12, 34, 56, 80];
        cutoff_alpha(&mut rgba, 4);
        assert_eq!(rgba, [12, 34, 56, 0, 12, 34, 56, 4, 12, 34, 56, 80]);
    }
    #[test]
    fn atlas_disappears_after_its_four_frames() {
        let pack: VfxPack = serde_json::from_str(include_str!(
            "../../../designs/scrapyard/batch-02/vfx/vfx.json"
        ))
        .expect("checked-in VFX metadata parses");
        for spec in pack.effects {
            assert_eq!(atlas_frame(&spec, 0.), Some(0));
            assert_eq!(atlas_frame(&spec, 3.9 / spec.fps), Some(3));
            assert_eq!(atlas_frame(&spec, 4.01 / spec.fps), None);
        }
    }
    #[test]
    fn muzzle_pivot_and_right_axis_follow_projected_aim() {
        let camera = Transform::from_xyz(0., 28., 23.)
            .looking_at(Vec3::ZERO, Vec3::Y)
            .rotation;
        let anchor = Vec3::new(3., 0.95, 2.);
        let pivot = [0.44, 0.515];
        for direction in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
            let transform = billboard_transform(anchor, camera, Some(direction), pivot, 1.4);
            let actual = transform.transform_point(Vec3::new(pivot[0] - 0.5, 0.5 - pivot[1], 0.));
            assert!(actual.distance(anchor) < 0.00001);
            let aim = (camera.inverse() * direction).truncate().normalize();
            let image_right = (camera.inverse() * (transform.rotation * Vec3::X)).truncate();
            assert!(image_right.dot(aim) > 0.9999);
        }
    }
}
