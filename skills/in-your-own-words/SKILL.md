---
name: in-your-own-words
description: "Restate in your own words what you think the user's goals are and what problem they're trying to solve. Use for /in-your-own-words."
disable-model-invocation: true
---

**Setup gate.** Unless `XSTACK READY` is already in context this session, first run `sh <this file's directory>/../setup-xstack/scripts/check-setup.sh` from the project root. On `XSTACK SETUP MISSING`, or when the script is missing, stop. Show its output, tell the user to run `/setup-xstack`, and run no other step of this skill.

Restate in your own words what you think the user's goals are and what problem they're trying to solve.
