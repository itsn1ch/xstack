---
name: setup-xstack
description: Configure xstack model roles and reasoning budget, the project code formatter (detected, or installed when missing), project issue tracker, publication labels, and domain-document pointers. Use for /setup-xstack, first-time xstack setup, or changing xstack models or tracker settings.
---

# Setup xstack

One setup flow for the bundled pstack workflows, `to-spec`, `to-evol`, and `to-tickets`. This is a prompt-driven skill: inspect the current environment, confirm actual choices, write configuration, and verify it. Run in the user's target project, not the installed plugin directory. Do not run implementation, publish issues, or generate tickets during setup.

## 1. Inspect existing state

Read [model configuration](references/models.md), [project configuration](references/project.md), and [code formatter](references/formatter.md).

- Follow model-reference steps 1 and 2 to discover available models and existing role settings. Keep the `pstack-models` locations used by the bundled consumers; a second xstack-only model file would be ignored by them.
- Read the target project's `AGENTS.md` / `CLAUDE.md`, their configuration pointers, and any existing `docs/agents/issue-tracker.md`, `triage-labels.md`, and `domain.md`. Reuse an existing Matt Pocock setup without requiring its setup skill.
- Follow formatter-reference steps 1 and 2 to list the project's languages and detect each one's formatter, and read any existing `docs/agents/formatter.md`.
- Inspect Git remotes, available tracker tooling, `.scratch/`, existing glossary and ADR paths, and monorepo boundaries. Inspect authentication read-only when using a remote tracker. Discovery alone is not permission to create labels or issues.

Complete when the current model choices, formatter per language, tracker destination, publication label mapping, domain pointers, and missing decisions are known. An existing configuration is the starting point, not permission to replace it with defaults.

## 2. Resolve choices and preview changes

Use the harness's structured-question tool for material choices. Reuse choices already supplied by the user. On a re-run, retain configured budget and model roles unless a change was requested; on first setup, follow model-reference step 3. Validate model availability with step 4 before any write. A role may use `inherit-parent` or `auto`, including when no explicit model selector is available.

For project choices, follow [project configuration](references/project.md): confirm the tracker and destination when not already configured. Offer GitHub, GitLab, local Markdown, or an available tracker integration such as Linear. For an unsupported tracker, collect its actual read/create/link operations and tooling rather than guessing them. Use local files only when selected, not as a silent fallback for failed authentication.

For the formatter, follow formatter-reference step 3. Retain an existing `docs/agents/formatter.md` unless the project changed formatters or the user asks.

Preview the exact changed model roles, model-settings destination, formatter per language with each proposed install, config file, and `format` script, tracker destination and operations, `ready-for-agent` mapping, domain layout, configuration files, and instruction block. Explain that model settings may be user-wide while tracker settings are project-local. Ask for approval of changes before writing, including any remote label creation. Batch related decisions instead of running separate setup interviews.

## 3. Persist the configuration

After approval:

1. Follow model-reference step 5 to write approved model settings, or retain the existing settings unchanged when no model change was requested. Under a harness-managed policy, use its supported configuration mechanism and report the actual location. If persistence is unavailable, report that part as blocked rather than claiming completion.
2. Run the approved formatter installs, config files, and scripts, then write `docs/agents/formatter.md` per formatter-reference step 4. Never reformat existing files here.
3. Follow project-reference **Write the configuration** to persist tracker operations, label mapping, domain pointers, the formatter pointer, and one instruction block in the target project.
4. If a remote label is required and its creation was approved, create only the missing configured label. Preserve existing labels and permissions. Setup never creates a sample issue.

Keep unrelated project instructions and user edits intact. Re-running with unchanged choices must not duplicate instruction blocks or label definitions.

## 4. Verify and report

Read the saved configuration back and resolve each pointer from the project root. Verify that:

- Every active model role is retained or intentionally changed and all explicit selectors are supported; panel entries preserve their intended count.
- Tracker type, destination, read/create operations, and blocking-link behavior are explicit. For a remote tracker, exercise read-only access to the chosen destination and verify the publication label exists. Missing credentials or permissions are a blocker for that tracker, not a successful setup.
- Every project language has a formatter entry, and each formatter's check command runs (formatter-reference step 5). A language recorded "none, declined" is reported, not hidden.
- The `ready-for-agent` role resolves to the saved label or local status; no skill has to infer it.
- Domain paths describe actual existing documents or clearly state that none exist yet. Every configuration pointer resolves, and exactly one owning instruction block (`Xstack` or the reused `Agent skills` block) contains those pointers.
- `to-spec`, `to-evol`, and `to-tickets` can obtain their configuration from the project files without another setup command.

Report saved and retained file locations, verification results, and any blocked part separately. Setup is complete only when model settings, the formatter configuration, and project publication configuration are usable. Mention new-session requirements for automatically loaded model rules. If the user requested only a model or tracker change, still check the retained side before declaring the unified setup complete.

After the required setup is verified, follow model-reference steps 7 and 8 for optional app-verification and formal-verification offers. Declining either does not block setup. Generating either requires the user's acceptance.
