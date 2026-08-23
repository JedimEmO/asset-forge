//! The set the subject stands on, shared by the headless renderer and the viewer.
//!
//! Both consumers deliberately use the same lights, ground and lens. If they
//! diverged, an agent's contact sheet and a human's viewport would disagree
//! about the same clip, and neither could be used to check the other.

use bevy::{camera::primitives::Aabb, prelude::*};

use crate::binding::SkeletonPaths;

/// Camera field of view, vertical, in degrees.
///
/// Deliberately long: a near-orthographic lens keeps limb lengths comparable
/// between poses, which is the whole point of putting them side by side.
pub const CAMERA_FOV_DEG: f32 = 30.0;
/// Fraction of headroom left around the subject when framing.
pub const FRAME_PADDING: f32 = 1.12;

/// Marks scenery that must not affect camera framing.
#[derive(Component)]
pub struct NoFrameBounds;

/// Marks the ground plane, so a render can move it under a subject that does
/// not stand on y = 0.
#[derive(Component)]
pub struct Ground;

/// The matte dielectric every material-free primitive is restyled into. It
/// is the stage's twin of the material the export tooling writes into every
/// shipped glb (the prop normalize and the rig export): metallic 0.0,
/// roughness 0.9, white base color factor. A local copy rather than an
/// import from a game's runtime crate, so the studio depends on no engine
/// code but Bevy's; a game that wants to shade a library mesh the way the
/// stage does copies these three numbers.
#[must_use]
pub fn body_material() -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        // Matte dielectric: the painted texture is the paint, and the
        // lighting it is meant to be read under is flat. A specular lobe here
        // would put a moving highlight on a face that already has its shading
        // painted into the texture.
        perceptual_roughness: 0.9,
        metallic: 0.0,
        ..default()
    }
}

/// The label `bevy_pbr` gives the `StandardMaterial` it builds for glTF's
/// *default* material — the one every material-free primitive wears.
const GLTF_DEFAULT_MATERIAL_LABEL: &str = "DefaultMaterial/std";

/// Restyle the glTF default material of every loaded file into the matte
/// surface a library character wears.
///
/// Shipped bodies carry their baked material, but plenty of glbs in a library
/// do not — clips, props, meshes whose look is `COLOR_0` — and `bevy_gltf`
/// dresses every material-free primitive in glTF's default material: white,
/// `metallic` 1.0. That is a specification default rather than a decision,
/// and it is the one surface a vertex-color palette cannot survive, because
/// a metal has no diffuse response at all: the palette comes back as a
/// smeared environment reflection.
///
/// Only the *default* material is touched. A file that authored materials of
/// its own keeps every one of them, because those are content. The asset is
/// rewritten in place rather than swapped for another handle, so the loader's
/// batching survives and primitives the stage never spawns are covered too.
/// Both consumers of the stage register it in `Update`.
pub fn stylize_gltf_default_material(
    mut events: MessageReader<AssetEvent<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    server: Res<AssetServer>,
) {
    for event in events.read() {
        let AssetEvent::Added { id } = event else {
            continue;
        };
        let is_default = server
            .get_path(*id)
            .is_some_and(|path| path.label() == Some(GLTF_DEFAULT_MATERIAL_LABEL));
        if !is_default {
            continue;
        }
        if let Some(mut material) = materials.get_mut(*id) {
            *material = body_material();
        }
    }
}

/// Switch back-face culling off on every material as it arrives, so what a
/// render shows is *geometry*.
///
/// A lift from a single front view can leave the rear of a skull simply
/// absent, and with culling on the hole is invisible from behind: the inside
/// faces of the front are culled too, and what shows is the background,
/// which reads as "dark hair". With culling off the inside of the face shows
/// through from behind — there is no rear surface there — and the fix is a
/// seed to sweep or a reference to revise, never a repair in Blender.
///
/// Runs on `Added` *and* `Modified` so a material the stylizer has just
/// rewritten is caught on the same pass, and writes only when something
/// needs changing, because writing emits `Modified` and an unconditional
/// write would wake itself every frame for ever. Registered after
/// [`stylize_gltf_default_material`], and only by a render that asked for it.
pub fn uncull_materials(
    mut events: MessageReader<AssetEvent<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for event in events.read() {
        let (AssetEvent::Added { id } | AssetEvent::Modified { id }) = event else {
            continue;
        };
        let Some(material) = materials.get(*id) else {
            continue;
        };
        if material.cull_mode.is_none() && material.double_sided {
            continue;
        }
        if let Some(mut material) = materials.get_mut(*id) {
            material.cull_mode = None;
            material.double_sided = true;
        }
    }
}

/// Lights and ground for the subject to stand on.
///
/// A cast shadow is not decoration: it is the only cue that says whether a foot
/// is on the floor or hovering above it, which is exactly the class of defect
/// this tool exists to catch.
///
/// The ratio between the three sources is art direction, not taste. The style
/// guide puts the shading *in the texture* — top-light bias, AO in the pits —
/// and asks the judging light to be flatter and more ambient than a realism
/// rig so those baked values are what a reviewer sees. A key that out-runs
/// the ambient by 20× re-lights the character and hides whether the palette
/// carries its own value contrast. Key:fill:ambient sits near 2:1:1 here,
/// against roughly 20:6:1 before; the key keeps its shadow map, which is the
/// foot-contact cue, and stays the brightest source, which is what stops the
/// subject going flat.
pub fn spawn_stage(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(40.0, 40.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.26, 0.27, 0.31),
            perceptual_roughness: 0.95,
            ..default()
        })),
        // Just below zero so a foot resting exactly on the ground plane does
        // not z-fight with it.
        Transform::from_xyz(0.0, -GROUND_CLEARANCE, 0.0),
        // The floor must not inflate the framing bounds to 40 metres.
        NoFrameBounds,
        Ground,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 4_200.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 9.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 2_100.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(-6.0, 3.0, -5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.66, 0.69, 0.78),
        brightness: 2_000.0,
        ..default()
    });
}

/// How far below the subject's lowest point the ground sits.
const GROUND_CLEARANCE: f32 = 0.002;

/// Put the ground just under `lowest_y`.
///
/// A body stands on y = 0 and the stage is built for it; a raw lift is a unit
/// cube about the origin, and a floor through its middle would hide the half
/// a reviewer is most likely to need. The view render moves the floor to the
/// bounds it measured. The clip sheet deliberately does not: a foot dropping
/// below the floor is a defect the sheet exists to show, and a floor that
/// followed it down would hide it.
pub fn ground_under(world: &mut World, lowest_y: f32) {
    let mut query = world.query_filtered::<&mut Transform, With<Ground>>();
    for mut transform in query.iter_mut(world) {
        transform.translation.y = lowest_y - GROUND_CLEARANCE;
    }
}

/// World-space bounds of everything renderable, or `None` if nothing has bounds yet.
pub fn world_bounds(world: &mut World) -> Option<(Vec3, Vec3)> {
    // Excluding the ground plane matters: a 40 m floor would swallow a 1.8 m
    // subject and frame the shot from orbit.
    let mut query = world.query_filtered::<(&Aabb, &GlobalTransform), Without<NoFrameBounds>>();
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    let mut any = false;
    for (aabb, transform) in query.iter(world) {
        let centre: Vec3 = aabb.center.into();
        let extents: Vec3 = aabb.half_extents.into();
        // Transform all eight corners: a rotated box's world AABB is not the
        // rotation of its local AABB.
        for i in 0..8 {
            let sign = Vec3::new(
                if i & 1 == 0 { -1.0 } else { 1.0 },
                if i & 2 == 0 { -1.0 } else { 1.0 },
                if i & 4 == 0 { -1.0 } else { 1.0 },
            );
            let corner = transform.transform_point(centre + extents * sign);
            lo = lo.min(corner);
            hi = hi.max(corner);
            any = true;
        }
    }
    any.then_some((lo, hi))
}

/// The glTF root node: a grandchild of the spawned entity, because
/// `WorldAssetRoot` inserts a scene-wrapper entity in between.
pub fn find_animation_root(world: &mut World, spawned: Entity) -> Option<Entity> {
    let wrappers: Vec<Entity> = world
        .get::<Children>(spawned)
        .map(|c| c.iter().collect())
        .unwrap_or_default();
    let mut best: Option<(usize, Entity)> = None;
    for wrapper in wrappers {
        let candidates: Vec<Entity> = world
            .get::<Children>(wrapper)
            .map(|c| c.iter().collect())
            .unwrap_or_default();
        for candidate in candidates {
            if world.get::<Name>(candidate).is_none() {
                continue;
            }
            let count = SkeletonPaths::from_world(world, candidate).len();
            if best.is_none_or(|(n, _)| count > n) {
                best = Some((count, candidate));
            }
        }
    }
    best.map(|(_, e)| e)
}

/// Park the playhead at `t`, paused.
///
/// Pausing is what makes sampling deterministic: Bevy's animation evaluation
/// takes no time input at all — it applies whatever `seek_time` says, every
/// frame, with no change detection — while only the *advance* system consults
/// the clock and skips paused animations. `set_seek_time` rather than `seek_to`
/// because the latter replays every animation event in the interval jumped.
pub fn seek(world: &mut World, root: Entity, node: AnimationNodeIndex, t: f32) {
    if let Some(mut player) = world.get_mut::<AnimationPlayer>(root) {
        let active = player.play(node);
        active.set_seek_time(t);
        active.pause();
    }
}

#[cfg(test)]
mod tests {
    use bevy::{asset::AssetPlugin, render::render_resource::Face};

    use super::*;

    /// Culling off has to survive the stylizer's rewrite and must not wake
    /// itself: after it has settled, a frame emits no further change.
    #[test]
    fn unculling_catches_added_and_modified_materials_and_then_rests() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .add_systems(Update, uncull_materials);
        let handle = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        app.update();
        app.update();
        let read = |app: &App| {
            let materials = app.world().resource::<Assets<StandardMaterial>>();
            let m = materials.get(&handle).expect("material");
            (m.cull_mode, m.double_sided)
        };
        assert_eq!(read(&app), (None, true));

        // The stylizer's rewrite: a whole new material under the same handle.
        if let Some(mut m) = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .get_mut(&handle)
        {
            *m = body_material();
        }
        assert_eq!(read(&app), (Some(Face::Back), false));
        app.update();
        app.update();
        assert_eq!(read(&app), (None, true));
    }

    #[test]
    fn the_ground_follows_the_subject_down_but_never_frames() {
        let mut world = World::new();
        world.spawn((
            Transform::from_xyz(0.0, -GROUND_CLEARANCE, 0.0),
            Ground,
            NoFrameBounds,
        ));
        ground_under(&mut world, -0.5);
        let mut query = world.query_filtered::<&Transform, With<Ground>>();
        let y = query.single(&world).expect("one ground").translation.y;
        assert!((y - (-0.5 - GROUND_CLEARANCE)).abs() < 1e-6);
    }

    #[test]
    fn bounds_skip_the_set_and_rotate_corners() {
        let mut world = World::new();
        let cube = Aabb::from_min_max(Vec3::splat(-0.5), Vec3::splat(0.5));
        world.spawn((
            cube,
            GlobalTransform::from(Transform::from_rotation(Quat::from_rotation_y(
                45.0_f32.to_radians(),
            ))),
        ));
        world.spawn((
            Aabb::from_min_max(Vec3::splat(-20.0), Vec3::splat(20.0)),
            GlobalTransform::IDENTITY,
            NoFrameBounds,
        ));
        let (lo, hi) = world_bounds(&mut world).expect("the cube has bounds");
        // A cube turned 45° about Y is √2 wide on X and Z, still 1 on Y.
        assert!((hi.x - lo.x - std::f32::consts::SQRT_2).abs() < 1e-4);
        assert!((hi.y - lo.y - 1.0).abs() < 1e-5);
    }
}
