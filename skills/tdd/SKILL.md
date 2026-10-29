---
name: tdd
description: "Red before green for every behavior change, features and bug fixes alike: an executable check that fails for the intended reason before production code changes, then passes after. Mandatory inside poteto-mode. Use for /tdd, a failing or regression test, or any feature or fix built under poteto-mode."
disable-model-invocation: true
---

# TDD

**Setup gate.** Unless `XSTACK READY` is already in context this session, first run `sh <this file's directory>/../setup-xstack/scripts/check-setup.sh` from the project root. On `XSTACK SETUP MISSING`, or when the script is missing, stop. Show its output, tell the user to run `/setup-xstack`, and run no other step of this skill.

Make the intended behavior executable before changing production code. For a bug, the check fails because the bug exists. For a feature, it fails because the behavior does not exist yet. The goal is a focused check that fails before the change and passes after it.

The red step is never skipped. What adapts is the kind of check. When a test would be impractical (broad harness setup, brittle mocks, slow end-to-end infrastructure, production-only state, or large unrelated fixture churn), the red step uses the closest executable check instead, per **If a Failing Test Is Impractical**. It still runs and fails before the change.

## Workflow

1. **Understand the behavior.** Identify the intended behavior, current behavior, affected path, and smallest observable reproduction. For a feature, each acceptance criterion is one intended behavior.
2. **Choose the narrowest executable check.** Prefer the closest unit, component, integration, or regression test already used for that codepath. If no practical test path is obvious, do not create one from scratch just to satisfy the workflow.
3. **Write the failing test first.** Add the smallest focused test that would have caught the bug, or that proves one acceptance criterion of the feature. The test should encode intended behavior, not mirror the current implementation.
4. **Run the new test before changing code.** Run only that test, through the runner's name or file filter, not the suite, as `worker fast --check "<filtered test command>"`. Confirm it fails for the intended reason. If it passes or fails for an unrelated reason, correct the test or reproduction before editing the implementation.
5. **Make the change.** Make the smallest production change that satisfies the intended behavior while preserving nearby contracts.
6. **Rerun the test.** Confirm the test now passes, then run the affected tests around it through the runner's own selection, in one `worker fast --check "<test>" --check "<affected tests>"` call. The full suite runs once per verifiable unit, per poteto-mode's **Tests and verification**, not after every criterion.
7. **Repeat per behavior.** For a feature, take the next acceptance criterion and go back to step 3. One red, one green at a time.

## If a Failing Test Is Impractical

Use the closest executable regression check instead: a targeted script, a reproduction command, browser automation, a snapshot comparison, a log assertion, or a focused integration check. Run it red before the change and green after, exactly like a test.

Prefer no new committed test over a bad test, but never skip the red check. A bad test is one that mostly tests mocks, encodes current implementation details, depends on timing or unrelated global state, needs expensive infrastructure for a small fix, or would be deleted immediately after proving the fix.

## Guardrails

- Do not change tests merely to match a wrong implementation.
- Do not weaken existing assertions unless the expected behavior has genuinely changed and the reason is clear.
- Keep the regression test focused on the bug. Avoid broad fixture churn or unrelated coverage expansion.
- If the bug is flaky, make the test deterministic where possible and document the signal being locked down.
- When the red check is one command over many inputs (each bad input, each endpoint, each fixture file), run it as one `subtask --keep-going` call with one JSONL `argv` or `http` task per input, red before and green after, per **Subtasks** in poteto-mode.
- If the bug exposes a broader class of failures, first land the focused regression path, then consider additional sibling coverage.

## Final Response

Report the evidence, not just the outcome:

- Name the failing-before test or executable check and the failure it produced.
- Name the passing-after test run and any nearby validation performed.
- If a committed test was impractical, state why and name the executable check that ran red before and green after.
