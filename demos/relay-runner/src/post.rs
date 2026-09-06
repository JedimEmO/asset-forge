//! Camera-only grading and combat lens effects. UI is composited afterwards.
use crate::{Options, Set, art::GameCamera, sim::Game};
use bevy::{
    post_process::{
        bloom::Bloom,
        dof::{DepthOfField, DepthOfFieldMode},
        effect_stack::{ChromaticAberration, LensDistortion, Vignette},
        motion_blur::MotionBlur,
    },
    prelude::*,
    render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection},
};

pub struct PostPlugin;
impl Plugin for PostPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, setup)
            .add_systems(Update, update.after(Set::Present));
    }
}
fn setup(mut commands: Commands, camera: Single<Entity, With<GameCamera>>) {
    commands.entity(*camera).insert((
        DepthOfField {
            mode: DepthOfFieldMode::Bokeh,
            focal_distance: 10.,
            sensor_height: 0.05,
            aperture_f_stops: 2.8,
            max_circle_of_confusion_diameter: 2.5,
            max_depth: 120.,
        },
        MotionBlur {
            shutter_angle: 0.26,
            samples: 3,
        },
        Vignette {
            intensity: 0.2,
            radius: 0.9,
            smoothness: 2.2,
            ..default()
        },
        ChromaticAberration {
            intensity: 0.,
            max_samples: 4,
            ..default()
        },
        LensDistortion {
            intensity: 0.,
            ..default()
        },
    ));
}
type LensQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut ColorGrading,
        &'static mut Vignette,
        &'static mut ChromaticAberration,
        &'static mut LensDistortion,
        &'static mut Bloom,
        &'static mut DepthOfField,
        &'static mut MotionBlur,
    ),
    With<GameCamera>,
>;
fn update(
    game: Res<Game>,
    options: Res<Options>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut enabled: Local<Option<bool>>,
    mut cameras: LensQuery,
) {
    let enabled = enabled.get_or_insert(options.post);
    if options.frames == 0 && keys.just_pressed(KeyCode::F6) {
        *enabled = !*enabled;
    }
    for (mut grading, mut vignette, mut fringe, mut lens, mut bloom, mut dof, mut blur) in
        &mut cameras
    {
        let focal_target = if game.phase == crate::sim::Phase::Title {
            4.5
        } else if game.focus {
            game.enemies
                .iter()
                .filter_map(|e| {
                    let delta = e.pos + Vec3::Y - game.camera();
                    let distance = delta.dot(game.aim);
                    (distance > 3. && delta.cross(game.aim).length() < 2.4).then_some(distance)
                })
                .min_by(f32::total_cmp)
                .unwrap_or(22.)
                .clamp(6., 45.)
        } else {
            10.
        };
        dof.focal_distance +=
            (focal_target - dof.focal_distance) * (1. - (-time.delta_secs() * 5.).exp());
        dof.aperture_f_stops = if game.focus {
            0.9
        } else if game.phase == crate::sim::Phase::Title {
            1.2
        } else {
            2.8
        };
        dof.max_circle_of_confusion_diameter = if game.focus { 5. } else { 2.5 };
        blur.shutter_angle = if game.phase != crate::sim::Phase::Playing || game.time < 0.2 {
            0.
        } else if game.focus {
            0.06
        } else if game.dash_time > 0. {
            0.48
        } else {
            0.26
        };
        blur.samples = 3;
        let pulse = (game.nova_flash / 0.7).clamp(0., 1.);
        let drive = if game.overdrive > 0. {
            (game.overdrive * 2.).min(1.)
        } else {
            0.
        };
        let hurt = (game.hurt / 0.65).clamp(0., 1.);
        if !*enabled {
            dof.aperture_f_stops = 10000.;
            blur.samples = 0;
            *grading = ColorGrading::default();
            vignette.intensity = 0.;
            fringe.intensity = 0.;
            lens.intensity = 0.;
            bloom.intensity = 0.20;
            continue;
        }
        *grading = ColorGrading {
            global: ColorGradingGlobal {
                exposure: 0.08 + pulse * 0.13,
                temperature: -0.004,
                post_saturation: 1.06 + drive * 0.06,
                ..default()
            },
            shadows: ColorGradingSection {
                saturation: 0.87,
                gamma: 0.96,
                lift: 0.,
                ..default()
            },
            midtones: ColorGradingSection {
                gamma: 0.98,
                saturation: 1.03,
                ..default()
            },
            highlights: ColorGradingSection {
                saturation: 0.92,
                gain: 1.02,
                ..default()
            },
        };
        vignette.intensity = 0.20 + hurt * 0.18 + drive * 0.05;
        vignette.color = if hurt > 0. {
            Color::srgb(0.09, 0.006, 0.013)
        } else {
            Color::srgb(0.003, 0.009, 0.025)
        };
        // Zero at rest. Effects grow radially, preserving the central aiming ray.
        fringe.intensity = pulse * 0.0035 + hurt * 0.0018 + drive * 0.0006;
        lens.intensity = pulse * 0.035;
        bloom.intensity = 0.24 + pulse * 0.14 + drive * 0.035;
    }
}
