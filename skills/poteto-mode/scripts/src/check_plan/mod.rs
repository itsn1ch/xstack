use regex::Regex;
use std::{
    env, fs,
    io::{self, Write},
    sync::LazyLock,
};

pub const RULE: &str = "Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.";
const SUB_BLOCKS: [&str; 9] = [
    "Depends on.",
    "Files.",
    "Build.",
    "You see.",
    "Verify, unit.",
    "Verify, live.",
    "Verify, perf.",
    "Review gate.",
    "Merge.",
];
const PROGRAM_H3: [&str; 5] = [
    "Arm the program",
    "Spawn owners",
    "PR mechanics",
    "Verdict and merge",
    "Boot recipe",
];
const PROGRAM_MARKERS: [&str; 3] = ["git show origin/main:", "/loop 1h", "status message"];
const HOW_TO_READ_MARKERS: [&str; 5] = [
    "One box is one unit of work",
    "names the evidence",
    "Check a box only when its evidence exists",
    "playbooks/",
    RULE,
];
const PERF_ITEMS: [&str; 4] = ["Metric.", "Probe.", "Baseline.", "Rule."];

#[derive(Debug, PartialEq, Eq)]
pub struct CheckResult {
    pub reports: Vec<String>,
    pub problems: Vec<String>,
    pub pr_sections: usize,
}
impl CheckResult {
    pub fn exit_code(&self) -> i32 {
        if self.problems.is_empty() {
            0
        } else {
            1
        }
    }
    pub fn stdout(&self) -> String {
        let mut output = String::new();
        for report in &self.reports {
            output.push_str(report);
            output.push('\n');
        }
        output.push_str(&format!(
            "{} PR sections, {} problems\n",
            self.pr_sections,
            self.problems.len()
        ));
        output
    }
}

#[derive(Clone, Copy)]
struct Line<'a> {
    n: usize,
    text: &'a str,
    code: bool,
}
struct Section<'a> {
    title: &'a str,
    n: usize,
    body: Vec<Line<'a>>,
}
impl Section<'_> {
    fn text(&self) -> String {
        self.body
            .iter()
            .map(|l| l.text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}
struct Block<'a> {
    name: &'a str,
    n: usize,
    rest: &'a str,
    lines: Vec<Line<'a>>,
}

fn js_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\t' | '\n' | '\u{000b}' | '\u{000c}' | '\r' | ' ' | '\u{00a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}
fn js_trim(text: &str) -> &str {
    text.trim_matches(js_whitespace)
}
fn js_linebreak(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}
fn lane_number(number: &f64) -> String {
    if number.is_infinite() {
        return "Infinity".into();
    }
    if *number >= 1e21 {
        let scientific = format!("{number:e}");
        let (mantissa, exponent) = scientific.split_once('e').unwrap();
        return format!("{mantissa}e+{exponent}");
    }
    number.to_string()
}

// CommonMark fences allow up to three leading spaces. Backtick info strings
// cannot contain backticks; tildes have no corresponding restriction.
fn fence_run(text: &str) -> Option<(u8, usize, &str)> {
    let bytes = text.as_bytes();
    let indent = bytes.iter().take_while(|&&b| b == b' ').count();
    if indent > 3 {
        return None;
    }
    let &ch = bytes.get(indent)?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let len = bytes[indent..].iter().take_while(|&&b| b == ch).count();
    Some((ch, len, &text[indent + len..]))
}
fn opening_fence(text: &str) -> Option<(u8, usize)> {
    let (ch, len, rest) = fence_run(text)?;
    (len >= 3 && !rest.contains(js_linebreak) && (ch != b'`' || !rest.contains('`')))
        .then_some((ch, len))
}
fn closing_fence(text: &str, opening: (u8, usize)) -> bool {
    fence_run(text).is_some_and(|(ch, len, rest)| {
        ch == opening.0 && len >= opening.1 && rest.bytes().all(|b| b == b' ' || b == b'\t')
    })
}

// Match maximal backtick runs, retaining unmatched delimiters as prose. Only
// an equally long run closes a span, including when its contents contain ticks.
fn strip_inline_code(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        let Some(relative) = text[cursor..].find('`') else {
            output.push_str(&text[cursor..]);
            break;
        };
        let start = cursor + relative;
        output.push_str(&text[cursor..start]);
        let len = bytes[start..].iter().take_while(|&&b| b == b'`').count();
        let mut search = start + len;
        let mut end = None;
        while search < bytes.len() {
            let Some(relative) = text[search..].find('`') else {
                break;
            };
            let tick = search + relative;
            if text[search..tick].contains(js_linebreak) {
                break;
            }
            let run = bytes[tick..].iter().take_while(|&&b| b == b'`').count();
            if run == len {
                end = Some(tick + run);
                break;
            }
            search = tick + run;
        }
        if let Some(end) = end {
            output.push('`');
            cursor = end;
        } else {
            output.push_str(&text[start..start + len]);
            cursor = start + len;
        }
    }
    output
}

fn boxes<'a>(lines: &[Line<'a>], pattern: &Regex) -> Vec<(usize, &'a str)> {
    lines
        .iter()
        .filter(|l| !l.code)
        .filter_map(|l| {
            pattern
                .captures(l.text)
                .map(|m| (l.n, m.get(1).unwrap().as_str()))
        })
        .collect()
}

fn box_count(lines: &[Line<'_>], pattern: &Regex) -> usize {
    lines
        .iter()
        .filter(|l| !l.code && pattern.is_match(l.text))
        .count()
}

struct Patterns {
    boxes: Regex,
    head: Regex,
    lanes: Regex,
    lane: Regex,
    screenshot: Regex,
    image: Regex,
    link: Regex,
    colon: Regex,
}
static PATTERNS: LazyLock<Patterns> = LazyLock::new(|| {
    Patterns {
    boxes: Regex::new(r"^[\t\n\x0b\x0c\r \u{00a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]*- \[[ x]\] ([^\r\n\u{2028}\u{2029}]*)$").unwrap(),
    head: Regex::new(r"^\*\*([^*]+)\*\*([^\r\n\u{2028}\u{2029}]*)$").unwrap(),
    lanes: Regex::new(r"Ten lanes on `[^`<>]+` at the PR head").unwrap(),
    lane: Regex::new(r"^Lane ([0-9]+)\. ").unwrap(),
    screenshot: Regex::new(r"Save `[^`]+`").unwrap(),
    image: Regex::new(r"!\[[^\]]*\]\([^)]*\)").unwrap(),
    link: Regex::new(r"\]\([^)]*\)").unwrap(),
    colon: Regex::new(r": [^\t\n\x0b\x0c\r \u{00a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]").unwrap(),
}
});
/// Check a Markdown plan, preserving one-based physical line diagnostics and
/// the report ordering of the original command. `file` is a diagnostic label.
pub fn check_plan(file: &str, source: &str) -> CheckResult {
    let Patterns {
        boxes: box_re,
        head: head_re,
        lanes: lanes_re,
        lane: lane_re,
        screenshot: screenshot_re,
        image: image_re,
        link: link_re,
        colon: colon_re,
    } = &*PATTERNS;
    let raw: Vec<&str> = source.split('\n').collect();
    let raw: Vec<&str> = raw
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if i + 1 < raw.len() {
                s.strip_suffix('\r').unwrap_or(s)
            } else {
                s
            }
        })
        .collect();
    let start = if raw.first() == Some(&"---") {
        raw.iter()
            .skip(1)
            .position(|s| *s == "---")
            .map_or(0, |i| i + 2)
    } else {
        0
    };
    let mut problems = Vec::new();
    let mut fail = |n, message: String| problems.push(format!("{file}:{n}: {message}"));
    let mut lines = Vec::new();
    let mut fence = None;
    for (i, &text) in raw.iter().enumerate().skip(start) {
        let mut code = fence.is_some();
        if fence.is_some_and(|f| closing_fence(text, f)) {
            code = false;
            fence = None;
        } else if fence.is_none() {
            fence = opening_fence(text);
            code = fence.is_some();
        }
        lines.push(Line {
            n: i + 1,
            text,
            code,
        });
        if code {
            continue;
        }
        let inline = strip_inline_code(text);
        let images = image_re.replace_all(&inline, "");
        let prose = link_re.replace_all(&images, "]");
        if prose.contains(['\u{2013}', '\u{2014}']) {
            fail(i + 1, "long dash".into());
        }
        if prose.contains(['\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}']) {
            fail(i + 1, "curly quote".into());
        }
        if colon_re.is_match(&prose) {
            fail(i + 1, "mid-sentence colon".into());
        }
    }
    let mut sections: Vec<Section<'_>> = Vec::new();
    for &line in &lines {
        if let Some(title) = (!line.code)
            .then(|| line.text.strip_prefix("## "))
            .flatten()
        {
            sections.push(Section {
                title: js_trim(title),
                n: line.n,
                body: Vec::new(),
            });
        } else if let Some(section) = sections.last_mut() {
            section.body.push(line);
        }
    }
    let find = |title| sections.iter().position(|s| s.title == title);
    let h1 = lines
        .iter()
        .position(|l| !l.code && l.text.starts_with("# "));
    if h1.is_none() {
        fail(1, "no H1 title".into());
    }
    let how = find("How to read this");
    if how.is_none() {
        fail(1, "no \"## How to read this\" section".into());
    }
    if let (Some(h1), Some(how)) = (h1, how) {
        let how = &sections[how];
        let intro = lines[h1 + 1..]
            .iter()
            .filter(|l| l.n < how.n && !js_trim(l.text).is_empty())
            .count();
        if intro >= 10 {
            fail(
                lines[h1].n,
                format!("intro is {intro} lines, under ten required"),
            );
        }
        let body = how.text();
        for marker in HOW_TO_READ_MARKERS {
            if !body.contains(marker) {
                fail(how.n, format!("How to read this lacks \"{marker}\""));
            }
        }
    }
    let program = find("Program checklist");
    if let Some(index) = program {
        let program = &sections[index];
        let h3s: Vec<&str> = program
            .body
            .iter()
            .filter(|l| !l.code)
            .filter_map(|l| l.text.strip_prefix("### ").map(js_trim))
            .collect();
        let mut cursor = 0;
        for name in PROGRAM_H3 {
            if let Some(at) = h3s
                .iter()
                .enumerate()
                .skip(cursor)
                .find_map(|(i, t)| t.starts_with(name).then_some(i))
            {
                cursor = at + 1;
            } else {
                fail(
                    program.n,
                    format!("Program checklist lacks \"### {name}\" in order"),
                );
            }
        }
        let body = program.text();
        for marker in PROGRAM_MARKERS {
            if !body.contains(marker) {
                fail(program.n, format!("Program checklist lacks \"{marker}\""));
            }
        }
    } else {
        fail(1, "no \"## Program checklist\" section".into());
    }
    let close = find("Close the program");
    if close.is_none() {
        fail(1, "no \"## Close the program\" section".into());
    }
    let prs = match (program, close) {
        (Some(p), Some(c)) if p < c => &sections[p + 1..c],
        _ => &sections[0..0],
    };
    if prs.is_empty() {
        fail(
            1,
            "no PR sections between Program checklist and Close the program".into(),
        );
    }
    let mut reports = Vec::new();
    for pr in prs {
        let mut heads: Vec<Block<'_>> = Vec::new();
        for &line in &pr.body {
            if line.code {
                continue;
            }
            if let Some(m) = head_re
                .captures(line.text)
                .filter(|m| SUB_BLOCKS.contains(&m.get(1).unwrap().as_str()))
            {
                heads.push(Block {
                    name: m.get(1).unwrap().as_str(),
                    n: line.n,
                    rest: js_trim(m.get(2).unwrap().as_str()),
                    lines: Vec::new(),
                });
            } else if let Some(head) = heads.last_mut() {
                head.lines.push(line);
            }
        }
        let names: Vec<&str> = heads.iter().map(|h| h.name).collect();
        if names != SUB_BLOCKS {
            fail(
                pr.n,
                format!(
                    "{}: sub-blocks are [{}], expected [{}]",
                    pr.title,
                    names.join(", "),
                    SUB_BLOCKS.join(", ")
                ),
            );
        }
        let block = |name| heads.iter().find(|h| h.name == name);
        if let Some(depends) = block("Depends on.") {
            if depends.rest.is_empty() {
                fail(depends.n, format!("{}: Depends on names nothing", pr.title));
            }
        }
        for name in ["Files.", "Build.", "You see.", "Verify, unit.", "Merge."] {
            if let Some(b) = block(name) {
                if box_count(&b.lines, box_re) == 0 {
                    fail(b.n, format!("{}: {name} has no box", pr.title));
                }
            }
        }
        for name in ["Verify, unit.", "Verify, live.", "Verify, perf."] {
            if let Some(b) = block(name) {
                if !b.rest.starts_with(RULE) {
                    fail(
                        b.n,
                        format!("{}: {name} does not open with the rule", pr.title),
                    );
                }
            }
        }
        if let Some(live) = block("Verify, live.") {
            if !lanes_re.is_match(live.rest) {
                fail(live.n, format!("{}: Verify, live lacks \"Ten lanes on `<swarm workers model>` at the PR head\" with the model filled in", pr.title));
            }
            let lanes = boxes(&live.lines, &box_re);
            let mut numbers: Vec<f64> = lanes
                .iter()
                .filter_map(|(_, t)| {
                    lane_re
                        .captures(t)
                        .map(|m| m[1].parse().unwrap_or(f64::INFINITY))
                })
                .collect();
            numbers.sort_by(f64::total_cmp);
            let numbers = numbers
                .iter()
                .map(lane_number)
                .collect::<Vec<_>>()
                .join(",");
            if numbers != "1,2,3,4,5,6,7,8,9,10" {
                fail(
                    live.n,
                    format!("{}: lanes are [{numbers}], expected 1 to 10", pr.title),
                );
            }
            for (n, text) in lanes {
                if let Some(m) = lane_re.captures(text) {
                    if !screenshot_re.is_match(text) {
                        fail(
                            n,
                            format!("{}: lane {} names no screenshot", pr.title, &m[1]),
                        );
                    } else if !text.contains("Pass when") {
                        fail(
                            n,
                            format!("{}: lane {} has no pass predicate", pr.title, &m[1]),
                        );
                    }
                } else {
                    fail(n, format!("{}: live box is not a lane", pr.title));
                }
            }
        }
        if let Some(perf) = block("Verify, perf.") {
            let items: Vec<&str> = boxes(&perf.lines, &box_re)
                .into_iter()
                .map(|(_, t)| t.split(' ').next().unwrap_or(""))
                .collect();
            if items != PERF_ITEMS {
                fail(
                    perf.n,
                    format!(
                        "{}: perf boxes are [{}], expected [{}]",
                        pr.title,
                        items.join(", "),
                        PERF_ITEMS.join(", ")
                    ),
                );
            }
        }
        if let Some(gate) = block("Review gate.") {
            let gate_boxes = box_count(&gate.lines, box_re);
            if gate.rest.starts_with("None.") {
                if gate_boxes != 0 {
                    fail(
                        gate.n,
                        format!("{}: Review gate says None but has boxes", pr.title),
                    );
                }
            } else {
                if gate_boxes == 0 {
                    fail(gate.n, format!("{}: Review gate has no box", pr.title));
                }
                let text = gate
                    .lines
                    .iter()
                    .map(|l| l.text)
                    .collect::<Vec<_>>()
                    .join("\n");
                for word in ["screenshot", "video", "operator"] {
                    if !text.contains(word) {
                        fail(
                            gate.n,
                            format!("{}: Review gate lacks \"{word}\"", pr.title),
                        );
                    }
                }
            }
        }
        const LABELS: [&str; 8] = [
            "files",
            "build",
            "you-see",
            "verify-unit",
            "verify-live",
            "verify-perf",
            "review-gate",
            "merge",
        ];
        let cells: Vec<String> = SUB_BLOCKS
            .iter()
            .skip(1)
            .zip(LABELS)
            .map(|(name, label)| {
                // Duplicate blocks overwrite counts, matching the original report.
                let count = heads
                    .iter()
                    .rev()
                    .find(|h| h.name == *name)
                    .map_or(0, |h| box_count(&h.lines, box_re));
                format!("{label}={count}")
            })
            .collect();
        reports.push(format!(
            "{}  boxes={}  {}",
            pr.title,
            box_count(&pr.body, box_re),
            cells.join(" ")
        ));
    }
    if let Some(close) = close {
        let tail = &sections[close + 1..];
        for section in tail {
            if !section.title.starts_with("Appendix") {
                fail(
                    section.n,
                    format!(
                        "\"## {}\" after Close the program is not an appendix",
                        section.title
                    ),
                );
            }
        }
        if !tail.iter().any(|s| s.title.contains("Prototype evidence")) {
            fail(
                sections[close].n,
                "no \"## Appendix ... Prototype evidence\" section".into(),
            );
        }
    }
    CheckResult {
        reports,
        problems,
        pr_sections: prs.len(),
    }
}

pub fn main() -> i32 {
    let Some(file) = env::args_os().nth(1) else {
        eprintln!("Usage: check-plan <plan.md>");
        return 2;
    };
    let label = file.to_string_lossy();
    let source = match fs::read(&file) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) => {
            eprintln!("{label}: {error}");
            return 1;
        }
    };
    let result = check_plan(&label, &source);
    if let Err(error) = io::stdout().lock().write_all(result.stdout().as_bytes()) {
        eprintln!("{error}");
        return 1;
    }
    for problem in &result.problems {
        eprintln!("{problem}");
    }
    result.exit_code()
}

#[cfg(test)]
mod tests;
