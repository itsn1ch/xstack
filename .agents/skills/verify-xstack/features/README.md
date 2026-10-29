# xstack verification map

From the repository root, build and doctor as in the parent skill, with exclusive disposable scratch and evidence directories. These user-facing recipes cover the primary CLI. Record command, stdout, stderr, exit status and observed results for each entry point; verify files independently. A failed or unreachable entry point is not proved by another entry point. Clean scratch after each run, retain proof, and hash it before and after cleanup.

- [File inspection](file-inspection.md): hash, lines, and grep.
- [Command fan-out](command-fanout.md): command execution and failure handling.
- [Mixed tasks](mixed-tasks.md): JSONL task files and result records.
