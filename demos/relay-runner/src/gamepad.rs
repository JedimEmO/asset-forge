use bevy::prelude::*;

#[derive(Default)]
pub struct PadInput {
    pub strafe: f32,
    pub look: Vec2,
    pub fire: bool,
    pub focus: bool,
    pub jump: bool,
    pub dash: bool,
    pub nova: bool,
    pub secondary: bool,
    pub reload: bool,
    pub start: bool,
}

// Radial dead zone preserves direction and restores the full analog range.
fn stick(value: Vec2) -> Vec2 {
    let length = value.length();
    if length <= 0.18 {
        Vec2::ZERO
    } else {
        value / length * ((length - 0.18) / 0.82).min(1.)
    }
}

pub fn read(pad: &Gamepad) -> PadInput {
    use GamepadButton::*;
    PadInput {
        strafe: (stick(pad.left_stick()).x + f32::from(pad.pressed(DPadRight))
            - f32::from(pad.pressed(DPadLeft)))
        .clamp(-1., 1.),
        look: stick(pad.right_stick()),
        fire: pad.pressed(RightTrigger2),
        focus: pad.pressed(LeftTrigger2),
        jump: pad.just_pressed(South),
        dash: pad.just_pressed(East),
        nova: pad.just_pressed(North),
        secondary: pad.just_pressed(RightTrigger),
        reload: pad.just_pressed(West),
        start: pad.just_pressed(Start),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_zone_preserves_direction_and_analog_range() {
        assert_eq!(stick(Vec2::new(0.1, -0.1)), Vec2::ZERO);
        assert_eq!(stick(Vec2::X), Vec2::X);
        let diagonal = stick(Vec2::splat(1.));
        assert!((diagonal.length() - 1.).abs() < 0.0001);
        let partial = stick(Vec2::new(0.59, 0.));
        assert!((partial.x - 0.5).abs() < 0.0001);
    }

    #[test]
    fn actions_are_edges_while_triggers_remain_held() {
        let mut pad = Gamepad::default();
        for button in [
            GamepadButton::South,
            GamepadButton::East,
            GamepadButton::North,
            GamepadButton::West,
            GamepadButton::RightTrigger,
            GamepadButton::Start,
            GamepadButton::RightTrigger2,
            GamepadButton::LeftTrigger2,
        ] {
            pad.digital_mut().press(button);
        }
        let input = read(&pad);
        assert!(
            input.jump
                && input.dash
                && input.nova
                && input.reload
                && input.secondary
                && input.start
        );
        assert!(input.fire && input.focus);
        pad.digital_mut().clear();
        let input = read(&pad);
        assert!(
            !input.jump
                && !input.dash
                && !input.nova
                && !input.reload
                && !input.secondary
                && !input.start
        );
        assert!(input.fire && input.focus);
    }
}
