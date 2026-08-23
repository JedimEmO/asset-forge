//! Head close-ups of a character glb — the review loop for generated faces.
//!
//! The `forge rig check` contact sheet frames the whole body, which makes a face two
//! dozen pixels tall; judging eye sockets and lips needs a camera half a
//! metre from the head. Renders front, three-quarter and profile PNGs.
//!
//! ```sh
//! cargo run -p forge_capture --example portrait -- model.glb out_prefix
//! ```

use bevy::prelude::*;
use bevy::world_serialization::{WorldAsset, WorldAssetRoot};
use forge_capture::{CaptureApp, CaptureSettings, save_png};

fn main() {
    let mut args = std::env::args().skip(1);
    let glb: std::path::PathBuf = args
        .next()
        .expect("usage: portrait <model.glb> <out_prefix>")
        .into();
    let prefix = args
        .next()
        .expect("usage: portrait <model.glb> <out_prefix>");
    let glb = glb.canonicalize().expect("model path exists");
    let asset_root = glb.parent().expect("model has a parent dir").to_path_buf();
    let file = glb
        .file_name()
        .expect("model is a file")
        .to_string_lossy()
        .into_owned();

    let mut app = CaptureApp::new(
        CaptureSettings {
            width: 640,
            height: 640,
            asset_root: Some(asset_root.to_string_lossy().into_owned()),
            ..CaptureSettings::default()
        },
        |app| {
            app.add_systems(Startup, light);
            // Generated characters carry no material, and bevy_gltf's
            // default for that is metallic 1.0 — which reads as chrome and
            // hides the painted face. Force the style guide's matte surface
            // here so portrait reviews judge the vertex colors, not a sheen.
            app.add_systems(Update, matte);
        },
    );

    let scene: Handle<WorldAsset> = app
        .world_mut()
        .resource::<AssetServer>()
        .load(format!("{file}#Scene0"));
    app.world_mut().spawn(WorldAssetRoot(scene));

    // Let the scene instance land before framing: a few update ticks are
    // enough for a local file, and the mesh count says when it arrived.
    for _ in 0..120 {
        app.update();
        let mut q = app.world_mut().query::<&Mesh3d>();
        if q.iter(app.world_mut()).count() > 0 {
            break;
        }
    }

    // The stylized skull grows downward from the pinned 1.80 m crown and
    // stands ~0.30 m tall, so the frame centres mid-face and stands back
    // far enough to keep the chin and crown both in shot.
    let focus = Vec3::new(0.0, 1.64, 0.03);
    let camera = app.spawn_camera_target(
        Transform::from_translation(focus + Vec3::new(0.0, 0.0, 0.60)).looking_at(focus, Vec3::Y),
        Msaa::Sample4,
    );

    let shots: [(&str, Vec3); 3] = [
        ("front", Vec3::new(0.0, 0.01, 0.46)),
        ("three_quarter", Vec3::new(0.30, 0.04, 0.36)),
        ("profile", Vec3::new(0.46, 0.01, 0.02)),
    ];
    for (name, offset) in shots {
        let transform = Transform::from_translation(focus + offset).looking_at(focus, Vec3::Y);
        *app.world_mut()
            .get_mut::<Transform>(camera)
            .expect("camera exists") = transform;
        app.update();
        match app.capture() {
            Ok(frame) => {
                let path = format!("{prefix}_{name}.png");
                save_png(&frame, &path).expect("png saves");
                println!("wrote {path}");
            }
            Err(err) => {
                eprintln!("FAIL: {err}");
                std::process::exit(1);
            }
        }
    }
}

fn matte(mut materials: ResMut<Assets<StandardMaterial>>) {
    for (_, material) in materials.iter_mut() {
        material.metallic = 0.0;
        material.perceptual_roughness = 0.9;
    }
}

fn light(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 11_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(1.5, 3.0, 2.5).looking_at(Vec3::new(0.0, 1.6, 0.0), Vec3::Y),
    ));
    // A cool fill from the other side keeps the shadowed cheek readable.
    commands.spawn((
        DirectionalLight {
            illuminance: 2_500.0,
            ..default()
        },
        Transform::from_xyz(-2.0, 2.0, 1.0).looking_at(Vec3::new(0.0, 1.6, 0.0), Vec3::Y),
    ));
}
