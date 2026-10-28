---
name: create-verification-skill
description: 'Generate a project-local verification skill that drives your app the way a user does — any language, framework, or platform. Use for /create-verification-skill, "make a control skill for this repo", or when a project has no scripted way to prove UI/CLI/service behavior.'
disable-model-invocation: true
---

# Create a verification skill

Every serious project needs a scripted way to drive the real app and prove behavior: launch it, exercise a feature the way a user would, and capture evidence. This skill generates that as a project-local skill (`<skills>/verify-<app>/`) tailored to the repo. `<skills>` is your harness's project skill folder: `.cursor/skills/` in Cursor, `.claude/skills/` in Claude Code, `.pi/skills/` in Pi, `.agents/skills/` in Codex, OpenCode, and others. If the repo already keeps skills in one of these, use that one. You write the generator's output for the next agent, not for a human: it will be read cold, mid-task, by an agent that has never seen the app.

## 0. Subtask and worker are mandatory

Every verification skill this generates, and this generator itself, runs its mechanical work through the two xstack binaries: `subtask` for fan-out (per **Subtasks** in poteto-mode) and `worker` for shared CPU slots and gates (per **Tests and verification** and **Background verification** in poteto-mode). Driving the app the way a user does stays the agent's own work.

For mechanical sweeps, follow poteto-mode's **Subtasks** sweep and check rules, including `--keep-going`, post-edit resweeps, and actual item, failure, and remaining-finding counts. If a tool is unavailable, continue safe investigation and report the failure; the generation prerequisite below still applies.

Before step 1, run `<xstack root>/subtasks/target/release/subtask hot`, unless a `SUBTASK HOT` line is already in context. On `SUBTASK COLD`, start the printed `cargo build` commands in the background and do step 1 while they build. Step 2 waits for `SUBTASK HOT`. Without Cargo, stop and report the generation as blocked with the install hint. A verification skill without these tools is not generated.

In the generated skill, call the binaries as `subtask` and `worker` when they are on `PATH` (`cargo install --locked --path subtasks` and `--path worker` from the xstack checkout), otherwise by the absolute paths `subtask hot` printed. Name the install command in its Prerequisites section.

## 1. Interview the repo, not the user

Answer these from the codebase and only ask the user what you cannot observe. Mechanical scans go through one `subtask` call each, not a loop of tool calls: `--grep` over a file list for routes, commands, menu labels, ARIA labels, and data attributes, and JSONL `argv` tasks for independent probes such as `--help` of each CLI subcommand.

- **Surface:** what does a user actually touch? A web UI, a CLI/TUI, a desktop app, an API, a mobile app, a library? A repo can have several; pick the primary one and note the rest.
- **Run:** how does the app start locally? Prefer the repo's own documented dev command (package scripts, Makefile, README quickstart). Note ports, env vars, seed data, auth.
- **Drive:** how can an agent interact with it programmatically? Existing harnesses first — Playwright/Cypress specs, expect scripts, PTY helpers, curl-able endpoints, a debug port. Only then pick a generic recipe: browser/CDP for web and Electron, a tmux/PTY harness for CLI/TUI, plain HTTP for services.
- **Observe:** what evidence can be captured? Screenshots, terminal transcripts, response bodies, logs, exit codes, DB state.
- **Isolate:** can two instances run side by side (ports, data dirs, profiles)? If not, say so in the generated skill: refusing to double-drive a shared instance beats corrupting the user's session.

If the checkout doesn't build or start as-is, fix that first (or report it precisely) before generating; a skill written against a broken base teaches wrong steps. When an irrelevant missing asset blocks startup (a static dir the API never serves, a sample config), the generated skill may create it, clearly marked as verification scaffolding, and remove it in cleanup.

## 2. Generate the skill

Write `<skills>/verify-<app>/SKILL.md` with YAML frontmatter (`name: verify-<app>` and a `description` that names the app, the surface, and when to reach for it — without frontmatter the skill never registers) and these sections, each grounded in what the interview actually found (no placeholders left):

- **Prerequisites:** `subtask hot` must print `SUBTASK HOT` before Launch. On `SUBTASK COLD`, build in the background and wait; never drive without the tools. Name the install command per step 0.

- **Launch:** the exact command that starts the app for verification, and how to tell it's ready (a log line, a prompt). Launch must reach a usable state from an empty data dir, including any migrations and seed data. Ready means usable, not just that a process answers. A database health check can pass while its init scripts are still running. Run every build, install, migration, and seed step through `worker run --check "<command>"`, so it takes a shared CPU slot instead of contending with background verification and other agents. The long-lived app process itself starts outside the slots, because it would hold one for its whole life. Include teardown. For a short-lived CLI or TUI there is no server to keep alive: launch means build the binary (or install deps) once, then start each drive in its own isolated PTY or tmux session.
- **Doctor:** one read-only check that answers "is this instance worth driving?" — process up, right version/build, port owned by us, auth valid. An agent runs this first whenever anything looks off. Doctor is one `subtask --keep-going --tasks <doctor.jsonl>` call that ships with the skill: `{"http": ..}` for each health and version endpoint, `{"argv": [..]}` for each port, process, or version command, and `{"lines": ..}` or `{"grep": ..}` for each log readiness line. Its summary line is the verdict, and its results file names the failing check.
- **Drive:** the harness recipe with real selectors/commands from this repo, not examples. Prefer stable handles (ARIA labels, data attributes, prompt strings, route paths) over coordinates and tab order. Before the first drive of a change, run the project's fast gate with `worker fast --check "<affected tests>" --check "<type check>"`, so a red build never reaches the app. Repeated, independent drives (the same request across many endpoints, the same CLI command across many inputs, one scripted journey per feature) go out as one `subtask` call. A step that needs to look at the result before choosing the next one stays a serial drive by the agent.
- **Evidence:** what to capture for a proof and where it goes. State the proof standards: exercise the real user path, not internal setters or test-only endpoints; capture the action and the resulting state, not just the final screen; verify side effects (files written, rows inserted, messages sent) alongside what's visible; mocks only where a production boundary already isolates the external system. Side effects are checked through one `subtask` call: `--hash` and `--lines` on files written, `--grep` on logs, `{"http": ..}` on read endpoints. After capture, the skill writes `evidence.jsonl` with `subtask --hash` over every proof artifact. When the safe path is a dry-run or test mode, verify what it actually skips by observing (files, network, git refs) rather than trusting its name: some dry-runs still touch the network or open a browser.
- **Cleanup:** how to tear down instances the run created. Never kill by process name; kill what you started. Cleanup removes instances and scratch state, never the evidence: proof artifacts survive the teardown, in a location the skill names. After teardown, the same `subtask --hash` call over the evidence list must match `evidence.jsonl`, which proves the proof survived.
- **Helpers:** any script the skill ships is executable and its invocation is shown in the skill body. A helper the reader has to reverse-engineer is not a helper. Ship the Doctor task file (`doctor.jsonl`) and the evidence list beside the skill.
- **Background:** when a journey runs without the agent (it launches, drives, asserts, and tears down by itself, such as a Playwright spec or an expect script), add it to the project's background checks so every commit runs it per **Background verification** in poteto-mode. `worker init` replaces the whole configuration, so first read `config` from `$(git rev-parse --path-format=absolute --git-common-dir)/xstack-worker/state.json`, then rerun `worker init` with every existing `--check`, `--copy`, and option plus `--check "<journey command>"`. Without an existing configuration, initialize it with the project's full suite and the journey, per step 1 of **Background verification**. Say in the skill which journeys run there and which need the agent.

## 3. Seed the feature map

Create `<skills>/verify-<app>/features/README.md` plus one file per user-facing feature you can identify (aim for the top 3-5 to start, from routes, commands, menus, or docs). Follow the shape in [`references/feature-map-example/`](references/feature-map-example/), with a README index and one file per feature. Each file answers, from the user's point of view: what the feature is, how to reach it, how to drive it with the harness, and what observable end state proves it works. The four H2s are `Sub-features`, `How to get to it (user POV)`, `Driving it with <harness>`, and `Gotchas`. If the seed data lacks a precondition that a journey needs, the journey reports a failure and the reason goes in that feature's `Gotchas`. Never weaken the journey to make it pass. The map is the repo's maintained verification source; a proof that drives one convenient entry point is incomplete when the map lists others.

## 4. Prove the generated skill before handing it over

Run its own instructions end to end once: prerequisites (`SUBTASK HOT`), launch through `worker run`, doctor through `subtask`, drive ONE mapped feature (one is enough; the map exists so later runs can cover the rest), capture evidence, clean up. After cleanup, confirm the evidence still exists at the named location with the skill's own `subtask --hash` check against `evidence.jsonl` — a cleanup that eats the proof fails this step. A generated skill that does its mechanical steps without `subtask` or its builds without `worker` fails this step too. Fix what fails, and run the generated cleanup after every failed iteration too, so broken attempts don't strand processes and ports. A generated skill that was never executed is a draft, not a deliverable. If you proved it in a stand-in for the real target (a container in place of a fresh VM, a local stack in place of a remote one), list every setup step the stand-in did that the target will not. Those steps stay unverified until a run on the real target covers them.

## 5. Offer the maintenance loop

Point the user at `/maintain-verification-skill` for keeping the map honest as the app changes. Suggest a cadence only if they ask.
