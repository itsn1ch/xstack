---
name: bro
description: Restate the last message in plain human language, with no jargon.
disable-model-invocation: true
---

**Setup gate.** Unless `XSTACK READY` is already in context this session, first run `sh <this file's directory>/../setup-xstack/scripts/check-setup.sh` from the project root. On `XSTACK SETUP MISSING`, or when the script is missing, stop. Show its output, tell the user to run `/setup-xstack`, and run no other step of this skill.

Restate your last message. Stop using jargon and speak coherently. State it more simply and concisely, like one human talking to another.
