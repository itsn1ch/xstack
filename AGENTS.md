# Project instructions

## Xstack

- Before publishing specs or tickets, read `docs/agents/issue-tracker.md`.
- When applying a publication label or status, read `docs/agents/triage-labels.md`.
- Before drafting specs or tickets, read `docs/agents/domain.md` and the relevant existing documents it identifies.
- Before every commit, run the formatter from `docs/agents/formatter.md` on every changed file it supports; report declined languages instead of guessing.
- Before every commit and every PR, run `/judgment` after the formatter; open a PR only on `JUDGMENT PASS`, per poteto-mode.
- At session start, unless a `SUBTASK HOT` line is already in context, run `subtask hot`, discovering the executable at `subtasks/target/release/subtask` in this checkout, then `subtask` on `PATH`, then `~/.cargo/bin/subtask`, in that order. Fan mechanical work out through `subtask` by default and run tests through `worker`, per **Subtasks** and **Tests and verification** in poteto-mode.
