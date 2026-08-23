//! The window itself: what it is called, and how to photograph it.
//!
//! Only these two pieces live here because they are about the *window* rather
//! than about any panel in it; everything drawn inside the window — the
//! library column, the transport, the metadata panel, the audio player — is
//! [`crate::studio`]'s.

use std::path::PathBuf;

use bevy::prelude::*;

/// Capture the window once and quit — a self-check for the studio's own UI.
///
/// Useful because a windowed app is otherwise unverifiable without a human at
/// the keyboard, and because it works under a virtual display in CI.
#[derive(Resource, Debug, Clone)]
pub struct AutoScreenshot {
    /// Where to write the PNG.
    pub path: PathBuf,
    /// Frames to let the scene load and settle first.
    pub after_frames: u32,
}

/// Wait for the scene to settle, grab the window, then quit.
pub fn auto_screenshot(
    mut commands: Commands,
    shot: Option<Res<AutoScreenshot>>,
    mut frame: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(shot) = shot else {
        return;
    };
    *frame += 1;
    if *frame == shot.after_frames {
        commands
            .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
            .observe(bevy::render::view::screenshot::save_to_disk(
                shot.path.clone(),
            ));
    }
    // The save is asynchronous, so leave room for the readback to land before
    // tearing the app down.
    if *frame > shot.after_frames + 60 {
        exit.write(AppExit::Success);
    }
}

/// Window title showing what is being inspected.
///
/// The model when there is one, the project alone when there is not: opened
/// on audio the stage is empty on purpose, and a title bar reading
/// "forge studio — " helps nobody. Takes the two strings rather than the
/// studio's config so the window's naming does not depend on how the panels
/// are configured.
#[must_use]
pub fn window_title(project: &str, model: Option<&str>) -> String {
    match model.map(str::trim).filter(|m| !m.is_empty()) {
        Some(model) => format!("forge studio — {project} — {model}"),
        None => format!("forge studio — {project}"),
    }
}

#[cfg(test)]
mod tests {
    use super::window_title;

    #[test]
    fn the_title_names_the_model_only_when_there_is_one() {
        assert_eq!(
            window_title("sample", Some("bodies/vex_runner.glb")),
            "forge studio — sample — bodies/vex_runner.glb"
        );
        assert_eq!(window_title("sample", Some("  ")), "forge studio — sample");
        assert_eq!(window_title("sample", None), "forge studio — sample");
    }
}
