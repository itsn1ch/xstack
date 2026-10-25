---
name: to-tickets
description: "Design before code and slice the design into tickets. Sketch types, signatures, and module structure, write the design contract (vocabulary with kinds and sources of truth, function intents and invariants, ordered flow), then slice it into ordered tracer-bullet tickets published to the configured tracker, or implement it directly. Use for /to-tickets, 'break this into tickets', 'architect this', 'design this', or non-trivial work where jumping to code would lock in the wrong shape."
disable-model-invocation: true
---

# To Tickets

Sketch types, function signatures, class shapes, and module boundaries with `not implemented` bodies and pseudocode. Synthesize across multiple model perspectives, then either fill in code against the chosen sketch or slice it into tickets.

Every design ships a **design contract**, per [`references/design-contract.md`](references/design-contract.md). It names each concept once with its kind and source of truth, states each public function's intent and invariants, and lists the flow in execution order. The contract is the source of truth for the code and for every ticket.

## Start

Pick the mode before Phase A, and name it in the todolist.

- **Tickets.** The default when a person invokes `/to-tickets`, unless the request asks to build, implement, or fix it now. Also used whenever the work does not fit one fresh context window, or when a caller such as Multi-phase plan asks for a ticket set. Phase D publishes tickets per [`references/tickets.md`](references/tickets.md) and writes no implementation code.
- **Implement.** The default when another skill or playbook routes design-before-code here (poteto-mode's Feature, Bug fix, Perf issue, and Refactoring, or **no-comments**), unless that caller asks for tickets. Also used when a person's request asks to build, implement, or fix it now ("design this and implement it"). Phase D writes the code.

State the chosen mode and the reason in the first reply line, so the invoker can correct it before Phase D. When the invoker points to an existing committed contract, or the work changes no function or type, skip Phases A to C and start at Phase D. An evolution spec from **to-evol** with Contract Impact does not skip. Ground the existing feature, then run Phase B over the changed parts of its contract.

Open a todolist with one entry per phase before starting.

1. Ground
2. Sketch and contract
3. Agree
4. Implement or publish tickets
5. Scrap

## Phase A: Ground the problem

Build a real mental model of every system the new code touches. Run the **how** skill over the relevant subsystems. Naming a file isn't grounding. Produce the architectural explanation `how` prescribes. If the design redefines ownership or layering, also run the **why** skill on the existing shape so the rationale becomes a constraint, not a guess.

Skip Phase A only when the work is genuinely greenfield with no surrounding system to integrate.

## Phase B: Sketch and contract

Run the **arena** skill with the design-sketch task and the Phase A grounding artifacts. Pass `references/runner-prompt.md` as each runner's prompt.

Take the runners from the `to-tickets runners` line in the pstack settings file, or from the older `architect runners` line when only that exists (`~/.cursor/rules/pstack-models.mdc` in Cursor, `~/.claude/rules/pstack-models.md` in Claude Code, `~/.agents/pstack-models.md` in other harnesses), in place of the `arena runners` line. If the file or that line is missing, use `claude-opus-5-5-xhigh` and `grok-4.7-xhigh-fast`. Alias and rejected entries follow the runner rules in the **arena** skill's Phase A.

Design it twice. Require at least two structurally distinct candidates before synthesis, even when the first looks sufficient. This is the **exhaust-the-design-space** principle skill made concrete. Whole-shape alternatives, not point fixes inside one shape.

Screen every candidate against [`references/design-red-flags.md`](references/design-red-flags.md) before synthesis. Assume the next contributor is an agent that sees only the files it opened, copies the nearest example, and takes the shortest path that compiles. Prefer the design where a change that looks right from one file is right for the whole repo.

Compare viable candidates on interface depth. Prefer the design that hides more complexity behind a smaller, simpler public surface. A rich interface can keep call chains short by concentrating capability instead of scattering it across layers.

Arena returns one synthesized design package. Its synthesis decision fills the rationale's "Synthesis decision" section.

Write the synthesized design's contract last, after the base and grafts are chosen. Graft names from different candidates into one vocabulary, so no synonym survives synthesis. Run the contract checks in `references/design-contract.md`. A failed check sends the design back into synthesis, not into implementation.

## Phase C: Agree (opt-in)

Default: proceed directly to Phase D with the synthesized design. No human checkpoint. Tickets mode still asks the user to approve the ticket breakdown inside Phase D.

Opt in to a checkpoint when the invoker explicitly asks: "/to-tickets with checkpoint," "stop and show me before implementing," or similar. Then surface the synthesized design and pause for sign-off.

The synthesis can ship as its own commit either way, as the "scaffold first" mode of the **foundational-thinking** principle skill. Planned and scoped breakage during fill-in is fine, per the **outcome-oriented-execution** principle skill. For adversarial pressure on the design before implementing, run the **interrogate** skill on the synthesized sketch.

If the human pushes back on the shape (in a checkpoint or after the fact), treat that as Phase A evidence. Re-ground and re-run Phase B before writing more code.

## Phase D: Implement or publish tickets

**Tickets mode.** Commit the sketch and the rationale with its contract, then follow [`references/tickets.md`](references/tickets.md) on that commit. The tickets implement the design, so this run writes no implementation code. Phase E still applies to whoever implements a ticket.

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

The caller's usage is written first and the type sketch derived from it. One file with new types and signatures for small changes. Module map plus type definitions for larger work. The rationale ships alongside, shaped per `references/rationale-template.md`, including the usage sketch, the synthesis decision, and the contract. In tickets mode, the published ticket set is also an output.
