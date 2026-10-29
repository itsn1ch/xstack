### Orchestrate

**You own the program, never the code. Author briefs, drain the queue, keep the frontier green, decide.** For a whole project handed to one standing coordinator chat: multi-day, many stacked PRs, dozens to hundreds of subagents, the human checking in twice a day instead of every five minutes. One task driven to a predicate is Autonomous run. One ambitious run needing a bespoke workflow is figure-it-out. Route here when the work outlives any single agent. Work one agent could finish inside the session's budget is not a program.

Run with the current harness's agent tools. Keep program state in its Project tracker and store; `orch` may maintain the local TSV and JSON bookkeeping below, but never spawns, waits, or wakes agents. Watch PR and CI events through subscriptions when available. Do not add polling loops or relay messages or status between agents.

Three rules carry the rest.

- Completions are queue events, not interrupts.
- Every spawn and every resume carries the standing orders verbatim.
- The brief is the product. A vague brief fails quietly, because a worker cannot ask you a question.

#### Roles and placement

- **Coordinator (this chat).** Frames, authors briefs, owns the human report, and makes judgment calls. It never authors or edits code, pushes PR branches, rebases, or merges. Each PR has one owner who carries it from build to merge and is the only writer of its branch. Conflicted merges, chain rebases, and fixes go to owners. Agents are spawned with the current harness's Task or equivalent subagent tool. Pick each subagent's reasoning level and fast or standard speed for the job, per the Subagents section. `orch` may record state at drain points.
- **Sub-coordinator.** Use one only when one area has many parallel workers. It owns that area's workers and kickoffs and sends one rollup, never raw reports. Stop at coordinator, sub-coordinator, worker. Keep roughly ten workers in flight per coordinator, capped by available hardware and harness limits, refilling as each finishes rather than waiting for a batch.
- **Worker / verifier.** Spawn a fresh worker for each independent task. Use a new cloud VM when the harness provides one, unless the task needs this machine for runtime control, local transcripts, simulators, or local-only auth. Otherwise use a supported isolated worker. Inline the context the worker cannot access. One writer per worktree or branch. Each PR gets an independent verifier on a different model family from its author; the owner merges only on that verifier's pass at the current head.

Fork when new work needs a worker's built state, such as its checkout, uncommitted changes, or running process, if the harness supports it. Message a worker only for a follow-up on the same work, never to check on it.

Depth stays at coordinator, track, worker. Author the track decomposition per project (build, landing, and verification are common cuts, not a required shape). Hard-coded swarm trees were tried and parked as too rigid.

#### Store layout

When using `orch`, create `orchestrate/<project-slug>/` in the current agent's store (path in the system prompt). Every file has exactly one writer. Owners publish facts, readers aggregate at read time. Install the Rust tools once with `cargo install --locked --path <poteto-mode>/scripts`, where `<poteto-mode>` is this skill's directory. Its canonical plain TSV and JSON stay readable without the CLI. Otherwise keep equivalent facts in the Project tracker and store using the harness's own tools.

- `preferences.md` is the standing-orders register: numbered lines, one constraint each (model policy, stack shape and count, verification bar, forbidden paths, escalation policy). Paste it verbatim into every spawn and every resume. Directives decay across resumes, and each dropped one costs a human turn. When you catch yourself restating an instruction, append the line before you act (principle-encode-lessons-in-structure).
- `overview.md` is the durable PR and issue DB. Append. Never rewrite wholesale per event.
- `units.tsv` has one row per unit: id, track, state, branch, PR, head SHA, brief path. Update rows in place.
- `frontier.json` is the computed merge frontier, per Stack safety.
- `ledger.tsv` is the verification ledger, per Verification.
- `inbox/` holds completion pointers. `gates.md` parks human gates (question, options, default on no answer).
- `decisions.tsv` is the trail via the show-me-your-work skill.
- `status.md` is derived from `units.tsv` and `ledger.tsv` at each drain, never hand-maintained. Regenerate it from the tables instead of narrating events into it.

#### The brief

Your prompts to agents are your only product, and a sloppy brief compounds into slop across the whole tree. Every spawn carries all of it. A field you cannot fill is a unit you have not scoped yet.

```
GOAL         one sentence, the outcome, executable by a stranger with no chat access
SCOPE        paths this unit may write; paths it may not; its exclusive worktree or branch
CONTEXT      pointers to files and PRs; upstream reports pasted in full when this unit
             depends on them, because workers cannot see siblings
ACCEPTANCE   checkable criteria, one per line
VERIFY       exact commands or the control-skill path, plus known gotchas
TIMEBOX      rough cap on runtime; on expiry, return partial findings and stop rather than run on
FORBIDDEN    no writes outside the assigned branch or scope; no base closure or
             retarget without the coordinator's call; plus unit-specific bans
REPORT       status, branch, head SHA, PRs, verdict, what you actually ran, deviations,
             suggested follow-ups
STANDING     <preferences.md pasted verbatim>
```

Size the brief to the unit. A one-command unit gets the template collapsed to a paragraph that still names goal, scope, the verify command, and the report shape. A 4KB scaffold around a two-line edit costs more to write and obey than the edit. Local spawns may reference the standing-orders file by store path. Verbatim paste is for cloud spawns and every resume.

A sub-coordinator brief adds its track boundary and unit list, its spawn budget with the cloud default and the local exception list, the drain protocol, and the rollup format (per child: name, status, PR, head SHA, verdict, one line, plus track status and frontier delta).

A dependency is a context relay, not just ordering. Undeclared upstream context makes the worker guess. Missing fields are a refuse-to-spawn condition. Audit one sampled worker brief per sub-coordinator per wave, concurrently with the wave it samples, never as a gate in front of it. A failing brief stops that track and fixes the sub-coordinator's instructions, not just the worker, because brief quality decays late in a run. Never resume-chain a brief. Respawn fresh with consolidated scope.

#### Steps

1. **Frame.** State the countable done predicate, scope, expected stacks, and budget. If one agent could finish inside that budget, run Autonomous run instead. Name tracks per project. A contested decomposition or one-way door goes through the arena skill before the pilot.
2. **Record state.** Open the trail via the show-me-your-work skill and record standing orders before any spawn. If using `orch`, run `orch init` and seed `frontier.json` from existing PRs with `orch frontier set --repo <repo-dir>`, adding `--forge origin` when appropriate.
3. **Pilot.** Take one unit through kickoff, owner, independent verifier, and merge before fanning out. The first normal unit is the pilot for near-identical cheap units. Fix the brief, verification recipe, and unit size from pilot evidence.
4. **Scale.** Spawn a rolling window of workers up to the hardware and harness cap, refilling as children finish. Spawn sub-coordinators only when an area has many parallel workers. Include each full upstream report a dependent kickoff needs. Sample sub-coordinators' briefs while workers run and fix their instructions when one fails.
5. **Supervise.** Read each diff or artifact, not just its summary. Check a PR's live state through the forge before reporting it. Send back work that drifts from scope; park nonblocking discoveries as follow-ups. Account for every worker as done, replaced, or absorbed. Record state with `orch` if using it.
6. **Land.** Landing runs alongside remaining work. Each owner lands only its own PR after the independent verifier passes its current head and applicable gates clear. Coordinate dependent branches bottom-up. Advance the recorded frontier on merge or new head.
7. **Close.** Reconcile every spawned agent as done, replaced, or absorbed. Confirm the predicate on the real artifact and each landed PR's verdict at its landing head. Audit the trail per show-me-your-work, including its cross-model review, and retain the store for the postmortem.

#### Queue and drain

- On completion, classify the result and update the Project tracker. If using `orch`, record completion pointers with `orch inbox push <agent> <unit> <status> [--report PATH]`, drain with `orch inbox drain`, and update units and ledger rows with `orch unit add`, `orch unit set`, and `orch ledger record`. `orch status` summarizes recorded state.
- Finish a critical section before processing a completion. Read the diff or artifact and its receipts rather than trusting the worker's summary. Account for each spawned child as done, replaced, or absorbed.
- Use event notifications for PR and CI changes when available. If the harness cannot provide a persistent wake, report that limit and provide resume context per **Autonomous run**; do not claim unattended supervision.

#### Stack safety

- When using `orch`, recompute `frontier.json` after each merge or stack mutation with `orch frontier set --repo <repo-dir>` and, for Origin, `--forge origin`. It walks the current branch's PR base chain and uses local branch refs for heads; a branch without a PR errors rather than guessing. Resolve it from a checkout with current refs.
- Coordinate topology changes within a stack serially, but each owner alone pushes, rebases, retargets, or merges its own branch. Closing or retargeting a base PR orphans its children, so an owner does that only on the coordinator's call. Delegate chain rebases and conflicts to the affected owners, never the coordinator.
- Babysitters follow `playbooks/babysit.md`, one per stack, scoped to one frontier generation. They report conflicts to the owning agent rather than changing topology.
- One retro watcher follows merged PRs for reverts, post-merge CI breaks, and orphaned follow-ups.

#### Verification

Every PR gets a verifier that did not author it, on a different model family. The owner merges only on that verifier's pass at the current head. If the harness cannot provide an independent verifier or different model family, report that blocker rather than inventing a pass. Read the diff or artifact and its receipts; CI green is an input, not a verdict, and behavioral work needs more than a type check.

When using `orch`, write verdicts with `orch ledger record` and check the current PR and head with `orch ledger check`. `ledger.tsv` records `live-ui-verified | unit-test-verified | type-check-only | verifier-blocked | verifier-failed` keyed by PR and head SHA. A worker's self-report is not an independent pass. `verifier-blocked` is not a pass; respawn when the environment heals. `verifier-failed` goes back for a fix. A new head voids the row and needs a new verdict.

A unit is not done until its output is externalized the moment it lands, never batched to the end of the run. An owner pushes its branch; record verifier receipts in the store, including a ledger row when using `orch`. Work that exists only on one VM when that VM dies was never done.

#### Liveness and failure

- Never resume an agent to check on it. A resume restarts an idle agent. Probe read-only: the ledger, `units.tsv`, `gh`, pushed branches, the cloud agent's status in the Cursor dashboard. Transcript mtime is not liveness.
- A silent death gets a synthetic postmortem row in the inbox (unit, failure mode, last evidence, options). Replan on evidence as it arrives. Never wait for full quiescence.
- Retry by mode: cap-hit or oom, respawn with smaller scope. Network-drop, retry as-is. Tool-error, retry on a different model. Unknown, retry once. Two retries, then abandon the unit and replan around it.
- A zombie that returns hours late reconciles against the current frontier and ledger before anything is accepted. Salvage unique findings through a fresh unit, never a blind merge.
- When continued spawning would produce garbage tree-wide (bad upstream output, broken acceptance, dead infra), write a stop line at the top of the standing orders, let in-flight work finish, fix the cause, clear it.
- Bound your own infra retries the same way you bound a child's. After a few consecutive tool aborts, stop retrying. Write a terminal handoff to durable state (what is done, where it lives, the exact command to resume) and end the run.
- After a restart, check which agents and cloud work actually survive in the current harness. Re-read standing orders and recorded units, recompute the frontier, and reattach surviving work by PR and branch. Replace lost workers and resume. When using `orch`, its stale store lock clears on the next write if the holder pid is gone.

#### Escalation

Reaches the human, batched into the status page rather than per item: irreversible actions (force-push to shared branches, deploys, deletions, closing someone else's PR), genuine product or preference calls no experiment settles, a standing order that contradicts observed reality, a program-level dead end that survived a replan. Park each as a `gates.md` entry before asking, and route work around it.

Never reaches the human: frontier nudges, rebase mechanics, retries, CI flake triage, review-thread triage, format fixes, scope the brief already forbids (refuse and continue), and "should I keep going". When in doubt, act and log.

Mid-run discoveries fix only what blocks the frontier. Everything else parks in follow-ups. At this fan-out a small scope leak multiplies into PRs nobody asked for.

**Reply:** at checkpoints and close: what landed against the predicate, each PR's live state and verdict with its link, what was dropped and why, and decisions waiting on the user. Include the frontier, tracks, store and trail paths, and counts from recorded state; use `units.tsv` and `ledger.tsv` when using `orch`.
