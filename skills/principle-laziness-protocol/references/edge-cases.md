# Laziness Protocol: best practices and edge cases

## Best practice, in order

1. **Write the outcomes.** Turn the request into outcomes you can check, one line each. A request with three asks has at least three outcomes.
2. **Find every place the problem lives.** Sweep for the behavior or text with `git ls-files | subtask --grep '<old text or value>' --keep-going` before you choose where to change it. The place the request names is one place, not the list.
3. **Choose the smallest change that meets every outcome.** No new layer, class, or option unless an outcome needs it. Leave unrelated code as it is.
4. **Test the edges of what changed.** Add or extend tests for the edge cases below that apply, and run them as one `worker fast --check "<test command>"` call, plus a `--check` for each type check or linter the project has. Repeat the step 2 sweep, which must now find nothing. A linter or formatter run alone is not this step.
5. **Report.** What changed and which outcome it meets, what you left alone and why the problem is still solved, and the test run that proves it.

## Edge cases

Each row is a case where the lazy move looks right and is wrong. The eval case in `evals/` that tests it is in the last column.

| Situation | Wrong lazy move | Right move | Eval |
|---|---|---|---|
| A one-line fix to a function with numeric limits | Fix the line and test the one value from the bug report | Fix the line, then test both sides of each limit, zero or negative, and non-numeric input | `laziness-boundary-tests` |
| The file around the bug has unrelated messy code | Refactor the mess in the same diff, or skip tests because the fix is "obviously" small | Leave the mess unchanged, mention it, and still test the empty and single-item cases of the fix | `laziness-unrelated-mess` |
| The request lists several asks | Do the first ask and call the rest out of scope | Meet every ask with the least code each needs, and test the interaction between them | `laziness-every-ask` |
| The same value is repeated in several files | Change only the file the request mentions | Change every copy, or put the value behind one source of truth when that diff is no larger; leave no stale copy | `laziness-repeated-value` |
| A text change where the wrong statement appears in more than one section | Edit the intro only and report that the formatter passed | Fix every place the statement appears, then check the text against the request. Where the fix needs a fact you lack (whether other builds exist), drop the false claim, write only what is known, and ask about the rest | `laziness-text-everywhere` |

## Signs the protocol was misused

- The report cites the protocol and lists no left-alone item with a reason.
- The report cites the protocol alone, with no Prove It Works and no partner principle from **Never alone**.
- The only proof is a formatter, linter, or type check, for a change in behavior or meaning.
- A search for the old behavior or text still finds it after the change, including a stale claim left "for the user to decide".
- A test exists for the reported input and for no input next to it.
