//! A drag-to-orbit camera for the viewer.

use bevy::{input::mouse::AccumulatedMouseScroll, prelude::*};

use crate::stage::{CAMERA_FOV_DEG, FRAME_PADDING};

/// Orbit state for the viewer camera.
#[derive(Component, Debug, Clone)]
pub struct OrbitCamera {
    /// Point the camera looks at and rotates about.
    pub focus: Vec3,
    /// Distance from the focus.
    pub distance: f32,
    /// Rotation about the world Y axis, radians.
    pub yaw: f32,
    /// Rotation above the horizon, radians.
    pub pitch: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            focus: Vec3::new(0.0, 1.0, 0.0),
            distance: 5.0,
            yaw: 38.0_f32.to_radians(),
            pitch: 10.0_f32.to_radians(),
        }
    }
}

impl OrbitCamera {
    /// Frame a sphere of `radius` about `centre` at the shared lens setting, so
    /// the viewer opens on the same shot the headless renderer would produce.
    pub fn frame(&mut self, centre: Vec3, radius: f32) {
        let half_fov = (CAMERA_FOV_DEG * 0.5).to_radians();
        self.focus = centre;
        self.distance = (radius * FRAME_PADDING / half_fov.tan()).max(0.2);
    }

    /// Camera transform for the current orbit state.
    #[must_use]
    pub fn transform(&self) -> Transform {
        // Yaw 0 places the camera on -Z: the face of a clip-posed subject
        // (baked clips play facing -Z), the back of one at rest (+Z). The
        // studio's usual stage is a body with a clip bound, so the default
        // yaw opens on a three-quarter front of what plays.
        let dir = Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            -self.yaw.cos() * self.pitch.cos(),
        );
        Transform::from_translation(self.focus + dir * self.distance)
            .looking_at(self.focus, Vec3::Y)
    }
}

/// True while the pointer is over UI that should swallow drags.
#[derive(Resource, Default, Debug)]
pub struct PointerOverUi(pub bool);

/// Drag to orbit, scroll to zoom.
pub fn orbit_camera(
    mut cameras: Query<(&mut OrbitCamera, &mut Transform)>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<bevy::input::mouse::AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    over_ui: Res<PointerOverUi>,
    mut dragging: Local<bool>,
) {
    // Latch the drag on press so that sweeping the cursor over a panel
    // mid-orbit does not abruptly stop the camera.
    if mouse.just_pressed(MouseButton::Left) && !over_ui.0 {
        *dragging = true;
    }
    if !mouse.pressed(MouseButton::Left) {
        *dragging = false;
    }

    for (mut orbit, mut transform) in &mut cameras {
        let mut changed = false;
        if *dragging && motion.delta != Vec2::ZERO {
            orbit.yaw -= motion.delta.x * 0.006;
            orbit.pitch = (orbit.pitch + motion.delta.y * 0.006)
                // Stop short of the poles: looking straight down the Y axis
                // makes `looking_at` ambiguous and the view snaps.
                .clamp(-1.45, 1.45);
            changed = true;
        }
        if scroll.delta.y != 0.0 && !over_ui.0 {
            orbit.distance = (orbit.distance * (1.0 - scroll.delta.y * 0.12)).clamp(0.3, 60.0);
            changed = true;
        }
        if changed {
            *transform = orbit.transform();
        }
    }
}
