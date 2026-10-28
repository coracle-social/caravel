//! The backend's inner layering, read from source: the shared models and the pure
//! rules beside them, then the database queries and commands, then the subsystems
//! that run those against a `Core`. Each layer uses only the ones before it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

struct Source {
    path: PathBuf,
    /// `models::relays`, `models::domain::policy`, `db::queries`, or `core::billing`.
    module: String,
    text: String,
}

fn sources() -> Vec<Source> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut sources = Vec::new();
    for (crate_name, dir) in [("domain", "domain"), ("db", "db"), ("core", "core")] {
        collect(&root.join(dir).join("src"), crate_name, &mut sources);
    }
    sources
}

fn collect(dir: &Path, module: &str, sources: &mut Vec<Source>) {
    collect_with(dir, module, false, sources);
}

/// `flatten` names every file in a layer directory by the directory, as queries
/// and commands are, rather than by its own name.
fn collect_with(dir: &Path, module: &str, flatten: bool, sources: &mut Vec<Source>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_str().unwrap().to_string();
        if path.is_dir() {
            let flatten = matches!(name.as_str(), "queries" | "commands");
            collect_with(&path, &format!("{module}::{name}"), flatten, sources);
        } else if name != "lib" && name != "mod" && name != "testing" {
            sources.push(Source {
                text: fs::read_to_string(&path).unwrap(),
                path,
                module: if flatten {
                    module.to_string()
                } else {
                    format!("{module}::{name}")
                },
            });
        }
    }
}

fn namespace(module: &str) -> &str {
    module.split_once("::").unwrap().0
}

/// The modules a file names through `crate::` or a `caravel_` crate, as
/// `<namespace>::<first segment>`, grouped imports included.
fn uses(text: &str, own: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for prefix in [
        "crate::",
        "caravel_domain::",
        "caravel_db::",
        "caravel_core::",
        "caravel_util::",
    ] {
        let namespace = match prefix {
            "crate::" => own,
            "caravel_domain::" => "domain",
            "caravel_db::" => "db",
            "caravel_core::" => "core",
            "caravel_util::" => "util",
            _ => unreachable!(),
        };
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
fn domain_uses_only_itself_and_foundations() {
    for source in sources() {
        if namespace(&source.module) != "domain" {
            continue;
        }
        for used in uses(&source.text, "domain") {
            let allowed = used.starts_with("domain::") || used.starts_with("util::");
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
fn queries_use_only_the_domain() {
    for source in sources() {
        if source.module != "db::queries" {
            continue;
        }
        for used in uses(&source.text, "db") {
            let allowed = used == "db::queries"
                || used.starts_with("domain::")
                || used.starts_with("util::")
                || used == "util::nostr";
            assert!(
                allowed,
                "{} (queries) may not use {used}",
                source.path.display()
            );
        }
    }
}

#[test]
fn commands_use_only_queries_and_the_domain() {
    for source in sources() {
        if source.module != "db::commands" {
            continue;
        }
        for used in uses(&source.text, "db") {
            let allowed = used == "db::queries"
                || used == "db::commands"
                || used.starts_with("domain::")
                || used.starts_with("util::")
                || used == "util::nostr";
            assert!(
                allowed,
                "{} (commands) may not use {used}",
                source.path.display()
            );
        }
    }
}

#[test]
fn core_uses_db_only_through_queries_and_commands() {
    for source in sources() {
        if namespace(&source.module) != "core" {
            continue;
        }
        for used in uses(&source.text, "core") {
            if let Some(used) = used.strip_prefix("db::") {
                assert!(
                    matches!(used, "queries" | "commands"),
                    "{} may not use db::{used}",
                    source.path.display()
                );
            }
        }
    }
}

#[test]
fn domain_is_pure() {
    for source in sources() {
        if namespace(&source.module) != "domain" {
            continue;
        }
        let name = source.path.display();
        assert!(!source.text.contains("async fn"), "{name} has async code");
        assert!(
            !source.text.contains("PgExecutor") && !source.text.contains("sqlx::query"),
            "{name} reads the database"
        );
    }
}

#[test]
fn only_queries_and_commands_query_and_only_subsystems_open_transactions() {
    let sql = Regex::new(r"sqlx::query|QueryBuilder").unwrap();
    for source in sources() {
        if matches!(source.module.as_str(), "db::queries" | "db::commands") {
            continue;
        }
        if let Some(found) = sql.find(&source.text) {
            panic!(
                "{} touches the database directly: {}",
                source.path.display(),
                found.as_str()
            );
        }
        assert!(
            !source.text.contains(".with_tx(") || namespace(&source.module) == "core",
            "{} opens a transaction outside a subsystem",
            source.path.display()
        );
    }
}

#[test]
fn queries_only_read_and_commands_do_not_open_transactions() {
    let write = Regex::new(r"INSERT INTO|DELETE FROM|UPDATE \w+ SET").unwrap();
    for source in sources() {
        let name = source.path.display();
        match source.module.as_str() {
            "db::queries" => assert!(!write.is_match(&source.text), "{name} writes"),
            "db::commands" => assert!(
                !source.text.contains(".with_tx("),
                "{name} opens a transaction"
            ),
            _ => {}
        }
    }
}

#[test]
fn subsystems_depend_on_each_other_without_cycles() {
    let sources = sources();
    let subsystems: BTreeSet<String> = sources
        .iter()
        .filter(|source| source.module.starts_with("core::"))
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
