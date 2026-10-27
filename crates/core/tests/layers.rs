//! The core's layering, read from its source: models, then the rules over them,
//! the queries that read them and the commands that write them, then the
//! subsystems that run those against a `Core`. Each layer uses only those before
//! it, only queries and commands touch the database, and no subsystem depends on
//! itself through others.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

/// Modules every layer may use.
const FOUNDATIONS: [&str; 5] = ["blank", "config", "crypto", "error", "flag"];

/// Foundations only subsystems may use, with the `Core` that carries them.
const RUNTIME: [&str; 3] = ["bus", "clock", "Core"];

/// Each layer and the layers it may use besides itself and the foundations.
const LAYERS: [(&str, &[&str]); 4] = [
    ("models", &[]),
    ("domain", &["models"]),
    ("queries", &["models", "domain"]),
    ("commands", &["models", "domain", "queries"]),
];

struct Source {
    path: PathBuf,
    /// The layer the file is in, or its subsystem's name.
    module: String,
    text: String,
}

fn sources() -> Vec<Source> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = Vec::new();
    for entry in fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            let layer = path.file_name().unwrap().to_str().unwrap().to_string();
            for file in fs::read_dir(&path).unwrap() {
                let file = file.unwrap().path();
                sources.push(Source {
                    text: fs::read_to_string(&file).unwrap(),
                    path: file,
                    module: layer.clone(),
                });
            }
        } else {
            let module = path.file_stem().unwrap().to_str().unwrap().to_string();
            if module != "lib" && module != "testing" {
                sources.push(Source {
                    text: fs::read_to_string(&path).unwrap(),
                    path,
                    module,
                });
            }
        }
    }
    sources
}

fn is_layer(module: &str) -> bool {
    LAYERS.iter().any(|(layer, _)| *layer == module)
}

fn is_foundation(module: &str) -> bool {
    FOUNDATIONS.contains(&module)
        || RUNTIME.contains(&module)
        || module == "Error"
        || module == "Result"
}

/// The top-level modules a file names through `crate::`, grouped imports included.
fn uses(text: &str) -> BTreeSet<String> {
    text.match_indices("crate::")
        .flat_map(|(at, prefix)| first_segments(&text[at + prefix.len()..]))
        .collect()
}

/// The first segment of each path a use tree starting `tree` names: `a::b` names
/// `a`, and `{a::{b, c}, d}` names `a` and `d`.
fn first_segments(tree: &str) -> Vec<String> {
    let Some(group) = tree.strip_prefix('{') else {
        let name: String = tree
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        return [name].into_iter().filter(|name| !name.is_empty()).collect();
    };

    // The group's own items, split on the commas outside its nested groups.
    let mut items = vec![String::new()];
    let mut depth = 0;
    for c in group.chars() {
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => break,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                items.push(String::new());
                continue;
            }
            _ => {}
        }
        items.last_mut().unwrap().push(c);
    }
    items
        .iter()
        .flat_map(|item| first_segments(item.trim()).into_iter().take(1))
        .collect()
}

#[test]
fn layers_use_only_the_layers_before_them() {
    for source in sources() {
        let Some((_, below)) = LAYERS.iter().find(|(layer, _)| *layer == source.module) else {
            continue;
        };
        for used in uses(&source.text) {
            let allowed = used == source.module
                || below.contains(&used.as_str())
                || FOUNDATIONS.contains(&used.as_str())
                || used == "Error"
                || used == "Result";
            assert!(
                allowed,
                "{} ({}) may not use {used}",
                source.path.display(),
                source.module
            );
        }
    }
}

#[test]
fn foundations_use_only_foundations() {
    for source in sources() {
        if !is_foundation(&source.module) {
            continue;
        }
        for used in uses(&source.text) {
            let allowed =
                FOUNDATIONS.contains(&used.as_str()) || used == "Error" || used == "Result";
            assert!(allowed, "{} may not use {used}", source.path.display());
        }
    }
}

#[test]
fn only_queries_and_commands_touch_the_database() {
    let sql = Regex::new(r"sqlx::query|QueryBuilder|\.with_tx\(").unwrap();
    for source in sources() {
        if matches!(source.module.as_str(), "queries" | "commands") {
            continue;
        }
        for found in sql.find_iter(&source.text) {
            let allowed = found.as_str() == ".with_tx(" && !is_layer(&source.module);
            assert!(
                allowed,
                "{} touches the database directly: {}",
                source.path.display(),
                found.as_str()
            );
        }
    }
}

#[test]
fn queries_only_read_and_rules_are_pure() {
    let write = Regex::new(r"INSERT INTO|DELETE FROM|UPDATE \w+ SET").unwrap();
    for source in sources() {
        let name = source.path.display();
        match source.module.as_str() {
            "queries" => assert!(!write.is_match(&source.text), "{name} writes"),
            "commands" => assert!(
                !source.text.contains(".with_tx("),
                "{name} opens a transaction"
            ),
            "models" | "domain" => {
                assert!(!source.text.contains("async fn"), "{name} has async code");
                assert!(
                    !source.text.contains("PgExecutor"),
                    "{name} reads the database"
                );
            }
            _ => {}
        }
    }
}

#[test]
fn subsystems_depend_on_each_other_without_cycles() {
    let sources = sources();
    let subsystems: BTreeSet<String> = sources
        .iter()
        .filter(|source| !is_layer(&source.module) && !is_foundation(&source.module))
        .map(|source| source.module.clone())
        .collect();
    // A subsystem split across a directory depends on what any of its files uses.
    let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for source in sources
        .iter()
        .filter(|source| subsystems.contains(&source.module))
    {
        let deps = uses(&source.text)
            .into_iter()
            .filter(|used| subsystems.contains(used) && *used != source.module);
        graph.entry(source.module.clone()).or_default().extend(deps);
    }

    // Peel off subsystems that depend on nothing left; whatever remains is a cycle.
    let mut remaining = graph;
    loop {
        let free: BTreeSet<String> = remaining
            .iter()
            .filter(|(_, deps)| deps.iter().all(|dep| !remaining.contains_key(dep)))
            .map(|(subsystem, _)| subsystem.clone())
            .collect();
        if free.is_empty() {
            break;
        }
        remaining.retain(|subsystem, _| !free.contains(subsystem));
    }
    assert!(
        remaining.is_empty(),
        "subsystems depend on each other in a cycle among {:?}",
        remaining.keys().collect::<Vec<_>>()
    );
}
