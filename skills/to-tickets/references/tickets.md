# Tickets

The to-tickets skill's Phase D tickets mode. Slice the committed design contract into **tickets**: tracer-bullet vertical slices, each declaring the tickets that **block** it, ordered so no ticket consumes what a later ticket produces. Adapted from Matt Pocock's `to-tickets`.

Read the project's agent instructions and follow their issue-tracker, triage-label, and domain-document pointers. Otherwise read `docs/agents/issue-tracker.md`, `docs/agents/triage-labels.md`, and `docs/agents/domain.md` from the project root, as configured by `/setup-xstack`. Resolve the `ready-for-agent` role to its saved label/status, including the Status field in the local template. Reuse existing project configuration; if required publication configuration is missing, direct the user to `/setup-xstack` before publishing.

## 1. Gather context

Work from whatever is already in the conversation context. If the user passes a reference (a spec path, an issue number or URL) as an argument, fetch it and read its full body and comments.

**Evolution specs.** When the source is an evolution spec from **to-evol**, slice only its delta:

- Apply its Contract Impact to the feature's existing contract first. The changed names, functions, invariants, and flow steps go through Phase B before any slicing. Steps the evolution inserts keep their position in the existing flow.
- New and Changed stories become tickets.
- Unchanged stories are not tickets. Each becomes a regression acceptance criterion on every ticket that touches its area, so its test stays green.
- Removed stories become removal tickets, sequenced expand then contract. The replacement path lands first, and the removal is blocked by it and by any data migration from the Compatibility section.
- Each ticket's Contract field names the updated contract, and its acceptance criteria cite the evolution spec's story and criterion IDs.

Look for opportunities to prefactor the code to make the implementation easier. "Make the change easy, then make the easy change." Ticket titles and descriptions use the project's domain glossary vocabulary and respect ADRs in the area you're touching.

## 2. Confirm the contract

Tickets are sliced from the design contract (`design-contract.md`). It is the source of truth for every name, function intent, invariant, and flow step a ticket mentions.

- Use the contract from this run's Phase B, committed with the sketch, or an existing committed contract the invoker points to. An existing contract still runs the checks below before any slicing.
- If the work changes no function or type (docs, config, copy), there is no contract. Write `contract: n/a, <reason>` at the top of the breakdown and slice by deliverable.

Run the contract checks. A failed check goes back to Phase B. Never paper over a contract gap inside a ticket.

## 3. Draft vertical slices

Break the work into **tracer bullet** tickets.

<vertical-slice-rules>

- Each slice cuts a narrow but COMPLETE path through every layer (schema, API, UI, tests): vertical, NOT a horizontal slice of one layer
- A completed slice is demoable or verifiable on its own
- Each slice is sized to fit in a single fresh context window
- Any prefactoring should be done first

</vertical-slice-rules>

Give each ticket its **blocking edges**: the other tickets that must complete before it can start. A ticket with no blockers can start immediately.

Slice along the contract, so the tickets follow the flow:

- **Coverage.** Every public function in the contract belongs to exactly one ticket, which owns it. Every invariant's Proof lands in the first ticket that establishes the invariant. Later tickets that touch its scope list it as preserved.
- **Flow order.** A ticket covers flow steps in order. Either a contiguous run of steps (steps 1 to 2, then 3 to 4), or a thin pass through the whole flow followed by tickets that deepen one step each, in step order. Never a ticket for step 3 before the ticket for step 2.
- **Edges from data.** A ticket is blocked by every ticket that owns a function producing a name it consumes. Add other blockers only for a real gate, and name the gate.
- **Numbering is execution order.** Number tickets in an order where every blocker has a lower number. Among tickets with no edge between them, keep flow step order. Tickets that can run in parallel share a blocker set and say so.
- **Vocabulary only.** Titles and bodies use contract names, spelled as in the vocabulary. No synonyms.

**Wide refactors are the exception to vertical slicing.** A **wide refactor** is one mechanical change (rename a column, retype a shared symbol) whose **blast radius** fans across the whole codebase, so a single edit breaks thousands of call sites at once and no vertical slice can land green. Don't force it into a tracer bullet; sequence it as **expand–contract**. First expand: add the new form beside the old so nothing breaks. Then migrate the call sites over in batches sized by blast radius (per package, per directory), each batch its own ticket blocked by the expand, keeping CI green batch to batch because the old form still exists. Finally contract: delete the old form once no caller remains, in a ticket blocked by every migrate batch. When even the batches can't stay green alone, keep the sequence but let them share an integration branch that all block a final integrate-and-verify ticket; green is promised only there. Its flow is expand, then each migrate batch, then contract, numbered in that order.

## 4. Check the order

Before showing the breakdown, walk the tickets from lowest number to highest and confirm each item. Fix the breakdown, not the check.

1. Every name a ticket consumes is produced by a lower-numbered ticket, by existing code, or by external input.
2. Every blocker has a lower number than the ticket it blocks, and the edges have no cycle.
3. Reading the tickets in number order walks each contract flow in step order, with no step skipped and none revisited out of order.
4. Every contract function has exactly one owning ticket, and every invariant has the ticket that writes its Proof.

With `contract: n/a`, check items 1 and 2 against the deliverables and skip items 3 and 4.

## 5. Quiz the user

Present the proposed breakdown as a numbered list, then one line per contract flow showing the ticket numbers in step order (`checkout flow: 01 → 02 → 03`). For each ticket, show:

- **Title**: short descriptive name
- **Blocked by**: which other tickets (if any) must complete first
- **What it delivers**: the end-to-end behaviour this ticket makes work
- **Flow steps**: which steps of which flow it covers
- **Functions**: each owned function with its one-sentence intent
- **Invariants**: IDs it establishes and IDs it preserves

Ask the user:

- Does the granularity feel right? (too coarse / too fine)
- Are the blocking edges correct: does each ticket only depend on tickets that genuinely gate it?
- Should any tickets be merged or split further?

Iterate until the user approves the breakdown. A change to a name, intent, invariant, or flow step goes into the contract first, then back through step 4.

## 6. Publish the tickets to the configured tracker

Publish the approved tickets. **How** depends on the project's configured tracker; the tickets are the same either way, only the shape of the blocking edges changes:

- **Local files** → write one file per ticket under `.scratch/<feature-slug>/issues/<NN>-<slug>.md`, numbered from `01` in dependency order (blockers first). Each file's "Blocked by" lists the numbers/titles it depends on. Use the per-ticket file template below: one ticket per file, never a single combined file.
- **A real issue tracker (GitHub, Linear, …)** → publish one issue per ticket in dependency order (blockers first) so each ticket's blocking edges can reference real identifiers. Use the platform's native blocking relationship where it has one; otherwise set each ticket's "Blocked by" to the blocking issues. If the source was an existing issue, make each ticket its sub-issue (tracker doc's operation). Apply the `ready-for-agent` triage label unless instructed otherwise; the tickets are agent-grabbable by construction.

Work the **frontier**: any ticket whose blockers are all done. For a purely linear chain that means top to bottom.

Do NOT close or modify any parent issue.

<local-ticket-template>

# <NN>: <Ticket title>

**What to build:** the end-to-end behaviour this ticket makes work, from the user's perspective, not a layer-by-layer implementation list.

**Blocked by:** the numbers/titles of the tickets that gate this one, or "None (can start immediately)".

**Status:** ready-for-agent

**Contract:** the path of the committed rationale's Contract section. It is the source of truth. If this ticket and the contract disagree, the contract wins. Stop and report the conflict.

**Flow:** the flow name and the steps this ticket covers, with what the steps before it produced and what the steps after it consume.

**Functions:** each owned function's signature and one-sentence intent, copied from the contract.

**Invariants:** each ID with its statement. Mark each `establish` (this ticket writes its Proof) or `preserve` (its Proof must stay green).

- [ ] Acceptance criterion 1
- [ ] Acceptance criterion 2
- [ ] Every invariant this ticket establishes has its Proof passing.
- [ ] Every invariant this ticket preserves still has its Proof passing.

</local-ticket-template>

<issue-template>

## Parent

A reference to the parent issue on the tracker (if the source was an existing issue, otherwise omit this section).

## What to build

The end-to-end behaviour this ticket makes work, from the user's perspective, not layer-by-layer implementation.

## Contract

The path of the committed rationale's Contract section. It is the source of truth. If this ticket and the contract disagree, the contract wins. Stop and report the conflict.

- **Flow:** the flow name and the steps this ticket covers, with what earlier steps produced and what later steps consume.
- **Functions:** each owned function's signature and one-sentence intent, copied from the contract.
- **Invariants:** each ID with its statement, marked `establish` or `preserve`.

## Acceptance criteria

- [ ] Criterion 1
- [ ] Criterion 2
- [ ] Every invariant this ticket establishes has its Proof passing.
- [ ] Every invariant this ticket preserves still has its Proof passing.

## Blocked by

- A reference to each blocking ticket, or "None (can start immediately)". Omit this section when blockers were set as native edges.

</issue-template>

In either form, avoid specific file paths or code snippets: they go stale fast. The contract path, the signatures, and the source-of-truth names copied from the contract are the exception, because the contract keeps them current. Another exception: if a prototype produced a snippet that encodes a decision more precisely than prose can (state machine, reducer, schema, type shape), inline it and note briefly that it came from a prototype. Trim to the decision-rich parts, not a working demo, just the important bits.
