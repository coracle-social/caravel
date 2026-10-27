---
name: pseud-mirror
description: Create or refresh a pseud mirror of a codebase, the `.pseud/` directory of reviewable pseudocode that tracks every source file. Use when asked to mirror, translate or make a whole repository reviewable as pseudocode, to bring the mirror up to date after code changes, or to set up `pseud check` in CI.
---

# Maintaining a pseud mirror

A mirror holds one pseud file per source file, plus `OVERVIEW.pseud` describing the system as a whole. It is committed next to the code, so every change to the code arrives with a matching change to the mirror, and a reviewer reads the pseud diff instead of the code diff. Translating a single file follows the `pseud-translate` skill; this skill covers the whole tree.

## Setup

Run `pseud init` at the project root if there is no `pseud.json`. It writes the config, creates `.pseud/`, and installs the skills. Then narrow `include` and `exclude` in `pseud.json` to the code worth reviewing: leave out generated code, vendored code and fixtures.

## Finding the work

`pseud status` lists every source file whose pseud file is `missing`, `stale` (the source changed since it was stamped) or `unstamped`, and pseud files whose source is gone (`orphan`). `pseud status --json` includes each file's significant line count for batching.

Delete orphans. Translate missing files. Update stale files following "Updating after a source change" in `pseud-translate`: reanchor first, then edit only what changed, so the mirror's diff shows exactly the code's change.

## Batching

When subagents are available, split the files into batches of about 1,500 significant source lines, keeping files from the same directory together so one agent sees a module's neighbours. Give each subagent:

- the list of source files and their pseud paths;
- the instruction to follow the `pseud-translate` skill, or the output of `pseud prompt <source>` for each file;
- the requirement that `pseud check <its files>` is clean before it reports back;
- for stale files, the instruction to make minimal edits.

Run batches in parallel. When they finish, run `pseud stamp` over the whole mirror, which resolves calls between files that no single batch could see, then `pseud check`, and fix what remains yourself. Check that module names follow the naming rule in `pseud-translate` consistently across batches, since cross-file effects depend on them.

## The overview

`OVERVIEW.pseud` is the first file a reviewer reads. It has no header and no source. Write it after the module files exist, from `pseud view -l 0`, which prints every module and its doc:

```pseud
module overview
  -- A nostr client library: relay pool, event verification, local cache.

  -- Entry points
  ~ `Pool::new` starts one connect loop per relay
  ~ `Cache::open` loads the event store from disk

  -- Flows
  ~ subscribe: `Pool::subscribe` -> REQ to relays -> `handle_relay_msg` -> dedupe -> PoolEvent stream
  ~ publish: `Pool::publish` -> EVENT to relays -> OK replies are ignored

  -- Shared state
  ~ `Pool` maps are behind RwLocks shared with every connect task
```

Names in backticks must match declarations in the mirror; `pseud check` warns when one does not. Keep the overview under a hundred lines, and update it when a change adds a module, an entry point or a flow.

## Finishing

Run `pseud check`. It must report no errors, and every warning should be fixed or deliberately accepted. Report the summary line's totals: files, significant source lines, pseud lines and ratio, and altitude sizes. List every `!!` flag the translation raised, since those are findings in their own right.

## CI

`pseud check --ci` fails when any pseud file is stale or missing, so a change cannot merge without its mirror update. Add it to the project's CI next to the tests.
