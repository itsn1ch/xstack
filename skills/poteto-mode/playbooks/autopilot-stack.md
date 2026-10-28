### Autopilot-stack

**Run Autopilot-full, but do not merge.** Hand verified PRs to the operator to review and land. Each audit tick names this playbook and re-reads it alongside Autopilot-full from the installed paths, or trunk when tracked; this playbook wins where they differ.

1. **In place of Autopilot-full step 5, stop at verified.** Each owner carries its PR through build, verification, and green CI at its current head, reports its verdict, and stops. Before handoff, the coordinator may assign each owner the bounded single-PR triage in `playbooks/babysit.md` to clear that PR's blockers without waiting for lower PRs to merge. No competing stack babysitter, cross-branch writes, merge, or auto-merge. After the handoff, nobody pushes without the operator's go.
2. **Stack only dependent work.** Independent PRs branch from trunk. When a PR builds on another, its owner branches from and targets that parent's branch per **Size and stacks** in `playbooks/opening-a-pr.md`. The stack root rebases onto trunk; each child rebases onto its parent. Only the owning agent writes, rebases, or retargets its own branch, coordinating bottom-up so there is one writer per branch. A new lower head restacks the PRs above it; a PR whose `git patch-id` the restack leaves unchanged keeps its verdict per `playbooks/shipping.md`. A conflict goes to the owner of the affected branch.
3. **Deliver, then keep hands off.** Each PR carries its verdict. Once handed off, a problem goes to the operator and a fix waits for the operator's go. During a separately authorized landing, only the post-merge retarget sanctioned by `playbooks/shipping.md` may change the chain.

**Choosing between the autopilots.** Autopilot-full when landing authority is granted. Autopilot-stack when the operator lands the PRs, whether the work is independent or dependent.

**Reply:** catch the operator up per the **ketchup** skill, with links to the stack root and tip, a one-line verdict per link, and anything parked or excluded with the reason.
