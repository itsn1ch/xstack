//! Checks that an epic's tickets form one continuous chain: every spec criterion reaches a ticket,
//! every ticket handoff matches on both sides, every name exists in the design contract,
//! and the 00 overview maps every ticket to the files it touches.
use regex::Regex;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use std::thread;

macro_rules! pattern {
    ($name:ident, $re:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($re).expect("valid pattern"));
    };
}

pattern!(AC_ID, r"US-\d+-AC\d+");
pattern!(AC_DEFINITION, r"\*\*(US-\d+-AC\d+):\*\*");
pattern!(INV_ID, r"INV-\d+");
pattern!(FILE_NAME, r"^(\d{2,})-[a-z0-9-]+\.md$");
pattern!(HEADING, r"(?m)^# (\d+): .+$");
pattern!(TICKET_NUMBER, r"\b(\d{2,})\b");
pattern!(FUNCTION_HEADER, r"(?m)^### `([^`]+)`");
pattern!(FUNCTION_PREFIX, r"^(?:(?:export|pub|async|function|func|def|fn)\s+)+");
pattern!(METHOD_RECEIVER, r"^\([^)]*\)\s*");
pattern!(FUNCTION_NAME, r"([A-Za-z_$][\w$]*)\s*(?:<[^>]*>)?\s*\(");
pattern!(INVARIANT_ROW, r"(?m)^\|\s*`?(INV-\d+)`?\s*\|[^|]*\|\s*(establish|preserve)\s*\|");
pattern!(BORROWED_TYPE, r"(?m)^\s*([A-Za-z_$][\w$]*)\s*//\s*from\s+(\d+)");
pattern!(TYPE_MARKER, r"//\s*(?:from|existing)");
pattern!(FLOW_IN, r"(?m)^- In: `([^`]+)`\s*(?:←|<-)\s*(\d+|existing|external)\b");
pattern!(FLOW_OUT, r"(?m)^- Out: `([^`]+)`\s*(?:→|->)\s*(\d+|final)\b");
pattern!(WHERE_LINE, r"(?m)^- \*\*Where\.\*\*(.*)$");
pattern!(BACKTICKED, r"`([^`]+)`");
pattern!(CODE_BLOCK, r"(?s)```[^\n]*\n(.*?)```");
pattern!(TREE_LINE, r"^([\s│├└─|`+\\-]*)([^\s\[]+)");
pattern!(TREE_TAG, r"\[(\d{2,}(?:\s*,\s*\d{2,})*)\]");

const FIELDS: [&str; 4] = ["Status", "Blocked by", "Source", "Contract"];
const REQUIRED: [&str; 5] = ["Goal", "Steps", "Acceptance criteria", "Verify", "Do not touch"];
const REQUIRED_WITH_CONTRACT: [&str; 4] = ["Types", "Functions", "Invariants", "Flow"];
const OVERVIEW_SECTIONS: [&str; 4] = ["Files", "Architecture", "Order", "In plain words"];

pub struct Report {
    pub errors: Vec<String>,
    /// Work tickets, not counting the 00 overview.
    pub tickets: usize,
}

struct Ticket {
    file: String,
    /// -1 when the file name has no number.
    number: i64,
    fields: HashMap<&'static str, String>,
    sections: HashMap<String, String>,
    no_contract: bool,
    blocked_by: Vec<i64>,
    criteria: BTreeSet<String>,
}

impl Ticket {
    fn section(&self, name: &str) -> &str {
        self.sections.get(name).map(String::as_str).unwrap_or("")
    }
}

fn pad(n: i64) -> String {
    format!("{n:02}")
}

fn word(name: &str) -> Regex {
    Regex::new(&format!(r"\b{}\b", regex::escape(name))).expect("escaped word")
}

fn parse_sections(text: &str) -> HashMap<String, String> {
    let mut sections = HashMap::new();
    let mut current: Option<(String, String)> = None;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("## ") {
            if let Some((name, body)) = current.take() {
                sections.insert(name, body);
            }
            current = Some((name.trim().to_string(), String::new()));
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some((name, body)) = current {
        sections.insert(name, body);
    }
    sections
}

fn design_section(spec: &str) -> String {
    let mut inside = false;
    let mut out = String::new();
    for line in spec.lines() {
        if line.trim_end() == "# Design" {
            inside = true;
            continue;
        }
        if inside && line.starts_with("# ") {
            break;
        }
        if inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn parse_ticket(file: &str, text: &str) -> (Ticket, Vec<String>) {
    let mut errors = Vec::new();
    let number = match FILE_NAME.captures(file) {
        Some(c) => c[1].parse().unwrap_or(-1),
        None => {
            errors.push(format!("{file}: name is not <NN>-<slug>.md"));
            -1
        }
    };
    match HEADING.captures(text) {
        None => errors.push(format!("{file}: no \"# <NN>: <title>\" heading")),
        Some(c) if c[1].parse::<i64>().ok() != Some(number) => {
            errors.push(format!("{file}: heading number {} does not match file name", &c[1]));
        }
        Some(_) => {}
    }
    let mut fields = HashMap::new();
    for name in FIELDS {
        let prefix = format!("**{name}:**");
        if let Some(value) = text.lines().find_map(|l| l.strip_prefix(prefix.as_str())) {
            fields.insert(name, value.trim().to_string());
        }
    }
    let sections = parse_sections(text);
    let field = |name: &str| fields.get(name).map(String::as_str).unwrap_or("");
    let no_contract = field("Contract").to_lowercase().contains("contract: n/a");
    let blocked = field("Blocked by");
    let blocked_by = if blocked.to_lowercase().starts_with("none") {
        Vec::new()
    } else {
        TICKET_NUMBER.captures_iter(blocked).filter_map(|c| c[1].parse().ok()).collect()
    };
    let criteria = AC_ID
        .find_iter(sections.get("Acceptance criteria").map(String::as_str).unwrap_or(""))
        .chain(AC_ID.find_iter(field("Source")))
        .map(|m| m.as_str().to_string())
        .collect();
    let ticket = Ticket { file: file.to_string(), number, fields, sections, no_contract, blocked_by, criteria };
    (ticket, errors)
}

/// Reads and parses every ticket file in parallel, one thread per file, keeping file-name order.
fn read_tickets(issues: &Path) -> (Vec<Ticket>, Vec<String>) {
    let mut names: Vec<String> = fs::read_dir(issues)
        .map(|dir| dir.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".md")).collect())
        .unwrap_or_default();
    names.sort();
    let parsed: Vec<(Ticket, Vec<String>)> = thread::scope(|scope| {
        let handles: Vec<_> = names
            .iter()
            .map(|name| scope.spawn(move || parse_ticket(name, &fs::read_to_string(issues.join(name)).unwrap_or_default())))
            .collect();
        handles.into_iter().map(|h| h.join().expect("ticket parser")).collect()
    });
    let mut errors = Vec::new();
    let tickets = parsed
        .into_iter()
        .map(|(ticket, mut parse_errors)| {
            errors.append(&mut parse_errors);
            ticket
        })
        .collect();
    (tickets, errors)
}

pub fn check_epic(epic: &Path) -> Report {
    let mut errors = Vec::new();
    let spec = fs::read_to_string(epic.join("spec.md")).ok();
    if spec.is_none() {
        errors.push("spec.md: missing".into());
    }
    let issues = epic.join("issues");
    if !issues.is_dir() {
        errors.push("issues/: missing".into());
        return Report { errors, tickets: 0 };
    }
    let design = spec.as_deref().map(design_section).unwrap_or_default();
    let (all, mut parse_errors) = read_tickets(&issues);
    errors.append(&mut parse_errors);
    let (overviews, tickets): (Vec<Ticket>, Vec<Ticket>) = all.into_iter().partition(|t| t.number == 0);
    let by_number: HashMap<i64, &Ticket> = tickets.iter().map(|t| (t.number, t)).collect();
    let spec_has_criteria = spec.as_deref().is_some_and(|s| AC_DEFINITION.is_match(s));

    check_numbering(&tickets, &mut errors);
    for t in &tickets {
        check_shape(t, spec_has_criteria, &mut errors);
    }
    check_blockers(&tickets, &by_number, &mut errors);
    let closure = blocker_closure(&tickets, &by_number);
    if let Some(spec) = &spec {
        check_spec_coverage(spec, &tickets, &mut errors);
    }
    check_functions(&tickets, &design, &mut errors);
    check_invariants(&tickets, &design, &mut errors);
    check_types(&tickets, &by_number, &closure, &mut errors);
    check_flow(&tickets, &by_number, &closure, &mut errors);
    match overviews.first() {
        Some(overview) => check_overview(overview, &tickets, &by_number, &mut errors),
        None if !tickets.is_empty() => errors.push("issues/00-overview.md: missing, the epic has no map".into()),
        None => {}
    }
    Report { errors, tickets: tickets.len() }
}

fn check_numbering(tickets: &[Ticket], errors: &mut Vec<String>) {
    for (i, t) in tickets.iter().enumerate() {
        let expected = i as i64 + 1;
        if t.number != expected {
            errors.push(format!("{}: expected ticket {}, numbers must run 01..N with no gap or duplicate", t.file, pad(expected)));
        }
    }
}

fn check_shape(t: &Ticket, spec_has_criteria: bool, errors: &mut Vec<String>) {
    for name in FIELDS {
        if !t.fields.contains_key(name) {
            errors.push(format!("{}: missing **{name}:** line", t.file));
        }
    }
    let extra: &[&str] = if t.no_contract { &[] } else { &REQUIRED_WITH_CONTRACT };
    for name in REQUIRED.iter().chain(extra) {
        if !t.sections.contains_key(*name) {
            errors.push(format!("{}: missing \"## {name}\" section", t.file));
        }
    }
    if spec_has_criteria && t.criteria.is_empty() && !t.no_contract {
        errors.push(format!("{}: names no spec criterion ID in Source or Acceptance criteria", t.file));
    }
}

fn check_blockers(tickets: &[Ticket], by_number: &HashMap<i64, &Ticket>, errors: &mut Vec<String>) {
    for t in tickets {
        for &b in &t.blocked_by {
            if b == 0 {
                errors.push(format!("{}: blocked by 00, but the overview is never a blocker", t.file));
            } else if !by_number.contains_key(&b) {
                errors.push(format!("{}: blocked by {}, which does not exist", t.file, pad(b)));
            } else if b >= t.number {
                errors.push(format!("{}: blocked by {}, which is not earlier", t.file, pad(b)));
            }
        }
    }
}

fn blocker_closure(tickets: &[Ticket], by_number: &HashMap<i64, &Ticket>) -> HashMap<i64, HashSet<i64>> {
    tickets
        .iter()
        .map(|t| {
            let mut seen = HashSet::new();
            let mut stack = t.blocked_by.clone();
            while let Some(n) = stack.pop() {
                if seen.contains(&n) || n >= t.number {
                    continue;
                }
                let Some(blocker) = by_number.get(&n) else { continue };
                seen.insert(n);
                stack.extend(&blocker.blocked_by);
            }
            (t.number, seen)
        })
        .collect()
}

fn check_spec_coverage(spec: &str, tickets: &[Ticket], errors: &mut Vec<String>) {
    // Only the first ID on a "(removed)" line is retired; later IDs name its replacement.
    let removed: HashSet<&str> = spec
        .lines()
        .filter(|l| l.to_lowercase().contains("(removed)"))
        .filter_map(|l| AC_ID.find(l).map(|m| m.as_str()))
        .collect();
    let mut defined: Vec<&str> = Vec::new();
    for c in AC_DEFINITION.captures_iter(spec) {
        let id = c.get(1).expect("group").as_str();
        if !defined.contains(&id) {
            defined.push(id);
        }
    }
    let covered: HashSet<&str> = tickets.iter().flat_map(|t| t.criteria.iter().map(String::as_str)).collect();
    for id in defined.iter().filter(|id| !removed.contains(*id)) {
        if !covered.contains(id) {
            errors.push(format!("spec.md: {id} is not delivered or guarded by any ticket"));
        }
    }
    for t in tickets {
        for id in &t.criteria {
            if !defined.contains(&id.as_str()) {
                errors.push(format!("{}: {id} is not defined in spec.md", t.file));
            } else if removed.contains(id.as_str()) {
                errors.push(format!("{}: {id} is removed in spec.md", t.file));
            }
        }
    }
}

fn function_name(signature: &str) -> String {
    let stripped = FUNCTION_PREFIX.replace(signature, "");
    let stripped = METHOD_RECEIVER.replace(&stripped, "");
    match FUNCTION_NAME.captures(&stripped) {
        Some(c) => c[1].to_string(),
        None => stripped.split_whitespace().next().unwrap_or("").to_string(),
    }
}

fn check_functions(tickets: &[Ticket], design: &str, errors: &mut Vec<String>) {
    let mut owner: HashMap<String, &str> = HashMap::new();
    for t in tickets {
        for c in FUNCTION_HEADER.captures_iter(t.section("Functions")) {
            let name = function_name(&c[1]);
            match owner.get(&name) {
                Some(first) => errors.push(format!("{}: function {name} is already owned by {first}", t.file)),
                None => {
                    owner.insert(name.clone(), &t.file);
                }
            }
            if !design.is_empty() && !word(&name).is_match(design) {
                errors.push(format!("{}: function {name} is not in the spec's # Design contract", t.file));
            }
        }
    }
}

fn check_invariants(tickets: &[Ticket], design: &str, errors: &mut Vec<String>) {
    let mut established_by: HashMap<String, &Ticket> = HashMap::new();
    let mut rows = Vec::new();
    for t in tickets {
        for c in INVARIANT_ROW.captures_iter(t.section("Invariants")) {
            let (id, role) = (c[1].to_string(), c[2].to_string());
            if role == "establish" {
                match established_by.get(&id) {
                    Some(first) => errors.push(format!("{}: {id} is already established by {}", t.file, first.file)),
                    None => {
                        established_by.insert(id.clone(), t);
                    }
                }
            }
            rows.push((t, id, role));
        }
    }
    let in_design: HashSet<&str> = INV_ID.find_iter(design).map(|m| m.as_str()).collect();
    for (t, id, role) in rows {
        if !design.is_empty() && !in_design.contains(id.as_str()) {
            errors.push(format!("{}: {id} is not in the spec's # Design contract", t.file));
        }
        if role != "preserve" {
            continue;
        }
        if let Some(by) = established_by.get(&id).filter(|by| by.number > t.number) {
            errors.push(format!("{}: preserves {id} before {} establishes it", t.file, by.file));
        }
    }
}

fn check_types(tickets: &[Ticket], by_number: &HashMap<i64, &Ticket>, closure: &HashMap<i64, HashSet<i64>>, errors: &mut Vec<String>) {
    for t in tickets {
        for c in BORROWED_TYPE.captures_iter(t.section("Types")) {
            let name = &c[1];
            let n: i64 = c[2].parse().unwrap_or(-1);
            let Some(source) = by_number.get(&n) else {
                errors.push(format!("{}: type {name} comes from {}, which does not exist", t.file, pad(n)));
                continue;
            };
            if !closure[&t.number].contains(&source.number) {
                errors.push(format!("{}: uses type {name} from {}, which does not block it", t.file, source.file));
            }
            let pattern = word(name);
            let defines = source.section("Types").lines().any(|line| !TYPE_MARKER.is_match(line) && pattern.is_match(line));
            if !defines {
                errors.push(format!("{}: type {name} is not defined in {}", t.file, source.file));
            }
        }
    }
}

struct Edges {
    ins: Vec<(String, String)>,
    outs: Vec<(String, String)>,
}

fn flow_edges(t: &Ticket) -> Edges {
    let flow = t.section("Flow");
    Edges {
        ins: FLOW_IN.captures_iter(flow).map(|c| (c[1].to_string(), c[2].to_string())).collect(),
        outs: FLOW_OUT.captures_iter(flow).map(|c| (c[1].to_string(), c[2].to_string())).collect(),
    }
}

fn check_flow(tickets: &[Ticket], by_number: &HashMap<i64, &Ticket>, closure: &HashMap<i64, HashSet<i64>>, errors: &mut Vec<String>) {
    let edges: HashMap<i64, Edges> = tickets.iter().map(|t| (t.number, flow_edges(t))).collect();
    let mut finals = 0;
    for t in tickets.iter().filter(|t| !t.no_contract) {
        let own = &edges[&t.number];
        if own.ins.is_empty() && own.outs.is_empty() {
            errors.push(format!("{}: Flow has no \"- In:\" or \"- Out:\" lines", t.file));
        }
        for (name, from) in &own.ins {
            let Ok(n) = from.parse::<i64>() else { continue };
            match by_number.get(&n) {
                None => errors.push(format!("{}: In {name} from {}, which does not exist", t.file, pad(n))),
                Some(source) if !closure[&t.number].contains(&source.number) => {
                    errors.push(format!("{}: In {name} from {}, which does not block it", t.file, source.file));
                }
                Some(source) => {
                    let matched = edges[&source.number].outs.iter().any(|(o, to)| o == name && to.parse::<i64>().ok() == Some(t.number));
                    if !matched {
                        errors.push(format!(
                            "{}: In {name} from {}, but that ticket has no matching \"Out: `{name}` → {}\"",
                            t.file,
                            source.file,
                            pad(t.number)
                        ));
                    }
                }
            }
        }
        for (name, to) in &own.outs {
            if to == "final" {
                finals += 1;
                continue;
            }
            let n: i64 = to.parse().unwrap_or(-1);
            match by_number.get(&n) {
                None => errors.push(format!("{}: Out {name} to {}, which does not exist", t.file, pad(n))),
                Some(target) => {
                    let matched = edges[&target.number].ins.iter().any(|(i, from)| i == name && from.parse::<i64>().ok() == Some(t.number));
                    if !matched {
                        errors.push(format!(
                            "{}: Out {name} to {}, but that ticket has no matching \"In: `{name}` ← {}\"",
                            t.file,
                            target.file,
                            pad(t.number)
                        ));
                    }
                }
            }
        }
    }
    if tickets.iter().any(|t| !t.no_contract) && finals == 0 {
        errors.push("issues/: no ticket has an \"Out: ... → final\" line, so no flow reaches the user".into());
    }
}

struct TreeEntry {
    path: String,
    tickets: Vec<i64>,
}

/// Parses the Files tree into full paths. Indentation is measured in characters, so box-drawing
/// prefixes and plain spaces nest the same way.
fn file_tree(files: &str) -> Option<Vec<TreeEntry>> {
    let block = CODE_BLOCK.captures(files)?;
    let mut entries = Vec::new();
    let mut parents: Vec<(usize, String)> = Vec::new();
    for line in block[1].lines() {
        let Some(c) = TREE_LINE.captures(line) else { continue };
        let column = c[1].chars().count();
        while parents.last().is_some_and(|(col, _)| *col >= column) {
            parents.pop();
        }
        let full = format!("{}{}", parents.last().map(|(_, p)| p.as_str()).unwrap_or(""), &c[2]);
        let tickets = TREE_TAG
            .captures(line)
            .map(|t| t[1].split(',').filter_map(|n| n.trim().parse().ok()).collect())
            .unwrap_or_default();
        entries.push(TreeEntry { path: full.trim_end_matches('/').to_string(), tickets });
        if c[2].ends_with('/') {
            parents.push((column, full));
        }
    }
    Some(entries)
}

fn check_overview(overview: &Ticket, tickets: &[Ticket], by_number: &HashMap<i64, &Ticket>, errors: &mut Vec<String>) {
    let f = &overview.file;
    for name in OVERVIEW_SECTIONS {
        match overview.sections.get(name) {
            None => errors.push(format!("{f}: missing \"## {name}\" section")),
            Some(body) if body.trim().is_empty() => errors.push(format!("{f}: \"## {name}\" is empty")),
            Some(_) => {}
        }
    }
    if overview.fields.get("Status").is_some_and(|s| s.contains("ready-for-agent")) {
        errors.push(format!("{f}: the overview is a map, not work, so it is never ready-for-agent"));
    }
    let order = overview.section("Order");
    for t in tickets {
        if !word(&pad(t.number)).is_match(order) {
            errors.push(format!("{f}: Order does not name ticket {}", pad(t.number)));
        }
    }
    let Some(entries) = file_tree(overview.section("Files")) else {
        if overview.sections.contains_key("Files") {
            errors.push(format!("{f}: Files has no tree in a code block"));
        }
        return;
    };
    for e in &entries {
        for n in &e.tickets {
            if !by_number.contains_key(n) {
                errors.push(format!("{f}: {} names ticket {}, which does not exist", e.path, pad(*n)));
            }
        }
    }
    for t in tickets {
        if !entries.iter().any(|e| e.tickets.contains(&t.number)) {
            errors.push(format!("{f}: ticket {} touches no file in Files", pad(t.number)));
        }
        for line in WHERE_LINE.captures_iter(t.section("Functions")) {
            for quoted in BACKTICKED.captures_iter(&line[1]) {
                let where_ = &quoted[1];
                let wanted = where_.trim_end_matches('/');
                if !wanted.contains('/') && !wanted.contains('.') {
                    continue;
                }
                // A folder Where matches the files under it.
                let hits: Vec<&TreeEntry> = entries
                    .iter()
                    .filter(|e| {
                        e.path == wanted
                            || e.path.ends_with(&format!("/{wanted}"))
                            || e.path.starts_with(&format!("{wanted}/"))
                            || e.path.contains(&format!("/{wanted}/"))
                    })
                    .collect();
                if hits.is_empty() {
                    errors.push(format!("{}: Where `{where_}` is not in {f}'s Files tree", t.file));
                } else if !hits.iter().any(|e| e.tickets.contains(&t.number)) {
                    errors.push(format!("{}: Where `{where_}` is in {f}'s Files tree without [{}]", t.file, pad(t.number)));
                }
            }
        }
    }
}
