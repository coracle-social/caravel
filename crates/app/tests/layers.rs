//! The workspace's layering, checked against `cargo metadata`: each crate depends
//! only on the layers below it, and third-party protocol clients stay in the layer
//! that owns them. Above the core, state changes only through its subsystems.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// Each workspace crate and the workspace crates it may depend on.
const LAYERS: [(&str, &[&str]); 9] = [
    ("caravel-util", &[]),
    ("caravel-relays", &["caravel-util"]),
    ("caravel-billing", &["caravel-util", "caravel-relays"]),
    (
        "caravel-operations",
        &["caravel-util", "caravel-relays", "caravel-billing"],
    ),
    (
        "caravel-core",
        &[
            "caravel-util",
            "caravel-relays",
            "caravel-billing",
            "caravel-operations",
        ],
    ),
    (
        "caravel-api",
        &[
            "caravel-util",
            "caravel-relays",
            "caravel-billing",
            "caravel-operations",
            "caravel-core",
        ],
    ),
    (
        "caravel-relay",
        &[
            "caravel-util",
            "caravel-relays",
            "caravel-billing",
            "caravel-operations",
            "caravel-core",
        ],
    ),
    ("caravel-worker", &["caravel-util", "caravel-core"]),
    (
        "caravel-app",
        &[
            "caravel-util",
            "caravel-relays",
            "caravel-billing",
            "caravel-operations",
            "caravel-core",
            "caravel-relay",
            "caravel-api",
            "caravel-worker",
        ],
    ),
];

/// Third-party crates and the only workspace crates that may depend on them.
const CONFINED: [(&str, &[&str]); 7] = [
    (
        "sqlx",
        &[
            "caravel-util",
            "caravel-relays",
            "caravel-billing",
            "caravel-operations",
        ],
    ),
    ("reqwest", &["caravel-util"]),
    ("tokio-tungstenite", &["caravel-util"]),
    ("coracle-kinds", &["caravel-util"]),
    ("axum", &["caravel-relay", "caravel-api", "caravel-app"]),
    ("tower-http", &["caravel-relay", "caravel-app"]),
    (
        "coracle-lib",
        &[
            "caravel-util",
            "caravel-relays",
            "caravel-billing",
            "caravel-operations",
            "caravel-core",
            "caravel-relay",
            "caravel-api",
        ],
    ),
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

/// The crates above the core may read through its queries, but change state only
/// through its subsystems, which own their transactions and tell the bus: their
/// code never names the core's commands or a way to open a transaction.
#[test]
fn only_core_subsystems_change_state() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for name in ["services/relay", "services/api", "app"] {
        for entry in fs::read_dir(crates.join(name).join("src")).unwrap() {
            let path = entry.unwrap().path();
            let text = fs::read_to_string(&path).unwrap();
            let words: BTreeSet<&str> = text
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .flat_map(|line| line.split(|c: char| !c.is_alphanumeric() && c != '_'))
                .collect();
            for word in ["commands", "with_tx", "begin", "try_begin"] {
                assert!(
                    !words.contains(word),
                    "{} changes state outside the core's subsystems: {word}",
                    path.display()
                );
            }
        }
    }
}
