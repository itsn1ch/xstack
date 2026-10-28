---
name: verify-xstack
description: Verify xstack's subtask command-line user workflows with real CLI invocations, artifacts, and shared worker slots.
---

# Verify xstack

The primary surface here is the `subtask` CLI; `worker` is the companion CLI, and xstack also ships agent skills and hooks. Work from the repository root. Read `features/README.md` and the relevant feature recipe first. Do not drive another user's state. The commands below use fixed `/tmp/xstack-verification-{scratch,evidence}` paths: refuse concurrent runs using those paths, or replace every occurrence and the evidence manifest consistently with per-run paths. Evidence is retained in `/tmp/xstack-verification-evidence`.

## Prerequisites

Rust and Cargo, Git, jq, a Bash-compatible shell, and the release binaries are required. Run `./subtasks/target/release/subtask hot` and require `SUBTASK HOT`; on COLD run the printed Cargo builds in the background and wait for HOT before proceeding. You may install both tools on PATH using `cargo install --locked --path subtasks` and `cargo install --locked --path worker`; otherwise use the checkout paths shown here (or the absolute paths printed by `subtask hot`).

## Launch

No server or seed database is required. Build both CLI binaries through a slot: `./worker/target/release/worker run --check 'cargo build --release --manifest-path subtasks/Cargo.toml && cargo build --release --manifest-path worker/Cargo.toml'`. Require success, then re-run `subtask hot`. Each drive is a separate short-lived CLI invocation; no long-lived process or port is used. Create empty scratch and evidence directories with `mkdir -p /tmp/xstack-verification-scratch /tmp/xstack-verification-evidence` only after ensuring they belong to this run.

## Doctor

Run `./subtasks/target/release/subtask --keep-going --tasks .agents/skills/verify-xstack/doctor.jsonl --out /tmp/xstack-verification-evidence/doctor.jsonl`. Require `SUBTASK OK`; inspect the results file for failed checks. The checks exercise the binaries' freshness and worker resource probe. This CLI needs no authentication or listening port.

## Drive

Before a change's first drive, run `./worker/target/release/worker fast --check 'cargo test --manifest-path subtasks/Cargo.toml' --check 'cargo check --manifest-path subtasks/Cargo.toml'`; require success. Adapt affected tests and type checks when verifying worker changes. Read the feature recipe and run actual CLI commands with isolated input and explicit output paths. Batch independent operations in a single `subtask` call; decisions depending on previous output remain serial. For interactive commands use a separate PTY; these mapped commands are noninteractive.

## Evidence

Capture the command/action, stdout, stderr and exit status as well as the resulting state; confirm written files, not just the command's message. Use production user paths, not internal setters or test-only endpoints. Use mocks only at an existing production boundary. For safe/dry-run paths observe files, network and git refs to determine what was actually skipped. Inspect side effects with a single `subtask --keep-going --tasks` call containing `hash`, `lines`, `grep`, or HTTP tasks as appropriate. Store proof in the evidence directory, then run `./subtasks/target/release/subtask --hash --tasks .agents/skills/verify-xstack/evidence-files.txt --out /tmp/xstack-verification-evidence/evidence.jsonl`. The manifest must list every proof artifact; do not include the hash output itself. Preserve the manifest with the evidence if modifying it for another journey.

## Cleanup

Remove only the scratch directory created by this run: `rm -rf /tmp/xstack-verification-scratch`. No app process is left running. Never kill by process name. Retain evidence. Repeat the same hash call with `--out /tmp/xstack-verification-evidence/evidence-after.jsonl`, then compare stable records, ignoring scheduling order and duration: `diff <(jq -S -s 'sort_by(.i) | map({i,kind,task,ok,code,output})' /tmp/xstack-verification-evidence/evidence.jsonl) <(jq -S -s 'sort_by(.i) | map({i,kind,task,ok,code,output})' /tmp/xstack-verification-evidence/evidence-after.jsonl)`. Require both hash summaries to be `SUBTASK OK`; a mismatch or missing artifact fails proof. On failed attempts perform this cleanup too; keep failure artifacts.

## Helpers

`doctor.jsonl` is the task list for the doctor command above; `evidence-files.txt` is the line-oriented input for the hash commands above. Both are data, not executable scripts. The feature files supply the actual user journeys.

## Background

These recipes require the agent to drive and inspect results; no autonomous journey is shipped or added to shared worker configuration. If a future journey is made autonomous, read `config` in the Git common directory's `xstack-worker/state.json` before any `worker init` (which replaces it), preserve every existing check, copy, and option, and add the journey; if no configuration exists, initialize with the full suite and journey. Do not silently replace existing configuration.

Use `/maintain-verification-skill` to keep this map aligned with the app.
