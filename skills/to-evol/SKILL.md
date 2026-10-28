---
name: to-evol
description: "Turn a conversation into a risk-adaptive evolution spec for a shipped feature: verified baseline, stable story IDs, compatibility, contract impact, and verified publication to the configured tracker."
disable-model-invocation: true
---

Produce an **evolution spec** for changing a shipped feature. Preserve the baseline-first design and specify only the agreed delta. Investigate until the change is sufficiently understood to implement and verify safely, not until every conceivable uncertainty is eliminated. For a feature that does not exist yet, use **to-spec** instead.

Read the project's agent instructions and follow their issue-tracker, triage-label, and domain-document pointers. Otherwise read `docs/agents/issue-tracker.md`, `docs/agents/triage-labels.md`, and `docs/agents/domain.md` from the project root, as configured by `/setup-xstack`. Resolve the `ready-for-agent` role to its saved label/status instead of assuming the literal name. Reuse existing project configuration; if required publication configuration is missing, direct the user to `/setup-xstack` before publishing. Continue reachable specification work without inventing configuration or silently switching trackers.

## Process

**Write tight.** One line per fact. Reference IDs and earlier sections instead of copying them. Omit optional fields that are empty. A section with nothing to say gets one line with its reason. Depth follows the risk level, never padding.

0. **Discover progressively and assess risk.** Reuse evidence already in context. Inspect in this order:
   1. Conversation and explicit user decisions.
   2. Project instructions, domain glossary, and relevant ADRs.
   3. Original feature specification.
   4. Relevant earlier evolution specifications, in publication order.
   5. Existing design contract: vocabulary, function intents, invariants, and flows.
   6. Directly affected code and tests.
   7. Referenced dependencies and callers, expanding only where evidence warrants.
   8. Relevant Git history when needed to resolve intent, divergence, or compatibility.

   The order is a priority, not a serial chain. Read the sources that need no earlier finding (usually 2 to 5) in one batch of parallel calls, then expand into 6 to 8 from what they show. Expansion reads that do not depend on each other also go out together.

   **Resume, don't restart.** If a draft of this evolution already exists (a local `# Evolution <NN>` section with a non-ready Status, a draft tracker issue, or this session's evidence map), continue from it and redo only the parts its new inputs change. A baseline from an earlier spec or draft counts as verified for the code and tests it cites only when `git diff --stat <its sha or date>..HEAD -- <those paths>` is empty. Re-read changed paths. Never reuse a baseline without that check.

   A local epic is one file, `.scratch/<epic-slug>/spec.md`: the original spec, its `# Design` section, and each evolution as a `# Evolution <NN>: <title>` section in publication order. One read covers all of them. Remote specs are the feature issue and linked evolutions. Inventory all prior evolution references and allocated story/criterion IDs; read the bodies relevant to the affected behavior and follow their references. Mandatory baseline evidence is not optional at SMALL. Missing historical specs or contracts are recorded, not fabricated.

   Maintain a compact internal evidence map: `Source | Relevant finding | Confidence (Known/Inferred/Unknown) | Affected story or contract | Remaining uncertainty`. Reuse it instead of rereading sources or copying large excerpts. Expand when a caller, invariant, shared writer, or divergence indicates additional impact.

   Select a level from evidence; record the level and its reason in Context. Escalate whenever new evidence reveals greater risk. Request size is not evidence; never downgrade to save tokens.

   | Level | Evidence and required depth |
   |---|---|
   | **SMALL** | Localized change with no meaningful behavioral, contract, migration, security, or compatibility impact. Verify the relevant baseline and precise delta; check hidden impacts before using this path. Keep stories, criteria, and required sections concise; avoid unnecessary questions. |
   | **STANDARD** | Existing behavior, multiple journeys, or internal contracts change without a CRITICAL trigger. Verify the affected baseline and dependencies, complete delta, contract impact, compatibility, and regression coverage. |
   | **CRITICAL** | Persistent data, financial calculations, authorization/permissions, public APIs, state transitions, shared invariants, concurrency, migrations, distributed workflows, or irreversible operations are affected. Trace relevant behavior end to end; verify affected invariants, existing data and active workflows, compatibility, failure/recovery, and rollback feasibility. Support criteria with evidence. |

1. **Establish the baseline and context card.** Actual code and tests establish today's behavior; where an earlier spec disagrees, code is the baseline. Record the disagreement and its evidence in Further Notes. This describes reality, not permission to perpetuate a defect or change a promised outcome silently: unresolved intent or compatibility conflicts return to clarification. If no earlier spec exists, say the baseline comes from code and tests alone.

   Keep a compact card, one line per item with evidence and **Known** (directly observed or explicitly decided), **Inferred** (derived from named evidence), or **Unknown** (no supporting evidence):
   - **Feature:** shipped feature's domain name and location.
   - **Actors:** current users and intended beneficiaries.
   - **Current behavior:** affected journeys today.
   - **Why now:** the user's problem or request.
   - **The change:** proposed additions, changes, or removals.
   - **Must not change:** behavior and invariants to preserve.
   - **Existing state:** prior records, mid-journey users, and dependent callers/integrations.
   - **Constraints:** established rollout, compliance, performance, or compatibility limits.
   - **Success:** observable outcome.

   Keep the working card internal; surface only the facts needed for a decision. Update changed lines rather than reconstructing it after every discovery. Include the final concise card in Context.

2. **Resolve uncertainty by consequence.** First classify gaps as **Resolvable** when code, tests, specs, contracts, or relevant history can answer them; inspect that evidence before asking. Classify remaining gaps:
   - **Blocking:** a wrong assumption changes behavior, violates an invariant, breaks compatibility, or makes a criterion unverifiable.
   - **Non-blocking:** does not materially affect the agreed outcome; record in Further Notes and proceed.

   Ask only genuine blocking decisions, each with an evidence-supported recommendation and its consequence. Blocking questions whose answers do not depend on each other go together in one structured question call, up to the tool's limit, most consequential first. A question whose framing depends on another answer waits for that answer. While the user answers, keep working on the parts the open questions do not affect. Ask about intent, not lookup facts. Follow consequential branches until their behavioral implications are resolved; reuse prior answers rather than asking equivalent questions. Show conflicts with code, contracts, earlier specs, or decisions and ask only if evidence cannot settle the intended outcome.

   An explicit user deferral of a blocking decision stays in Further Notes with its affected stories/contracts and keeps the entire evolution draft. Do not treat an inferred answer as confirmed or place unresolved choices in committed criteria. Non-blocking Inferred/Unknown entries need not become Known.

3. **Classify the complete delta.** Place each agreed item against the verified baseline into exactly one class:
   - **New:** behavior absent from the baseline.
   - **Changed:** baseline outcome, conditions, or limits change.
   - **Unchanged:** adjacent or touched behavior at real regression risk, not the whole feature.
   - **Removed:** behavior goes away.

   Resolve unclassifiable items through evidence or step 2. Use the minimum stories needed for complete observable coverage; preserve IDs under the rules below. Identify affected contract elements and compatibility outcomes before committing criteria.

   For every affected invariant, record its existing rule, proposed rule (including preserved/retired), applicability, operations capable of violating it, conflicting existing data, and externally verifiable criteria/tests. Keep invariant IDs and distinguish verified constraints from derived risks. A derived risk prompts investigation, not an invented domain requirement.

   For CRITICAL changes, evaluate applicable concurrent modification/races, duplicate execution, retries/idempotency, partial failure, transaction boundaries, migration failure, mid-journey state, and rollback after new-format writes. Trace relevant writers and failure boundaries. Record material findings in Compatibility/Contract Impact and reference their criteria from Testing Decisions. Briefly justify inapplicable risk categories together; do not invent locks, transactions, migrations, or numeric guarantees.

4. **Plan tests and write stories.** Prefer existing high-level behavioral seams and test prior art. Exercise Changed and at-risk Unchanged behavior together where practical; separate seams when real boundaries require them. Reuse seams without a routine approval question; clarify only a choice that blocks a verifiable outcome. Add no testing abstraction without demonstrated value.

   Apply the template's coverage and ID rules. Map existing users/records, negative requirements, boundaries, and important failures to distinct criteria. For CRITICAL changes, include applicable consistency, concurrency, migration, and recovery scenarios. Define expected observable results from evidence or confirmed decisions, not implementation details. Testing Decisions references criterion IDs rather than duplicating scenarios; distinguish existing coverage, planned coverage, and checks actually run.

5. **Check traceability and stop investigating.** Verify `User request → Baseline → Delta → Story → Acceptance criteria → Contract impact → Test coverage` using concise references within the existing sections, not a second narrative or mandatory extra document. Every New, Changed, or Removed item needs an observable story/criterion; every at-risk Unchanged item needs regression coverage. Each affected contract element and compatibility outcome must link to those IDs, or have a supported no-impact reason.

   Investigation is sufficient when the affected baseline is verified, every in-scope delta is classified, significant uncertainties are resolved or explicitly deferred, affected invariants are identified, compatibility outcomes are defined, criteria are objectively testable, and regression coverage addresses meaningful risks. Stop at that boundary; unrelated implementation detail adds no requirement. Deferrals permit completion of a draft, not readiness.

6. **Publish safely and verify.** Retain all template sections and contract fields at every level; SMALL may use a sentence or a supported “Unchanged / Not applicable” with a baseline reference. Reference unchanged content instead of duplicating it.

   Readiness requires step 5's checks, usable configured publication operations, and no unresolved blocking decision or unknown applicable Compatibility outcome. Explicitly deferred blockers keep the entire spec draft, not partly ready. Report the precise blocker; use a supported non-ready status, or retain the draft in the response if no safe draft operation exists. Never apply the ready mapping to a draft.

   For local Markdown, append one `# Evolution <NN>: <title>` section to the end of the epic's `spec.md`, with the template's sections as `##` headings under it. `<NN>` starts at `01` or increments the highest existing evolution heading. Put a `Status:` line right under the heading, set to the configured publication value only when ready. Create no other file. Never edit the original spec or an earlier evolution section. To resume a draft, edit only its own section. The `# Design` section belongs to to-tickets, which updates it from Contract Impact.

   For a remote tracker, use configured operations to create the evolution issue (link operations that need only its identifier run together in one batch), reference the original and earlier evolutions in Feature, and link to the original and previous evolution without rewriting historical spec bodies. Apply the mapped ready label/status only after content and required links are verified. No additional triage is needed.

   Read back the local file or remote issue and verify its complete body, stable references/numbering, required links, and actual label/status. Report its path/URL and verified status. A failed or partial operation is not publication success: inspect the created artifact before retrying to avoid duplicates; withhold readiness until required operations succeed.

The tickets come from **to-tickets**, which updates the design contract first and then slices only the delta. In the same session, to-tickets reuses this run's evidence map and Baseline as its Phase A grounding, so it does not re-read the feature.

<evol-spec-template>

## Feature

The feature being evolved, by its domain name, with links to its original spec, earlier evolution specs in order, and its design contract.

## Context

Risk level and evidence-supported reason, followed by the final context card from step 1. Retain Known/Inferred/Unknown labels and concise evidence references. Point non-blocking uncertainties and explicitly deferred blocking decisions to Further Notes; do not relabel them Known.

## Baseline

What the feature does today, from the user's perspective, in a few sentences, followed by:

- **Existing stories:** the baseline story IDs and titles this evolution touches, from the latest spec that defines each one.
- **Sources:** each spec, contract, and code or test area the baseline was read from.

## Problem Statement

What the user cannot do, or does badly, with the feature as it is today, from the user's perspective.

## Solution

How the feature changes, from the user's perspective. Say what stays the same as clearly as what changes.

## Delta Summary

One table, one row per story, in User Stories order. Link each row to the request/decision it covers; use concise baseline → proposed outcome comparisons.

| Story | Class | Change in one line |
|---|---|---|
| <stable ID> | <New/Changed/Unchanged/Removed> | <request reference; baseline → proposed outcome or preserved behavior> |

## User Stories

Use the minimum non-duplicative set of stories covering the agreed delta, grouped by journey or capability. Coverage, not a target count, determines completeness.

### Coverage rules

- Apply **to-spec**'s coverage rules, not its interview or seam-approval process: real actors, observable outcomes, relevant full journeys including failures/boundaries, established quality constraints only, explicit negative requirements, **Confirmed** versus **Derived** basis, and unresolved choices in Further Notes.
- **Changed** stories state the baseline behavior and the new behavior side by side, so a reader sees exactly what moves.
- **Unchanged** stories exist to catch regressions. Their acceptance criteria describe today's behavior precisely enough that a test written from them fails if the evolution breaks it.
- **Removed** stories state what the user sees instead after removal (an error, a redirect, a replacement path) and what happens to data the removed behavior created.
- Follow each journey through existing state, not only fresh state: a user, record, or setting created before the evolution must reach a defined outcome.

### Story IDs

- Keep every baseline story ID. A Changed, Unchanged, or Removed story uses its existing ID.
- A New story takes the next unused ID after the highest ID in the feature's specs. Never reuse an ID, including a Removed one.
- Acceptance criterion IDs follow the same rule within each story. Keep criteria that still hold, revise changed criteria in place, and append after the highest allocated number. Record retired IDs as `(removed)` references with their replacement criterion or removal outcome, outside the active acceptance list so tickets do not reintroduce retired behavior. Never renumber or recycle IDs. If the baseline has no IDs, allocate once after checking all historical allocations and record the mapping to baseline outcomes.

### Per-story structure

Use one numbered entry per independently meaningful outcome:

1. **US-004: <short outcome title>**
   - **Class:** New, Changed, Unchanged, or Removed.
   - **Story:** As an <actual actor>, I want to <observable capability>, so that <specific user benefit>.
   - **Baseline:** For Changed, Unchanged, and Removed stories, today's behavior in one or two sentences, citing the spec or code it came from. Omit for New.
   - **Basis:** Confirmed or Derived, followed by the conversation decision or established domain rule supporting this outcome.
   - **Preconditions:** Relevant starting state, ownership, permissions, or existing data, including state created before this evolution. Omit if none are needed.
   - **Acceptance criteria:**
     - **US-004-AC1:** Given <state>, when <action>, then <observable result>.
     - Mark each revised criterion `(changed)` and each added one `(new)`. Add criteria for applicable alternative, failure, boundary, and pre-existing-state behavior. Each criterion must cover a distinct condition or boundary; use concrete inputs and results where known; avoid subjective criteria such as "works correctly" or "is user-friendly".
   - **Dependencies:** Other story IDs only when their outcomes genuinely gate this one. Omit when independent; these are not implementation-ticket blocking edges.

## Compatibility

One entry per Changed or Removed story, and per New story that changes shared data or a public interface. Cite story/criterion IDs; shared outcomes may be stated once and referenced:

- **Existing users:** what someone mid-journey or used to the old behavior experiences on the first use after release.
- **Existing data:** what happens to records created under the baseline. Migrated, defaulted, left as is, or deleted, and how that is verified.
- **Existing callers:** API clients, integrations, and other features that depend on the old behavior, and whether they keep working, need a version or flag, or break by agreement.
- **Rollout and rollback:** established release approach and data-format compatibility in both directions. Evaluate data written after release before claiming rollback safe. If rollback cannot restore the previous behavior safely, state the limitation and required recovery strategy, linked to its criteria. Unsupported strategy choices remain decisions, not promises.

Unknown applicable outcomes go in Further Notes and block readiness for the entire evolution. A supported no-impact statement is sufficient; no rollout mechanism is required without evidence.

## Contract Impact

How the evolution affects the design contract, linked to story/criterion IDs. **to-tickets** updates the contract before slicing only the delta. If absent, write "No design contract exists"; to-tickets creates one when needed. For copy/docs/config that changes no function or type, state the supported `contract: n/a` reason used by to-tickets. Retain each field below; references or brief no-impact statements suffice.

- **Vocabulary:** additions with kind and source of truth/writer; renames, removals, or kind changes with affected callers and open tickets.
- **Functions:** additions/removals and changes to intent, pre/postconditions, errors, or idempotency; identify preserved affected functions by reference.
- **Invariants:** affected invariant IDs with the rule comparison, applicability, violating operations, existing-data conflicts, and proof criteria/tests required by step 3. Preserved invariants still need coverage when at risk.
- **Flows:** affected steps and state transitions in execution order, including failure/recovery paths where relevant. Reference unchanged steps rather than copying flows.

## Implementation Decisions

Only decisions actually made: affected modules/interfaces, established architecture, schema/migration or API/versioning choices, and relevant interactions. Cite the supporting decision or constraint; leave design choices not needed for behavioral correctness to **to-tickets**.

Do NOT include specific file paths or code snippets. They may end up being outdated very quickly.

Exception: if a prototype produced a snippet that encodes a decision more precisely than prose can (state machine, reducer, schema, type shape), inline it within the relevant decision and note briefly that it came from a prototype. Trim to the decision-rich parts, not a working demo, just the important bits.

## Testing Decisions

Reference criterion IDs rather than restating scenarios. Include:

- Existing high-level behavioral seams, relevant modules, and prior test art.
- Existing coverage for at-risk Unchanged criteria; tests to revise for Changed/Removed criteria and gaps needing new coverage.
- How pre-existing records and active workflows are represented.
- Applicable boundary, failure, consistency, concurrency, migration, and recovery coverage; link invariant proofs.
- Evidence already exercised, separately from planned tests. For CRITICAL criteria, link the rule/decision supporting the expected result.

## Out of Scope

A description of the things that are out of scope for this evolution, including baseline behavior that stays as is without being at risk.

## Further Notes

Spec/code disagreements, non-blocking uncertainties, and explicitly deferred blockers. For each unresolved item, state its evidence, consequence, affected IDs, and whether it blocks readiness. Keep unsupported proposals outside committed stories and criteria.

</evol-spec-template>
