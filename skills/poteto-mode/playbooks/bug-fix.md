### Bug fix

**You own this task. Plan, review, verify.** Delegate investigation and the fix to subagents, stay in the lead. Pick each subagent's reasoning level and fast or standard speed for the job, per the Subagents section.

Be scientific. Every shipped line traces to runtime evidence. Belt-and-suspenders that "might help" is a hypothesis, not a fix. It does not ship. When evidence refutes a hypothesis, revert what it motivated. The smallest change the evidence justifies ships, nothing more.

1. Reproduce it yourself on the matching surface via the control skill (Non-negotiables), even when a debug or instrumentation protocol says to ask the user to reproduce. Ask the user only with a stated, specific reason the control surface cannot reach the target, and only after driving it as far as it goes. If it won't reproduce directly, synthesize the trigger, tighten conditions, or instrument until it fires.
2. Binary-search the cause. Form candidate hypotheses, then rule them out until one survives. Seed them with `how` over the affected subsystem and **why** for regression history. Each pass, take the split that cuts the most remaining problem space, get runtime evidence, and eliminate. When program state is unclear, add instrumentation or logging and read it as the code runs. Do not guess. For direct boundary tracing when useful, run **diving** separately under its own Read, Trace, Compare, Isolate, Verify, Report procedure; hand it the step 1 repro and reuse its evidence instead of retracing. Drive a long or stubborn hunt with the available wake mechanism per **Autonomous run**. Confirm the surviving mechanism with runtime evidence before the step-3 design and interrogate fan-out.
3. Plan the fix. If it crosses a function boundary, run `to-tickets` in **design-only** mode first. Pass the investigation's grounding; reuse it while fresh and complete and re-ground changed or uncovered subsystems as needed. Delegate implementation to a subagent using your configured bug-fix model (default `grok-4.7-xhigh-fast`) with a specific scope. If implementation exposes a wrong architecture, re-ground and redesign per `to-tickets` Phase E.
4. Verify on the same surface. The original repro now passes. "Inconclusive" or wrong-surface is not a pass. Flag it. Unit tests show branch behavior, not bug absence.
5. Stage the commits so the failing repro lands before the fix in git history, per the **tdd** skill. The red step is mandatory. When a committed test is impractical, the red step is the executable repro from step 1.
   This is the canonical **sequence-verifiable-units** principle skill, the failing test first and the fix on top.
6. Run **Opening a PR**.

**Reply:** what was broken, root cause, fix, how you verified. Paste failing-then-passing repro output verbatim.
