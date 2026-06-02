---
name: rebuild-skill
description: Overhaul an existing skill by becoming an expert in what it does, then rewriting it from verified ground truth. Use whenever the user wants to rebuild, improve, fix, refresh, or modernize a specific skill — e.g. "/rebuild-skill billing-model", "this skill is stale, fix it", "make the deploy skill correct again", "the X skill's instructions are wrong". Kicks off a dynamic multi-agent workflow that researches the skill's domain before touching its prose, so improvements rest on what's actually true rather than a copyedit of what was already there.
---

# Rebuild a skill

You are handed the name of an existing skill and asked to make it genuinely better. The argument is the skill name (e.g. `billing-model` from `/rebuild-skill billing-model`). If no name was given, ask which skill to rebuild before going further.

The trap to avoid: treating this as a writing task. A skill encodes how to do something well, so you can only improve it as far as you actually understand the thing it does. A rewrite that polishes the prose but never checks the claims against reality usually makes the skill *worse* — more confident and more wrong. So the order here is deliberate: gain real expertise and gather ground truth first, rewrite from that position second, and verify every claim against its source third.

The bulk of that work fans out across many agents, so this skill runs as a **dynamic Workflow** — "dynamic" because the prerequisite research is shaped by what the skill actually does, which you discover by reading it. You scout inline to find the skill and figure out *what expertise the rebuild needs*, then hand that work-list to a Workflow. Invoking the Workflow tool here is expected — these instructions are your opt-in.

## Step 0 — Locate the skill

Find the skill directory before anything else. Skills live under `.agents/skills`:

```bash
NAME="<the-name-argument>"
find .agents/skills -maxdepth 2 -type d -name "$NAME" 2>/dev/null
```

If exactly one match, that's your target. If none, stop and ask the user — rebuilding the wrong copy wastes the whole run. Note the directory and the `name:` in its frontmatter; both are preserved unchanged through the rebuild.

## Step 1 — Understand it (inline)

Read the target `SKILL.md` end to end, plus every bundled resource (`scripts/`, `references/`, `assets/`). Build a clear picture of:

- **Purpose** — what capability is this supposed to give Claude?
- **Triggering** — when is it meant to fire? Is the description tuned for that, or does it over/under-trigger?
- **Claims** — what factual or procedural assertions does it make? (API behaviors, commands, file layouts, domain rules, step sequences.) These are what you'll verify.
- **Domain** — what does Claude need to be expert in to do this task well? This is the seed for the prerequisite work.

## Step 2 — Scope the prerequisite work (inline)

Decide *what gaining expertise looks like for this particular skill*, and break it into independent facets — one investigable chunk each. The right facets depend entirely on the skill's type:

- A skill that **documents a codebase domain** (like `billing-model`) → trace the actual code paths it describes; confirm each documented rule still matches the implementation.
- A skill that **runs a procedure** (deploy, release, migration) → walk the real procedure and its tools; find steps that are stale, reordered, or missing.
- A skill that **relies on an external API or library** → fetch the authoritative docs and verify every behavior the skill claims. The user expects third-party behavior backed by docs, never asserted from memory.
- A skill that **produces an artifact** (a doc, chart, config) → actually produce one with its current instructions and note where it breaks or falls short.

Most skills are a blend. Aim for 3–6 facets that together cover the skill's claims and its domain. Each facet should be answerable by one agent reading real sources (code, docs, tools) — not by guessing.

## Step 3 — Run the dynamic workflow

Read [references/workflow-template.js](references/workflow-template.js), adapt the `facets` and prompts to the skill you scoped, and run it with the Workflow tool. Pass the skill path, name, and facets via the `args` field so the script stays generic.

The workflow runs these phases:

1. **Investigate** (fan-out, one agent per facet) — each agent reads real sources to gather ground truth: what is actually true, what the skill currently claims, and exactly where the two diverge. Every finding cites a file path or URL.
2. **Synthesize** — one agent folds the findings and the current skill into a concrete rebuild plan (what to fix, add, cut, restructure), grounded in the verified facts and in skill-writing principles.
3. **Draft** — one agent rewrites `SKILL.md` and any resources in place, preserving the directory and the `name`. It returns the list of claims a reviewer should check, each with its supporting evidence.
4. **Verify** (fan-out, one agent per claim) — each agent adversarially tries to *refute* its claim by going back to the cited source. Anything unsupported, outdated, or contradicted is flagged.
5. **Correct** (only if anything was refuted) — one agent fixes the flagged claims in the files: correct them to match ground truth, or cut them.

The workflow returns the files written and the verification verdicts. This is "ensuring correctness" made concrete: nothing survives in the rebuilt skill that an independent skeptic couldn't confirm against its source.

## Step 4 — Review with the user

Snapshot the original before the workflow writes anything (`cp -r <skill-dir> /tmp/<name>-before`) so you can show a clean diff. After the workflow returns:

- Show `git diff` (or a diff against the snapshot) of the skill files.
- Summarize what changed and the ground truth behind each substantive change — especially anything the verify phase had to correct.
- Surface any claim the workflow *couldn't* confirm rather than quietly keeping it.

If the user wants quantitative confidence that the rebuild triggers and performs better, offer to hand off to the `skill-creator` skill, which runs the eval/benchmark loop. This skill's job is the correctness-grounded rewrite; `skill-creator` is the measurement loop.

## Principles to preserve

- **Expertise before prose.** Never edit a claim you haven't grounded in a real source. The investigate phase exists so the rewrite comes from knowledge, not from rephrasing the old text.
- **Cite or cut.** Every technical claim in the rebuilt skill traces to a file path or URL. If verification can't confirm it, fix it or remove it — don't ship confident guesses.
- **Preserve identity.** Keep the directory name and the frontmatter `name` so the skill stays the same skill. Improve the description for triggering; don't rename the thing.
- **Apply skill-writing craft.** Progressive disclosure (lean `SKILL.md`, detail in `references/`), explain the *why* rather than piling on MUSTs, and a description that states both what it does and when to fire.
