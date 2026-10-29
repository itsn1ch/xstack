# Run mixed tasks

Users submit a JSONL list of different task types and inspect one result per task.

## Sub-features

- `mixed-argv`: run a command without shell interpolation.
- `mixed-lines`: read a fixture alongside the command.
- `mixed-results`: persist per-task outcomes.

## How to get to it (user POV)

Run `subtask --keep-going --tasks <jsonl> --out <jsonl>`.

## Driving it with subtask

Preconditions: launch, doctor, and fast gate passed; isolated scratch and evidence directories exist.

- Create `/tmp/xstack-verification-scratch/input.txt` containing `alpha` and a task file with `{"argv":["printf","hello"]}` and `{"lines":"/tmp/xstack-verification-scratch/input.txt"}`, each on its own line.
- Run `./subtasks/target/release/subtask --keep-going --tasks /tmp/xstack-verification-scratch/tasks.jsonl --out /tmp/xstack-verification-evidence/mixed.jsonl`; require `SUBTASK OK` and inspect both records for `hello` and the line count `code: 1`.
- Capture command, stdout, stderr and exit status; include the task file and results in the proof manifest before hashing.

## Gotchas

- A task file is JSONL, not a JSON array. Preserve a copy of it in the evidence directory before scratch cleanup.
- HTTP tasks can contact real services; use only authorized endpoints.
