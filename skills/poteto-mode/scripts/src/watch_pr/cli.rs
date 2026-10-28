use super::github::{discover_stack, resolve_context, GhReader, ProcessRunner};
use super::policy::{run_queued, run_simple, status_query_payload, Stamp};
use super::render::{render_json, render_pretty};
use super::types::*;
use serde_json::Value;
use std::io::{self, Write};
use std::time::{Duration, Instant};

const HELP:&str="Usage: watch-pr [options]\n\nWatch one pull request, a connected stack, or an immutable queued stack.\nJSON (NDJSON while polling) is the default; --pretty renders human text.\n\nOptions:\n  --owner <owner>             GitHub repository owner\n  --repo <repo>               GitHub repository name\n  --pr <number>               pull request number\n  --stack                     watch the connected open stack\n  --queued-stack              watch the captured stack until all PRs merge\n  --stack-prs <n,...>          frozen bottom-to-top queue (queued mode only)\n  --interval <seconds>        poll interval (default: 60)\n  --sweep-interval <seconds>  whole-stack sweep interval (default: 300)\n  --timeout <seconds>         deadline; 0 disables it (default: 0)\n  --max-query-errors <count>  consecutive query-error budget (default: 5)\n  --status-only               print one status table and exit 0\n  --allow-draft               do not treat a draft as a merge gate\n  --pretty                    render human text instead of JSON\n  -h, --help                  display help for command\n";
#[derive(Debug)]
pub struct Options {
    pub owner: Option<String>,
    pub repo: Option<String>,
    pub pr: Option<u64>,
    pub mode: Mode,
    pub stack_prs: Vec<u64>,
    pub status_only: bool,
    pub pretty: bool,
    pub polling: PollingOptions,
}
fn js_number(value: &str) -> Result<f64, ()> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(0.0);
    }
    for (prefix, base) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = value.strip_prefix(prefix) {
            return u64::from_str_radix(digits, base)
                .map(|n| n as f64)
                .map_err(|_| ());
        }
    }
    value.parse().map_err(|_| ())
}
pub fn parse_pr(value: &str) -> Result<u64, String> {
    positive_integer(value.strip_prefix('#').unwrap_or(value))
}
fn positive_integer(value: &str) -> Result<u64, String> {
    let n = js_number(value).map_err(|_| "must be a positive integer".to_string())?;
    if !n.is_finite() || n <= 0.0 || n.fract() != 0.0 || n >= u64::MAX as f64 {
        return Err("must be a positive integer".into());
    }
    Ok(n as u64)
}
fn parse_stack(value: &str) -> Result<Vec<u64>, String> {
    let values = value
        .split(',')
        .map(|p| parse_pr(p.trim()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = std::collections::HashSet::new();
    if values.iter().any(|n| !seen.insert(*n)) {
        return Err("contains a duplicate PR".into());
    }
    Ok(values)
}
fn positive(value: &str, allow_zero: bool) -> Result<f64, String> {
    let n = js_number(value).unwrap_or(f64::NAN);
    if !n.is_finite() || n < 0.0 || (!allow_zero && n == 0.0) {
        Err(if allow_zero {
            "must be zero or greater"
        } else {
            "must be greater than zero"
        }
        .into())
    } else {
        Ok(n)
    }
}
pub fn parse_args(argv: &[String]) -> Result<Option<Options>, String> {
    let mut options = Options {
        owner: None,
        repo: None,
        pr: None,
        mode: Mode::Single,
        stack_prs: vec![],
        status_only: false,
        pretty: false,
        polling: PollingOptions {
            interval: 60.0,
            sweep_interval: 300.0,
            timeout: 0.0,
            max_query_errors: 5,
            allow_draft: false,
        },
    };
    let mut stack = false;
    let mut queued = false;
    let mut explicit_stack = false;
    let mut i = 0;
    while i < argv.len() {
        let arg = &argv[i];
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
        let takes_value = matches!(
            name,
            "--owner"
                | "--repo"
                | "--pr"
                | "--stack-prs"
                | "--interval"
                | "--sweep-interval"
                | "--timeout"
                | "--max-query-errors"
        );
        let value = if takes_value {
            if let Some(value) = inline {
                Some(value)
            } else {
                i += 1;
                Some(
                    argv.get(i)
                        .filter(|v| !v.starts_with("--"))
                        .ok_or_else(|| format!("error: option '{name}' argument missing\n"))?
                        .as_str(),
                )
            }
        } else {
            None
        };
        let invalid = |reason: String| {
            format!(
                "error: option '{} <{}>' argument '{}' is invalid. {}\n",
                name,
                match name {
                    "--pr" => "number",
                    "--max-query-errors" => "count",
                    "--stack-prs" => "n,...",
                    _ => "seconds",
                },
                value.unwrap_or(""),
                reason
            )
        };
        match name {
            "--owner" => options.owner = Some(value.unwrap().into()),
            "--repo" => options.repo = Some(value.unwrap().into()),
            "--pr" => options.pr = Some(parse_pr(value.unwrap()).map_err(invalid)?),
            "--stack-prs" => {
                options.stack_prs = parse_stack(value.unwrap()).map_err(invalid)?;
                explicit_stack = true;
            }
            "--interval" => {
                options.polling.interval = positive(value.unwrap(), false).map_err(invalid)?
            }
            "--sweep-interval" => {
                options.polling.sweep_interval = positive(value.unwrap(), false).map_err(invalid)?
            }
            "--timeout" => {
                options.polling.timeout = positive(value.unwrap(), true).map_err(invalid)?
            }
            "--max-query-errors" => {
                options.polling.max_query_errors =
                    positive_integer(value.unwrap()).map_err(invalid)?
            }
            "--stack" if inline.is_none() => stack = true,
            "--queued-stack" if inline.is_none() => queued = true,
            "--status-only" if inline.is_none() => options.status_only = true,
            "--pretty" if inline.is_none() => options.pretty = true,
            "--allow-draft" if inline.is_none() => options.polling.allow_draft = true,
            "--help" | "-h" if inline.is_none() => return Ok(None),
            "--" if i + 1 == argv.len() => break,
            _ => return Err(format!("error: unknown option '{arg}'\n")),
        }
        i += 1;
    }
    if stack && queued {
        return Err("error: option '--stack' cannot be used with option '--queued-stack'\n".into());
    }
    if explicit_stack && !queued {
        return Err("error: --stack-prs requires --queued-stack\n".into());
    }
    options.mode = if queued {
        Mode::QueuedStack
    } else if stack {
        Mode::Stack
    } else {
        Mode::Single
    };
    Ok(Some(options))
}
pub fn main_with(
    argv: &[String],
    reader: &mut dyn Reader,
    clock: &mut dyn Clock,
    stdout: &mut dyn FnMut(String),
    stderr: &mut dyn FnMut(String),
) -> i32 {
    let options = match parse_args(argv) {
        Ok(Some(options)) => options,
        Ok(None) => {
            stdout(HELP.into());
            return 0;
        }
        Err(error) => {
            stderr(error);
            return 64;
        }
    };
    let render: fn(&Value) -> String = if options.pretty {
        render_pretty
    } else {
        render_json
    };
    let contexts = (|| {
        let seed = resolve_context(
            reader,
            options.owner.as_deref(),
            options.repo.as_deref(),
            options.pr.or_else(|| options.stack_prs.first().copied()),
        )?;
        if !options.stack_prs.is_empty() {
            Ok(options
                .stack_prs
                .iter()
                .map(|n| seed.with_number(*n))
                .collect())
        } else if options.mode == Mode::Single {
            Ok(vec![seed])
        } else {
            discover_stack(reader, &seed)
        }
    })();
    let contexts = match contexts {
        Ok(c) => c,
        Err(failure) => {
            if failure.kind == "command-spawn" {
                stderr(format!("{}\n", failure.detail));
                return 1;
            }
            let verdict = Stamp::new(options.mode).stamp(clock, status_query_payload(1, failure));
            stdout(render(&verdict));
            return 7;
        }
    };
    let verdict = {
        let mut emit = |v| stdout(render(&v));
        if options.mode == Mode::QueuedStack && !options.status_only {
            run_queued(reader, clock, &mut emit, &contexts, &options.polling)
        } else {
            run_simple(
                reader,
                clock,
                &mut emit,
                &contexts,
                options.mode,
                options.status_only,
                &options.polling,
            )
        }
    };
    // Spawn failures were uncaught runtime errors in the original CLI, not
    // retryable GitHub query failures. Keep their stderr-only exit-1 contract.
    if verdict["blocker"]["failure"]["kind"] == "command-spawn" {
        stderr(format!(
            "{}\n",
            verdict["blocker"]["failure"]["detail"]
                .as_str()
                .unwrap_or("could not run command")
        ));
        return 1;
    }
    stdout(render(&verdict));
    verdict["exitCode"].as_i64().unwrap_or(7) as i32
}
struct RealClock {
    started: Instant,
}
impl Clock for RealClock {
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }
    fn observed_at(&self) -> String {
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }
    fn sleep(&mut self, seconds: f64) {
        std::thread::sleep(Duration::from_secs_f64(seconds));
    }
}
pub fn main() -> i32 {
    let argv = std::env::args().skip(1).collect::<Vec<_>>();
    let mut reader = GhReader {
        runner: ProcessRunner,
    };
    let mut clock = RealClock {
        started: Instant::now(),
    };
    main_with(
        &argv,
        &mut reader,
        &mut clock,
        &mut |s| {
            let _ = io::stdout().write_all(s.as_bytes());
        },
        &mut |s| {
            let _ = io::stderr().write_all(s.as_bytes());
        },
    )
}
