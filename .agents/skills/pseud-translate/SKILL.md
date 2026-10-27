---
name: pseud-translate
description: Translate source code in any language into pseud, the reviewable pseudocode checked by the `pseud` CLI. Use when converting a file, snippet or diff into pseudocode for human review, writing or editing `.pseud` files, or updating a pseud file after its source changed.
---

# Translating code into pseud

A pseud file lets a human review code without reading it. The reviewer reads the pseud, judges the design, behavior and risks, and follows anchors into the source only where something looks wrong. Write for that reader: someone who knows how software works, does not know this codebase, and has a fraction of the time it would take to read the source.

The `pseud` CLI checks everything structural: names, anchors, coverage, effects, exact quotes. What it cannot check is whether the prose is true. Every step must be something the code actually does.

## Budget

Every declaration costs at least one line and usually two, a header and a doc. A file of small declarations therefore compresses to 2–3x, a file of substantial functions to 4–6x, and boilerplate, accessors and tests to 20x or more. Spend lines where the decisions are, and do not drop declarations or decisions to hit a ratio. `pseud check` warns only when a file of 60 or more significant lines compresses less than 2.5x, which means the translation is transliterating.

Give a function a body only when its control flow holds decisions a reviewer should judge: conditions, ordering, retries, limits, error handling that does something other than propagate. A body of three to eight steps is typical. More than fifteen means the steps are too literal. A function whose behavior fits in one sentence gets that sentence as its doc, on its own line or on the header line when the header stays under 100 characters: `fn split_range(s) -> (u32, u32)?  -- "start,count", count 1 when missing`.

## Syntax

```pseud
module relay::pool
  -- One line saying what this module is for. Required; it is all the L0 view shows.

  pub const MAX_RELAYS = 64                                             @20
  pub data Pool                                                         @49-54
    conns: shared Map<url, Conn>  -- also held by every connect task
  pub enum PoolError = TooManyRelays | BadUrl(url)                      @36-42

  impl Pool                                                             @56-166
    pub async fn subscribe(self, filters) -> SubId  !rand               @79-90
      -- Store filters under a random id and send REQ to every connected relay.
    fn spawn_connect_loop(self, url)  !spawn                            @116-165
      -- Background task per relay that reconnects forever.
      loop
        connect to url; on failure log it and skip to the backoff  !net
        pump messages until an error or close
          incoming text -> handle_relay_msg
        sleep backoff(++attempt)
      !! the task never exits, even after the Pool is dropped           @123-163
        nothing holds a handle to cancel it                             @121

  fn backoff(attempt) -> Duration                                       @208-213
    -- 1s doubling per attempt, capped at 5m, with ±10% jitter.
    exact                                                               @209
      let exp = BACKOFF_BASE.saturating_mul(1u32 << attempt.min(16));

  tests                                                                 @215-242
    ~ rejects bad urls; backoff grows and stays capped
```

- Indent two spaces per level. A line ends with optional `!effects`, separated from the content by two spaces, then an optional `-- comment`, then an optional `@anchor`. The 100-character limit counts indentation but not effects or anchor.
- Declarations: `[pub|async|unsafe|static|abstract|override|extern] kind name [signature]`. Kinds are `module data enum class interface impl tests` (containers) and `fn test type const global macro`.
- The first `--` lines under a declaration are its doc. A comment on the declaration line is its inline doc.
- Fields and variants inside `data`, `enum`, `class` and `interface` are members, one per line, or inline in the signature when small: `data Conn { tx, connected_at }`, `enum Mode = Read | Write`. Methods inside an `interface` are `fn` declarations and need anchors.
- Statements at module level, such as a script's argument parsing and dispatch, are anchored steps directly under `module`.
- Steps are free text. Start with a keyword when one fits: `if`, `else`, `for each`, `while`, `loop`, `match`, `return`, `fail`, `retry`, `spawn`, `emit`. Match arms are `Pattern -> action`. A second line under a step is a sub-step, not a continuation, so split a long step into sub-steps.
- `~ text  @a-b` describes a region instead of translating it.
- `!! text` flags a concern and `?? text` an open question. Lines indented under a flag continue it.
- `exact` quotes source verbatim in an indented block.
- Put text containing `--`, or quoted source syntax, in backticks or double quotes. A bare ` -- ` starts a comment.
- `pseud spec` prints the full grammar.

## Names and signatures

Use the source's exact identifiers for declarations, so a reviewer can search for them. Qualify methods by nesting them under their `impl` or `class`, not by renaming.

Name each module by the path the language imports it by: `source::diff` for Rust's `crate::source::diff`, the crate name for `lib.rs`, `main` for a binary's `main.rs`, `app.models.user` in Python, `src/lib/cache` in TypeScript. Cross-file effect propagation depends on these names.

Simplify signatures to what a caller needs to know:

| Source | Pseud |
| --- | --- |
| `&T`, `&mut T`, `Box<T>`, `Rc<T>`, lifetimes, `dyn`, `impl Trait` | `T` |
| `&mut self` / `&self` | `mut self` / `self` |
| `Arc<Mutex<T>>`, `Arc<RwLock<T>>` | `shared T` |
| `Option<T>`, `T \| null`, `Optional[T]` | `T?` |
| `Result<T, E>`, throws `E`, `(T, error)` | `-> T fails E`; `-> T fails` when the error type is the crate's general one |
| `Vec<T>`, `T[]`, `list[T]` | `[T]` |
| `HashMap<K, V>`, `Record<K, V>`, `dict[K, V]` | `Map<K, V>` |
| `HashSet<T>` | `Set<T>` |
| `impl Iterator<Item = T>`, generators, streams | `iter T`, `stream T` |
| a tuple whose positions mean something | `(added, removed, hunks)`, with names rather than types |
| `String`, `&str` | `str` |
| a parameter whose type is obvious from its name | the name alone: `url`, `filters` |

Keep integer widths where overflow or wire format matters. Members may share a type, `old_start, new_start: u32`, and record a default, `model: str = "gpt-4o"`. A constant list is `const EXTS = [rs, py, go]`; a long table is a `const` with one indented member line per entry that matters, each anchored if useful.

## What to keep

- Every exported type, function and constant, by name. A run of trivial public accessors may be one summary that names them: `~ accessors: file_path, lang_of, kind_of  @80-96`.
- Conditions and limits that change behavior, with their values: `fail TooManyRelays if more than MAX_RELAYS urls`.
- Ordering that matters: what happens before a lock is taken, before a write is committed, before a response is sent.
- What state a function reads and writes, and who else holds it.
- Error handling that does anything except propagate: retries, fallbacks, swallowed errors, partial writes.
- Concurrency: what runs in the background, what waits on what, what is shared.

## What to drop

- Imports, module declarations such as Rust's `mod x;`, and attributes and decorators, unless one changes behavior (`#[serde(default)]`, `@transactional`). These lines do not count toward coverage.
- Local type annotations, borrows, clones, conversions, iterator plumbing, `.await`, and error plumbing such as `?` and `map_err`. Errors propagate unless a step says otherwise, and `async` on the header covers every await.
- Logging, unless logging is the only thing a branch does.
- Boilerplate implementations: write `impl Display for Session  -- "Session(user, expires)"` and give it no body.

Steps describe intent at the level a reviewer judges, not the syntax that achieves it. `dedupe by event id in seen` is a step; `if !seen.insert(event.id.clone()) { return; }` is source code, and `pseud check` warns about it.

## Anchors

Every declaration directly inside the module or another container needs an `@start-end` anchor covering its whole item. Anchors use the numbered source lines you were given. Steps, members, flags, notes and exact blocks may carry anchors when pointing at a specific line helps the reviewer.

Coverage counts the significant source lines that anchors account for and must be at least 90%. Everything in the source must be accounted for by a declaration or a `~` summary; nothing may silently disappear. A container covers only its own header and footer once its children are anchored, so a method left out of an `impl` shows up as a gap.

## Effects

`!unsafe !panic !exec !net !db !fs !env !io !global !lock !spawn !time !rand`

`pseud stamp` adds the effects its detectors find in each declaration's source, copies callees' effects to their callers, and copies every effect up to the enclosing declarations. Within a file it follows calls by name; across files it follows imported names and calls qualified by a module or type name, such as `diff::load(…)`, when the mirror's module names follow the naming rule above. It never removes an effect. Add the effects it cannot see, such as a `!db` reached through a method on a field, and put an effect on a step when it pinpoints where in a body the effect happens.

Detectors are textual and sometimes wrong, for example on an `.expect()` that a preceding check makes unreachable. Waive a wrong effect on its declaration with a reason: `fn chunks(text) -> [str]  !-panic  -- expect is guarded by the length check`. Tag `!panic` for explicit panics, unwraps, asserts and exits; tag implicit ones, such as out-of-range indexing, only when they look reachable, and flag them. Incidental effects are real: a function that reads `$HOME` to find its cache directory has `!env`.

Write `not`, never `!`, for negation. A `!word` after a single space is text, and `pseud check` warns about it (P006).

## Flags

Flags are the most valuable lines in the file, because a reviewer reads them first. Use `!!` for a likely bug, a race, unbounded growth, a swallowed error, a security problem, dead code, or behavior that contradicts a name or doc. For a bug that needs a rare trigger, say what the trigger is. Use `??` when intent is ambiguous, or when a claim depends on something you cannot check from the source, such as a library's behavior or a grammar version.

Views hoist flags out of folded declarations and prefix them with the declaration's name, so a flag must make sense on its own line under the module. Put the evidence in notes indented under the flag, each with its own anchor, including anchors into other files. Say how you confirmed a flag when you ran something to confirm it.

```pseud
!! a "\ No newline at end of file" line ends the hunk, so later + lines are not counted   @282
  git emits -y, \, +z, \ for a changed last line, which gives added = 0
```

When the same defect appears in several files, flag each occurrence and explain it fully once: `!! trailing comment after the final return counts as an early return (as in lang::rust)`. Never flag style, naming or formatting.

## Exact blocks

Quote code verbatim only where the precision is what needs reviewing: bit arithmetic, cryptography, SQL, regular expressions, security checks, money. Keep a block under five lines and anchor it. `pseud check` verifies the quote against the source.

## Tests

Put a test module under `tests`. Summarize it with `~` lines naming the behaviors covered, or give a `test` line per behavior when the list is the point. Effects inside tests do not propagate.

## Workflow

1. Get the source with line numbers. `pseud prompt <source>` prints it with these instructions.
2. Write `<mirror>/<source path>.pseud`, starting at the `module` line.
3. Run `pseud stamp <file>` to write the header, derive effects and align anchors.
4. Run `pseud check <file>` and fix every error and warning:
   - C001 or C002: add the missing declaration, or a `~` summary for the uncovered lines.
   - A004: the anchor does not contain the name; fix the line numbers.
   - E003: add the effect, waive it with `!-name` and a reason, or anchor the declaration more tightly if the line belongs elsewhere.
   - D004: rewrite the step as intent, or quote the syntax in backticks.
   - P005: a ` -- ` meant as text started a comment; quote it.
   - P006: separate effects from the content with two spaces, or quote the `!word`.
   - A006: the anchor swallows another definition; declare it, summarize it, or shorten the anchor.
   - E005: the waiver no longer suppresses anything; remove it.
   - D001: the translation is too literal; drop bodies from simple functions and merge steps.
5. Repeat until `pseud check` is clean.

## Updating after a source change

1. Run `pseud reanchor <file>`. It shifts anchors to the new line numbers, using the source version recorded at the last stamp, and prints `changed` for every declaration whose lines the change touched. Without that version, pass a copy of it with `--old <file>`, or fix anchors by hand from the numbered source.
2. Edit only the declarations reanchor reported, and declare whatever new code `pseud check` reports as uncovered. Remove effects the change took away, since `pseud stamp` only adds them. Leave every other line byte-identical so the pseud diff shows the real change.
3. Re-verify every `!!` and `??` in the file against the new source. Delete flags whose problem is fixed; re-anchor the ones that still hold.
4. Stamp and check as above.
