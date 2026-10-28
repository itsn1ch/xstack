---
name: diving
description: "Find where the code first breaks its contract and do not stop until it is found or an evidenced blocker makes that impossible. A fixed procedure (Read, Trace, Compare, Isolate, Verify, Report) follows the real call path from source to the first wrong hop, or the first violated behavioral boundary for missing operations, state, auth, concurrency, and async defects. A missing fact triggers more code investigation, never an UNKNOWN verdict. Every verification level runs through the swarm skill, capped by hardware. Every claim carries evidence gathered this session. No hypothesis brainstorming, no invented names, no verdict stronger than its evidence. Read-only. Use for /diving, 'dive into this bug', 'find the root cause', 'prove why this breaks'."
disable-model-invocation: true
---

# Diving

**Trace first. Reason from evidence. Verify only what matters.** Find where the code first breaks its contract, prove what can be proven, and stop once it is found.

**Do not stop at uncertainty.** Investigate until the earliest violated boundary is found, or a concrete, evidenced blocker makes further investigation impossible. Be relentless in tracing code, conservative with hardware, and uncompromising about evidence.

Diving is diagnosis only. It never fixes, refactors, or leaves a change in the user's tree. A fix goes to poteto-mode's Bug fix playbook with this report as its step 2. The report carries enough evidence that the fixer does not retrace the path.

## What you are looking for

**First wrong hop.** The earliest point in the execution flow where an output violates its source of truth while its inputs are still correct. A wrong hop with a wrong input is downstream of the cause, not the cause. Use this for linear flows, which is the default.

**First violated behavioral boundary.** The same idea when the defect is not a wrong output, as in these cases:

- A missing operation.
- A wrong state transition.
- An authorization violation.
- Concurrent operations.
- Async execution or event ordering.
- A transaction left inconsistent.
- Stale state.
- Duplicate processing.

It is the earliest point where an invariant or a required interaction breaks.

## Iron rules

No exceptions. A report that breaks one is not a diving report.

1. **Evidence or nothing.** Every factual claim about the codebase cites evidence gathered in this investigation. That is a `path:line` you read, a command with its actual output, a test result, a log or trace record, or a commit SHA.
2. **Unknown is a task, not an answer.** Mark a missing fact `UNKNOWN: <the read or run that would settle it>`, then go get it (see **Resolve every open fact**). Never turn it into an assumption, and never leave it as the diagnosis. "Probably", "likely", "should", "seems", "I think", "root cause unknown", "needs further investigation", and "cannot determine from available information" never appear in a report.
3. **No invented names.** Every file, function, variable, value, and line you name was opened, searched, or printed in this investigation.
4. **No invented hops.** A hop exists only once you have read its implementation. A function name is not evidence of what it does.
5. **Static is not runtime.** Reading code is not running it. Source order is not runtime order. Unobserved ordering is an open fact to resolve.
6. **Recorded is not executed.** Logs and traces from elsewhere are recorded evidence. A run in this investigation is executed evidence. Label each. A user's report is evidence of the report, not proof of the cause.
7. **No invented contracts.** When the expected behavior is not established by a source of truth, keep searching every tier in **Source of truth**. If none defines it, report the missing contract as a blocker. Do not supply one.
8. **No verdict stronger than its evidence.**
9. **Leave the tree as found.** No permanent change. Never touch uncommitted user changes. Probes live outside the tree or in an isolated worktree, and you show their removal.

## Source of truth

The expected behavior comes from, strongest first:

1. Explicit requirements.
2. Accepted specs and evolution specs.
3. Design contracts and invariants.
4. API and database schemas.
5. Domain rules.
6. Existing regression tests.
7. Relevant configuration.

The code shows what the program does, not what it should do. When two sources conflict, report both citations. Do not pick one.

## Procedure

Run the steps in this order. Do not skip one, and do not reorder them. Keep only this state between steps:

- The current verified boundary.
- The verified upstream path.
- The unverified segment that remains.
- The contract that applies.
- The evidence references.
- The open facts, each with the source that can settle it.

### A. Locate

Start from what the caller handed you. A repro command, a stack trace, a failing test, or an earlier diving report for the same symptom already locates part of the path. A verified hop from an earlier report stays verified when `git diff --stat <its sha> -- <its paths>` is empty. Re-verify only hops whose files changed.

Find the exact code tied to the symptom with targeted searches. Search for the error string, route, handler, function, query, or the state mutation. Send the independent searches together in one batch, not one after another. Record three things:

- The entry point.
- The symptom location.
- The files in between.

Read function bodies and small ranges, not whole files. Do not open unrelated modules.

**Regression shortcut.** When the symptom is a regression with a known-good revision, and a level-1 probe tells good from bad deterministically, run `git bisect run <probe>` in an isolated worktree as one swarm level-1 check. It narrows the search to one commit's diff. Trace the hops that diff touches first. The commit is history, not the boundary. Steps B to G still apply in full. For a flow too large to locate this way, run the **how** skill on that slice, then open each location it names yourself before you use it.

### B. Trace

Follow the actual caller-to-callee chain from the entry point to the symptom. Pick the shorter direction. When the symptom is a concrete wrong value at a known location (a stack frame, an assertion, a log line), start there and walk backward through the producers of the wrong value until a hop's inputs hold. Walk forward from the entry point when the defect is a missing operation or an ordering problem. Either direction records the same hops. For each hop, record these:

- The `path:line`.
- The caller and the callee.
- The input or prior state.
- The output or next state.
- The contract or invariant that applies.

Rules for the trace:

- Read each callee's implementation before recording its hop.
- Do not trace branches the path does not take.
- Do not re-read a hop you have already verified.
- Stop going downstream once the symptom is reached.

**Switch to a causal graph only when the code shows the failure is non-linear.** That means shared mutable state with more than one writer, an `await` or callback between a read and its dependent write, a queue, an event, a retry, or a second process. Record these things:

- The relevant operations.
- The shared state.
- The ordering or invariant the operations must keep.
- The ordering you actually observed, if any.
- The orderings that remain unverified.

Draw a diagram only when it helps you find the boundary.

### C. Compare

At each hop, compare the implementation against its source of truth. Mark the hop **holds**, **violates**, or **UNKNOWN**. UNKNOWN is a working mark only. Every UNKNOWN hop on the path goes through **Resolve every open fact** before you isolate. A hop with no established contract stays open until a source of truth settles it. If no source in **Source of truth** defines it, that is the missing-contract blocker below.

### D. Isolate

Find the earliest hop that violates its contract. Whenever a suspected boundary receives an input that is already wrong, it is not the cause. Trace backward to the producer of that input and repeat until you reach a hop whose inputs hold.

- **Linear flow.** All three must be shown. The hop's inputs satisfy their contracts. The hop violates its contract. The violation propagates to the symptom through the hops after it.
- **Long linear flow.** Once the path is traced, bisect the unverified segment. Check the midpoint. If it holds, continue downstream. If it violates, continue upstream. Repeat until one hop has inputs that hold and an output that violates. Bisect only when an error cannot be masked, corrected, or introduced again independently further down. Otherwise check each hop in order.
- **Missing operation.** All four must be shown:
  - A source of truth requires the operation.
  - The traced path reaches the point where the operation is required.
  - That path does not perform it.
  - No other path on the trace supplies it.

  Absent code has no line number. Cite the verified caller, the enclosing branch, or the surrounding code where the operation belongs.
- **Non-linear flow.** Find the earliest interaction or invariant that breaks, such as a read and a dependent write separated by an `await`.

An inconclusive first pass is not a result. If the earliest boundary is not yet established, escalate per **Resolve every open fact**. Report a narrowest verified range only under an objective blocker, with its verified upstream end and its verified violating end.

### Resolve every open fact

Run this loop whenever a hop, a contract, or an ordering is open. It runs between any steps and repeats until no open fact affects the conclusion.

1. Name the exact missing evidence.
2. Name the code, caller, dependency, contract, test, configuration, or history that can settle it.
3. Read or check that source.
4. Continue the trace from what it shows.

When direct tracing still does not isolate the boundary, escalate in this order and stop at the first step that settles it:

1. Trace the upstream dependencies.
2. Read every writer and reader of the affected state.
3. Compare the competing execution paths against the contract.
4. Read the existing tests for the boundary.
5. Read the configuration and the state transitions.
6. Run targeted static analysis, such as a call-site listing or a type check of one file, through the swarm skill at level 0, or level 1 when it executes a tool.
7. Run a minimal isolated probe through the swarm skill at level 1.
8. Read targeted history per step F, when it can resolve the behavior.

Steps 1 to 5 are reads you do yourself, like Locate and Trace. Steps 6 and 7 are checks, and every check goes through step E. Escalate investigation depth, never hardware. Incomplete static tracing is not a reason for a browser, an emulator, Docker, a full build, a full suite, or more infrastructure.

**Objective blockers.** Only these end the loop without a boundary:

- The required source code is unavailable.
- A necessary external system cannot be accessed.
- The required runtime state no longer exists.
- Verification would need an unsafe operation, such as a write to production data.
- A necessary run exceeds an explicit hardware or permission limit.
- No source of truth defines the expected behavior, after every tier in **Source of truth** was searched. The evidence is the list of places searched.

Each blocker needs evidence: the failed read, the denied access, the missing file, or the stated limit. Lack of effort, unexplored source, and an untested assumption are never blockers. Never invent a cause to avoid reporting one.

### E. Verify, always through the swarm skill

Every check at every level runs through the **swarm** skill. The root never runs the checks itself. One check per worker. Each check targets one boundary or one fact. Inside a swarm worker, a check that runs tests goes through `worker fast --check` or `worker run --check`, and a check that repeats one command, request, or file read over many inputs goes through one `subtask` call, per **Tests and verification** and **Subtasks** in poteto-mode.

Pick each check's level from this ladder. Use the cheapest level that can settle the fact.

| Level | What a worker runs | When |
|---|---|---|
| **0. Static** | Reads source, contracts, schemas, call sites, and existing tests for one boundary. No execution. | Always first. Default for every check. |
| **1. Probe** | One focused unit test, an isolated script, a pure-function call, or one assertion. | When static reading cannot settle the fact. |
| **2. Focused integration** | The smallest existing integration test for the boundary, with existing fakes where they keep the mechanism. | Only when the mechanism needs a real integration. |
| **3. Full system** | The running app or an end-to-end suite. | Exceptional. Needs a stated reason and the user's permission first. |

Choose the checks by what the boundary needs:

- An upstream hop holds.
- The boundary violates its contract, through an assertion of the expected output against the actual one.
- The specific failing condition triggers it.
- Substituting one isolated dependency changes the outcome.
- A controlled counterexample.
- For concurrency, the invariant fails under one minimal forced interleaving and holds under the serial order.

Each worker's brief stands alone and carries these things:

- The boundary and the exact `path:line` range.
- The contract it must meet.
- The level.
- The exact command for level 1 or above.
- "Read-only. Probes outside the tree. Report the removal."
- The return format: `PASS`, `ISSUES`, or `BLOCKED`, with the actual output or the lines read.

Independent checks go out in one wave. That includes the upstream-holds checks, the boundary-violates check, and several bisection midpoints at once. A check that depends on another result waits for the next wave. While a wave runs, the root keeps tracing the open facts the wave does not settle and drafts the next wave's briefs. It never waits idle on workers.

Cap the workers per the swarm skill's step 4: poteto-mode's hardware cap, plus the test slots in **Tests and verification** for checks that run tests. A verification swarm is not budget-capped. Without those policies, fall back to `max(1, min(floor(cores / 2), floor(available GB / 3)))` local workers. Use at most one level 2 check at a time. Never run a level 3 check without permission. When the harness has no subagent tool, run the checks yourself one after another, in the same format.

Never use these to raise a verdict:

- Browser automation, or manual UI testing.
- Emulators.
- Docker.
- A full build.
- A full suite.
- Multiple services.
- An end-to-end run.

When hardware blocks the level a fact needs, first look for a cheaper level or another source that settles the same fact. If the evidence still establishes the root cause, keep that verdict and name the block. The block becomes the hardware blocker only when the fact is needed to localize the boundary and nothing cheaper settles it.

A disappearing symptom alone never proves the cause. A check is discriminating only when it touches the one boundary and nothing else.

### F. History, when it can change the diagnosis

Run targeted history only when it can tell you one of these:

- When the regression entered.
- Which change broke the contract.
- Whether the behavior changed on purpose.
- Whether a caller or a dependency changed.

Use `git blame -L`, `git log -L`, `git log -S '<exact text>'`, and `git show <sha>` on the boundary's lines only. When that is more than a few commands, run them as one `subtask --keep-going` call with one JSONL `argv` task per command, per **Subtasks** in poteto-mode. A line's last edit is not when the bug entered. Report uncertainty in the history as it is. Run the **why** skill, narrowed to the relevant commits, only when the intent behind a change matters to the diagnosis.

### G. Stop and report

A diagnosis is complete only when it has all six:

1. The earliest violated behavioral boundary.
2. Its exact `path:line`, or the verified surrounding location of a missing transition.
3. The source of truth it violates, cited.
4. The causal path from that boundary to the symptom.
5. The evidence for each of these.
6. Verification that matches the verdict, with no open fact left that would change the conclusion.

Missing one, go back to **Resolve every open fact**. Stop without all six only under an objective blocker, with verdict BLOCKED. Once complete, stop. Do not expand the investigation to make the report longer.

Before you send the report, confirm the tree matches how you found it (`git status`). Then delete every sentence that carries no evidence.

## Verdicts

Each verdict means one thing. They are not steps on a ladder you must climb. UNKNOWN is never a verdict. The first three name a root cause. Prefer the strongest one the evidence supports.

- **PROVEN.** The boundary and its mechanism are established by direct evidence, and a discriminating check actually ran and confirmed them.
- **LOCATED.** A specific boundary is supported by executed or recorded runtime evidence, but the decisive causal check could not run because of an objective blocker for that check. Name the blocker. If nothing objective stops the check, run it instead of reporting LOCATED.
- **STATICALLY SUPPORTED.** Source code and an authoritative contract establish the violation, without runtime evidence. This is a complete diagnosis, not PROVEN, and the report says so. Do not force a runtime check when this already serves the user's purpose.
- **BLOCKED.** An objective blocker prevents localization. Exceptional. Never a substitute for investigation. A swarm worker returning BLOCKED for one check is not this verdict. It is an open fact to route around.

## Report

Write in plain words through the **unslop** skill. Give the shortest complete diagnosis. No unrelated architecture, and no investigation transcript unless asked.

```
Verdict: PROVEN | LOCATED | STATICALLY SUPPORTED | BLOCKED

First violated boundary: path:line, or the verified surrounding location
for a missing operation. Say "first wrong hop" when the flow is linear.
For BLOCKED, the narrowest verified range instead.

Why it fails: two to four sentences. The contract, how the code breaks
it, and how that reaches the symptom.

Evidence:
- path:line, what it shows
- the authority: the expected behavior and where it is established
- each swarm check: level, command or lines read, actual result (executed / recorded)

Causal path: entry (path:line) -> ... -> violated boundary (path:line) -> symptom

Verification: the checks run, their actual results, and any limitation.

History: the commit and date when established, or why history was skipped.

Blocker: for BLOCKED only. The exact blocker, the evidence that it blocks,
and the specific artifact or access needed to finish. "none" otherwise.
```
