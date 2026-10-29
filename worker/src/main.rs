//! The worker verifies commits in the background and checks their results before a push.
//! It runs each check as a separate job and sets concurrency from available CPU and memory.
//!
//!   worker probe                 report CPU count, CPU load, available memory, and job limits
//!   worker init --check "<cmd>" [--check ...] [--copy <path>] [--parallel N] [--commits N] [--job-mem-mb N]
//!               [--timeout-ms N] [--base <ref>]
//!                                A check containing {shard} runs once per shard.
//!                                The worker replaces {shard} and {shards} with the shard index and count.
//!                                It replaces {slot} and exports XSTACK_WORKER_SLOT so checks keep separate state.
//!                                Background jobs get 1 to commits times the expanded check count, `run` gets
//!                                run1 to run<cores>, and `fast` gets fast<pid>-<n>, so the three never collide.
//!                                Without --parallel, the worker adjusts concurrency to available resources.
//!   worker enqueue [<ref>]       queue a commit, start the worker, and print the unverified commit count
//!                                The default ref is HEAD.
//!   worker status <base>         show one line per commit in base..HEAD
//!   worker wait [<ref>]          block until a result lands, then exit with 0 for a pass or 1 for a failure;
//!                                run it in the background, above unverified depth 3, or after `GATE BLOCKED`
//!   worker gate <base> [--each]  exit with 0 only if HEAD passed, or every commit passed with --each
//!   worker fast --check "<cmd>" [--check ...]
//!                                Run all checks in the foreground. The first failure stops the rest.
//!   worker run --check "<cmd>"   run a command in the foreground using a shared CPU slot
//!   worker split [-j N] [--each] [--keep-going] -- <cmd> [args...]
//!                                Read items from stdin and run size-balanced batches of at most 32 items.
//!                                Idle workers take half another worker's unstarted items.
//!                                With --each, run once per item and replace {} with the item.
//!   worker diff-budget [--base <ref>] [--max N] [--exclude <path or suffix>]
//!                                Exit with 1 if the number of changed lines exceeds the review budget.
//!   worker drain                 process the queue, started by enqueue, gate, and wait
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use xstack_worker::budget::{self, DEFAULT_MAX_LINES};
use xstack_worker::git::short;
use xstack_worker::probe::{Meter, DEFAULT_JOB_MEM_MB, RESERVE};
use xstack_worker::queue::{self, InitOptions, Row};
use xstack_worker::run::{self, DEFAULT_TIMEOUT_MS};
use xstack_worker::scheduler;
use xstack_worker::state::{CheckResult, Status};
use xstack_worker::Result;

#[derive(Default)]
struct Args {
    positional: Vec<String>,
    check: Vec<String>,
    copy: Vec<String>,
    exclude: Vec<String>,
    timeout_ms: Option<u64>,
    parallel: Option<usize>,
    commits: Option<usize>,
    job_mem_mb: Option<u64>,
    jobs: Option<usize>,
    max: Option<u64>,
    base: Option<String>,
    repo: Option<PathBuf>,
    each: bool,
    keep_going: bool,
    rest: Vec<String>,
}

fn number<T: std::str::FromStr>(flag: &str, value: Option<String>) -> Result<T> {
    value
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| format!("{flag} needs a number"))
}

fn parse(argv: Vec<String>) -> Result<Args> {
    let mut args = Args::default();
    let mut it = argv.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--check" => args.check.push(it.next().ok_or("--check needs a command")?),
            "--copy" => args.copy.push(it.next().ok_or("--copy needs a path")?),
            "--exclude" => args
                .exclude
                .push(it.next().ok_or("--exclude needs a path")?),
            "--timeout-ms" => args.timeout_ms = Some(number("--timeout-ms", it.next())?),
            "--parallel" => args.parallel = Some(number("--parallel", it.next())?),
            "--commits" => args.commits = Some(number("--commits", it.next())?),
            "--job-mem-mb" => args.job_mem_mb = Some(number("--job-mem-mb", it.next())?),
            "-j" | "--jobs" => args.jobs = Some(number("--jobs", it.next())?),
            "--max" => args.max = Some(number("--max", it.next())?),
            "--base" => args.base = Some(it.next().ok_or("--base needs a ref")?),
            "--repo" => args.repo = Some(PathBuf::from(it.next().ok_or("--repo needs a path")?)),
            "--each" => args.each = true,
            "--keep-going" => args.keep_going = true,
            "--" => {
                args.rest = it.by_ref().collect();
            }
            _ => args.positional.push(a),
        }
    }
    Ok(args)
}

fn print_rows(rows: &[Row]) {
    for row in rows {
        let cover = row
            .covered_by
            .as_ref()
            .map(|(sha, status)| format!(" (covered by {}: {})", short(sha), status.as_str()))
            .unwrap_or_default();
        println!(
            "{} {:<7} {}{}",
            short(&row.sha),
            row.status,
            row.subject,
            cover
        );
        print_checks(&row.checks);
    }
}

fn print_checks(checks: &[CheckResult]) {
    for check in checks {
        if check.exit != 0 {
            let note = if check.preexisting {
                " (also red at baseline)"
            } else {
                ""
            };
            println!("    FAIL {}{note}", check.cmd);
            for line in check.output.lines() {
                println!("      {line}");
            }
        } else if check.flaky {
            println!("    FLAKY {} (failed once, passed on rerun)", check.cmd);
        }
    }
}

fn exe() -> Result<PathBuf> {
    std::env::current_exe().map_err(|e| format!("cannot find own binary: {e}"))
}

fn main() -> ExitCode {
    let mut argv = std::env::args().skip(1);
    let command = argv.next().unwrap_or_default();
    match parse(argv.collect()).and_then(|args| dispatch(&command, args)) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("worker: {message}");
            ExitCode::from(2)
        }
    }
}

fn dispatch(command: &str, args: Args) -> Result<u8> {
    let repo = match &args.repo {
        Some(repo) => repo.clone(),
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let repo = repo.as_path();
    match command {
        "init" => {
            let entry = queue::init(
                repo,
                InitOptions {
                    checks: args.check,
                    copy: args.copy,
                    timeout_ms: args.timeout_ms,
                    base: args.base,
                    parallel: args.parallel,
                    commits: args.commits,
                    job_mem_mb: args.job_mem_mb,
                },
            )?;
            queue::start_worker(repo, &exe()?)?;
            println!(
                "initialized, baseline {} {}",
                short(&entry.sha),
                entry.status.as_str()
            );
            Ok(0)
        }
        "enqueue" => {
            let reference = args
                .positional
                .first()
                .map(String::as_str)
                .unwrap_or("HEAD");
            let entry = queue::enqueue(repo, reference, true, false)?;
            queue::start_worker(repo, &exe()?)?;
            let reused = entry
                .reused_from
                .as_ref()
                .map(|s| format!(" (same tree as {})", short(s)))
                .unwrap_or_default();
            let depth = queue::unverified_depth(repo)?;
            println!(
                "{} {}{reused}, unverified depth {depth}",
                short(&entry.sha),
                entry.status.as_str()
            );
            if depth > 3 {
                println!("unverified depth above 3: run `worker wait` on the oldest pending commit before the next task");
            } else {
                println!("verifying in the background: read the result with `worker status <base>`, not a foreground `worker wait`");
            }
            Ok(0)
        }
        "status" => {
            let base = args.positional.first().ok_or("status needs <base>")?;
            print_rows(&queue::status(repo, base)?);
            Ok(0)
        }
        "wait" => {
            let reference = args
                .positional
                .first()
                .map(String::as_str)
                .unwrap_or("HEAD");
            let result =
                queue::wait_for(repo, reference, None, Some(&exe()?))?.ok_or("no result")?;
            println!("{} {}", short(&result.sha), result.status.as_str());
            print_checks(&result.checks);
            Ok(if result.status == Status::Pass { 0 } else { 1 })
        }
        "gate" => {
            let base = args.positional.first().ok_or("gate needs <base>")?;
            let result = queue::gate(repo, base, args.each)?;
            if !result.queued.is_empty() {
                queue::start_worker(repo, &exe()?)?;
            }
            for problem in &result.problems {
                eprintln!("GATE {problem}");
            }
            println!("{}", if result.ok { "GATE OK" } else { "GATE BLOCKED" });
            Ok(if result.ok { 0 } else { 1 })
        }
        "fast" => {
            if args.check.is_empty() {
                return Err("fast needs at least one --check".into());
            }
            let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
            let result = run::fast(
                &cwd,
                &args.check,
                args.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS),
            );
            match result.first_failed.and_then(|i| result.results[i].as_ref()) {
                None => {
                    println!("FAST OK {} check(s) in {} ms", args.check.len(), result.ms);
                    Ok(0)
                }
                Some(failed) => {
                    eprintln!(
                        "FAST FAIL {} (exit {}) after {} ms\n{}",
                        failed.cmd, failed.exit, result.ms, failed.output
                    );
                    Ok(1)
                }
            }
        }
        "run" => {
            let [cmd] = args.check.as_slice() else {
                return Err("run needs exactly one --check".into());
            };
            Ok(queue::run_in_slot(repo, cmd)?.clamp(0, 255) as u8)
        }
        "split" => split(&args),
        "diff-budget" => {
            let max = args.max.unwrap_or(DEFAULT_MAX_LINES);
            let measured =
                budget::measure(repo, args.base.as_deref().unwrap_or("HEAD"), &args.exclude)?;
            if measured.total <= max {
                println!("DIFF OK {} line(s) changed (budget {max})", measured.total);
                return Ok(0);
            }
            println!("DIFF OVER {} line(s) changed (budget {max}). Split this into smaller reviewable results.", measured.total);
            for (path, count) in measured.files.iter().take(10) {
                println!("    {count:>6} {path}");
            }
            Ok(1)
        }
        "drain" => {
            scheduler::drain(repo)?;
            Ok(0)
        }
        "probe" => {
            let (_, power) = Meter::quick();
            let memory = power
                .mem_free_mb
                .map(|m| format!("{m} MB free"))
                .unwrap_or_else(|| "memory unknown".into());
            println!(
                "POWER {} core(s), {:.1} busy, {memory}",
                power.cores, power.busy
            );
            println!(
                "BURST background {} job(s) at low priority ({RESERVE} core kept for you), foreground {} job(s)",
                power.width(RESERVE, 0, args.job_mem_mb.unwrap_or(DEFAULT_JOB_MEM_MB)),
                power.width(0, 0, args.job_mem_mb.unwrap_or(DEFAULT_JOB_MEM_MB))
            );
            Ok(0)
        }
        _ => {
            eprintln!("usage: worker probe|init|enqueue|status|wait|gate|fast|run|split|diff-budget|drain (see the header of src/main.rs)");
            Ok(2)
        }
    }
}

fn split(args: &Args) -> Result<u8> {
    if args.rest.is_empty() {
        return Err("split needs a command after --".into());
    }
    let items: Vec<String> = io::stdin()
        .lock()
        .lines()
        .map_while(|l| l.ok())
        .filter(|l| !l.trim().is_empty())
        .collect();
    if items.is_empty() {
        println!("SPLIT OK nothing to run");
        return Ok(0);
    }
    // Without -j, choose concurrency from available CPU and memory.
    let workers = args
        .jobs
        .unwrap_or_else(|| {
            Meter::start()
                .1
                .width(0, 0, args.job_mem_mb.unwrap_or(DEFAULT_JOB_MEM_MB))
        })
        .max(1);
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let result = xstack_worker::split::run(
        Path::new(&cwd),
        &items,
        &args.rest,
        xstack_worker::split::SplitOptions {
            workers,
            each: args.each,
            fail_fast: !args.keep_going,
            timeout_ms: args.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS),
        },
    );
    let failed: Vec<&CheckResult> = match result.first_failed {
        None => Vec::new(),
        Some(first) if !args.keep_going => result.results[first].iter().collect(),
        Some(_) => result
            .results
            .iter()
            .flatten()
            .filter(|r| r.exit != 0)
            .collect(),
    };
    if failed.is_empty() {
        println!(
            "SPLIT OK {} job(s) over {} item(s) on {workers} core(s) in {} ms",
            result.results.len(),
            items.len(),
            result.ms
        );
        return Ok(0);
    }
    for f in &failed {
        eprintln!("SPLIT FAIL {} (exit {})\n{}", f.cmd, f.exit, f.output);
    }
    Ok(1)
}
