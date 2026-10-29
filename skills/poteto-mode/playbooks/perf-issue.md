### Perf issue

**You own the measurement story. Plan, review, verify the numbers.** Tie every fix to a measurement, don't read source instead of measuring.

1. Capture a baseline trace via the matching control skill. Vet the baseline, and each later number, with the **benchmark-checklist** skill.
2. `how` to ground hypotheses. Don't claim a perf ceiling without running it first.
   Try the performance mantras in order, cheapest first:
   1. Don't do it. Stop work whose result nothing uses rather than cheapening it.
   2. Do it, but don't do it again.
   3. Do it less.
   4. Do it later.
   5. Do it when they're not looking.
   6. Do it concurrently.
   7. Do it cheaper.

   When an earlier mantra meets the target, stop.

3. Plan the fix from the trace. The red step per the **tdd** skill is the measured baseline failing the target, recorded before the fix. If it crosses a function boundary, run `to-tickets` in **design-only** mode first, passing the grounding; reuse it while fresh and complete and re-ground changed or uncovered subsystems as needed. Delegate implementation to a subagent using your configured perf-issue model (default `grok-4.7-xhigh-fast`). Pick each subagent's reasoning level and fast or standard speed for the job, per the Subagents section. Review the diff. Capture a post-fix trace. If implementation exposes a wrong architecture, re-ground and redesign per `to-tickets` Phase E.
   Apply the **sequence-verifiable-units** principle skill, verifying each attempt before trying the next.
4. Parse and compare the artifacts (JSON to sqlite, diff). "Inconclusive" or wrong-surface is not a pass. Flag it.
5. Cite the measurement in the PR.
6. Run **Opening a PR**.

For sustained improvement against a metric rather than a one-off fix, use the Hillclimb playbook (`playbooks/hillclimb.md`).

**Reply:** baseline number, post-fix number, delta, artifact path.
