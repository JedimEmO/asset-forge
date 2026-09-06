//! Screen-space hit feedback, projected from scrolling combat positions.
use crate::{
    art::GameCamera,
    sim::{Game, Phase},
};
use bevy::{prelude::*, transform::TransformSystems, world_serialization::WorldInstanceReady};
use std::collections::HashMap;

pub struct CombatFeedbackPlugin;
#[derive(Component)]
pub struct EnemySurface;
#[derive(Component)]
struct Feedback {
    id: u64,
    locator: bool,
}
#[derive(Resource)]
struct FeedbackFont(Handle<Font>);
impl Plugin for CombatFeedbackPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            |mut commands: Commands, server: Res<AssetServer>| {
                commands.insert_resource(FeedbackFont(server.load("fonts/Lato-Bold.ttf")));
            },
        )
        .add_observer(highlight)
        .add_systems(PostUpdate, present.after(TransformSystems::Propagate));
    }
}
fn highlight(
    event: On<WorldInstanceReady>,
    roots: Query<(), With<EnemySurface>>,
    children: Query<&Children>,
    mut meshes: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: Local<HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>>,
) {
    if !roots.contains(event.entity) {
        return;
    }
    for entity in children.iter_descendants(event.entity) {
        let Ok(mut handle) = meshes.get_mut(entity) else {
            continue;
        };
        let original = handle.0.id();
        if let Some(h) = cache.get(&original) {
            handle.0 = h.clone();
            continue;
        }
        let Some(mut material) = materials.get(&handle.0).cloned() else {
            continue;
        };
        // Texture-modulated fill preserves authored detail and doesn't brighten shared sky ships.
        material.emissive_texture = material.base_color_texture.clone();
        material.emissive = LinearRgba::new(0.85, 0.65, 0.48, 1.);
        let h = materials.add(material);
        cache.insert(original, h.clone());
        handle.0 = h;
    }
}
fn present(
    mut commands: Commands,
    game: Res<Game>,
    font: Res<FeedbackFont>,
    cameras: Query<(&Camera, &GlobalTransform), With<GameCamera>>,
    mut texts: Query<(
        Entity,
        &Feedback,
        &mut Node,
        &mut TextColor,
        &mut Visibility,
    )>,
) {
    let Ok((camera, transform)) = cameras.single() else {
        return;
    };
    let active = matches!(game.phase, Phase::Playing | Phase::Paused);
    let mut rows = Vec::new();
    if active {
        for n in &game.damage_numbers {
            let spread = ((n.id % 5) as f32 - 2.) * 12.;
            rows.push((
                n.id,
                false,
                n.pos,
                Vec2::new(18. + spread, -22. - n.age * 65.),
                if n.critical {
                    format!("{} CRIT", n.amount)
                } else {
                    n.amount.to_string()
                },
                if n.critical {
                    Color::srgb(1., 0.78, 0.25)
                } else {
                    Color::srgb(0.92, 0.97, 1.)
                },
                if n.critical { 25. } else { 19. },
                ((0.85 - n.age) / 0.3).clamp(0., 1.),
            ));
        }
        for e in &game.enemies {
            if e.pos.z < -3. && e.pos.z > -65. {
                rows.push((
                    e.id,
                    true,
                    e.pos + Vec3::Y * 2.65,
                    Vec2::new(-5., -6.),
                    "v".into(),
                    Color::srgb(1., 0.52, 0.25),
                    17.,
                    0.8,
                ));
            }
        }
    }
    for (entity, feedback, _, _, _) in &texts {
        if !rows
            .iter()
            .any(|r| r.0 == feedback.id && r.1 == feedback.locator)
        {
            commands.entity(entity).despawn();
        }
    }
    for (id, locator, pos, offset, label, color, size, alpha) in rows {
        let projected = camera.world_to_viewport(transform, pos).ok();
        let visible = projected.is_some_and(|p| {
            camera
                .logical_viewport_size()
                .is_some_and(|s| p.x > 0. && p.y > 0. && p.x < s.x - 80. && p.y < s.y - 30.)
        });
        let p = projected.unwrap_or_default() + offset;
        let visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if let Some((_, _, mut node, mut tint, mut vis)) = texts
            .iter_mut()
            .find(|(_, f, _, _, _)| f.id == id && f.locator == locator)
        {
            node.left = px(p.x);
            node.top = px(p.y);
            tint.0 = color.with_alpha(alpha);
            *vis = visibility;
        } else {
            commands.spawn((
                Feedback { id, locator },
                Node {
                    position_type: PositionType::Absolute,
                    left: px(p.x),
                    top: px(p.y),
                    ..default()
                },
                Text::new(label),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: FontSize::Px(size),
                    ..default()
                },
                TextColor(color.with_alpha(alpha)),
                TextShadow {
                    offset: Vec2::splat(1.5),
                    color: Color::BLACK,
                },
                GlobalZIndex(8),
                visibility,
            ));
        }
    }
}
