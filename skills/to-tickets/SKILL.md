---
name: to-tickets
description: "Design before code and slice the design into tickets. Sketch types, signatures, and module structure, write the design contract (vocabulary with kinds and sources of truth, function intents and invariants, ordered flow), then slice it into ordered tracer-bullet tickets published to the configured tracker, or implement it directly. Use for /to-tickets, 'break this into tickets', 'architect this', 'design this', or non-trivial work where jumping to code would lock in the wrong shape."
disable-model-invocation: true
---

# To Tickets

Sketch types, function signatures, class shapes, and module boundaries, as the Design section's Types block in Tickets mode, or as code with `not implemented` bodies and pseudocode in Implement mode. Synthesize across multiple model perspectives, then either fill in code against the chosen sketch or slice it into tickets.

Every design ships a **design contract**, per [`references/design-contract.md`](references/design-contract.md). It names each concept once with its kind and source of truth, states each public function's intent and invariants, and lists the flow in execution order. The contract is the source of truth for the code and for every ticket.

**Files.** This skill adds no design files, sketch files, or rationale files to the repo. With a local tracker, an epic is one folder: `.scratch/<epic-slug>/spec.md` and `.scratch/<epic-slug>/issues/`. The design goes into the `# Design` section of that `spec.md`, per [`references/rationale-template.md`](references/rationale-template.md). Tickets go into `issues/`, one file each, led by `00-overview.md`: the epic's map with the folder and file tree to be built, the architecture, the ticket order, and a plain-words summary written with the **bro** skill. With a remote tracker, the design is one comment on the spec issue, and each ticket is an issue. In Implement mode the design goes into the PR description, written through `/technical-writing` then `/unslop` per **PR prose** in poteto-mode's `playbooks/opening-a-pr.md`, and the type sketch becomes the code itself.

## Start

Pick the mode before Phase A, and name it in the todolist.

- **Tickets.** The default when a person invokes `/to-tickets`, unless the request asks to build, implement, or fix it now. Also used whenever the work does not fit one fresh context window, or when a caller such as Multi-phase plan asks for a ticket set. Phase D publishes tickets per [`references/tickets.md`](references/tickets.md) and writes no implementation code.
- **Implement.** The default when another skill or playbook routes design-before-code here (poteto-mode's Feature, Bug fix, Perf issue, and Refactoring, or **no-comments**), unless that caller asks for tickets. Also used when a person's request asks to build, implement, or fix it now ("design this and implement it"). Phase D writes the code.

State the chosen mode and the reason in the first reply line, so the invoker can correct it before Phase D. When the invoker points to an existing contract (the epic's `# Design` section), or the work changes no function or type, skip Phases A to C and start at Phase D. An evolution spec from **to-evol** with Contract Impact does not skip. Ground the existing feature, then run Phase B over the changed parts of its contract.

**Subtasks by default.** Make `subtask` hot before the first step, per **Subtasks** in poteto-mode: skip it when a `SUBTASK HOT` line is already in context, otherwise run `<xstack root>/subtasks/target/release/subtask hot`, and on `SUBTASK COLD` start its build in the background and keep going. Phase A's mechanical grounding (callers, usages, and existing names over a file list) and Phase D's tracker calls go through `subtask`.

Open a todolist with one entry per phase before starting.

1. Ground
2. Sketch and contract
3. Agree
4. Implement or publish tickets
5. Scrap

## Phase A: Ground the problem

Build a real mental model of every system the new code touches. Reuse grounding the caller already produced this session (a `how` output, a to-evol evidence map and Baseline, a diving trace). Check it is fresh with `git diff --stat <its sha> -- <its paths>` and re-read only changed files. Run the **how** skill only over the subsystems that grounding does not cover. Naming a file isn't grounding. Produce the architectural explanation `how` prescribes. If the design redefines ownership or layering, also run the **why** skill on the existing shape so the rationale becomes a constraint, not a guess.

Skip Phase A only when the work is genuinely greenfield with no surrounding system to integrate.

## Phase B: Sketch and contract

Run the **arena** skill with the design-sketch task and the Phase A grounding artifacts. Pass `references/runner-prompt.md` as each runner's prompt.

**Fast arena.** A design arena produces text, not code, so it runs lean without dropping a seat.

- No worktrees and no output files. Each runner returns its whole candidate in its reply, in the compact shape `runner-prompt.md` fixes.
- All runners spawn in one message. The root reads each candidate the moment it returns.
- The rubric is fixed: interface depth, contract checks pass, invariants enforced by the strongest mechanism, flow order, red-flag screen. Do not derive a new one per run.
- When the candidates converge on one shape, ship the consensus shape and skip the cross-judge, per arena Phase E. Convergence satisfies **Design it twice** below only when each candidate's Decision lines name a structurally distinct whole shape it considered and rejected. Otherwise spawn one more runner, told to produce a different whole shape, and compare. When they diverge, spawn the cross-judge as soon as the last candidate returns and score in parallel with it.
- One round, plus that single extra runner when needed. A second full round runs only when every candidate fails the contract checks or the candidates diverge so far that Phase A was under-specified.

Take the runners from the `to-tickets runners` line in the pstack settings file, or from the older `architect runners` line when only that exists (`~/.cursor/rules/pstack-models.mdc` in Cursor, `~/.claude/rules/pstack-models.md` in Claude Code, `~/.agents/pstack-models.md` in other harnesses), in place of the `arena runners` line. If the file or that line is missing, use `claude-opus-5-5-xhigh` and `grok-4.7-xhigh-fast`. Alias and rejected entries follow the runner rules in the **arena** skill's Phase A.

Design it twice. Require at least two structurally distinct candidates before synthesis, even when the first looks sufficient. This is the **exhaust-the-design-space** principle skill made concrete. Whole-shape alternatives, not point fixes inside one shape.

Screen every candidate against [`references/design-red-flags.md`](references/design-red-flags.md) before synthesis. Assume the next contributor is an agent that sees only the files it opened, copies the nearest example, and takes the shortest path that compiles. Prefer the design where a change that looks right from one file is right for the whole repo.

Compare viable candidates on interface depth. Prefer the design that hides more complexity behind a smaller, simpler public surface. A rich interface can keep call chains short by concentrating capability instead of scattering it across layers.

While the runners work, the root pipelines per poteto-mode's **Fastest route**. It drafts the contract checks against the grounding, loads the tracker configuration for Tickets mode, and lists the flows the tickets will follow. It never waits idle on the arena.

When this run changes an existing contract (an evolution spec, a pushback, or a follow-up), the runners sketch only the changed vocabulary, functions, invariants, and flow steps plus the parts that consume them. Unaffected contract entries carry forward unchanged and still pass the contract checks.

Arena returns one synthesized design package. Its synthesis decision fills the Design section's "Decision" lines.

Write the synthesized design's contract last, after the base and grafts are chosen. Graft names from different candidates into one vocabulary, so no synonym survives synthesis. Run the contract checks in `references/design-contract.md`. A failed check sends the design back into synthesis, not into implementation.

## Phase C: Agree (opt-in)

Default: proceed directly to Phase D with the synthesized design. No human checkpoint. Tickets mode still asks the user to approve the ticket breakdown inside Phase D.

Opt in to a checkpoint when the invoker explicitly asks: "/to-tickets with checkpoint," "stop and show me before implementing," or similar. Then surface the synthesized design and pause for sign-off.

In Implement mode, the synthesized sketch can ship as its own commit before the bodies are filled in, as the "scaffold first" mode of the **foundational-thinking** principle skill. Planned and scoped breakage during fill-in is fine, per the **outcome-oriented-execution** principle skill. For adversarial pressure on the design before implementing, run the **interrogate** skill on the synthesized sketch.

If the human pushes back on the shape (in a checkpoint or after the fact), treat that as Phase A evidence. Re-ground and re-run Phase B before writing more code. Do it incrementally. Re-ground only what the pushback touches, re-run Phase B over the affected modules and the contract entries that depend on them, and keep the rest. A pushback on the whole shape re-runs both phases in full.

## Phase D: Implement or publish tickets

**Tickets mode.** Write the Design section (local: `# Design` in the epic's `spec.md`, created with only a title and that section when no spec exists; remote: one comment on the spec issue), then follow [`references/tickets.md`](references/tickets.md). Each ticket carries its own copy of the types, functions, and invariants it needs, so an implementer reads one ticket and the contract, nothing else. Tickets ship only after the Rust checker `scripts/check-tickets` prints `OK`, proving the chain from spec criteria through every ticket handoff to the final result has no gap. The tickets implement the design, so this run writes no implementation code. Phase E still applies to whoever implements a ticket.

**Implement mode.** Implement in flow order. Build the steps of each flow from step 1 upward, so every function is written after the functions that produce its inputs. Replace `not implemented` bodies with code, pseudocode with logic. Each invariant's Proof is written with the first function that must preserve it. The sketch and its contract bind the implementation. Deviations from it are signal to surface, not friction to absorb silently. If a function needs a parameter the sketch didn't anticipate, ask whether the sketch was wrong, the requirement was missed, or the implementation is overreaching. A deviation that changes a vocabulary name, an intent, an invariant, or a flow step is a contract change. Update the contract in the same commit and rerun its checks.

## Phase E: Scrap when the architecture is wrong

If implementation keeps producing friction the sketch can't absorb, throw the sketch out. Don't bolt fixes onto a wrong design, per the **redesign-from-first-principles** and **fix-root-causes** principle skills.

The signal is a *pattern*, not single instances. Tells:

- The same shape of workaround appearing repeatedly across unrelated code.
- Multiple unrelated edge cases that all need special-case branches.
- Types that need escape hatches (`any`, casts, optional fields always set in practice) to compile.
- The "we need a lock" reflex when the sketch said the state wasn't shared.
- Callers having to know the abstraction's internal rules to use it.
- Two or more independent Phase D deviations of the same shape across the implementation.

Use judgment. A few edge cases don't condemn an architecture. Some problems are legitimately complex. Complexity in the data is not complexity in the design.

When you scrap:

1. Re-run the **how** skill over what's been built.
2. Redesign as if the new constraints had been day-one assumptions, per redesign-from-first-principles.
3. Subtract before adding, per the **subtract-before-you-add** principle skill. The new sketch should be smaller than the old one before it grows.
4. Return to Phase B and re-run arena.

## Outputs

The caller's usage is written first and the type sketch derived from it. The Design section, shaped per `references/rationale-template.md`, holds the usage, the type sketch, the contract, and the decision. In tickets mode, the published ticket set is the other output. No other file is written.
