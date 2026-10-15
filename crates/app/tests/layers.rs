//! The workspace's layering, checked against `cargo metadata`: each crate depends
//! only on the layers below it, and third-party protocol clients stay in the layer
//! that owns them.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use serde_json::Value;

/// Each workspace crate and the workspace crates it may depend on.
const LAYERS: [(&str, &[&str]); 6] = [
    ("caravel-db", &[]),
    ("caravel-adapters", &[]),
    ("caravel-core", &["caravel-db", "caravel-adapters"]),
    ("caravel-relay", &["caravel-core"]),
    ("caravel-api", &["caravel-core"]),
    (
        "caravel-app",
        &[
            "caravel-db",
            "caravel-adapters",
            "caravel-core",
            "caravel-relay",
            "caravel-api",
        ],
    ),
];

/// Third-party crates and the only workspace crates that may depend on them.
const CONFINED: [(&str, &[&str]); 6] = [
    ("nostr-sdk", &["caravel-adapters"]),
    ("nwc", &["caravel-adapters"]),
    ("reqwest", &["caravel-adapters"]),
    ("axum", &["caravel-relay", "caravel-api", "caravel-app"]),
    ("tower-http", &["caravel-app"]),
    ("coracle-lib", &["caravel-core", "caravel-relay"]),
];

/// Each workspace crate's normal (not dev or build) dependencies.
fn dependencies() -> BTreeMap<String, BTreeSet<String>> {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to run cargo metadata");
    let metadata: Value = serde_json::from_slice(&output.stdout).expect("invalid cargo metadata");

    metadata["packages"]
        .as_array()
        .expect("no packages")
        .iter()
        .map(|package| {
            let deps = package["dependencies"]
                .as_array()
                .expect("no dependencies")
                .iter()
                .filter(|dep| dep["kind"].is_null())
                .map(|dep| dep["name"].as_str().unwrap().to_string())
                .collect();
            (package["name"].as_str().unwrap().to_string(), deps)
        })
        .collect()
}

#[test]
fn every_workspace_crate_has_a_layer() {
    let layered: BTreeSet<&str> = LAYERS.iter().map(|(name, _)| *name).collect();
    for name in dependencies().keys() {
        assert!(layered.contains(name.as_str()), "{name} has no layer");
    }
}

#[test]
fn crates_depend_only_on_the_layers_below_them() {
    let dependencies = dependencies();

    for (name, allowed) in LAYERS {
        for dep in &dependencies[name] {
            if dep.starts_with("caravel-") {
                assert!(
                    allowed.contains(&dep.as_str()),
                    "{name} may not depend on {dep}"
                );
            }
        }
    }
}

#[test]
fn protocol_clients_stay_in_their_layer() {
    for (name, deps) in dependencies() {
        for (confined, owners) in CONFINED {
            if deps.contains(confined) {
                assert!(
                    owners.contains(&name.as_str()),
                    "{name} may not depend on {confined}"
                );
            }
        }
    }
}
