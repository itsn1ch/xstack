# Set up xstack

Configure model roles for the bundled pstack workflows and project publication settings for `to-spec`, `to-evol`, and `to-tickets` with one command.

## Install the package

Follow the local installation instructions in the [package README](../../README.md). Install xstack, not the upstream pstack package, to get the unified setup. Claude Code plugin commands use the `xstack:` namespace, so `/setup-xstack` becomes `/xstack:setup-xstack`. The [Claude Code adapter](../harness/claude-code.md) covers tool differences.

## Pick your models

Run:

```text
/setup-xstack
```

[`/setup-xstack`](../../skills/setup-xstack/SKILL.md) detects available models, reads existing choices, and configures reasoning budget and model roles. It also configures the project's issue tracker, publication label mapping, and domain-document pointers. Confirm the combined preview before it writes. It retains existing pstack and Matt Pocock configuration instead of starting a second setup.

The defaults run at `xhigh` reasoning, the same as the `large` budget. `unlimited` lifts each model to its highest tier, up to `max`. Opus goes to `max`. Grok tops out at `xhigh`, so it stays there. `medium` and `small` lower the reasoning and spend fewer tokens.

You only override what you care about. A role with no line in the model rule keeps the skill's default. A re-run retains existing choices unless you request a change. Models and tracker settings remain separate in scope: the model rule may be user-wide, while tracker configuration belongs to the current project.

You might be wondering what happens if you use Auto. Set a role to `inherit-parent` or `auto` and pstack omits the subagent `model` field, so the subagent inherits your parent chat model. Both values mean the same thing, and neither is a model slug. For a panel role the value is a list, and one subagent runs per entry, so the list length sets the panel size. Setup also configures `swarm workers`, the default model for every `/swarm` worker unless a race names a model for each arm.

## Configure project publication

The same setup flow records the selected tracker and destination in `docs/agents/issue-tracker.md`, the `ready-for-agent` mapping in `docs/agents/triage-labels.md`, and glossary/ADR pointers in `docs/agents/domain.md`. Existing project instruction pointers can keep equivalent files elsewhere. Setup updates one existing root instruction file without duplicating configuration.

For local Markdown, specs go to `.scratch/<feature-slug>/spec.md` and individual tickets to `.scratch/<feature-slug>/issues/`. Remote trackers require verified read access and explicit publication operations. Setup does not publish sample issues. If access fails, it reports the blocker instead of silently switching to local files.

## Accept the verification offers, or don't

At the end of setup, `/setup-xstack` looks for a way to prove app behavior in your project, either a `verify-*` skill or an existing harness. If it finds neither, it offers once to generate one with [`/create-verification-skill`](../../skills/create-verification-skill/SKILL.md).

Say yes and it writes `.cursor/skills/verify-<app>/`, a project-local skill that teaches agents to drive your app the way a user does. It proves the skill works once before handing it over. Say no and setup moves on. You can run `/create-verification-skill` yourself any time. [Verify and ship](./06-verify-and-ship.md#create-a-project-verification-skill) covers it in depth.

If you're new to pstack, say yes. An agent that can check its own work keeps going until the check passes. An agent that can't hands every result back to you to check by hand. Of everything in this guide, the verification skill pays off the most.

Then setup offers once to generate a formal verification skill with [`/create-formal-verification-skill`](../../skills/create-formal-verification-skill/SKILL.md), unless the project already has one. Tests check the cases someone thought to write. A formal verification skill checks one key property of your code, such as "a job never runs twice at once", in every case within a stated scope. Say yes when your code has concurrency, retries, a state machine, or a parser, where a bug is costly and tests rarely reach the case that breaks. Otherwise say no, and run it later if that changes. [Verify and ship](./06-verify-and-ship.md#prove-a-property-with-a-formal-verification-skill) covers it.

After setup, start a new chat. The model rule applies to new sessions.

## Keep the cost in check

pstack spends extra tokens on subagents and review panels. That's the price of the rigor. To spend fewer:

- Rerun `/setup-xstack` and request a smaller reasoning budget or cheaper models; tracker settings remain unchanged.
- Set a role to `auto` or `inherit-parent` so it runs on the chat's own model.
- Shorten a panel list. Each entry runs one subagent.
- Save `/poteto-mode` for work that needs rigor. A small, obvious edit doesn't.

## Run your first task

Pick something real but small, and describe it the way you'd describe it to a colleague:

```text
/poteto-mode add a --json flag to this command. text output stays byte-identical. verify both.
```

Watch the todo list. Its first items are the matched playbook's steps copied in, the Feature playbook for this prompt. If `/poteto-mode` skips a step, the step stays in the list with `skip: <reason>`, so you can see what it chose not to do.

From here you can type normal follow-ups. To keep `/poteto-mode` on for the whole chat, pick it from the `/` menu with Option+Enter (Mac) or Alt+Enter (Windows) instead of Enter. That makes it a [Custom Mode](https://cursor.com/docs/skills#using-a-skill-as-a-custom-mode), which stays in context on every turn until you exit it. Custom Modes are available in the Agents Window and the CLI. Plain Enter attaches the skill to one message, and it fades as the chat moves on.

Next: [Route work through `/poteto-mode`](./02-poteto-mode.md).
