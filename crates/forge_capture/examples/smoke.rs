//! Proves offscreen rendering works with no window and no display server.
//!
//! Run it with the display explicitly removed from the environment — if this
//! passes, nothing in the capture path needs X11 or Wayland:
//!
//! ```sh
//! env -u DISPLAY -u WAYLAND_DISPLAY cargo run -p forge_capture --example smoke
//! ```
//!
//! Prints the GPU adapter it selected, because "it rendered" and "it rendered
//! on the GPU you expected" are different claims — a silent fall back to
//! llvmpipe still produces a valid PNG, just slowly.

use bevy::{prelude::*, render::renderer::RenderAdapterInfo};
use forge_capture::{CaptureApp, CaptureSettings, ink_fraction, save_png};

fn main() -> std::process::ExitCode {
    let mut app = CaptureApp::new(
        CaptureSettings {
            width: 512,
            height: 512,
            ..CaptureSettings::default()
        },
        |app| {
            app.add_systems(Startup, spawn_scene);
        },
    );

    app.spawn_camera_target(
        Transform::from_xyz(3.5, 3.0, 6.0).looking_at(Vec3::new(0.0, 0.75, 0.0), Vec3::Y),
        Msaa::Sample4,
    );

    let adapter = app
        .world_mut()
        .get_resource::<RenderAdapterInfo>()
        .map_or_else(|| "<unknown>".to_owned(), |info| info.name.clone());

    let frame = match app.capture() {
        Ok(frame) => frame,
        Err(err) => {
            eprintln!("FAIL: {err}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let ink = ink_fraction(&frame);
    let bytes = frame.data.as_ref().map_or(0, Vec::len);
    println!("adapter: {adapter}");
    println!(
        "frame:   {}x{}, {bytes} bytes, ink {:.1}%",
        frame.width(),
        frame.height(),
        ink * 100.0
    );
    describe_pixels(&frame);

    if ink < 0.01 {
        eprintln!("FAIL: frame is {:.2}% ink — nothing rendered", ink * 100.0);
        return std::process::ExitCode::FAILURE;
    }

    if let Some(path) = std::env::args().nth(1) {
        match save_png(&frame, &path) {
            Ok(()) => println!("wrote:   {path}"),
            Err(err) => {
                eprintln!("FAIL: {err}");
                return std::process::ExitCode::FAILURE;
            }
        }
    }

    println!("OK: rendered offscreen with no display server");
    std::process::ExitCode::SUCCESS
}

/// Enough detail to tell "nothing rendered" apart from "rendered but uniform".
fn describe_pixels(frame: &Image) {
    let Some(data) = frame.data.as_ref() else {
        println!("pixels:  <no data>");
        return;
    };
    let w = frame.width() as usize;
    let centre = ((frame.height() as usize / 2) * w + w / 2) * 4;
    let mut lo = [255u8; 4];
    let mut hi = [0u8; 4];
    let mut distinct = std::collections::HashSet::new();
    for px in data.chunks_exact(4) {
        for c in 0..4 {
            lo[c] = lo[c].min(px[c]);
            hi[c] = hi[c].max(px[c]);
        }
        if distinct.len() < 64 {
            distinct.insert([px[0], px[1], px[2], px[3]]);
        }
    }
    println!(
        "pixels:  corner={:?} centre={:?}",
        &data[0..4],
        &data[centre..centre + 4]
    );
    println!(
        "         min={lo:?} max={hi:?} distinct>={}",
        distinct.len()
    );
}

fn spawn_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.5, 1.5, 1.5))),
        MeshMaterial3d(materials.add(Color::srgb(0.85, 0.45, 0.25))),
        Transform::from_xyz(0.0, 0.75, 0.0),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(12.0, 12.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.30, 0.32, 0.36))),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
