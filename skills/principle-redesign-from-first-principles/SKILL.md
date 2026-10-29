---
name: principle-redesign-from-first-principles
description: "Apply when integrating a new requirement into an existing design."
disable-model-invocation: true
---

# Redesign From First Principles

**Setup gate.** Unless `XSTACK READY` is already in context this session, first run `sh <this file's directory>/../setup-xstack/scripts/check-setup.sh` from the project root. On `XSTACK SETUP MISSING`, or when the script is missing, stop. Show its output, tell the user to run `/setup-xstack`, and run no other step of this skill.

When integrating a change, don't bolt it onto the existing design. Redesign as if the requirement had been a foundational assumption from the start.

- Read all affected files and understand the current design
- Ask: "if we were writing this from scratch with this new requirement, what would we build?"
- Propagate the change through every reference: types, docs, examples, rationale sections
- Think about the whole redesign, then deliver it incrementally

This is the method for preserving option value when integrating changes into an existing design.
