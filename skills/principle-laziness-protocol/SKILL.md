---
name: principle-laziness-protocol
description: "Apply when refactoring, evaluating diff size, or tempted to add abstractions, layers, or signal threading. Bias toward deletion and the smallest change that solves the problem."
disable-model-invocation: true
---

# Laziness Protocol

- **Consolidate decisions.** Do not repeat the same choice in several places. Put it behind one source of truth and pass the result as a simple flag.
- **Minimize the diff.** Make the smallest change that solves the problem. Fewer lines beat "elegant" boilerplate.
- **Scope comes from the request.** Before you cut the diff, list what the request needs as outcomes you can check. This principle shrinks how you meet them, never how many you meet. If an outcome is still unmet, the diff is too small, not lean.
- **Lazy diff, full tests.** A small diff never means fewer tests. Every behavior the change touches gets its edge cases tested and run before you report: empty or zero input, both sides of every boundary, invalid input, the error path, a repeated call, and each existing caller the change reaches. A text change gets the same sweep over every place the old text appears, not only the one the request names. When a stale sentence also states a fact you cannot confirm, remove the stale claim and keep only what you know, then name the open fact in the report. Never leave the stale claim in place to wait for an answer.
- **Worker and subtask, always.** Run the edge-case tests as one `worker fast --check "<test command>"` call, and add `--check` for the type check or linter the project has. Run every sweep through `subtask`: `git ls-files | subtask --grep '<old text or value>' --keep-going` (`rg --files` instead of `git ls-files` where git is unavailable) for each place the old text or value lives, and one JSONL `argv` task per edge input when the behavior is a command. Use the binary paths from the `SUBTASK HOT` line; without one, run `<xstack root>/subtasks/target/release/subtask hot` first, where `<xstack root>` is two folders above this skill's folder, per **Subtasks** in poteto-mode. If a binary still cannot run, run the same tests and sweep directly and say which binary failed and why in the report.
- **Report what you left alone.** When this principle limits a change, the report names what changed and which outcome each change meets, what you left unchanged and why the problem is solved without it, and proof that fits the change: tests or a run for behavior, a check against the request for text. A passing formatter or linter proves style, not that the request is done.

## Never alone

This principle only says how small a change may be. It never decides on its own whether the work is done, so it always applies with partners. Read each partner's leaf skill, apply it, and name it in the report next to this one with the choice it changed.

- **Always: [Prove It Works](../principle-prove-it-works/SKILL.md).** The small diff is checked against the real artifact.
- **And at least one that fits the task:**
  - A bug fix: [Fix Root Causes](../principle-fix-root-causes/SKILL.md). The smallest change sits at the first wrong place, not as a guard at the symptom.
  - Tests added or changed: [Test Behavior, Not Implementation](../principle-test-behavior-not-implementation/SKILL.md).
  - A refactor or a diff you are sizing: [Subtract Before You Add](../principle-subtract-before-you-add/SKILL.md) or [Minimize Reader Load](../principle-minimize-reader-load/SKILL.md).
  - A new requirement in an existing design: [Redesign from First Principles](../principle-redesign-from-first-principles/SKILL.md). The small diff must be the one the design would have had from day one, not a patch.
  - A value or decision repeated in several places: [Model the Domain](../principle-model-the-domain/SKILL.md).
  - A sweep or a run of similar edits: [Sequence Work into Verifiable Units](../principle-sequence-verifiable-units/SKILL.md).
  - A product or scope tradeoff: [Experience First](../principle-experience-first/SKILL.md).

When a partner asks for more than this principle allows, the partner decides whether an outcome is met, and this principle decides only how small the way to meet it is.

Worked best practices and edge cases: [`references/edge-cases.md`](references/edge-cases.md). The `laziness-*` cases in the plugin's `evals/` folder test each one.
