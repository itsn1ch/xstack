---
name: to-components
description: "Use when the user wants reusable UI components extracted from an existing app, repeated markup pulled into a component, a large screen or page split into components, or duplicated UI consolidated, in any framework, without changing what the app renders or does."
disable-model-invocation: true
---

# To components

**Setup gate.** Unless `XSTACK READY` is already in context this session, first run `sh <this file's directory>/../setup-xstack/scripts/check-setup.sh` from the project root. On `XSTACK SETUP MISSING`, or when the script is missing, stop. Show its output, tell the user to run `/setup-xstack`, and run no other step of this skill.

Extract UI components from an existing app so that the app renders and behaves exactly as before. The project decides the framework, the file layout, the naming, and where shared code lives. Learn them from the code. A request for "as reusable as possible" or "generic" components does not lift any rule below.

## Steps

1. **Survey.** Find the framework and language, how the project writes and names a component, where its shared components already live, how it styles them, and how it runs its tests. Read the folders the code actually uses. Do not assume `components/`, `shared/`, or `ui/`. List every existing component. Before writing a new one, check this list, and use the existing component wherever it already renders the same markup.
2. **Choose candidates.** A candidate is markup with one responsibility that appears in more than one place. Length alone, or a similar look, does not make a candidate. For each candidate, count its consumers: the parent components or files that would render it.
3. **Apply the merge rule.** Two sites are one component only when they render the same element with the same role and the same structure. When unifying them needs a parameter that changes the element, the ARIA role, or the layout for one site only, they are different components. Keep them apart, and list the rejected merge in the report.
4. **Apply the placement rule.**
   - A candidate with two or more consumers goes where the project keeps shared components.
   - A candidate with one consumer, such as fields repeated inside one form, stays local to that consumer: in its file, or beside it if that is the project's convention. Do not put it in the shared location.
   - Domain logic, such as prices, permissions, or business rules, stays with its caller. A component receives the computed result as an input.
5. **Define the contract.** Before moving code, write down for each component: its responsibility in one sentence, its inputs and outputs, its events or callbacks, its children or slots, who owns its state, and its dependencies, including styles. Keep the public interfaces of existing code as they are, unless the user approves a change.
6. **Capture the current output.** Render every affected screen with inputs that exercise each branch the extraction touches: escaping, empty lists, every conditional class or state, optional attributes, and each variant of a repeated item. Save the output, or use the project's snapshot or visual tests when it has them. Run the existing tests and record the result.
7. **Extract.** Move only the markup and the logic it needs. Update imports, exports, and every consumer. Leave unrelated code alone, including code style on lines the extraction does not move.
8. **Verify.** Render the same inputs again and diff against step 6. The diff must be empty, or every difference must be one the user approved. Run the existing tests. Unless the user asks, do not change an existing test to make it pass. Check that interactions, focus, ARIA attributes, and responsive classes survive. A check you could not run is reported as not run. It is never reported as passed.

## Report

Reply with this exact shape:

```markdown
## Extracted

| Component | Was in | Now in | Responsibility | Inputs → outputs | Consumers | Verified by |
| --------- | ------ | ------ | -------------- | ---------------- | --------- | ----------- |

## Reused

<existing components now used in place of copied markup, with the files that now use them, or "none">

## Not extracted

<each candidate you rejected and the rule that rejected it: one consumer, merge rule, domain logic, or not a single responsibility>

## Checks

<each check from step 8 with its result. Name any check that was not run.>
```

Extraction is complete when each component has one responsibility and its consumers use it, and when the step 8 diff is empty and the tests pass.
