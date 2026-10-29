# Domain documents

No project glossary or architecture decision records exist yet. The single-context conventions are `GLOSSARY.md` and `docs/adr/`; these are future locations, not existing pointers. Do not create empty placeholders. Before drafting specs or tickets, read relevant documents if these paths are later populated.

## Existing layout

- `README.md`: package overview and workflow entry points.
- `docs/guide/`: workflow guides.
- `docs/harness/claude-code.md`: Claude Code adapter.
- `skills/`: bundled prompt-driven workflows.
- `worker/Cargo.toml`: standalone Rust verification-worker package.
- `subtasks/Cargo.toml`: standalone Rust mechanical-subtask package.
- `skills/poteto-mode/scripts/Cargo.toml`: standalone Rust workflow utilities.
- `skills/to-tickets/scripts/check-tickets/Cargo.toml`: standalone Rust ticket checker.
- `skills/verify-worker-result-selection/Cargo.toml`: bounded verification harness for worker result selection.
- `skills/statechart/scripts/`: JavaScript statechart tools.
- `automations/benny/`: automation skills and configuration templates.

There is no root Cargo workspace and no configured bounded-context glossary split. Read the relevant package or skill before drafting its changes; do not infer a shared build boundary.
