### Bug fix

**You own this task. Plan, review, verify.** Delegate investigation and the fix to subagents, stay in the lead. Pick each subagent's reasoning level and fast or standard speed for the job, per the Subagents section.

Be scientific. Every shipped line traces to runtime evidence. Belt-and-suspenders that "might help" is a hypothesis, not a fix. It does not ship. When evidence refutes a hypothesis, revert what it motivated. The smallest change the evidence justifies ships, nothing more.

1. Reproduce it yourself on the matching surface via the control skill (Non-negotiables), even when a debug or instrumentation protocol says to ask the user to reproduce. Ask the user only with a stated, specific reason the control surface cannot reach the target, and only after driving it as far as it goes. If it won't reproduce directly, synthesize the trigger, tighten conditions, or instrument until it fires.
2. Find the cause with the **diving** skill. Its iron rules hold here: evidence for every claim, no assumptions, its fixed Read, Trace, Compare, Isolate, Verify procedure with every verification level run through the **swarm** skill, and a verdict no stronger than its evidence. It does not stop at an inconclusive pass. It resolves every open fact until the earliest violated boundary is found or an objective blocker is evidenced. Hand diving the step 1 repro (its command, surface, and observed output) as its symptom and entry point, so diving does not reproduce or locate again. Its report is this step's output, and step 3 reuses its traced path instead of re-reading it. A fix proceeds on PROVEN or LOCATED. On STATICALLY SUPPORTED, get the runtime evidence the fix needs before changing code, unless the contract and code already settle the change. On BLOCKED, supply what the report says is missing, or stop and report. Drive a long or stubborn hunt with Cursor's `/loop` command. Confirm the mechanism with runtime evidence before the step-3 design and interrogate fan-out.
3. Plan the fix. If it crosses a function boundary, design it first at the size the tier earns, per **Scale the ceremony to the tier**. Delegate implementation to a subagent using your configured bug-fix model (default `grok-4.7-xhigh-fast`) with a specific scope.
4. Verify on the same surface. The original repro now passes. "Inconclusive" or wrong-surface is not a pass. Flag it. Unit tests show branch behavior, not bug absence.
5. Stage the commits so the failing repro lands before the fix in git history, per the **tdd** skill. The red step is mandatory. When a committed test is impractical, the red step is the executable repro from step 1.
   This is the canonical **sequence-verifiable-units** principle skill, the failing test first and the fix on top.
6. Run **Opening a PR**.

**Reply:** what was broken, root cause, fix, how you verified. Paste failing-then-passing repro output verbatim.
