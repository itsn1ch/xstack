# Xstack

Use Xstack to write specifications, design changes, create tickets, and build software with AI agents. The package contains 62 skills from pstack and adaptations of `to-spec`, `to-evol`, and `to-tickets`.

## Install the skills

Xstack installs with the [`skills`](https://github.com/vercel-labs/skills) CLI. Install Node.js **22.20 or later** (including npm and `npx`) and Git first.

### Install from GitHub

Open a terminal in the project where you want to use Xstack, then run:

```bash
npx skills add itsn1ch/xstack --skill '*'
```

Choose the coding agents you use when prompted. Skills are installed into the current project by default. You do not need to clone this repository first.

To install all Xstack skills for a specific agent without prompts:

```bash
# OpenCode
npx skills add itsn1ch/xstack --skill '*' -a opencode -y

# Claude Code
npx skills add itsn1ch/xstack --skill '*' -a claude-code -y

# Cursor
npx skills add itsn1ch/xstack --skill '*' -a cursor -y
```

Add `-g` to make the skills available across your projects:

```bash
npx skills add itsn1ch/xstack --skill '*' -a opencode -g -y
```

To browse the available skills or install only a few:

```bash
npx skills add itsn1ch/xstack --list
npx skills add itsn1ch/xstack --skill setup-xstack --skill poteto-mode --skill diving --skill tdd
```

After installation, follow [Set up your project](#set-up-your-project) and run `/setup-xstack` in your coding agent.

### Install from a local folder

If you already cloned or downloaded Xstack, run the installer **from your target project** and pass the path to the Xstack repository:

```bash
npx skills add /path/to/xstack --skill '*'
```

The source folder must contain this README and the `skills/` directory. Replace `/path/to/xstack` with its actual path. For example, if the Xstack repository is a sibling of your project, use `../xstack`.

If you are running from the parent directory of the Xstack repository, you can inspect the skills or install them globally:

1. List the skills without installing them:

   ```bash
   npx skills add ./xstack --list
   ```

2. Install every skill globally for OpenCode:

   ```bash
   npx skills add ./xstack --skill '*' -a opencode -g -y
   ```

### Installation options

| Flag                      | Effect                                                                          |
| ------------------------- | ------------------------------------------------------------------------------- |
| `-l`, `--list`            | List the skills, then exit without installing.                                  |
| `-s`, `--skill <names…>`  | Install these skills. `'*'` installs all.                                       |
| `-a`, `--agent <agents…>` | Install for these agents, such as `claude-code` or `cursor`. `'*'` targets all. |
| `-g`, `--global`          | Install in your user directory instead of the project.                          |
| `-y`, `--yes`             | Skip confirmation prompts.                                                      |
| `--copy`                  | Copy files instead of symlinking them.                                          |
| `--all`                   | Same as `--skill '*' --agent '*' -y`.                                           |

Verify the installation with `npx skills list`, or `npx skills list -g` for a global installation. If your agent does not show the new skills, restart its session and open your target project again.

To remove a skill, run `npx skills remove <name>`, or `npx skills remove <name> -g` for a global installation. With Bun, replace `npx` with `bun x`.

Keep the package directory intact. Some skills use scripts and documents outside their own directory, so copying only `SKILL.md` is not enough.

The `skills` installer installs skills. It does not register subagents or plugin automations. For plugin installations, use the manifest for your agent app:

- Claude Code: `.claude-plugin/plugin.json`.
- Cursor: `.cursor-plugin/plugin.json`.
- General format: `plugin.json`.

Plugin support depends on your agent app. Cursor-specific features may not work elsewhere. For Claude Code, read the [command and tool adapter](docs/harness/claude-code.md).

### Install the poteto-mode command-line tools

The `orch`, `watch-pr`, `check-plan`, and `worktree-audit` commands are Rust binaries. From the package directory, install them with:

```sh
cargo install --locked --path skills/poteto-mode/scripts
```

The build requires Rust and Cargo. Add Cargo's binary directory to `PATH` if it is not there already. The tools do not require Bun, Node.js, or shell wrappers. `watch-pr` requires an authenticated GitHub CLI. `orch frontier set` uses Git and the selected `gh` or `origin` CLI. `worktree-audit` uses Git and reads PR metadata through Origin or GitHub CLI when available. Existing `orch` stores keep their TSV, JSON, and Markdown formats.

Run the tools' tests with:

```sh
cargo test --locked --manifest-path skills/poteto-mode/scripts/Cargo.toml
```

### Install the verification worker

```sh
cargo install --locked --path worker
worker probe
```

`cargo install` puts the binary in `~/.cargo/bin`. Add that directory to `PATH` to run `worker` and `subtask` by name. The session hook and the skills also find the binaries without it. To learn what the worker checks and when, read [How the worker verifies your changes](#how-the-worker-verifies-your-changes).

`worker split` distributes input items across worker queues. Each command receives at most 32 items. When a worker finishes its queue, it takes half another worker's remaining items. Running commands keep their arguments and do not restart. `-j N` limits concurrent commands, not the total number of batches.

Use `--each` when each input item needs its own test command. Use `--keep-going` to finish pending batches after a failure. The default stops pending work and cancels running siblings on the first failure. Timeouts apply to each command.

The background scheduler shares queued checks and native test shards. It cannot divide a test process that has already started. Use the test runner's shard support for background checks instead of nesting `worker split` inside an outer worker check.

### Install the subtask runner

```sh
cargo install --locked --path subtasks
subtask hot
```

`subtask hot` checks that `subtask` and `worker` match their sources, warms the CPU sample, and prints one `SUBTASK HOT` line, or `SUBTASK COLD` with the rebuild command. In Claude Code the plugin's `hooks/hooks.json` runs it at every session start, and starts the build in the background when a binary is missing or stale, so `subtask` is the default for mechanical fan-out without a manual step. `/setup-xstack` builds both binaries and adds the same pointer to the project's `## Xstack` block for other harnesses.

`subtask` runs subtasks for an agent without any model call: one shell command per item (`--sh "cmd {}"`, or 32 items per process without `{}`), HTTP requests (`--http`), and file reads (`--grep`, `--hash`, `--lines`), or JSONL lines that mix them. Each kind has its own Tokio pool, sized from idle cores, free memory, and the descriptor limit, and split evenly across every `subtask` running at once. The verification worker registers in the same share, so a background verification takes its even share of the cores and leaves the HTTP and file pools to subtasks. Results stream to a JSONL file (`--out`). Stdout prints a one-line summary and the first failures. By default, `subtask` stops at the first failure. `--keep-going` runs every task.

In a sandbox with a read-only temp directory, `subtask` still runs. It runs alone instead of joining the share, and it writes the results file to the working directory. If that directory is also read-only, it prints the JSONL results to stdout above the summary. `worker probe` and `subtask probe` reuse a recent CPU sample, so they return in a few milliseconds.

#### Shared skill execution

Any Xstack skill may invoke `subtask skill` through a command tool authorized by its host. Run it from the workspace root. The host still decides whether the command may run; subtask does not query or replace host permissions.

```sh
printf '%s\n' '{"id":"source","operation":"lines","path":"README.md"}' |
  subtask skill --skill judgment
```

The shared entry point accepts up to 64 JSONL operations and returns one ordered JSON result per operation. `read`, `lines`, `hash`, and `grep` inspect regular workspace files. `read` returns at most 4,000 bytes. Inputs are limited to 8 MiB per file and 32 MiB per batch. Each result identifies the caller-supplied skill label and operation ID; the label is not an authenticated identity. `grep` may return `null` for `truncated` when the existing output tail cannot determine whether truncation occurred. Requested file contents are not automatically redacted; avoid requesting secrets or putting them in IDs and paths.

`replace` requires `--allow-write`, `"authorized":true`, the file's BLAKE3 `"expected_hash"`, and strings `"old"` and `"new"`. The old string must occur exactly once. The operation locks the file and rejects a stale hash. Its lock coordinates cooperating subtask processes only; other programs can still race the final replacement. These controls are separate from host approval. Do not grant them when the calling skill is read-only. A timeout during a replacement may leave a private `.xstack-write-*` file in the workspace; inspect and remove it before retrying.

This entry point requires Linux Landlock ABI 3 or later and fails closed when confinement is unavailable. Its default deadline is 30 seconds for the entire batch; `--timeout-ms` accepts 1 to 300,000 milliseconds. It does not run shell commands, network requests, or verification tools. Unsupported operations return an explicit error; use the original host-approved workflow or `worker` for checks. If the executable is unavailable or the host denies it, retain the original skill workflow. The legacy subtask options described above do not acquire these restrictions.

See `subtask skill --help` and the `subtask-execution` skill for the shared instruction. When installing selected skills, include `subtask-execution` or use the Xstack setup instructions. The agent chooses when to use it; no skill invocation is intercepted automatically.

#### Browser tasks

Browser tasks require `agent-browser` on `PATH` and its browser installed:

```sh
npm install -g agent-browser
agent-browser install
```

Write one task per JSONL line. `session` names the persistent agent-browser session; `commands` is an array of argument arrays, each passed directly to `agent-browser --session <name>` without shell interpolation. For example, with a local server running on port 8000:

```sh
cat > browser-tasks.jsonl <<'EOF'
{"browser":{"session":"catalog","commands":[["open","http://localhost:8000/catalog"],["snapshot","-i"]]}}
{"browser":{"session":"account","commands":[["open","http://localhost:8000/account"],["snapshot","-i"]]}}
EOF
subtask --tasks browser-tasks.jsonl --out browser-results.jsonl --keep-going
```

Commands within a task run in order; the first failure stops that sequence. Different sessions can run concurrently, subject to the shared CPU resource gate and `--cpu` limit. Within one `subtask` run, tasks using the same session do not overlap, but their order across separate tasks is not guaranteed: put dependent actions in one task. A session name must be nonempty and cannot contain path separators. Do not put `--session`, `--cdp`, `--auto-connect`, `--profile`, `--config`, `--provider`, `--session-name`, or `-p` in a command. The runner rejects shared connection settings in agent-browser environment variables and configuration. Before reusing existing sessions, ensure they do not attach to a shared browser; the runner cannot inspect how a previously started daemon was connected. Separate `subtask` processes and external agent-browser commands are not coordinated.

Browser tasks are queued in a private temporary disk spool so a long backlog for one session does not prevent the runner from reading a later independent session. The spool is created with owner-only permissions and immediately unlinked; the operating system removes its contents when the run ends. If the temporary directory is unavailable or the disk fills, the run fails instead of silently dropping tasks. A crash between file creation and unlink can leave an empty owner-only file. Other task types retain their existing bounded queues and read-only temporary-directory behavior.

Results stream to JSONL with action numbers, exit codes, and the CLI's output and errors. The action header does not repeat command arguments, but the CLI output or the input task file may still contain sensitive data. Protect both files. `--timeout-ms` applies to each action, not time spent waiting for a session or CPU slot.

Invalid JSON or UTF-8 is reported without echoing the input. With `--keep-going`, later valid JSONL lines still run. A fatal spool or results-file error stops the run even if standard input remains open.

After a failed browser task, later tasks in the same run cannot reuse its session. A timed-out or interrupted CLI may leave its separate browser daemon executing the action; the runner does not close or kill that daemon. This precaution does not persist across `subtask` runs and does not control other clients. Before reusing the session, confirm that the action has stopped and its side effects are understood. To keep safety tracking bounded, after 1,024 distinct failed sessions the runner rejects further browser tasks in that run. Sessions remain open until explicitly closed, for example with `agent-browser --session catalog close`.

## Set up your project

Open the project you want to work on, not the Xstack installation directory. Run:

```text
/setup-xstack
```

`setup-xstack` reads your existing configuration and asks for approval before changing it. It configures:

- AI models for each role and their reasoning budget.
- Where to publish specifications and tickets, using local Markdown files or a service such as GitHub.
- The project label or status for work that agents can pick up. Skills resolve the actual value through the `ready-for-agent` mapping.
- A code formatter for each language. If a formatter is missing, setup asks for approval before installing one.
- The `worker` and `subtask` binaries. Setup builds them when they are missing or stale, and checks that `subtask hot` and `worker probe` run. If `~/.cargo/bin` is not on `PATH`, setup offers to add it.
- The locations of your project glossary and architecture decision records.

By default, project configuration lives in these files:

- `docs/agents/issue-tracker.md`
- `docs/agents/triage-labels.md`
- `docs/agents/formatter.md`
- `docs/agents/domain.md`

If your project instructions point to configuration elsewhere, use those locations. Existing Matt Pocock setup files remain supported. Model settings may apply across your user account, while project configuration stays in the project.

Run `/setup-xstack` again to change settings. It preserves choices you did not ask to change. Setup does not create sample issues, implement features, or reformat the project. During code changes, `poteto-mode` must run the formatter on changed files before committing.

## Choose a task

Pick the skill that matches your goal:

- To turn a conversation into a specification for a new feature, use [`/to-spec`](skills/to-spec/SKILL.md).
- To change a feature that already exists, use [`/to-evol`](skills/to-evol/SKILL.md).
- To design code and then create tickets or implement it, use [`/to-tickets`](skills/to-tickets/SKILL.md).
- To build or change code, use [`/poteto-mode`](skills/poteto-mode/SKILL.md) with a goal and a way to verify the result.
- To find the cause of a bug without changing code, use [`/diving`](skills/diving/SKILL.md).
- If you are unsure which skill to use, ask [`/poteto-help`](skills/poteto-help/SKILL.md).

For example, to implement approved tickets:

```text
/poteto-mode implement the approved tickets
```

For an existing feature, use `/to-evol` to check the code and tests before specifying changes. It selects SMALL, STANDARD, or CRITICAL investigation based on risk. Changes to persistent data, permissions, financial calculations, and public APIs receive deeper checks.

`to-evol` preserves story and acceptance-criterion IDs. The specification covers changed behavior, behavior that must stay the same, and effects on existing users, data, and integrations. Unresolved decisions that affect correctness keep the specification in draft. Historical specifications stay unchanged.

Once the specification is ready, use `/to-tickets`. It updates the design contract: the agreed names, function behavior, rules that must remain true, and execution order. For an evolution specification, it creates tickets only for the agreed changes.

In Claude Code plugin mode, add `xstack:` before the skill name. For example, `/setup-xstack` becomes `/xstack:setup-xstack`, and `/to-evol` becomes `/xstack:to-evol`. In other apps, follow the app's command format.

For more task examples, read the [usage guide](docs/guide/README.md).

## How the worker verifies your changes

A full test suite can take minutes. If an agent waits for it after every commit, ten commits cost half an hour of waiting. If the agent skips it, a broken commit can reach a PR. `worker` splits verification into three layers, so the agent waits only for fast checks and nothing unverified leaves the machine.

```mermaid
flowchart LR
  E[Edit] --> F["worker fast"]
  F -->|FAST OK| C[Commit]
  C --> Q["worker enqueue"]
  Q -.->|background| S["worker status"]
  S --> G["worker gate"]
  G -->|GATE OK| P[Push or PR]
```

1. **`worker fast` runs before each commit.** It runs the affected tests, the formatter check, and the type check at the same time. The first failure stops the other checks, so the error appears without a wait for the slowest one. The target is under 30 seconds.
2. **`worker enqueue` runs the full suite after each commit.** A detached worker checks out that exact commit in its own worktree and runs every check at low priority. The agent starts the next task at once. At each task boundary, `worker status` reads the result. A failure stops new work until a fix lands. When more than three commits wait for a result, the agent waits for the oldest one first.
3. **`worker gate` runs before anything leaves the machine.** Push, a new PR, a ship, and a report that the work is done each need `GATE OK`. On a pending check, the gate waits for it. This wait and the three-commit limit are the only times the agent blocks on verification.

Full test suites share the cores through slots. A foreground `worker run --check` takes a slot from the top. The background worker takes slots from the bottom and always leaves the top slot free. `subtask` runs share the same cores evenly with the background worker.

The worker has no tiers. The Easy, Medium, and High tiers in `poteto-mode` estimate how long a task takes one agent. They set how many agents write code in parallel. Every tier runs all three verification layers at full strength.

## Skills

### Setup and help

- [`setup-xstack`](skills/setup-xstack/SKILL.md): configure models, budget, formatter, issue tracker, labels, and domain docs.
- [`poteto-help`](skills/poteto-help/SKILL.md): pick the right skill, playbook, or principle for a task.

### Specify and design

- [`to-spec`](skills/to-spec/SKILL.md): turn the conversation into a spec for a new feature.
- [`to-evol`](skills/to-evol/SKILL.md): write an evolution spec for a feature that already ships.
- [`to-tickets`](skills/to-tickets/SKILL.md): write the design contract, then implement it or slice it into tickets.
- [`figure-it-out`](skills/figure-it-out/SKILL.md): design an auditable playbook for large work no other playbook fits.
- [`sharpen`](skills/sharpen/SKILL.md): make a task deeper, not bigger, and rewrite its prompt.
- [`statechart`](skills/statechart/SKILL.md): draw a system's states as a Mermaid statechart from a TLA+ model.

### Build

- [`poteto-mode`](skills/poteto-mode/SKILL.md): build or change code with subagents, TDD, formatting, and verification.
- [`tdd`](skills/tdd/SKILL.md): a failing check before every behavior change, then a passing one.
- [`arena`](skills/arena/SKILL.md): run parallel candidates and graft the best parts into one.
- [`swarm`](skills/swarm/SKILL.md): fan out parallel workers and return one report.
- [`typescript-best-practices`](skills/typescript-best-practices/SKILL.md): rules for `.ts` and `.tsx` files.
- [`make-bot-ui`](skills/make-bot-ui/SKILL.md): build a page that wakes a bot through a webhook.

### Debug and review

- [`diving`](skills/diving/SKILL.md): find and prove the first point where code breaks its contract. Read-only.
- [`blast-radius`](skills/blast-radius/SKILL.md): find what a change could break outside its diff.
- [`interrogate`](skills/interrogate/SKILL.md): adversarial review by several models.
- [`no-comments`](skills/no-comments/SKILL.md): review comments and fix the accepted findings.
- [`judgment`](skills/judgment/SKILL.md): judge each finished ticket once in one background subagent before a push or PR (the feature against the ask, consistency with the existing codebase, the writing, the spacing) and nag until it is clean. Read-only.
- [`benchmark-checklist`](skills/benchmark-checklist/SKILL.md): vet a performance measurement before you report it.
- [`correct`](skills/correct/SKILL.md): make repeated agent mistakes impossible with structure, types, lints, or tests.

### Verify

- [`create-verification-skill`](skills/create-verification-skill/SKILL.md): generate a skill that drives your app like a user.
- [`maintain-verification-skill`](skills/maintain-verification-skill/SKILL.md): keep that skill in line with the app.
- [`create-formal-verification-skill`](skills/create-formal-verification-skill/SKILL.md): generate formal verification for the riskiest code.
- [`maintain-formal-verification-skill`](skills/maintain-formal-verification-skill/SKILL.md): update formal models after the code changes.

### Understand and explain

- [`how`](skills/how/SKILL.md): explain how a subsystem works and where code belongs.
- [`why`](skills/why/SKILL.md): find the cited reasons behind a decision or regression.
- [`teach`](skills/teach/SKILL.md): combine `how` and `why` into one plain explanation.
- [`bro`](skills/bro/SKILL.md): restate the last message without jargon.
- [`in-your-own-words`](skills/in-your-own-words/SKILL.md): restate the user's goal and problem.

### Track and catch up

- [`ketchup`](skills/ketchup/SKILL.md) and [`ketchup-video`](skills/ketchup-video/SKILL.md): summarize what changed and what waits on you.
- [`recall`](skills/recall/SKILL.md): rebuild your recent working context.
- [`show-me-your-work`](skills/show-me-your-work/SKILL.md): keep a decision log for long or unattended work.
- [`reflect`](skills/reflect/SKILL.md): review the transcript and turn lessons into skill edits.
- [`automate-me`](skills/automate-me/SKILL.md): capture your working style as a personal skill.

### Writing

- [`unslop`](skills/unslop/SKILL.md): cut AI tells from any text.
- [`technical-writing`](skills/technical-writing/SKILL.md): a layered standard for docs, RFCs, and PR descriptions.

### Principles

24 one-rule skills, named `principle-*`, that `poteto-mode` reads and cites. Examples: [`principle-fix-root-causes`](skills/principle-fix-root-causes/SKILL.md), [`principle-prove-it-works`](skills/principle-prove-it-works/SKILL.md), and [`principle-subtract-before-you-add`](skills/principle-subtract-before-you-add/SKILL.md). Run `npx skills add itsn1ch/xstack --list` for the full list.

## Sources and licenses

Review the source licenses before redistributing this package:

- [pstack MIT license](licenses/pstack-LICENSE), copyright 2026 Lauren Tan. This package adapts the local pstack version `0.15.32`. Its setup is merged into `setup-xstack`, and `architect` is merged into `to-tickets`.
- [Matt Pocock skills MIT license](licenses/matt-pocock-LICENSE), copyright 2026 Matt Pocock. This package includes adaptations of `to-spec` and `to-tickets`. `to-evol` derives from `to-spec` under the same license.
