---
name: to-spec
description: "Turn the current conversation into a spec and publish it to the project issue tracker: no interview, just synthesis of what you've already discussed."
disable-model-invocation: true
---

This skill takes the current conversation context and codebase understanding and produces a spec. Do NOT interview the user; just synthesize what you already know.

Read the project's agent instructions and follow their issue-tracker, triage-label, and domain-document pointers. Otherwise read `docs/agents/issue-tracker.md`, `docs/agents/triage-labels.md`, and `docs/agents/domain.md` from the project root, as configured by `/setup-xstack`. Resolve the `ready-for-agent` role to its saved label/status instead of assuming the literal name. Reuse existing project configuration; if required publication configuration is missing, direct the user to `/setup-xstack` before publishing. This is configuration, not a new requirements interview.

**Mechanical batches.** Make `subtask` hot before the first step, per **Subtasks** in poteto-mode: skip it when a `SUBTASK HOT` line is already in context; otherwise run `<xstack root>/subtasks/target/release/subtask hot`. On `SUBTASK COLD` or `SUBTASK BUILDING`, start the printed build commands in the background and do mechanical work directly until hot. Batch independent reads, searches, references, and draft processing through `subtask`: use `--grep` for domain terms, story IDs, or test seams over an already-discovered file list, `--lines` to size candidate files, and JSONL `argv` tasks for independent CLI reads. One batch per independent wave, not a loop of tool calls; dependent operations stay ordered. Reading code to understand it requires judgment, not a mechanical batch.

For every sweep, follow poteto-mode's **Subtasks** sweep and check rules: batch the in-scope mechanical searches with `--keep-going`, resweep affected scope after edits, and report observed items, failures, and remaining findings. The agent decides what findings mean.

**Delegation with ownership.** Root owns scope, requirements, seam approval, design selection, conflicts, and final publication. Delegate meaningful independent judgment only when the work warrants it; do not force splits or use subagents for mechanical work. Give delegates the same relevant conversation and repository grounding, scope, constraints, and explicit output bounds. Assign disjoint writing ownership and integrate their results; do not take over detailed writing already delegated. Continue independent root work while delegates or checks run, and consume their results when needed rather than polling.

When a nontrivial implementation design is needed to specify an agreed outcome, request two independent design candidates with the same grounding and requirements. Each must give complete relevant types and signatures, data ownership, invariants, flows, and rejected structural alternatives. Root selects a base, preserves candidate consensus and the unchanged contract, and resolves conflicts; delegate any substantial synthesis with explicit candidate inputs, conflicts, and bounds. Do not invent a design decision or force a design arena for a simple spec or an unresolved choice.

**Checks and reporting.** Run applicable independent mechanical checks in parallel through `worker`, using affected checks while iterating and all required gates before publication; retain any applicable TDD, formatting, contract, and actual-surface verification requirements. Root owns the final decision and reviews the results. Do not treat pending, failed, unavailable, or unrun checks as passing, and do not publish as ready if a required gate or seam approval is missing. In the handoff report actual subagent count, `subtask` batches and work, `worker` invocations and checks, outcomes, and any unverified or blocked work; report zero or unavailable explicitly rather than implying execution.

## Process

**Write tight.** One line per fact. No section restates another; reference story and criterion IDs instead. Omit optional fields that are empty. A section with nothing to say gets one line ("None."). Coverage stays complete. Length comes only from real stories and criteria.

1. Explore the repo to understand the current state of the codebase, if you haven't already. Use the project's domain glossary vocabulary throughout the spec, and respect any ADRs in the area you're touching.

2. Sketch out the seams at which you're going to test the feature. Existing seams should be preferred to new ones. Use the highest seam possible. If new seams are needed, propose them at the highest point you can. The fewer seams across the codebase, the better - the ideal number is one.

Check with the user that these seams match their expectations. Keep drafting step 3 while you wait, since seams shape only Testing Decisions. Fold the answer in before publishing.

3. Build the User Stories section using the coverage rules and per-story structure below. Check every agreed outcome and constraint against the stories before publishing. Each in-scope requirement must map to at least one story and observable acceptance criterion; merge stories that repeat the same actor, outcome, and conditions. Preserve existing story IDs when revising a spec so later tickets can refer to them.

4. Write the spec using the template below, then publish it using the configured tracker operations. For local Markdown, create the epic folder and write only `.scratch/<epic-slug>/spec.md`, starting with `# <Epic title>` and a `Status:` line holding the configured publication status. Later skills add to this same file and an `issues/` folder beside it, never other files; for a remote tracker, create the spec issue and apply the label mapped from `ready-for-agent`. No additional triage is needed. If an unresolved decision prevents writing a verifiable in-scope outcome, report the blocker and keep the spec as a draft rather than marking it ready.

<spec-template>

## Problem Statement

The problem that the user is facing, from the user's perspective.

## Solution

The solution to the problem, from the user's perspective.

## User Stories

Write a complete, non-duplicative set of stories covering the agreed feature, grouped by user journey or capability. Coverage, not a minimum story count, determines completeness. Enrich the detail of agreed behavior; additional features need evidence from the conversation or established project behavior.

### Coverage rules

- Identify the actual actors and their goals from the conversation and relevant domain documents. Write each story as a user-observable outcome, not a database, endpoint, component, or other implementation task.
- Follow each relevant journey from its entry conditions through the main action to the resulting state. Account for role/ownership boundaries, alternative paths, validation and failure outcomes, empty or boundary states, and recovery or repeated actions when these affect the agreed behavior.
- Include accessibility, privacy, performance, and other quality constraints only when the conversation or existing project requirements establish them. Keep exact limits when known; put unknown thresholds in Further Notes instead of inventing numbers.
- Cover negative requirements explicitly: what must remain unchanged, what an unauthorized actor cannot do, or what must not happen on failure, where supported by the agreed scope.
- Distinguish **Confirmed** requirements stated or approved in the conversation from **Derived** behavior supported by an existing project rule or necessarily implied by a confirmed outcome. Cite the relevant conversation decision or named domain rule briefly. A plausible idea alone is not a requirement.
- Put unresolved choices and unsupported proposals in **Further Notes**, outside the committed stories and acceptance criteria. Keep excluded behavior in **Out of Scope**. Do not start a new requirements interview to fill the list.

### Per-story structure

Use one numbered entry per independently meaningful outcome:

1. **US-001: <short outcome title>**
   - **Story:** As an <actual actor>, I want to <observable capability>, so that <specific user benefit>.
   - **Basis:** Confirmed or Derived, followed by the conversation decision or established domain rule supporting this outcome.
   - **Preconditions:** Relevant starting state, ownership, permissions, or existing data. Omit if none are needed.
   - **Acceptance criteria:**
     - **US-001-AC1:** Given <state>, when <action>, then <observable result>.
     - Add criteria for applicable alternative, failure, and boundary behavior. Each criterion must cover a distinct condition or boundary; merge criteria that merely restate the same observable result. Use concrete inputs and results where known; avoid subjective criteria such as "works correctly" or "is user-friendly".
   - **Dependencies:** Other story IDs only when their outcomes genuinely gate this one. Omit when independent; these are not implementation-ticket blocking edges.

Assign unique IDs (`US-001`, `US-002`, ...) on first creation. Keep them stable across revisions, even if stories are regrouped or removed. Use the story's ID as the prefix for its acceptance criteria so they can be traced from tickets and tests. Keep independent examples as separate criteria; do not split one end-to-end outcome into implementation-layer stories.

<user-story-example>

Context for this example: the user agreed that customers can view balances only for their own accounts; a customer without accounts sees an empty state.

1. **US-001: View my account balances**
   - **Story:** As a bank customer, I want to view the balances of my accounts, so that I know the funds available to me.
   - **Basis:** Confirmed: customers may view their own balances, including the agreed empty state.
   - **Preconditions:** The customer is signed in.
   - **Acceptance criteria:**
     - **US-001-AC1:** Given the customer owns two accounts, when they open their account overview, then both accounts are shown with their respective balances.
     - **US-001-AC2:** Given another customer owns an account, when the signed-in customer opens their overview, then that other customer's account and balance are not shown.
     - **US-001-AC3:** Given the customer owns no accounts, when they open their overview, then an empty state is shown without fabricated accounts or balances.

</user-story-example>

Before leaving this section, verify that every agreed actor, outcome, and relevant constraint has coverage, every criterion is externally observable, and no story contradicts Out of Scope. An unresolved publication-blocking decision belongs in Further Notes and prevents ready-for-agent status; an optional future idea does not.

## Implementation Decisions

A list of implementation decisions that were made. This can include:

- The modules that will be built/modified
- The interfaces of those modules that will be modified
- Technical clarifications from the developer
- Architectural decisions
- Schema changes
- API contracts
- Specific interactions

Do NOT include specific file paths or code snippets. They may end up being outdated very quickly.

Exception: if a prototype produced a snippet that encodes a decision more precisely than prose can (state machine, reducer, schema, type shape), inline it within the relevant decision and note briefly that it came from a prototype. Trim to the decision-rich parts, not a working demo, just the important bits.

## Testing Decisions

A list of testing decisions that were made. Include:

- A description of what makes a good test (only test external behavior, not implementation details)
- Which modules will be tested
- Prior art for the tests (i.e. similar types of tests in the codebase)

## Out of Scope

A description of the things that are out of scope for this spec.

## Further Notes

Any further notes about the feature.

</spec-template>
