//! Browser metadata is copied verbatim from the accepted v16 package.
//!
//! GPU/audio payloads still load asynchronously through AssetServer. The small
//! synchronous metadata snapshot avoids native filesystem access during Startup.
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
pub fn read(root: &Path, relative: &str) -> serde_json::Value {
    let path = root.join(relative);
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("invalid metadata {}: {error}", path.display()))
}

#[cfg(any(target_arch = "wasm32", test))]
fn embedded(relative: &str) -> &'static str {
    match relative {
        "grounding.json" => include_str!("../web-metadata/grounding.json"),
        "attachment.json" => include_str!("../web-metadata/attachment.json"),
        "scavenger/library.json" => include_str!("../web-metadata/scavenger/library.json"),
        "showcase/library.json" => include_str!("../web-metadata/showcase/library.json"),
        _ => panic!("metadata missing from browser snapshot: {relative}"),
    }
}

#[cfg(target_arch = "wasm32")]
pub fn read(_root: &Path, relative: &str) -> serde_json::Value {
    serde_json::from_str(embedded(relative)).expect("valid embedded runtime metadata")
}

#[cfg(not(target_arch = "wasm32"))]
pub fn exists(root: &Path, relative: &str) -> bool {
    root.join(relative).exists()
}

#[cfg(target_arch = "wasm32")]
pub fn exists(_root: &Path, relative: &str) -> bool {
    // The deployment builder checks these hashes against the actual payload.
    // A present inventory entry is only a declaration: Ready still waits for
    // AssetServer to load the scene and every dependency successfully.
    use std::sync::LazyLock;
    static INVENTORY: LazyLock<serde_json::Value> = LazyLock::new(|| {
        serde_json::from_str(include_str!("../web-metadata/asset-sha256.json"))
            .expect("valid browser asset inventory")
    });
    INVENTORY.get(relative).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_snapshot_contains_gameplay_metadata_and_all_enemy_models() {
        let library: serde_json::Value =
            serde_json::from_str(embedded("showcase/library.json")).unwrap();
        let inventory: serde_json::Value =
            serde_json::from_str(include_str!("../web-metadata/asset-sha256.json")).unwrap();
        for name in ["relay_drone", "relay_interceptor", "relay_heavy"] {
            let row = library["models"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["name"] == name)
                .unwrap();
            assert!(row["bounds_m"].is_array());
            assert!(inventory[format!("showcase/models/{name}.glb")].is_string());
        }
        for relative in [
            "grounding.json",
            "attachment.json",
            "scavenger/library.json",
        ] {
            let value: serde_json::Value = serde_json::from_str(embedded(relative)).unwrap();
            assert!(value.is_object());
            assert!(inventory[relative].is_string());
        }
    }
}
