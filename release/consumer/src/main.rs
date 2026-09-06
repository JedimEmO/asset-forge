//! A standalone delivery consumer: no Forge crate or compile-time asset paths.
use bevy::{
    animation::AnimationEvent,
    app::AnimationSystems,
    gltf::Gltf,
    mesh::{
        VertexAttributeValues,
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
    },
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
    time::TimeUpdateStrategy,
    transform::TransformSystems,
    world_serialization::WorldInstanceReady,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::Duration,
};

fn read(path: impl AsRef<std::path::Path>) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("read delivery metadata"))
        .expect("valid JSON")
}
fn number(v: &Value) -> f32 {
    v.as_f64().expect("measured number") as f32
}
fn vec3(v: &Value) -> Vec3 {
    Vec3::new(number(&v[0]), number(&v[1]), number(&v[2]))
}
fn quat(v: &Value) -> Quat {
    Quat::from_xyzw(number(&v[0]), number(&v[1]), number(&v[2]), number(&v[3]))
}
fn transform(t: &Value, r: &Value) -> Transform {
    Transform::from_translation(vec3(t)).with_rotation(quat(r))
}

#[derive(Resource)]
struct Delivery {
    root: PathBuf,
    config: Value,
    manifests: Vec<Value>,
    ground: Value,
    attachment: Value,
    bundles: Vec<Handle<Gltf>>,
    companions: Vec<Handle<Gltf>>,
    audio: Vec<Handle<AudioSource>>,
    spawned: bool,
}
#[derive(Resource, Default)]
struct Evidence {
    frames: u32,
    ready: BTreeSet<String>,
    events: BTreeMap<String, u32>,
    expected: BTreeSet<String>,
    moved: BTreeSet<String>,
    captures: u32,
    socket_checks: u32,
    max_grounding: f32,
    distance: f32,
    foot_samples: u64,
    minimum_foot_y: Option<f32>,
    started: bool,
    warm_frames: u32,
}
#[derive(Component)]
struct Actor {
    name: String,
    graph: Handle<AnimationGraph>,
    node: AnimationNodeIndex,
    speed: f32,
    controller: Entity,
    looped: bool,
}
#[derive(Component)]
struct Playing {
    visual: Entity,
}
#[derive(Component)]
struct SocketCheck {
    bone: Entity,
    local: Transform,
    initial: Option<Vec3>,
    moved: bool,
}
#[derive(Component)]
struct MotionCheck {
    animation: String,
    initial: Option<Quat>,
}
#[derive(AnimationEvent, Clone)]
struct Cue {
    key: String,
}

fn main() {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("argument: absolute fixture directory"),
    )
    .canonicalize()
    .unwrap();
    assert!(
        !root.join("runtime.json").exists(),
        "preserve prior run: use a fresh delivery directory or move runtime.json and review PNGs together"
    );
    let config = read(root.join("assets/fixture.json"));
    let manifests = config["actors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            read(
                root.join("assets")
                    .join(a["namespace"].as_str().unwrap())
                    .join("library.json"),
            )
        })
        .collect();
    let ground = read(root.join("assets/grounding.json"));
    let attachment = read(root.join("assets/attachment.json"));
    let asset_path = root.join("assets").to_str().unwrap().to_owned();
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(bevy::render::RenderPlugin {
                    synchronous_pipeline_compilation: true,
                    ..default()
                })
                .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
                .set(AssetPlugin {
                    file_path: asset_path,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Forge consumer | rusher / armed scavenger".into(),
                        resolution: (1200, 800).into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 30.0,
        )))
        .insert_resource(GlobalAmbientLight {
            brightness: 800.,
            ..default()
        })
        .insert_resource(Delivery {
            root,
            config,
            manifests,
            ground,
            attachment,
            bundles: vec![],
            companions: vec![],
            audio: vec![],
            spawned: false,
        })
        .init_resource::<Evidence>()
        .add_systems(Startup, setup)
        .add_systems(Update, (spawn_loaded, start_playback).chain())
        .add_systems(
            PostUpdate,
            grounding
                .after(AnimationSystems)
                .before(TransformSystems::Propagate),
        )
        .add_systems(Last, (check_floor, inspect).chain())
        .add_observer(ready)
        .add_observer(|cue: On<Cue>, mut evidence: ResMut<Evidence>| {
            *evidence.events.entry(cue.key.clone()).or_default() += 1;
        })
        .run();
}
fn setup(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut d: ResMut<Delivery>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    d.bundles = d.config["actors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| server.load(a["bundle"].as_str().unwrap().to_owned()))
        .collect();
    d.companions = ["prop", "weapon"]
        .iter()
        .map(|key| server.load(d.config[key].as_str().unwrap().to_owned()))
        .collect();
    d.audio = d
        .manifests
        .iter()
        .enumerate()
        .flat_map(|(i, m)| m["audio"].as_array().unwrap().iter().map(move |a| (i, a)))
        .map(|(i, a)| {
            server.load(format!(
                "{}/{}",
                d.config["actors"][i]["namespace"].as_str().unwrap(),
                a["path"].as_str().unwrap()
            ))
        })
        .collect();
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0., 5.5, -10.5).looking_at(Vec3::new(0., 0.8, 0.), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 7000.,
            ..default()
        },
        Transform::from_xyz(4., 7., -5.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(24., 18.))),
        MeshMaterial3d(materials.add(Color::srgb(0.19, 0.23, 0.26))),
    ));
    commands.spawn((
        WorldAssetRoot(server.load(
            GltfAssetLabel::Scene(0).from_asset(d.config["prop"].as_str().unwrap().to_owned()),
        )),
        Transform::from_xyz(4.5, 0., 1.3),
    ));
    commands.spawn((Text::new("FRONT rusher: swipe / run / idle     REAR scavenger: fire / run / aim\nManifest events | separate controller and grounding | runtime hand_l attachment"),TextFont{font_size:FontSize::Px(20.),..default()},Node{position_type:PositionType::Absolute,top:px(15),left:px(15),..default()}));
}
fn spawn_loaded(
    mut commands: Commands,
    mut d: ResMut<Delivery>,
    gltfs: Res<Assets<Gltf>>,
    mut clips: ResMut<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut evidence: ResMut<Evidence>,
) {
    if d.spawned || !d.bundles.iter().all(|h| gltfs.contains(h)) {
        return;
    }
    for (row, actor) in d.config["actors"].as_array().unwrap().iter().enumerate() {
        let gltf = gltfs.get(&d.bundles[row]).unwrap();
        for (col, selected) in actor["clips"].as_array().unwrap().iter().enumerate() {
            let name = selected["animation"].as_str().unwrap();
            let metadata = d.manifests[row]["clips"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["name"] == selected["library"])
                .unwrap();
            let handle = gltf
                .named_animations
                .get(name)
                .expect("named animation delivered")
                .clone();
            let mut clip = clips.get_mut(&handle).expect("animation loaded");
            for event in metadata["events"].as_array().unwrap() {
                let key = format!("{name}:{}", event["name"].as_str().unwrap());
                evidence.expected.insert(key.clone());
                clip.add_event(number(&event["time_s"]), Cue { key });
            }
            let (graph, node) = AnimationGraph::from_clip(handle);
            let controller = commands
                .spawn((
                    Transform::from_xyz((col as f32 - 1.) * 3., 0., (row as f32 - 0.5) * 4.),
                    Visibility::default(),
                ))
                .id();
            let speed = if col == 1 {
                number(&metadata["root_motion"]["avg_speed_mps"]) * number(&actor["motion_scale"])
            } else {
                0.
            };
            commands.entity(controller).with_children(|p| {
                p.spawn((
                    WorldAssetRoot(gltf.scenes[0].clone()),
                    Transform::default(),
                    Actor {
                        name: name.into(),
                        graph: graphs.add(graph),
                        node,
                        speed,
                        controller,
                        looped: metadata["looped"].as_bool().unwrap(),
                    },
                ));
            });
        }
    }
    d.spawned = true;
}
fn ready(
    event: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    actors: Query<&Actor>,
    mut players: Query<&mut AnimationPlayer>,
    names: Query<(&Name, &Transform)>,
    server: Res<AssetServer>,
    d: Res<Delivery>,
    mut evidence: ResMut<Evidence>,
) {
    let Ok(actor) = actors.get(event.entity) else {
        return;
    };
    let mut count = 0;
    for child in children.iter_descendants(event.entity) {
        if let Ok(mut player) = players.get_mut(child) {
            let active = player.play(actor.node);
            active.pause();
            if actor.looped {
                active.repeat();
            }
            commands.entity(child).insert((
                AnimationGraphHandle(actor.graph.clone()),
                Playing {
                    visual: event.entity,
                },
            ));
            count += 1;
        }
        if let Ok((name, _t)) = names.get(child) {
            if name.as_str() == "LeftHand" {
                commands.entity(child).insert(MotionCheck {
                    animation: actor.name.clone(),
                    initial: None,
                });
            }
            if actor.name.starts_with("scrapyard_") {
                let socket = d.manifests[1]["rig"]["sockets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|s| s["name"] == d.attachment["socket"])
                    .unwrap();
                if name.as_str() == socket["bone"].as_str().unwrap() {
                    let local =
                        transform(&socket["translation"], &socket["rotation"]).mul_transform(
                            transform(&d.attachment["translation"], &d.attachment["rotation_xyzw"]),
                        );
                    commands.entity(child).with_children(|p| {
                        p.spawn((
                            WorldAssetRoot(
                                server.load(
                                    GltfAssetLabel::Scene(0).from_asset(
                                        d.config["weapon"].as_str().unwrap().to_owned(),
                                    ),
                                ),
                            ),
                            local,
                            SocketCheck {
                                bone: child,
                                local,
                                initial: None,
                                moved: false,
                            },
                        ));
                    });
                }
            }
        }
    }
    assert_eq!(count, 1, "exactly one animation player per character");
    evidence.ready.insert(actor.name.clone());
}
fn offset(samples: &[Value], time: f32) -> f32 {
    let i = samples.partition_point(|s| number(&s["t"]) < time);
    if i == 0 {
        return number(&samples[0]["vertical_offset_m"]);
    }
    if i == samples.len() {
        return number(&samples[i - 1]["vertical_offset_m"]);
    }
    let (a, b) = (&samples[i - 1], &samples[i]);
    let weight = (time - number(&a["t"])) / (number(&b["t"]) - number(&a["t"]));
    number(&a["vertical_offset_m"])
        + (number(&b["vertical_offset_m"]) - number(&a["vertical_offset_m"])) * weight
}
fn grounding(
    d: Res<Delivery>,
    players: Query<(&AnimationPlayer, &Playing)>,
    actors: Query<&Actor>,
    mut transforms: Query<&mut Transform>,
    mut evidence: ResMut<Evidence>,
) {
    for (player, playing) in &players {
        let actor = actors.get(playing.visual).unwrap();
        let Some(active) = player.animation(actor.node) else {
            continue;
        };
        let row = d.ground["clips"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["animation"] == actor.name)
            .unwrap();
        let y = offset(row["samples"].as_array().unwrap(), active.seek_time());
        transforms.get_mut(playing.visual).unwrap().translation.y = y;
        evidence.max_grounding = evidence.max_grounding.max(y);
        // Demonstrate controller ownership for half a second, then hold position for review.
        if evidence.started && evidence.frames < 15 {
            let dz = actor.speed / 30.;
            transforms.get_mut(actor.controller).unwrap().translation.z -= dz;
            evidence.distance += dz;
        }
    }
}
fn inspect(
    mut commands: Commands,
    d: Res<Delivery>,
    mut evidence: ResMut<Evidence>,
    mut bones: Query<(&mut MotionCheck, &Transform)>,
    globals: Query<&GlobalTransform>,
    mut sockets: Query<(&mut SocketCheck, &GlobalTransform)>,
    skins: Query<&SkinnedMesh>,
    server: Res<AssetServer>,
    mut exit: MessageWriter<AppExit>,
    mut wait: Local<u32>,
) {
    *wait += 1;
    assert!(*wait < 1800, "consumer timed out loading or capturing");
    if !evidence.started
        || evidence.ready.len() != 6
        || !d
            .companions
            .iter()
            .all(|h| server.is_loaded_with_dependencies(h))
        || !d
            .audio
            .iter()
            .all(|h| server.is_loaded_with_dependencies(h))
    {
        return;
    }
    evidence.frames += 1;
    for (mut check, t) in &mut bones {
        if let Some(initial) = check.initial {
            if initial.angle_between(t.rotation) > 0.00001 {
                evidence.moved.insert(check.animation.clone());
            }
        } else {
            check.initial = Some(t.rotation);
        }
    }
    for (mut check, actual) in &mut sockets {
        let expected = globals.get(check.bone).unwrap().mul_transform(check.local);
        assert!(
            actual.affine().abs_diff_eq(expected.affine(), 0.0001),
            "socket must follow animated bone"
        );
        let pos = actual.translation();
        if let Some(initial) = check.initial {
            check.moved |= initial.distance(pos) > 0.0001;
        } else {
            check.initial = Some(pos);
        }
        evidence.socket_checks += 1;
    }
    if [6, 12, 30, 90].contains(&evidence.frames) {
        let path = d.root.join(format!("review-{}.png", evidence.frames));
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path))
            .observe(|_: On<ScreenshotCaptured>, mut e: ResMut<Evidence>| {
                e.captures += 1;
            });
    }
    if evidence.frames >= 120 && evidence.captures == 4 {
        assert_eq!(
            evidence.moved.len(),
            6,
            "all named animations change actual bone transforms"
        );
        assert!(skins.iter().count() >= 6);
        assert_eq!(sockets.iter().count(), 3);
        assert!(sockets.iter().all(|(s, _)| s.moved));
        assert!(
            evidence
                .expected
                .iter()
                .all(|key| evidence.events.get(key).copied().unwrap_or(0) > 0),
            "every authored event fired"
        );
        assert!(evidence.foot_samples > 0);
        assert!(evidence.distance > 0. && evidence.max_grounding > 0.);
        for path in [
            d.config["prop"].as_str().unwrap(),
            d.config["weapon"].as_str().unwrap(),
        ] {
            let handle: Handle<Gltf> = server.load(path.to_owned());
            assert!(
                server.is_loaded_with_dependencies(&handle),
                "companion mesh loaded"
            );
        }
        let report = json!({"status":"passed","animations":evidence.ready,"bone_motion":evidence.moved,"events":evidence.events,"socket_transform_checks":evidence.socket_checks,"max_grounding_m":evidence.max_grounding,"controller_distance_sum_m":evidence.distance,"screenshots":4,"foot_vertex_samples":evidence.foot_samples,"minimum_foot_y_m":evidence.minimum_foot_y,"audio_loaded":d.audio.len(),"scope":"Flat floor; six independent clips, no transition or terrain IK qualification"});
        std::fs::write(
            d.root.join("runtime.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        println!("{report}");
        exit.write(AppExit::Success);
    }
}

fn check_floor(
    mut evidence: ResMut<Evidence>,
    skins: Query<(&SkinnedMesh, &Mesh3d)>,
    meshes: Res<Assets<Mesh>>,
    bindposes: Res<Assets<SkinnedMeshInverseBindposes>>,
    joints: Query<(&Name, &GlobalTransform)>,
) {
    if !evidence.started || evidence.ready.len() != 6 {
        return;
    }
    for (skin, handle) in &skins {
        let mesh = meshes.get(&handle.0).unwrap();
        let VertexAttributeValues::Float32x3(positions) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap()
        else {
            panic!("position format")
        };
        let VertexAttributeValues::Uint16x4(indices) =
            mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX).unwrap()
        else {
            panic!("joint format")
        };
        let VertexAttributeValues::Float32x4(weights) =
            mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT).unwrap()
        else {
            panic!("weight format")
        };
        let inverse = bindposes.get(&skin.inverse_bindposes).unwrap();
        let bones: Vec<_> = skin
            .joints
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let (name, world) = joints.get(*e).unwrap();
                (
                    name.contains("Foot") || name.contains("Toe"),
                    world.to_matrix() * inverse[i],
                )
            })
            .collect();
        for ((p, ids), w) in positions.iter().zip(indices).zip(weights) {
            let foot_weight: f32 = (0..4)
                .filter(|i| bones[ids[*i] as usize].0)
                .map(|i| w[i])
                .sum();
            if foot_weight < 0.5 {
                continue;
            }
            let y: f32 = (0..4)
                .map(|i| {
                    bones[ids[i] as usize]
                        .1
                        .transform_point3(Vec3::from_array(*p))
                        .y
                        * w[i]
                })
                .sum();
            assert!(
                y >= -0.002,
                "engine-skinned foot penetrates flat floor: {y}"
            );
            evidence.minimum_foot_y = Some(evidence.minimum_foot_y.map_or(y, |old| old.min(y)));
            evidence.foot_samples += 1;
        }
    }
}

fn start_playback(
    d: Res<Delivery>,
    server: Res<AssetServer>,
    mut evidence: ResMut<Evidence>,
    mut players: Query<&mut AnimationPlayer>,
) {
    if evidence.started
        || evidence.ready.len() != 6
        || !d
            .companions
            .iter()
            .all(|h| server.is_loaded_with_dependencies(h))
        || !d
            .audio
            .iter()
            .all(|h| server.is_loaded_with_dependencies(h))
    {
        return;
    }
    evidence.warm_frames += 1;
    if evidence.warm_frames < 20 {
        return;
    }
    for mut player in &mut players {
        for (_, animation) in player.playing_animations_mut() {
            animation.resume();
        }
    }
    evidence.started = true;
}
