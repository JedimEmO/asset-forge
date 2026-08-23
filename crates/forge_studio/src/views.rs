//! Camera placements around a subject.

use bevy::prelude::*;

/// Where the camera sits relative to the subject.
///
/// Angles are chosen to match the stick-figure contact sheets animation review
/// tools conventionally produce, so a rendered sheet and a plotted one can be
/// compared without re-orienting your mental model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Three-quarter front-left. The default: reads depth and silhouette at once.
    ThreeQuarter,
    /// Straight on, from the direction the subject faces.
    Front,
    /// From directly behind.
    Back,
    /// The subject's left side.
    Left,
    /// The subject's right side.
    Right,
    /// Looking down. Useful for foot placement and travel direction.
    Top,
}

impl View {
    /// All views, in a stable order.
    pub const ALL: [Self; 6] = [
        Self::ThreeQuarter,
        Self::Front,
        Self::Back,
        Self::Left,
        Self::Right,
        Self::Top,
    ];

    /// The four walk-around views a mesh is looked at from before it is
    /// rigged: the whole figure from the front, from behind and from each
    /// side. One front view underdetermines a shape; these four are the orbit
    /// a reviewer would otherwise have to do by hand in the studio.
    pub const WALK_AROUND: [Self; 4] = [Self::Front, Self::Back, Self::Left, Self::Right];

    /// Parse a lowercase name such as `three_quarter` or `front`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "three_quarter" | "threequarter" | "hero" | "3q" => Some(Self::ThreeQuarter),
            "front" => Some(Self::Front),
            "back" => Some(Self::Back),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "top" => Some(Self::Top),
            _ => None,
        }
    }

    /// The name [`View::parse`] accepts.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ThreeQuarter => "three_quarter",
            Self::Front => "front",
            Self::Back => "back",
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
        }
    }

    /// Yaw and pitch in degrees. Yaw 0 faces the subject head-on.
    #[must_use]
    pub fn yaw_pitch_deg(self) -> (f32, f32) {
        match self {
            Self::ThreeQuarter => (38.0, 10.0),
            Self::Front => (0.0, 6.0),
            Self::Back => (180.0, 6.0),
            Self::Left => (-90.0, 6.0),
            Self::Right => (90.0, 6.0),
            Self::Top => (38.0, 72.0),
        }
    }

    /// Unit vector from the subject's centre toward the camera.
    ///
    /// Subjects face **-Z**, so yaw 0 puts the camera on the -Z side looking
    /// back at the face.
    #[must_use]
    pub fn direction(self) -> Vec3 {
        let (yaw, pitch) = self.yaw_pitch_deg();
        direction_from(yaw, pitch)
    }

    /// Camera transform framing a sphere of `radius` about `centre`.
    ///
    /// `vertical_fov_deg` must match the camera's own field of view, and
    /// `padding` leaves headroom so a limb at full extension does not touch the
    /// frame edge.
    #[must_use]
    pub fn camera_transform(
        self,
        centre: Vec3,
        radius: f32,
        vertical_fov_deg: f32,
        padding: f32,
    ) -> Transform {
        frame_sphere(self.direction(), centre, radius, vertical_fov_deg, padding)
    }
}

/// A close-up of the top of the subject — the head, on a character.
///
/// These three exist because of a specific wound: a single-image lift can
/// leave the rear of the skull simply absent, and every downstream gate — fit
/// check, bone heat, rig check, the walk sheet — passed that mesh. Only a
/// human orbiting the studio camera caught it, three re-rigs later. The back
/// and back-top close-ups are that orbit, run before the GPU minute and the
/// rig minute are spent. They are a separate type from [`View`] because they
/// are framed on a different sphere: the top of the bounds, not the whole of
/// them, and a [`View`] used as a head close-up would be a second framing
/// convention hiding behind one name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadView {
    /// The face, looking very slightly down.
    Front,
    /// The back of the skull, looking very slightly down.
    Back,
    /// The back of the skull from behind and above, where a missing crown
    /// shows as the inside of the face.
    BackTop,
}

impl HeadView {
    /// All head views, in the order they appear on a sheet.
    pub const ALL: [Self; 3] = [Self::Front, Self::Back, Self::BackTop];

    /// Parse a lowercase name such as `head_front`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "head_front" => Some(Self::Front),
            "head_back" => Some(Self::Back),
            "head_back_top" => Some(Self::BackTop),
            _ => None,
        }
    }

    /// The name [`HeadView::parse`] accepts.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Front => "head_front",
            Self::Back => "head_back",
            Self::BackTop => "head_back_top",
        }
    }

    /// Yaw and pitch in degrees. The front and back views look down by a
    /// few degrees on purpose — a crown is only visible from above the
    /// horizon — and the back-top view looks down at 45°.
    #[must_use]
    pub fn yaw_pitch_deg(self) -> (f32, f32) {
        match self {
            Self::Front => (0.0, 3.0),
            Self::Back => (180.0, 3.0),
            Self::BackTop => (180.0, 45.0),
        }
    }

    /// Unit vector from the head's centre toward the camera.
    #[must_use]
    pub fn direction(self) -> Vec3 {
        let (yaw, pitch) = self.yaw_pitch_deg();
        direction_from(yaw, pitch)
    }

    /// Camera transform framing a sphere of `radius` about `centre`.
    #[must_use]
    pub fn camera_transform(
        self,
        centre: Vec3,
        radius: f32,
        vertical_fov_deg: f32,
        padding: f32,
    ) -> Transform {
        frame_sphere(self.direction(), centre, radius, vertical_fov_deg, padding)
    }
}

/// Unit vector toward a camera at `yaw` degrees about Y and `pitch` degrees
/// above the horizon, with yaw 0 on the -Z side.
fn direction_from(yaw_deg: f32, pitch_deg: f32) -> Vec3 {
    let (yaw, pitch) = (yaw_deg.to_radians(), pitch_deg.to_radians());
    Vec3::new(
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        -yaw.cos() * pitch.cos(),
    )
    .normalize()
}

/// A camera on `direction` from `centre`, far enough back that a sphere of
/// `radius` fills the vertical field of view with `padding` to spare.
fn frame_sphere(
    direction: Vec3,
    centre: Vec3,
    radius: f32,
    vertical_fov_deg: f32,
    padding: f32,
) -> Transform {
    let half_fov = (vertical_fov_deg * 0.5).to_radians();
    let distance = (radius * padding / half_fov.tan()).max(0.1);
    Transform::from_translation(centre + direction * distance).looking_at(centre, Vec3::Y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_round_trips() {
        for view in View::ALL {
            assert_eq!(View::parse(view.name()), Some(view));
        }
        for view in HeadView::ALL {
            assert_eq!(HeadView::parse(view.name()), Some(view));
            assert_eq!(View::parse(view.name()), None, "{}", view.name());
        }
    }

    #[test]
    fn front_sits_on_minus_z_and_back_on_plus_z() {
        assert!(View::Front.direction().z < -0.9);
        assert!(View::Back.direction().z > 0.9);
        assert!(HeadView::Front.direction().z < -0.9);
        assert!(HeadView::BackTop.direction().y > 0.6);
        assert!(HeadView::BackTop.direction().z > 0.6);
    }

    #[test]
    fn a_bigger_sphere_pushes_the_camera_back() {
        let near = View::Front.camera_transform(Vec3::ZERO, 0.5, 30.0, 1.12);
        let far = View::Front.camera_transform(Vec3::ZERO, 1.0, 30.0, 1.12);
        assert!(far.translation.length() > near.translation.length() * 1.9);
    }
}
