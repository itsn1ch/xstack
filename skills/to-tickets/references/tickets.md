# Tickets

The to-tickets skill's Phase D tickets mode. Slice the design contract into **tickets**: tracer-bullet vertical slices, each declaring the tickets that **block** it, ordered so no ticket consumes what a later ticket produces. Adapted from Matt Pocock's `to-tickets`.

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

- Use the contract from this run's Phase B, or an existing contract in the epic's Design section that the invoker points to. An existing contract still runs the checks below before any slicing.
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

5. **Spec to tickets.** Every active acceptance criterion in `spec.md` is delivered or guarded by at least one ticket, and every criterion ID a ticket cites exists in `spec.md` and is not removed. Unchanged stories from an evolution count only when their criteria appear on a ticket as regression criteria.
6. **Ticket to ticket.** Every `In` from a ticket matches an `Out` to it in that ticket, with the same name, and the reverse. The source ticket is in the consumer's blocker chain. Every type marked `// from <NN>` is defined in ticket NN, and NN blocks it. At least one `Out` reaches `final`.
7. **Overview to tickets.** Ticket `00` exists with Files, Architecture, Order, and In plain words. Every bracketed number in Files is a real ticket, every work ticket touches at least one file there, every **Where.** path is in the tree with that ticket's number, and Order names every work ticket.
8. **Ticket to design.** Every owned function and every invariant ID exists in the `# Design` contract. No function has two owners. No ticket preserves an invariant before the ticket that establishes it.

With `contract: n/a`, check items 1, 2, and 5 against the deliverables and skip the rest. Ticket `00` is still written, with Files listing the docs or config it changes.

**Run the checker. It is the gate, not a reading.** Write the draft tickets in the template below into a scratch copy of the epic folder (its `spec.md` and existing `issues/`, so evolution tickets are checked against the tickets already there), then run:

```
cargo build --release --manifest-path <to-tickets skill dir>/scripts/check-tickets/Cargo.toml  # once per machine
<to-tickets skill dir>/scripts/check-tickets/target/release/check-tickets <scratch epic dir>
```

Every `FAIL` line names the ticket and the break. Fix the breakdown or the contract, never the check, and rerun until it prints `OK`. Two parts stay a reading, because a contract can name functions and invariants that existing code already delivers: item 3's flow walk, and item 4's rule that every new contract function has an owning ticket and every new invariant has the ticket that writes its Proof. Show the `OK` line with the breakdown in step 5. Items 1 to 8 hold for remote trackers too, because the check runs on the drafts before anything is published.

## 5. Quiz the user

Present ticket `00` first: its Files tree, Architecture, Order, and In plain words. Then the proposed breakdown as a numbered list. For each ticket, show:

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

Iterate until the user approves the breakdown. A change to a name, intent, invariant, or flow step goes into the contract first, then back through step 4. Rework only the tickets the change touches and the tickets that consume what they produce. Keep every other approved ticket as is, and rerun the step 4 walk over the whole set, since it is cheap.

## 6. Publish the tickets to the configured tracker

Publish the approved tickets. **How** depends on the project's configured tracker; the tickets are the same either way, only the shape of the blocking edges changes:

- **Local files** → write one file per ticket under `.scratch/<epic-slug>/issues/<NN>-<slug>.md`, plus `00-overview.md`, in the same epic folder as `spec.md`, numbered in dependency order (blockers first). Start at `01`, or after the highest existing number when an evolution adds tickets to an epic. Write nothing else. Each file's "Blocked by" lists the numbers/titles it depends on. Use the per-ticket file template below: one ticket per file, never a single combined file. The numbers are known before writing, so write every file in one batch of parallel calls.
- **A real issue tracker (GitHub, Linear, …)** → publish one issue per ticket in dependency order (blockers first) so each ticket's blocking edges can reference real identifiers. Publish in waves. Every ticket whose blockers already have identifiers goes out in one batch of parallel calls, then the next wave. A linear chain stays one at a time. Use the platform's native blocking relationship where it has one; otherwise set each ticket's "Blocked by" to the blocking issues. If the source was an existing issue, make each ticket its sub-issue (tracker doc's operation). Apply the `ready-for-agent` triage label unless instructed otherwise; the tickets are agent-grabbable by construction.

After publishing local files, run the checker once more on the real epic folder and report its `OK` line. A `FAIL` there means a write went wrong. Fix it before reporting done.

Work the **frontier**: any ticket whose blockers are all done. For a purely linear chain that means top to bottom.

Do NOT close or modify any parent issue.

Ticket `00` is the map of the epic, not work. It comes first so a reader sees what will exist and how it fits before opening any work ticket. It is never `ready-for-agent`, never a blocker, and is updated in place when an evolution adds tickets.

<overview-template>

# 00: Overview

**Status:** reference
**Contract:** `.scratch/<epic-slug>/spec.md`, section `# Design`.

## Files

Every folder and file the epic creates or changes, as one tree in a code block. Each file line ends with the tickets that touch it in brackets, then its purpose in a few words. A folder line ends with `/`. Every work ticket appears on at least one line, and every path a ticket's **Where.** names appears in the tree with that ticket's number.

```
src/
├── orders/
│   ├── model.ts          [01, 02] Order and OrderStatus
│   └── pay-order.ts      [02] payOrder
└── shipping/
    └── ship-order.ts     [03] shipOrder
```

## Architecture

The modules and how data moves between them, from the contract's flows. One small diagram (Mermaid or plain text) with each module, each arrow labeled by the vocabulary name it carries, and the ticket number that builds it. Then at most three lines on the boundaries that matter: who owns which state, where validation happens, what is external.

## Order

One line per contract flow with the ticket numbers in step order (`checkout flow: 01 → 02 → 03`), then the groups that can run in parallel. Every work ticket appears here.

## In plain words

Written last, by applying the **bro** skill to this whole overview. What gets built, in what order, and what the user can do at the end, the way one person tells another. No jargon, no type names, no ticket mechanics. At most eight sentences.

</overview-template>

Every ticket is self-contained. An implementer that reads only this ticket and the contract can build it without a design decision of its own. If a step would need one, the decision belongs in the contract first, and the ticket is not ready.

<local-ticket-template>

# <NN>: <Ticket title>

**Status:** ready-for-agent
**Blocked by:** the numbers and titles of the tickets that gate this one, or "None (can start immediately)".
**Source:** the spec story and criterion IDs this ticket delivers (`US-003-AC1`), plus the evolution number when it came from one.
**Contract:** `.scratch/<epic-slug>/spec.md`, section `# Design`. If this ticket and the contract disagree, the contract wins. Stop and report the conflict.

## Goal

One to three sentences. What works end to end after this ticket, from the user's view.

## Types

```<language>
// Every type this ticket creates or changes, copied exactly from the contract.
// A type it only uses is one line: `<Name> // from <NN>` or `<Name> // existing`.
```

## Functions

One entry per function this ticket owns, copied from the contract:

### `<exact signature>`

- **Intent.** One sentence, the caller's view.
- **Where.** The owner module, from the contract, as a backticked path that appears in `00`'s Files tree.
- **Pre.** Checkable predicates.
- **Post.** Checkable predicates.
- **Errors.** Each case and the state left behind.
- **Idempotency.** Second call and crash halfway.

## Invariants

| ID | Statement | Role | Proof |
|---|---|---|---|
| `INV-1` | copied from the contract | establish or preserve | the test that fails when it breaks |

## Flow

The flow name and the steps this ticket covers, then one line per name crossing the ticket's edge, in this exact form:

- In: `<Name>` ← <NN>, or ← existing, or ← external
- Out: `<Name>` → <NN>, or → final

An `In` from a ticket must match an `Out` to this ticket in that ticket, with the same name, and the reverse. `→ final` marks the result the user observes.

## Steps

Numbered, in flow order. Each step names one acceptance criterion or one function: write its failing test and see it fail, then implement until it passes. The last step runs the Verify commands.

## Acceptance criteria

- [ ] `US-003-AC1`: Given <state>, when <action>, then <observable result>.
- [ ] Every invariant this ticket establishes has its Proof passing.
- [ ] Every invariant this ticket preserves still has its Proof passing.

## Verify

The exact commands: the affected tests, and the formatter check on changed files.

## Do not touch

Functions and types owned by other tickets, by number. Behavior outside Goal.

</local-ticket-template>

**Remote tracker.** Ticket `00` is published first as an issue titled `00: Overview`, without the `ready-for-agent` label. Work issues use the same sections without the Status line. Add `## Parent` with the parent issue when the source was an existing issue. The Contract line points to the design comment on the spec issue. Omit Blocked by when blockers are set as native edges.

Types, signatures, and owner modules are copied into every ticket because they make it executable without guessing. Copies never drift. A contract change updates every open ticket that carries a changed entry in the same change, per `design-contract.md`.

**`contract: n/a`.** The Contract line reads `contract: n/a, <reason>`. Omit Types, Functions, Invariants, and Flow. Steps name deliverables instead of functions. Avoid other file paths and code: implementation bodies, line numbers, and paths the contract does not name go stale fast. A prototype snippet that encodes a decision more precisely than prose (state machine, reducer, schema) may be inlined, trimmed to the decision, and marked as from a prototype.
