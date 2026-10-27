---
name: pseud-review
description: Help a human review code through its pseud mirror - walking a change at the right altitude, answering questions from the pseud and the source behind it, and auditing whether a translation is faithful. Use when asked to review a change, branch or codebase that has a `.pseud/` mirror, to explain what changed in pseudocode, or to verify pseud files against their source.
---

# Reviewing through pseud

The reviewer's time is the scarce resource. Start high, descend only where something needs judgment, and back every claim about behavior with the source when the pseud is not precise enough to settle it.

## A change

1. Run `pseud status`. If files are stale or missing, the mirror does not describe the change; update it first with the `pseud-mirror` skill.
2. Run `pseud diff <base>`, for example `pseud diff main`. It lists declarations added, removed and changed, effect changes, and flags raised or cleared.
3. Present the change top-down:
   - effects gained, most dangerous first (`!unsafe`, `!panic`, `!exec`, `!net`, `!db`), since a new effect is a new capability;
   - new and cleared `!!` and `??` flags;
   - added and removed declarations, then changed signatures, at altitude 1 (`pseud diff -l 1`);
   - changed bodies at altitude 2, only for the declarations the reviewer picks or that carry new effects or flags.
4. For anything that looks wrong, open the anchored source lines and quote them.

`pseud html` writes a single-page browser for the mirror, with altitude buttons, effect filters, the flag list, and source behind every anchor.

## A question

Answer from the pseud when its resolution is enough: what a function is for, what it touches, what calls what. `pseud view -e net` and `pseud view --flagged` narrow the mirror to what matters. When the question is about a detail the pseud summarizes, such as an exact condition, an error path or a value, read the anchored source and answer from that. Say which one you used.

## An audit

Mechanical checks catch wrong names, missing code and undetected effects, but not prose that misdescribes what the code does. Audit a translation by sampling declarations, weighted toward those with bodies, effects or flags, and comparing each step with its anchored source. For a whole mirror, use a subagent that did not write the translation. Report each mismatch with the pseud line, the source lines and what the source does instead, then fix the pseud or add a `!!` flag where the source is the thing that is wrong.
