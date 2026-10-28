use super::store::*;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const HELP: &str = "Usage: orch [--store <dir>] [--json] [--force] <command>\n\nPlain-file orchestrate bookkeeping\n\nOptions:\n  --store <dir>  store directory (or ORCH_STORE)\n  --json         print complete rows as JSON\n  --force        steal an existing store lock\n  -h, --help     display help for command\n\nCommands:\n  init           initialize the store\n  unit           manage work units\n  ledger         manage verification records\n  inbox          manage agent pointers\n  gate           manage decision gates\n  frontier       manage the stack frontier\n  status         render status.md and print a summary\n  standing       manage standing orders\n  help [command] display help for command\n";

pub(super) struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}
struct Parsed {
    store: Option<String>,
    json: bool,
    force: bool,
    command: Vec<String>,
    options: BTreeMap<String, String>,
    positionals: Vec<String>,
    help: bool,
}
struct Render {
    value: Value,
    compact: String,
}
impl Render {
    fn new(value: impl Serialize, compact: String) -> Result<Self> {
        Ok(Self {
            value: serde_json::to_value(value)?,
            compact,
        })
    }
}
fn argument_number(value: &str) -> Result<u64> {
    if value.is_empty() || value.starts_with('0') || !value.bytes().all(|v| v.is_ascii_digit()) {
        return Err(Error::user("must be a positive integer"));
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|v| *v <= 9_007_199_254_740_991)
        .ok_or_else(|| Error::user("must be a positive integer"))
}
fn pr_list(value: &str) -> Result<Vec<u64>> {
    let parts: Vec<_> = value.split(',').collect();
    if parts.iter().any(|v| v.is_empty()) {
        return Err(Error::user("requires a comma-separated PR list"));
    }
    parts.iter().map(|v| argument_number(v)).collect()
}
fn help(command: &[String]) -> String {
    if command.is_empty() {
        return HELP.into();
    }
    let group = command[0].as_str();
    let leaf = command.get(1).map(String::as_str).unwrap_or("");
    if group == "frontier" && leaf == "set" {
        return "Usage: orch frontier set [options]\n\nresolve the pull request base chain and set the frontier\n\nOptions:\n  --repo <dir>     repository directory (or ORCH_REPO)\n  --forge <forge>  pull request forge (choices: gh, origin; default: gh)\n  --prs <n,...>    optional expected pull request order pin\n  -h, --help       display help for command\n".into();
    }
    let children = match group {
        "unit" => "add <id>\n  set <id>\n  get <id>\n  list\n  counts",
        "ledger" => "record <pr> <sha> <verdict>\n  check <pr> <sha>\n  summary",
        "inbox" => "push <agent> <unit> <status>\n  drain\n  count",
        "gate" => "park <id>\n  list\n  resolve <id>",
        "frontier" => "set\n  show",
        "standing" => "show\n  add <line>",
        _ => "",
    };
    if leaf.is_empty() && !children.is_empty() {
        return format!("Usage: orch {group} [command]\n\nCommands:\n  {children}\n");
    }
    let (_, _, required, allowed) =
        specification(command).unwrap_or((0, 0, Vec::new(), Vec::new()));
    let mut text = format!("Usage: orch {} [options]\n\nOptions:\n", command.join(" "));
    for name in allowed {
        if name == "peek" {
            text.push_str("  --peek  read without draining\n");
        } else {
            text.push_str(&format!(
                "  --{name} <{}>{}\n",
                option_label(name),
                if required.contains(&name) {
                    " (required)"
                } else {
                    ""
                }
            ));
        }
    }
    text.push_str("  -h, --help  display help for command\n");
    text
}
fn option_label(name: &str) -> &str {
    match name {
        "brief" | "evidence" | "report" => "path",
        "pr" => "number",
        "default" | "answer" => "answer",
        "repo" => "dir",
        "prs" => "n,...",
        _ => name,
    }
}
fn specification(
    command: &[String],
) -> Option<(usize, usize, Vec<&'static str>, Vec<&'static str>)> {
    let names: Vec<_> = command.iter().map(String::as_str).collect();
    let (count, required, allowed): (usize, &[&str], &[&str]) = match names.as_slice() {
        ["init"] | ["status"] => (0, &[], &[]),
        ["unit", "add"] => (1, &["track"], &["track", "brief"]),
        ["unit", "set"] => (1, &["state"], &["state", "branch", "pr", "sha"]),
        ["unit", "get"] => (1, &[], &[]),
        ["unit", "list"] => (0, &[], &["state", "track"]),
        ["unit", "counts"]
        | ["ledger", "summary"]
        | ["inbox", "count"]
        | ["gate", "list"]
        | ["frontier", "show"]
        | ["standing", "show"] => (0, &[], &[]),
        ["ledger", "record"] => (3, &["evidence"], &["evidence", "verifier"]),
        ["ledger", "check"] => (2, &[], &[]),
        ["inbox", "push"] => (3, &[], &["report"]),
        ["inbox", "drain"] => (0, &[], &["peek"]),
        ["gate", "park"] => (
            1,
            &["question", "options", "default"],
            &["question", "options", "default"],
        ),
        ["gate", "resolve"] => (1, &["answer"], &["answer"]),
        ["frontier", "set"] => (0, &[], &["repo", "forge", "prs"]),
        ["standing", "add"] => (1, &[], &[]),
        _ => return None,
    };
    Some((count, count, required.to_vec(), allowed.to_vec()))
}
fn parse(args: &[String], env_store: Option<String>, parsed: &mut Parsed) -> Result<()> {
    parsed.store = env_store;
    let mut index = 0;
    let mut positional_only = false;
    while index < args.len() {
        let arg = &args[index];
        index += 1;
        if !positional_only && arg == "--" {
            positional_only = true;
            continue;
        }
        if !positional_only && matches!(arg.as_str(), "--help" | "-h") {
            parsed.help = true;
            continue;
        }
        if !positional_only && arg.starts_with('-') && arg != "-" {
            let (name, inline) = arg
                .split_once('=')
                .map(|(name, value)| (name, Some(value)))
                .unwrap_or((arg.as_str(), None));
            if matches!(name, "--json" | "--force" | "--peek") {
                if inline.is_some() {
                    return Err(Error::usage(format!(
                        "option '{name}' does not take a value"
                    )));
                }
                match name {
                    "--json" => parsed.json = true,
                    "--force" => parsed.force = true,
                    _ => {
                        parsed.options.insert("peek".into(), String::new());
                    }
                }
                continue;
            }
            let key = name.strip_prefix("--").unwrap_or(name);
            let global = key == "store";
            let allowed =
                specification(&parsed.command).is_some_and(|(_, _, _, opts)| opts.contains(&key));
            if !global && !allowed {
                return Err(Error::usage(format!("unknown option '{name}'")));
            }
            let value = if let Some(value) = inline {
                value.to_string()
            } else {
                let value = args.get(index).ok_or_else(|| {
                    Error::usage(format!(
                        "option '{name} <{}>' argument missing",
                        option_label(key)
                    ))
                })?;
                index += 1;
                value.clone()
            };
            if global {
                parsed.store = Some(value);
            } else {
                parsed.options.insert(key.into(), value);
            }
            continue;
        }
        if !positional_only && parsed.command.is_empty() {
            if arg == "help" {
                parsed.help = true;
                continue;
            }
            if !matches!(
                arg.as_str(),
                "init" | "unit" | "ledger" | "inbox" | "gate" | "frontier" | "status" | "standing"
            ) {
                return Err(Error::usage(format!("unknown command '{arg}'")));
            }
            parsed.command.push(arg.clone());
        } else if !positional_only
            && parsed.command.len() == 1
            && !matches!(parsed.command[0].as_str(), "init" | "status")
        {
            parsed.command.push(arg.clone());
            if specification(&parsed.command).is_none() {
                return Err(Error::usage(format!("unknown command '{arg}'")));
            }
        } else {
            parsed.positionals.push(arg.clone());
        }
    }
    if parsed.help {
        return Ok(());
    }
    if let Some((min, max, required, allowed)) = specification(&parsed.command) {
        for name in parsed.options.keys() {
            if !allowed.contains(&name.as_str()) {
                return Err(Error::usage(format!("unknown option '--{name}'")));
            }
        }
        if parsed.positionals.len() < min {
            return Err(Error::usage("missing required argument"));
        }
        if parsed.positionals.len() > max {
            return Err(Error::usage("too many arguments"));
        }
        for name in required {
            if !parsed.options.contains_key(name) {
                return Err(Error::usage(format!(
                    "required option '--{name} <{}>' not specified",
                    option_label(name)
                )));
            }
        }
        if let Some(pr) = parsed.options.get("pr") {
            argument_number(pr)?;
        }
        if parsed.command.first().is_some_and(|s| s == "ledger")
            && parsed
                .command
                .get(1)
                .is_some_and(|s| s == "record" || s == "check")
        {
            argument_number(&parsed.positionals[0])?;
        }
        if parsed.command == ["ledger", "record"] {
            parse_verdict(&parsed.positionals[2])?;
        }
        if let Some(prs) = parsed.options.get("prs") {
            pr_list(prs)?;
        }
        if let Some(forge) = parsed.options.get("forge") {
            if parse_forge(forge).is_err() {
                return Err(Error::usage(format!("option '--forge <forge>' argument '{forge}' is invalid. Allowed choices are gh, origin.")));
            }
        }
    }
    Ok(())
}
fn compact_rows<T>(
    rows: &[T],
    format: impl Fn(&T) -> String,
    empty: &str,
    unlimited: bool,
) -> String {
    if rows.is_empty() {
        return empty.into();
    }
    let limit = if unlimited { rows.len() } else { 4 };
    let mut lines: Vec<_> = rows.iter().take(limit).map(format).collect();
    if rows.len() > limit {
        lines.push(format!("... {} more; use --json", rows.len() - limit));
    }
    lines.join("\n")
}
fn frontier_line(frontier: &Frontier) -> String {
    let prs = if frontier.prs.is_empty() {
        "none".into()
    } else {
        frontier
            .prs
            .iter()
            .map(|r| format!("{}#{}@{}:{}", r.branches, r.pr, r.sha, r.state))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "generation={} prs={} lowest-unmerged={}",
        frontier.generation,
        prs,
        frontier
            .lowest_unmerged
            .map(|n| n.to_string())
            .unwrap_or_else(|| "none".into())
    )
}
fn operation(store: &mut Store, parsed: &Parsed, env_repo: Option<&str>) -> Result<Render> {
    let p = &parsed.positionals;
    let option = |name: &str| parsed.options.get(name).map(String::as_str);
    let required = |name: &str| option(name).expect("required options checked before dispatch");
    let command: Vec<_> = parsed.command.iter().map(String::as_str).collect();
    match command.as_slice() {
        ["init"] => {
            let result = store.init()?;
            Render::new(
                &result,
                format!("initialized {}", store.directory.display()),
            )
        }
        ["unit", "add"] => {
            let row = store.unit_add(&p[0], required("track"), option("brief"))?;
            Render::new(&row, row.cells().join("\t"))
        }
        ["unit", "set"] => {
            let row = store.unit_set(
                &p[0],
                required("state"),
                option("branch"),
                option("pr").map(argument_number).transpose()?,
                option("sha"),
            )?;
            Render::new(&row, row.cells().join("\t"))
        }
        ["unit", "get"] => {
            let row = store.unit_get(&p[0])?;
            Render::new(&row, row.cells().join("\t"))
        }
        ["unit", "list"] => {
            let rows = store.unit_list(option("state"), option("track"))?;
            Render::new(
                &rows,
                compact_rows(&rows, |r| r.cells().join("\t"), "(no units)", false),
            )
        }
        ["unit", "counts"] => {
            let counts = store.unit_counts()?;
            Render::new(&counts, count_line(&counts))
        }
        ["ledger", "record"] => {
            let row = store.ledger_record(
                argument_number(&p[0])?,
                &p[1],
                &p[2],
                required("evidence"),
                option("verifier"),
            )?;
            Render::new(&row, format!("{}\t{}\t{}", row.pr, row.sha, row.verdict))
        }
        ["ledger", "check"] => {
            let row = store.ledger_check(argument_number(&p[0])?, &p[1])?;
            Render::new(&row, row.verdict.clone())
        }
        ["ledger", "summary"] => {
            let counts = store.ledger_summary()?;
            Render::new(&counts, count_line(&counts))
        }
        ["inbox", "push"] => {
            let (pointer, filename) = store.inbox_push(&p[0], &p[1], &p[2], option("report"))?;
            Render::new(
                &pointer,
                format!("{}\t{}\t{}", pointer.unit, pointer.status, filename),
            )
        }
        ["inbox", "drain"] => {
            let rows = if option("peek").is_some() {
                store.inbox_peek()?
            } else {
                store.inbox_drain()?
            };
            Render::new(
                &rows,
                compact_rows(&rows, |r| r.cells().join("\t"), "(empty)", true),
            )
        }
        ["inbox", "count"] => {
            let count = store.inbox_peek()?.len();
            Render::new(json!({"count": count}), count.to_string())
        }
        ["gate", "park"] => {
            let gate = store.gate_park(
                &p[0],
                required("question"),
                required("options"),
                required("default"),
            )?;
            Render::new(&gate, format!("{}\topen", gate.id))
        }
        ["gate", "list"] => {
            let rows = store.gate_list()?;
            Render::new(
                &rows,
                compact_rows(
                    &rows,
                    |g| {
                        [&g.id, &g.question, &g.options, &g.default_answer]
                            .map(String::as_str)
                            .join("\t")
                    },
                    "(no open gates)",
                    false,
                ),
            )
        }
        ["gate", "resolve"] => {
            let gate = store.gate_resolve(&p[0], required("answer"))?;
            Render::new(
                &gate,
                format!(
                    "{}\tresolved\t{}",
                    gate.id,
                    gate.answer.as_deref().unwrap_or("")
                ),
            )
        }
        ["frontier", "set"] => {
            let repo = option("repo")
                .or(env_repo)
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| Error::usage("set --repo <dir> or ORCH_REPO"))?;
            let prs = option("prs").map(pr_list).transpose()?;
            let frontier =
                store.frontier_set(repo, option("forge").unwrap_or("gh"), prs.as_deref())?;
            Render::new(&frontier, frontier_line(&frontier))
        }
        ["frontier", "show"] => {
            let frontier = store.frontier_show()?;
            Render::new(&frontier, frontier_line(&frontier))
        }
        ["standing", "show"] => {
            let rows = store.standing_show()?;
            Render::new(
                &rows,
                compact_rows(
                    &rows,
                    |r| format!("{}. {}", r.number, r.line),
                    "(no standing orders)",
                    false,
                ),
            )
        }
        ["standing", "add"] => {
            let row = store.standing_add(&p[0])?;
            Render::new(&row, format!("{}. {}", row.number, row.line))
        }
        ["status"] => {
            let report = store.status_render()?;
            let summary = &report.summary;
            let visible: Vec<_> = summary
                .open_gate_ids
                .iter()
                .take(4)
                .map(String::as_str)
                .collect();
            let more = if summary.open_gate_ids.len() > 4 {
                format!(",+{} more", summary.open_gate_ids.len() - 4)
            } else {
                String::new()
            };
            let ids = if visible.is_empty() {
                String::new()
            } else {
                format!("; ids={}{more}", visible.join(","))
            };
            let compact = format!(
                "counts: units={}; states={}; ledger={}\nchanged: {}\ngates open: {}{}",
                report.units.len(),
                count_line(&summary.unit_states),
                count_line(&summary.ledger_verdicts),
                report.changed,
                summary.open_gate_ids.len(),
                ids
            );
            Render::new(&report, compact)
        }
        _ => Err(Error::usage("a valid command is required")),
    }
}
pub(super) fn run(args: &[String], env_store: Option<String>, env_repo: Option<String>) -> Output {
    let mut parsed = Parsed {
        store: None,
        json: false,
        force: false,
        command: Vec::new(),
        options: BTreeMap::new(),
        positionals: Vec::new(),
        help: false,
    };
    let mut output = Output {
        code: 0,
        stdout: String::new(),
        stderr: String::new(),
    };
    let result = (|| -> Result<Option<Render>> {
        parse(args, env_store, &mut parsed)?;
        if parsed.help {
            output.stdout = help(&parsed.command);
            return Ok(None);
        }
        let directory = parsed
            .store
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| Error::usage("set --store <dir> or ORCH_STORE"))?;
        let mut store = Store::open(directory, parsed.force)?;
        let result = operation(&mut store, &parsed, env_repo.as_deref());
        output.stderr.push_str(&store.warnings.concat());
        // Like the original finally block, a close failure takes precedence.
        store.close()?;
        result.map(Some)
    })();
    match result {
        Ok(Some(result)) => {
            output.stdout = if parsed.json {
                serde_json::to_string_pretty(&result.value).expect("JSON value serializes")
            } else {
                result.compact
            };
            if !output.stdout.ends_with('\n') {
                output.stdout.push('\n');
            }
        }
        Ok(None) => {}
        Err(error) => {
            output.code = error.code;
            if let Some(value) = error.output {
                output.stdout = if parsed.json {
                    serde_json::to_string_pretty(&value).expect("JSON value serializes")
                } else {
                    error.message
                };
                output.stdout.push('\n');
            } else {
                output
                    .stderr
                    .push_str(&format!("error: {}\n", error.message));
                if error.usage {
                    output.stderr.push_str(HELP);
                }
            }
        }
    }
    output
}
