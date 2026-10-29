# Fan out commands

Users run independent shell commands for a list of inputs and inspect their individual outcomes.

## Sub-features

- `command-each`: substitute each item into a command.
- `command-failure`: report failing commands while continuing other work.

## How to get to it (user POV)

Run `subtask --sh` with input items on stdin and optional `--keep-going`.

## Driving it with subtask

Preconditions: launch, doctor, and fast gate passed; scratch and evidence directories belong to this run.

- Run `printf 'one\ntwo\n' | ./subtasks/target/release/subtask --sh 'printf "item=%s\\n" {}' --out /tmp/xstack-verification-evidence/commands.jsonl`; require two successful records containing `item=one` and `item=two`.
- Run `printf '0\n1\n' | ./subtasks/target/release/subtask --keep-going --sh 'exit {}' --out /tmp/xstack-verification-evidence/failure.jsonl`; require a nonzero summary and both success and failure records. Capture the command, stdout, stderr and exit status and add all artifacts to the evidence manifest.

## Gotchas

- A failing task makes the overall command fail; that is expected for the failure journey.
- Inputs substituted into shell templates are shell data: do not use untrusted input without accounting for shell interpretation.
