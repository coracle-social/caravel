//! The backend's inner layering, read from source: each domain module's models and
//! the pure rules beside them, then the queries that read and the commands that
//! write, then the core that adapts them to the services above.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Model,
    Query,
    Command,
    Core,
}

struct Source {
    path: PathBuf,
    /// The crate and module the file belongs to, e.g. `relays::access`.
    module: String,
    kind: Kind,
    text: String,
}

fn sources() -> Vec<Source> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut sources = Vec::new();
    for (dir, crate_name) in [
        ("domain/relays", "relays"),
        ("domain/billing", "billing"),
        ("domain/operations", "operations"),
    ] {
        collect(&root.join(dir).join("src"), crate_name, &mut sources);
    }
    collect_core(&root.join("core").join("src"), &mut sources);
    sources
}

fn collect(dir: &Path, module: &str, sources: &mut Vec<Source>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_str().unwrap().to_string();
        if path.is_dir() {
            collect(&path, &format!("{module}::{name}"), sources);
        } else if !matches!(name.as_str(), "lib" | "mod") {
            let mut parts: Vec<&str> = module.split("::").collect();
            parts.push(&name);
            sources.push(Source {
                text: fs::read_to_string(&path).unwrap(),
                path,
                kind: match parts.contains(&"queries") {
                    true => Kind::Query,
                    false if parts.contains(&"commands") => Kind::Command,
                    false => Kind::Model,
                },
                module: module.to_string(),
            });
        }
    }
}

/// The core's files, each a subsystem of the adapter layer.
fn collect_core(dir: &Path, sources: &mut Vec<Source>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_str().unwrap().to_string();
        if path.is_dir() {
            collect_core(&path, sources);
        } else if !matches!(name.as_str(), "lib" | "mod") {
            sources.push(Source {
                text: fs::read_to_string(&path).unwrap(),
                path,
                kind: Kind::Core,
                module: format!("core::{name}"),
            });
        }
    }
}

/// The workspace crates a file names through a `caravel_` crate, or `own` for
/// `crate::`, grouped imports included.
fn uses(text: &str, own: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for (prefix, namespace) in [
        ("crate::", own),
        ("caravel_relays::", "relays"),
        ("caravel_billing::", "billing"),
        ("caravel_operations::", "operations"),
        ("caravel_core::", "core"),
        ("caravel_util::", "util"),
    ] {
        for (at, _) in text.match_indices(prefix) {
            for name in first_segments(&text[at + prefix.len()..]) {
                names.insert(format!("{namespace}::{name}"));
            }
        }
    }
    names
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
fn models_are_pure() {
    for source in sources() {
        if source.kind != Kind::Model {
            continue;
        }
        let name = source.path.display();
        assert!(!source.text.contains("async fn"), "{name} has async code");
        assert!(
            !source.text.contains("PgExecutor")
                && !source.text.contains("PgConnection")
                && !source.text.contains("sqlx::query"),
            "{name} reads the database"
        );
    }
}

#[test]
fn queries_only_read() {
    let write = Regex::new(r"INSERT INTO|DELETE FROM|UPDATE \w+ SET").unwrap();
    for source in sources() {
        if source.kind == Kind::Query {
            assert!(
                !write.is_match(&source.text),
                "{} writes",
                source.path.display()
            );
        }
    }
}

#[test]
fn commands_do_not_open_transactions() {
    for source in sources() {
        if source.kind == Kind::Command {
            assert!(
                !source.text.contains(".with_tx(") && !source.text.contains(".begin()"),
                "{} opens a transaction",
                source.path.display()
            );
        }
    }
}

#[test]
fn only_queries_and_commands_touch_the_database() {
    let sql = Regex::new(r"sqlx::query|QueryBuilder").unwrap();
    for source in sources() {
        if matches!(source.kind, Kind::Query | Kind::Command) {
            continue;
        }
        if let Some(found) = sql.find(&source.text) {
            panic!(
                "{} touches the database directly: {}",
                source.path.display(),
                found.as_str()
            );
        }
    }
}

#[test]
fn a_module_stays_below_the_modules_above_it() {
    // Within a crate the modules may depend on each other in any direction, but
    // never upward: relays knows nothing of billing or operations.
    let upward: [(&str, &[&str]); 4] = [
        ("relays", &["billing", "operations", "core"]),
        ("billing", &["operations", "core"]),
        ("operations", &["core"]),
        ("core", &[]),
    ];
    for source in sources() {
        let own = source.module.split("::").next().unwrap();
        for used in uses(&source.text, own) {
            let namespace = used.split("::").next().unwrap();
            if let Some((_, forbidden)) = upward.iter().find(|(crate_name, _)| *crate_name == own)
                && forbidden.contains(&namespace)
            {
                panic!(
                    "{} ({}) may not use {used}",
                    source.path.display(),
                    source.module
                );
            }
        }
    }
}

/// Every Rust file under `dir`, following its subdirectories.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(rust_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
    found
}

#[test]
fn services_reach_the_database_only_through_the_core() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for dir in ["services/api", "services/relay", "services/worker"] {
        for path in rust_files(&root.join(dir).join("src")) {
            let text = fs::read_to_string(&path).unwrap();
            let words: BTreeSet<&str> = text
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .flat_map(|line| line.split(|c: char| !c.is_alphanumeric() && c != '_'))
                .collect();
            for word in ["commands", "with_tx", "begin", "sqlx", "queries"] {
                assert!(
                    !words.contains(word),
                    "{} reaches past the core: {word}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn subsystems_depend_on_each_other_without_cycles() {
    let sources = sources();
    let subsystems: BTreeSet<String> = sources
        .iter()
        .filter(|source| source.kind == Kind::Core)
        .map(|source| source.module.clone())
        .collect();
    // A subsystem split across a directory depends on what any of its files uses.
    let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for source in sources
        .iter()
        .filter(|source| subsystems.contains(&source.module))
    {
        let deps = uses(&source.text, "core")
            .into_iter()
            .map(|used| format!("core::{used}"))
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
