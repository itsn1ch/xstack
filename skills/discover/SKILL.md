---
name: discover
description: "Use when the user wants a topic of the codebase discovered and recorded, asks what a module is for, why it has its shape, or what must stay true before changing it, or wants intent, reason, and invariants saved as Markdown key-value entries under docs/discover/."
disable-model-invocation: true
---

# Discover

**Setup gate.** Unless `XSTACK READY` is already in context this session, first run `sh <this file's directory>/../setup-xstack/scripts/check-setup.sh` from the project root. On `XSTACK SETUP MISSING`, or when the script is missing, stop. Show its output, tell the user to run `/setup-xstack`, and run no other step of this skill.

Read one topic of the codebase and save what each unit in it is for, why it has its shape, and what must stay true, as checked key-value entries in `docs/discover/<topic>.md`. The file is a record for the next person who changes the code. `/how` explains how code runs and `/why` investigates history in depth. Use them when the user asks for an explanation instead of a record.

## The file

The file is `docs/discover/<topic>.md` in the project root, where `<topic>` is the topic in kebab-case. It has a header and then one entry per unit, in this exact shape:

```markdown
# Discover: worker slot locks

- topic: worker slot locks
- scope: worker/src/run.rs, worker/src/queue.rs
- commit: 1aa5d1a
- date: 2026-11-15

## slot-lock

- intent: Limit how many test commands run at once across every worker process on the machine.
- reason: Worker processes share no memory, so a lock file is their only shared counter. (source: inferred)
- invariant: A background check waits while another process holds its slot. (guard: a_busy_slot_held_by_another_process_makes_the_background_check_wait worker/tests/worker.rs:614)
- invariant: Foreground and background runs of one repository take slots from the same lock files. (guard: unguarded)
- evidence: worker/src/run.rs:81, worker/src/queue.rs:204, worker/src/queue.rs:214
```

The header lines are `topic`, `scope` (the paths read), `commit` (`git rev-parse --short HEAD`), and `date`. Each entry has these keys and no others:

| Key         | Count     | What it holds                                                                                                                                                                                                                                                                            |
| ----------- | --------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `## <key>`  | 1         | The unit's handle in kebab-case, unique in the file: a type, function, file, or behavior.                                                                                                                                                                                                |
| `intent`    | 1         | The goal the unit serves for its callers, in one sentence. It names a purpose, not a mechanism. "Limit how many test commands run at once" is an intent. "Polls `try_lock` with backoff" is a mechanism.                                                                                 |
| `reason`    | 1         | Why the unit has this shape instead of the obvious alternative. It ends with `(source: <ref> "<quote>")`, where `<ref>` is a `path:line`, a commit SHA, or `PR #<n>`, and `<quote>` is words copied from that line or message that say why. Otherwise it ends with `(source: inferred)`. |
| `invariant` | 1 or more | One property that must stay true for callers, one per line, ending with `(guard: <name> <path:line>)` or `(guard: unguarded)`. The name is the test, assert, or type that fails when the property breaks, and the `path:line` is the assertion or type that checks it.                   |
| `evidence`  | 1         | Comma-separated `path:line` references, relative to the project root, where the unit is defined and where its invariants are kept.                                                                                                                                                       |

## Steps

1. **Scope.** Take the topic from the user. Without one, ask for it once. List the files that implement it and their tests, and put the source files in `scope`.
2. **Read.** Read the scoped code and its tests. Split the topic into units a caller relies on. A unit earns an entry when changing it could break a caller. Constants, timings, and names are details of a unit, not units.
3. **Find each reason's source.** Check, in order, comments and docs near the code, `git log -L <start>,<end>:<path>` or `git log --follow -- <path>` for the commit that introduced it, and the PR named in that commit (`gh pr view <n>` when available). Cite the first source whose words say why, and copy those words into the quote. A comment that says what the code does or what it is for states the intent, not the reason, so it is not a source. When no source says why, write the reason you infer and tag it `(source: inferred)`. Run `/why` when the user wants a reason confirmed beyond that.
4. **Test each invariant.** An invariant is a property whose violation breaks a caller. Ask what breaks if it stops being true. If nothing a caller sees breaks, it is a detail, so drop it. Write one property per line. Search the tests for one that fails when the property breaks. Read its body, and cite it only when one of its assertions checks the property: the guard is the test name and the `path:line` of that assertion. A test whose name sounds right but whose assertions check something else is not a guard. Without one, write `(guard: unguarded)`.
5. **Write the file.** Create `docs/discover/` when it is missing. When the file already exists, keep entries whose keys you did not rediscover, replace entries you did, and update `commit` and `date`. Delete an entry whose evidence no longer exists.
6. **Validate.** Run `sh <this file's directory>/scripts/check-discover.sh docs/discover/<topic>.md` from the project root. It checks that the header has `topic`, `scope`, `commit` (a commit in the repository), and a `YYYY-MM-DD` `date` once each, that every non-blank line in an entry is one of the four keys at the start of the line, that every `path:line` stays inside the project, that each guard's name appears in its file, that every evidence and guard `path:line` exists, and that each source's quote has at least three words and appears on its line, in its commit message, or in its PR through `gh`. A `PR #<n>` source fails without `gh`, so cite the commit instead when `gh` is unavailable. Fix every line it prints and rerun it until it prints `DISCOVER OK`.

## Reply

Give the file path, the number of entries, the number of invariants and how many are `unguarded`, and the number of reasons tagged `inferred`. List the unguarded invariants, since each is a candidate for a test.
