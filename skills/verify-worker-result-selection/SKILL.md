---
name: verify-worker-result-selection
description: Check worker verification-result reuse, configuration validity, and supersession when changing worker state or verification gates.
---

# Verify worker result selection

Use this when changing `worker/src/state.rs` result validity, `own_result` or `result_for`, or their callers that rely on verification results, especially `worker/src/queue.rs` (`enqueue`, `wait_for`, `unverified_depth`, `gate`).

**Property:** A returned result is final, belongs to the current check/copy configuration, and comes from the requested commit, an identical tree, or (unless an own result is required) a skipped commit's supersession chain; if an eligible result exists, one is returned. An own result takes precedence over supersession.

**Harm:** Accepting a stale or unrelated pass can let a gate report verification when the relevant checks did not pass.

## Run

From the repository root:

```sh
CARGO_TARGET_DIR=/tmp/xstack-formal-result-selection cargo run --locked --manifest-path skills/verify-worker-result-selection/Cargo.toml
```

The harness compiles and calls the real Rust `State::result_for`, rather than a translation of it. It is exhaustive finite-state generated-input checking, not a proof for all states or a verifier of Rust execution in general. Rust/Cargo 1.99.1 and the checked-in `Cargo.lock` were used to confirm it; Cargo may need network access to fetch the locked crates on a new machine.

It enumerates 13,122 states: zero to two entries, each with five statuses, two trees, current or old configuration key, and no link or a link to either entry or a missing entry; the current configuration is present or absent. It runs 104,976 queries, including each represented commit and an absent commit, both values of `own`, cycles and dangling links. For represented commits the query tree matches the entry's tree. It checks both that a returned source is eligible and that an eligible source is not lost. It deliberately permits any eligible result when more than one exists; it does not check which same-tree result is selected. It does not cover more than two entries, arbitrary check contents, SHA/tree collisions or inconsistent commit/tree input, queue creation, actual Git ancestry, scheduler interleavings, check execution, process crashes, or the gate's baseline logic. In particular, it does not prove supersession links really point to descendants.

On failure, read the counterexample's state and query: map `a`/`b` to commit IDs, `t0`/`t1` to trees and the key to the current or old checks/copy configuration. Check the candidate source and the skipped chain against the property. A bad source or missing eligible result is a code finding; a mismatch between those abstract values and the changed code's intended contract may instead mean the harness assumptions need revision. Do not silence a genuine finding by weakening the property.

## Keep the check in sync

Covered code: `worker/src/state.rs` `config_key`, `Status::is_final`, `State::valid`, `State::own_result`, and `State::result_for`. The queue callers above are impact pointers, not verified by this harness. Last confirmed against repository commit `9076a3e8f22b023e48dc7abcf7e1b52a60b70316`. When any covered code changes after that commit, compare the verifier's domains and independent allowed-source rule with the new code and intended contract; update the harness if appropriate, run it, and move the sync commit forward only after confirming the verification describes the code at that new commit. Use `/maintain-formal-verification-skill` for out-of-sync verification.

To check sensitivity, temporarily remove `State::valid`'s configuration-key requirement in `worker/src/state.rs`, run the same command and confirm it fails with an old or absent configuration in its counterexample; restore only that scratch change and rerun to confirm the baseline. Never keep this mutation in production code.
