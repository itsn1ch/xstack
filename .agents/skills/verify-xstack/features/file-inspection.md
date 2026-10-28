# Inspect files

Users can inspect files concurrently without launching shell commands for each file.

## Sub-features

- `file-hash`: calculate a file's content digest.
- `file-lines`: read file lines.
- `file-grep`: search file text.

## How to get to it (user POV)

Run `subtask --hash`, `subtask --lines`, or `subtask --grep` with a newline-separated path list on stdin or in `--tasks`.

## Driving it with subtask

Preconditions: launch, doctor, and fast gate passed; scratch is empty and evidence directory exists.

- Create a user fixture: `printf 'alpha\nbeta\n' > /tmp/xstack-verification-scratch/input.txt`.
- Capture the action and status: `printf '%s\n' 'printf path | subtask --lines --out results.jsonl' > /tmp/xstack-verification-evidence/action.txt; printf '%s\n' /tmp/xstack-verification-scratch/input.txt | ./subtasks/target/release/subtask --lines --out /tmp/xstack-verification-evidence/results.jsonl >> /tmp/xstack-verification-evidence/action.txt 2>&1; echo "exit=$?" >> /tmp/xstack-verification-evidence/action.txt`. Require `SUBTASK OK`, `exit=0`, and a successful record with `code: 2`, the line count.
- Hash entry: `printf '%s\n' /tmp/xstack-verification-scratch/input.txt | ./subtasks/target/release/subtask --hash --out /tmp/xstack-verification-evidence/file-hash.jsonl`; require a digest matching an independent checksum.
- Grep entry: `printf '%s\n' /tmp/xstack-verification-scratch/input.txt | ./subtasks/target/release/subtask --grep beta --out /tmp/xstack-verification-evidence/file-grep.jsonl`; require the `beta` match.
- Include any additional proof files in the evidence list before hashing. The first lines entry alone is sufficient for the generator's initial proof, not for claiming the other entries.

## Gotchas

- `--tasks` is a list of paths in template mode, but JSONL in mixed-task mode.
- A successful summary is not a substitute for reading the result records.
