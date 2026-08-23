//! Can `forge_motion::skeleton` and `rigs/humanoid/motion_skeleton.json`
//! drift apart? This test says no.
//!
//! The driven layout exists twice on purpose: as Rust constants here, because
//! a take's rotation array is indexed by them at bake time and a data file
//! that fails to load is a worse failure than a constant; and as data in the
//! rig profile, because the Python motion review reads it and the profile is
//! meant to be complete without the Rust source. Nothing at runtime ties the
//! two together, so this test is that tie: every field the JSON publishes is
//! held equal to the constant it mirrors.

mod common;

use common::repo_path;
use forge_motion::skeleton::{
    ARM_JOINTS, CONTACT_COLUMNS, FEET, HANDS, HEAD, JOINT_COUNT, JOINTS, LEFT, PARENTS, RIGHT, ROOT,
};
use serde_json::Value;

fn profile() -> Value {
    let path = repo_path("rigs/humanoid/motion_skeleton.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} is an array"))
        .iter()
        .map(|v| {
            v.as_str()
                .unwrap_or_else(|| panic!("{key} holds strings"))
                .to_owned()
        })
        .collect()
}

fn indices(value: &Value, key: &str) -> Vec<usize> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} is an array"))
        .iter()
        .map(|v| {
            usize::try_from(v.as_u64().unwrap_or_else(|| panic!("{key} holds indices")))
                .expect("fits usize")
        })
        .collect()
}

fn index(value: &Value, key: &str) -> usize {
    usize::try_from(
        value[key]
            .as_u64()
            .unwrap_or_else(|| panic!("{key} is an index")),
    )
    .expect("fits usize")
}

#[test]
fn the_profile_names_this_layout() {
    let profile = profile();
    assert_eq!(profile["schema"].as_u64(), Some(1));
    assert_eq!(profile["name"].as_str(), Some("cskel27"));
}

#[test]
fn joints_and_parents_match() {
    let profile = profile();
    assert_eq!(strings(&profile, "joints"), JOINTS.map(str::to_owned));

    let parents: Vec<Option<usize>> = profile["parents"]
        .as_array()
        .expect("parents is an array")
        .iter()
        .map(|v| {
            if v.is_null() {
                None
            } else {
                Some(usize::try_from(v.as_u64().expect("parent index")).expect("fits usize"))
            }
        })
        .collect();
    assert_eq!(parents, PARENTS);
    assert_eq!(parents.len(), JOINT_COUNT);
}

#[test]
fn named_indices_match() {
    let profile = profile();
    assert_eq!(index(&profile, "root"), ROOT);
    assert_eq!(index(&profile, "head"), HEAD);
    assert_eq!(indices(&profile, "hands"), HANDS);
    assert_eq!(indices(&profile, "feet"), FEET);
    assert_eq!(indices(&profile, "contact_columns"), CONTACT_COLUMNS);
    assert_eq!(indices(&profile, "right"), RIGHT);
    assert_eq!(indices(&profile, "left"), LEFT);
}

#[test]
fn arm_joints_match() {
    let profile = profile();
    assert_eq!(
        strings(&profile, "arm_joints"),
        ARM_JOINTS.map(str::to_owned)
    );
}

/// The profile publishes no field this test does not check. A key added to
/// the JSON without a constant here would be read by Python and by nothing
/// in Rust — the drift this test exists to catch, arriving from the other
/// side.
#[test]
fn every_profile_field_is_checked() {
    let profile = profile();
    let mut keys: Vec<&str> = profile
        .as_object()
        .expect("profile is an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "arm_joints",
            "contact_columns",
            "feet",
            "hands",
            "head",
            "joints",
            "left",
            "name",
            "note",
            "parents",
            "right",
            "root",
            "schema",
        ]
    );
}
