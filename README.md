# Xstack

Use Xstack to write specifications, design changes, create tickets, and build software with AI agents. The package contains 61 skills from pstack and adaptations of `to-spec`, `to-evol`, and `to-tickets`.

## Install the skills

Xstack installs with the [`skills`](https://github.com/vercel-labs/skills) CLI. It needs Node.js for `npx`. Run each command from the parent directory of `xstack`.

1. List the skills without installing them:

   ```bash
   npx skills add ./xstack --list
   ```

2. Install every skill into the current project. The CLI asks which agents to install for:

   ```bash
   npx skills add ./xstack --skill '*'
   ```

3. Optional: target an agent, install for your user account, or skip the prompts:

   ```bash
   # Only Claude Code
   npx skills add ./xstack --skill '*' -a claude-code

   # User directory instead of the project, no prompts
   npx skills add ./xstack --skill '*' -a claude-code -g -y

   # A few skills only
   npx skills add ./xstack --skill poteto-mode --skill diving --skill tdd
   ```

| Flag | Effect |
|---|---|
| `-l`, `--list` | List the skills, then exit without installing. |
| `-s`, `--skill <names…>` | Install these skills. `'*'` installs all. |
| `-a`, `--agent <agents…>` | Install for these agents, such as `claude-code` or `cursor`. `'*'` targets all. |
| `-g`, `--global` | Install in your user directory instead of the project. |
| `-y`, `--yes` | Skip confirmation prompts. |
| `--copy` | Copy files instead of symlinking them. |
| `--all` | Same as `--skill '*' --agent '*' -y`. |

To remove a skill, run `npx skills remove <name>`. With Bun, replace `npx` with `bun x`.

Keep the package directory intact. Some skills use scripts and documents outside their own directory, so copying only `SKILL.md` is not enough.

The `skills` installer installs skills. It does not register subagents or plugin automations. For plugin installations, use the manifest for your agent app:

- Claude Code: `.claude-plugin/plugin.json`.
- Cursor: `.cursor-plugin/plugin.json`.
- General format: `plugin.json`.

Plugin support depends on your agent app. Cursor-specific features may not work elsewhere. For Claude Code, read the [command and tool adapter](docs/harness/claude-code.md).

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

24 one-rule skills, named `principle-*`, that `poteto-mode` reads and cites. Examples: [`principle-fix-root-causes`](skills/principle-fix-root-causes/SKILL.md), [`principle-prove-it-works`](skills/principle-prove-it-works/SKILL.md), and [`principle-subtract-before-you-add`](skills/principle-subtract-before-you-add/SKILL.md). Run `npx skills add ./xstack --list` for the full list.

## Sources and licenses

Review the source licenses before redistributing this package:

- [pstack MIT license](licenses/pstack-LICENSE), copyright 2026 Lauren Tan. This package adapts the local pstack version `0.15.32`. Its setup is merged into `setup-xstack`, and `architect` is merged into `to-tickets`.
- [Matt Pocock skills MIT license](licenses/matt-pocock-LICENSE), copyright 2026 Matt Pocock. This package includes adaptations of `to-spec` and `to-tickets`. `to-evol` derives from `to-spec` under the same license.
