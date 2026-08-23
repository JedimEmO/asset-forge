//! Colours and type sizes for the viewer's UI.
//!
//! Hand-rolled rather than pulled from a UI toolkit: the viewer needs a list, a
//! few buttons and a scrub bar, and a whole immediate-mode dependency to draw
//! them would be the largest thing in the crate.

use bevy::prelude::*;

/// Panel background, deliberately opaque so 3D behind it cannot distract.
pub const PANEL: Color = Color::srgba(0.09, 0.09, 0.11, 0.94);
/// Divider and outline colour.
pub const BORDER: Color = Color::srgb(0.22, 0.23, 0.27);
/// Primary text.
pub const TEXT: Color = Color::srgb(0.90, 0.91, 0.94);
/// Secondary text: metadata labels, hints.
pub const TEXT_DIM: Color = Color::srgb(0.55, 0.57, 0.63);
/// The selected clip, and the scrub-bar fill.
pub const ACCENT: Color = Color::srgb(0.36, 0.68, 0.95);
/// A row under the cursor.
pub const HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.07);
/// A pressed control.
pub const PRESSED: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);
/// Anything not currently active.
pub const IDLE: Color = Color::NONE;
/// Warning text, for a clip that binds to nothing.
pub const WARN: Color = Color::srgb(0.95, 0.55, 0.35);

/// Body text size.
pub const FONT: f32 = 13.0;
/// Section heading size.
pub const FONT_SMALL: f32 = 11.0;

/// Width of the studio's left column: the tab bar and the library list.
pub const LEFT_PANEL_WIDTH: f32 = 250.0;
/// Width of the studio's right column: the metadata panel.
///
/// Wider than the left one because a record's rows are a label beside a
/// value, and a recipe line or a hash at 250 wraps into something nobody can
/// read; the width was set when the forge's sliders lived here and the
/// metadata panel inherited it.
pub const RIGHT_PANEL_WIDTH: f32 = 300.0;
/// Height of the transport bar.
pub const TRANSPORT_HEIGHT: f32 = 54.0;

/// A text bundle at the given size and colour.
#[must_use]
pub fn label(text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: size.into(),
            ..default()
        },
        TextColor(color),
    )
}
