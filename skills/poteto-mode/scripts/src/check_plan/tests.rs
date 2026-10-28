use super::*;

fn valid_plan() -> String {
    let lanes = (1..=10)
        .map(|i| format!("- [ ] Lane {i}. Save `lane-{i}.png`. Pass when evidence exists."))
        .collect::<Vec<_>>()
        .join("\n");
    format!("# Demo\n\n## How to read this\nOne box is one unit of work and names the evidence. Check a box only when its evidence exists. See playbooks/.\n{RULE}\n\n## Program checklist\n### Arm the program\n### Spawn owners\n### PR mechanics\n### Verdict and merge\n### Boot recipe\n`git show origin/main:` /loop 1h status message\n\n## PR 1\n**Depends on.** None.\n**Files.**\n- [ ] Do the thing.\n**Build.**\n- [ ] Do the thing.\n**You see.**\n- [ ] Do the thing.\n**Verify, unit.** {RULE}\n- [ ] Check the thing.\n**Verify, live.** {RULE} Ten lanes on `example-model` at the PR head\n{lanes}\n**Verify, perf.** {RULE}\n- [ ] Metric. Recorded.\n- [ ] Probe. Recorded.\n- [ ] Baseline. Recorded.\n- [ ] Rule. Recorded.\n**Review gate.** None.\n**Merge.**\n- [ ] Merge when approved.\n\n## Close the program\nClose.\n\n## Appendix Prototype evidence\nRecorded.\n")
}
fn run(example: &str, expected: i32) -> CheckResult {
    check(&(valid_plan() + example), expected)
}
fn check(plan: &str, expected: i32) -> CheckResult {
    let result = check_plan("plan.md", plan);
    assert_eq!(result.exit_code(), expected, "{:?}", result.problems);
    result
}
fn contains(result: &CheckResult, message: &str) {
    assert!(
        result.problems.iter().any(|p| p.contains(message)),
        "missing {message:?} in {:?}",
        result.problems
    );
}

#[test]
fn accepts_ordinary_indented_and_longer_closing_fences() {
    for indent in ["", " ", "  ", "   "] {
        run(
            &format!("{indent}```markdown\n## Example only\n{indent}```\n"),
            0,
        );
    }
    run("```markdown\n## Example only\n````\n", 0);
}
#[test]
fn accepts_nested_backticks_and_tilde_info_strings() {
    run("````markdown\n```markdown\n## Example only\n```\n````\n", 0);
    run("~~~markdown `info`\n## Example only\n ~~~\n", 0);
}
#[test]
fn keeps_mismatched_shorter_and_suffixed_closers_inside_fence() {
    for closer in ["~~~", "```", "``` info"] {
        run(
            &format!("````markdown\n{closer}\n## Example only\n````\n"),
            0,
        );
    }
}
#[test]
fn rejects_headings_and_prose_outside_proper_close() {
    for suffix in ["  ", "\t"] {
        contains(
            &run(
                &format!("```markdown\n## Example only\n```{suffix}\n## Example only\n"),
                1,
            ),
            "\"## Example only\" after Close the program is not an appendix",
        );
    }
    let result = run("```markdown\nThis prose has a mid-sentence colon: inside code\n```\nThis prose has a mid-sentence colon: bad.\n", 1);
    contains(&result, "mid-sentence colon");
    assert_eq!(result.problems.len(), 1);
}
#[test]
fn rejects_backtick_info_strings_containing_backtick() {
    contains(
        &run("```markdown `invalid`\n## Example only\n```\n", 1),
        "\"## Example only\" after Close the program is not an appendix",
    );
}
#[test]
fn ignores_punctuation_inside_matching_multi_backtick_spans() {
    for count in [2, 3, 4] {
        let delimiter = "`".repeat(count);
        run(
            &format!("Use {delimiter}echo `x`: “quoted” — example{delimiter}.\n"),
            0,
        );
    }
}
#[test]
fn checks_prose_following_multi_backtick_span_and_unmatched_delimiters() {
    contains(
        &run("Use ``echo `x`: y``. Bad: prose.\n", 1),
        "mid-sentence colon",
    );
    contains(
        &run("Use ``unclosed: bad` here.\n", 1),
        "mid-sentence colon",
    );
}
#[test]
fn skips_front_matter_before_title() {
    check(
        &format!("---\nname: demo: with a colon\n---\n{}", valid_plan()),
        0,
    );
}
#[test]
fn rejects_long_dashes_and_curly_quotes_in_prose() {
    contains(&run("A long dash — here.\n", 1), "long dash");
    contains(&run("A “quoted” word.\n", 1), "curly quote");
}
#[test]
fn requires_screenshot_video_and_operator_in_boxed_gate() {
    let gated = |text| {
        valid_plan().replace(
            "**Review gate.** None.",
            &format!("**Review gate.** Operator review.\n- [ ] {text}"),
        )
    };
    check(&gated("The operator reviews the screenshot and video."), 0);
    contains(
        &check(&gated("The operator reviews the screenshot."), 1),
        "Review gate lacks \"video\"",
    );
    contains(
        &check(
            &valid_plan().replace(
                "**Review gate.** None.",
                "**Review gate.** Operator review of screenshot and video.",
            ),
            1,
        ),
        "Review gate has no box",
    );
}
#[test]
fn preserves_report_and_physical_line_numbers() {
    let result = check(&valid_plan(), 0);
    assert_eq!(result.stdout(), "PR 1  boxes=19  files=1 build=1 you-see=1 verify-unit=1 verify-live=10 verify-perf=4 review-gate=0 merge=1\n1 PR sections, 0 problems\n");
    let plan = format!(
        "---\nname: demo\n---\n{}Bad: prose — “quote”.\n",
        valid_plan()
    );
    let n = plan.lines().count();
    let result = check(&plan, 1);
    assert_eq!(
        result.problems,
        vec![
            format!("plan.md:{n}: long dash"),
            format!("plan.md:{n}: curly quote"),
            format!("plan.md:{n}: mid-sentence colon")
        ]
    );
    assert_eq!(
        check(&valid_plan().replace('\n', "\r\n"), 0).stdout(),
        check(&valid_plan(), 0).stdout()
    );
}
#[test]
fn checks_all_required_sections_and_appendix() {
    let result = check("", 1);
    assert_eq!(
        result.problems,
        [
            "plan.md:1: no H1 title",
            "plan.md:1: no \"## How to read this\" section",
            "plan.md:1: no \"## Program checklist\" section",
            "plan.md:1: no \"## Close the program\" section",
            "plan.md:1: no PR sections between Program checklist and Close the program"
        ]
    );
    contains(
        &check(
            &valid_plan().replace("## Appendix Prototype evidence", "## Appendix Other"),
            1,
        ),
        "no \"## Appendix ... Prototype evidence\" section",
    );
    // Prototype evidence title matching is deliberately independent of Appendix prefix.
    let result = check(
        &valid_plan().replace("## Appendix Prototype evidence", "## Prototype evidence"),
        1,
    );
    assert_eq!(result.problems.len(), 1);
    contains(&result, "after Close the program is not an appendix");
    contains(
        &check(&valid_plan().replace("## PR 1", "PR 1"), 1),
        "no PR sections",
    );
}
#[test]
fn checks_intro_markers_and_program_order() {
    let intro = (0..10).map(|_| "Intro.\n").collect::<String>();
    contains(
        &check(
            &valid_plan().replace("# Demo\n", &format!("# Demo\n{intro}")),
            1,
        ),
        "intro is 10 lines, under ten required",
    );
    for marker in HOW_TO_READ_MARKERS {
        contains(
            &check(&valid_plan().replacen(marker, "Removed", 1), 1),
            &format!("How to read this lacks \"{marker}\""),
        );
    }
    for marker in PROGRAM_MARKERS {
        contains(
            &check(&valid_plan().replace(marker, "Removed"), 1),
            &format!("Program checklist lacks \"{marker}\""),
        );
    }
    let plan = valid_plan().replace(
        "### Arm the program\n### Spawn owners",
        "### Spawn owners\n### Arm the program",
    );
    contains(
        &check(&plan, 1),
        "Program checklist lacks \"### Spawn owners\" in order",
    );
    check(
        &valid_plan().replace("### Boot recipe", "### Boot recipe details"),
        0,
    );
}
#[test]
fn checks_subblock_order_dependency_and_required_boxes() {
    contains(
        &check(
            &valid_plan().replace("**Depends on.** None.", "**Depends on.**"),
            1,
        ),
        "Depends on names nothing",
    );
    for name in SUB_BLOCKS {
        contains(
            &check(
                &valid_plan().replace(&format!("**{name}**"), "**Other.**"),
                1,
            ),
            "sub-blocks are",
        );
    }
    for name in ["Files.", "Build.", "You see.", "Verify, unit.", "Merge."] {
        let plan = valid_plan();
        let start = plan.find(&format!("**{name}**")).unwrap();
        let box_start = start + plan[start..].find("- [ ]").unwrap();
        let end = box_start + plan[box_start..].find('\n').unwrap();
        let mut broken = plan.clone();
        broken.replace_range(box_start..end, "Not a box.");
        contains(&check(&broken, 1), &format!("{name} has no box"));
    }
    check(&valid_plan().replace("- [ ]", "- [x]"), 0);
}
#[test]
fn checks_verification_rules_model_lane_numbers_and_evidence() {
    for name in ["Verify, unit.", "Verify, live.", "Verify, perf."] {
        contains(
            &check(
                &valid_plan().replace(
                    &format!("**{name}** {RULE}"),
                    &format!("**{name}** Missing rule."),
                ),
                1,
            ),
            &format!("{name} does not open with the rule"),
        );
    }
    for model in ["", "<swarm workers model>"] {
        contains(
            &check(&valid_plan().replace("example-model", model), 1),
            "Verify, live lacks",
        );
    }
    contains(
        &check(&valid_plan().replace("Lane 10.", "Lane 9."), 1),
        "lanes are [1,2,3,4,5,6,7,8,9,9], expected 1 to 10",
    );
    contains(
        &check(&valid_plan().replace("Lane 1. ", "Not a lane. "), 1),
        "live box is not a lane",
    );
    contains(
        &check(
            &valid_plan().replace("Save `lane-1.png`", "Save evidence"),
            1,
        ),
        "lane 1 names no screenshot",
    );
    contains(
        &check(&valid_plan().replacen("Pass when", "Pass if", 1), 1),
        "lane 1 has no pass predicate",
    );
    contains(
        &check(
            &valid_plan().replace("Metric. Recorded.", "Metric Recorded."),
            1,
        ),
        "perf boxes are [Metric, Probe., Baseline., Rule.]",
    );
}
#[test]
fn checks_none_gate_boxes_and_body_only_evidence() {
    contains(
        &check(
            &valid_plan().replace(
                "**Review gate.** None.",
                "**Review gate.** None.\n- [ ] screenshot video operator",
            ),
            1,
        ),
        "Review gate says None but has boxes",
    );
    let result = check(
        &valid_plan().replace(
            "**Review gate.** None.",
            "**Review gate.** screenshot video operator\n- [ ] Review.",
        ),
        1,
    );
    for word in ["screenshot", "video", "operator"] {
        contains(&result, &format!("Review gate lacks \"{word}\""));
    }
}
#[test]
fn ignores_fenced_structures_and_link_destinations_but_not_labels() {
    run("![“hidden”—: punctuation](https://example.test/a: b) [safe](https://example.test/—: “x”)\n", 0);
    contains(&run("[visible —](https://example.test/)\n", 1), "long dash");
    // Four spaces do not open a fence in this checker.
    contains(&run("    ```\n## Other\n", 1), "after Close the program");
    let plan = valid_plan().replace("- [ ] Do the thing.", "```\n- [ ] Do the thing.\n```");
    let result = check(&plan, 1);
    contains(&result, "Files. has no box");
    contains(&result, "Build. has no box");
    contains(&result, "You see. has no box");
}
#[test]
fn duplicate_blocks_use_first_for_validation_last_for_report() {
    let result = check(
        &valid_plan().replace("**Files.**", "**Files.**\n- [ ] First.\n**Files.**"),
        1,
    );
    contains(
        &result,
        "sub-blocks are [Depends on., Files., Files., Build.",
    );
    assert!(result.reports[0].contains("boxes=20  files=1"));
}

#[test]
fn retains_javascript_whitespace_and_line_terminator_semantics() {
    check(
        &valid_plan().replace("## How to read this", "## \u{feff}How to read this\u{feff}"),
        0,
    );
    check(&valid_plan().replace("- [ ]", "\u{feff}- [ ]"), 0);
    contains(
        &check(
            &valid_plan().replace("- [ ] Do the thing.", "\u{0085}- [ ] Do the thing."),
            1,
        ),
        "Files. has no box",
    );
    run("A colon: \u{feff}is followed by whitespace.\n", 0);
    contains(
        &run("A colon: \u{0085}is not followed by JS whitespace.\n", 1),
        "mid-sentence colon",
    );
    contains(
        &run("Use `hidden\u{2028}Bad: prose`.\n", 1),
        "mid-sentence colon",
    );
    contains(
        &run("```info\u{2028}invalid\n## Outside\n", 1),
        "after Close the program",
    );
    assert_eq!(lane_number(&1e21), "1e+21");
    assert_eq!(lane_number(&f64::INFINITY), "Infinity");
}

#[test]
fn preserves_unterminated_front_matter_and_fenced_marker_behavior() {
    contains(
        &check(&format!("---\nname: invalid\n{}", valid_plan()), 1),
        "plan.md:2: mid-sentence colon",
    );
    check(
        &valid_plan().replace(
            "`git show origin/main:` /loop 1h status message",
            "```\ngit show origin/main: /loop 1h status message\n```",
        ),
        0,
    );
    run("```markdown\nBad: prose — “quote”.\n## Outside\n", 0);
}
