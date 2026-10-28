# Project configuration

`to-spec`, `to-evol`, and `to-tickets` read these paths relative to the target project root, independently of where xstack is installed:

- `docs/agents/issue-tracker.md`: tracker type, destination, and publication operations.
- `docs/agents/triage-labels.md`: the `ready-for-agent` role mapped to the project's actual label or status.
- `docs/agents/domain.md`: where to read the project's glossary and architecture decisions.

**poteto-mode** reads `docs/agents/formatter.md` (written per [code formatter](formatter.md)) for the formatter it must run before every commit.

Reuse existing configuration at these paths, including configuration created by Matt Pocock's setup. If project instructions already point to equivalent files elsewhere, use those as the authoritative files and preserve the pointer rather than maintaining a second copy. Skills follow those pointers before the conventional paths.

## Select the tracker

A configured tracker wins over remote inference. Otherwise propose GitHub for a GitHub remote, GitLab for a GitLab remote, or local Markdown for a project without a remote. Confirm an unconfigured destination before persisting it, especially when multiple remotes or projects exist. A re-run retains the selected tracker unless the user requests a switch.

### Local Markdown

Record these operations in the tracker configuration:

- Type: local Markdown. Root: `.scratch/` in this project.
- An epic is one folder, `.scratch/<epic-slug>/`, holding only `spec.md` and `issues/`.
- `spec.md` holds the spec, its `# Design` section (written by to-tickets), and each evolution as an appended `# Evolution <NN>: <title>` section.
- `issues/00-overview.md` is the epic's map (file tree, architecture, order, plain-words summary). It is never `ready-for-agent`.
- Each ticket is `.scratch/<epic-slug>/issues/<NN>-<slug>.md`, numbered from `01` in dependency order. Use one file per ticket.
- Read an issue or spec from its path; append comments under `## Comments` when needed.
- Publication means writing the corresponding file. Preserve existing files; choose an unused slug or obtain approval before replacing an existing spec.
- Dependencies are `Blocked by` ticket numbers/titles; no remote issue links exist.
- The configured `ready-for-agent` status goes in the spec's `Status:` line, each evolution section's `Status:` line, and the ticket's `Status` field from its template.

Setup writes configuration only. It does not create a sample spec, ticket, or `.scratch/` hierarchy.

### GitHub

Record the exact host and `OWNER/REPO`, plus the available integration or `gh` CLI. For `gh`, verify read-only access with `gh repo view OWNER/REPO` using the selected host's authentication, and inspect its labels. Store explicit destination arguments, not a dependency on the current shell's default repository.

Record how to read issue bodies and comments (`gh issue view`), create issues (`gh issue create --repo OWNER/REPO --title ... --body-file ... --label ...`), and refer to issue identifiers. For an existing source issue, record the integration's supported sub-issue operation. Use native blocking links when the integration supports them; otherwise record `Blocked by` references in the issue body. Verify support before recording a native operation. A sub-issue or blocking operation that is required but unavailable is a blocker, not permission to silently omit the relationship.

Inspect existing labels and map `ready-for-agent` to an existing appropriate label where configured. If a new label is needed, include `gh label create` with explicit repository/host in the approval preview. Perform only the approved label creation. Setup does not create, close, or edit issues or PRs.

### GitLab

Record the exact host and project path or ID, plus the available integration or `glab` CLI. Verify read-only access and authentication against that project. Inspect `glab ... --help` for the installed version before recording read/create, labels, parent-child, or blocking-link commands. Store explicit project arguments. Preserve configured labels; create a missing publication label only after approval. If native blocking links are unavailable, record body references. If a required parent-child operation is unavailable, report it as blocked. Setup does not create an issue to test permissions.

### Another tracker integration

Record the verified integration/tool names, exact project/team destination, read issue and comments operation, create operation, label/status mapping, and parent-child/dependency operations. Ask only for details that repo configuration and available tooling cannot supply. Verify read-only project access. Missing tooling or authentication must be reported before declaring setup complete; no invented commands or silent local fallback.

## Labels used by this package

Only `ready-for-agent` is required by the two selected Matt Pocock skills. Reuse an existing label vocabulary and preserve all other mappings. Otherwise write a role-to-value table with `ready-for-agent` as the initial value and allow the user to choose a different existing label/status. Include the tracker label's exact spelling and case. For local Markdown this is a status string, not a remote label.

When a skill names the `ready-for-agent` role, it uses the mapped value, including in local ticket templates. These two skills do not require the full triage workflow or installation of a triage skill.

## Domain documents

Inspect actual glossary, ADR, and monorepo paths. Record the existing layout and instruct consumers to read the relevant glossary and ADRs before drafting specs or tickets. Use a single-context convention (`GLOSSARY.md` and `docs/adr/`) when no domain documents exist, explicitly noting that they do not exist yet. Do not create empty glossary or ADR placeholders. Ask about multiple bounded contexts only when the project actually has them and the existing pointers do not resolve the choice.

## Write the configuration

Write the approved tracker configuration and required label mapping at the authoritative paths above, plus domain consumer rules. Preserve useful existing operations, custom sections, and other label roles. Configuration stores identifiers and workflow, never credential values.

Update one existing root instruction file: prefer `CLAUDE.md` when present, otherwise `AGENTS.md`; if neither exists, create `AGENTS.md`. If both exist, update only the one that owns the existing configuration pointers, or `CLAUDE.md` when neither does. Do not copy configuration into both.

Maintain exactly one `## Xstack` block containing six short context pointers:

- Before publishing specs or tickets, read the authoritative issue-tracker configuration.
- When applying a publication label or status, read the authoritative triage-label mapping.
- Before drafting specs or tickets, read the domain-document configuration and relevant existing documents.
- Before every commit, run the formatter from the formatter configuration on every changed file.
- Before every commit and every PR, run `/judgment` after the formatter and open the PR only on `JUDGMENT PASS`, per poteto-mode.
- At session start, unless a `SUBTASK HOT` line is already in context, run `subtask hot` from `<xstack root>/subtasks/target/release`, `PATH`, or `~/.cargo/bin`, in that order. Fan mechanical work out through `subtask` by default, and run tests through `worker`, per **Subtasks** and **Tests and verification** in poteto-mode.

Use the actual paths in the pointers. Update an existing Xstack block in place. If an existing `## Agent skills` block already owns these pointers, update that block instead of duplicating it. Preserve unrelated instructions. Re-read both configuration files and instruction pointers after writing.
