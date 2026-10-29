---
name: subtask-execution
description: Use the shared subtask executor for explicit mechanical file operations when the host permits command execution.
---

# Shared subtask execution

**Setup gate.** Unless `XSTACK READY` is already in context this session, first run `sh <this file's directory>/../setup-xstack/scripts/check-setup.sh` from the project root. On `XSTACK SETUP MISSING`, or when the script is missing, stop. Show its output, tell the user to run `/setup-xstack`, and run no other step of this skill.

For independent mechanical work, run `subtask skill --help` through a host-authorized command tool. Run it from the workspace root and submit JSONL operations on standard input. The executable validates paths and operation limits before executing them. Read its results as evidence; the agent interprets them.

The default mode is read-only. A replacement needs both the `--allow-write` flag and `authorized: true` on that operation. These are subtask operation controls, not host permission decisions. Follow the calling skill's restrictions, and never request write authorization for a read-only workflow.

If the host denies the command, do not execute it. If `subtask` or its required sandbox is unavailable, or an operation is unsupported, keep the calling skill's existing execution path and obtain any required host approval there. Do not turn skill instructions into commands automatically.
