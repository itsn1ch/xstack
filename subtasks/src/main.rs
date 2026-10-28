//! Runs subtasks for an agent without any model call. The agent's own model call stays its only one;
//! everything mechanical fans out here, sharing the machine with the verification worker.
//!
//!   subtask probe                print this run's share of each pool right now
//!   subtask hot                  session start: warm the CPU sample, check that `subtask` and `worker` are
//!                                built from the current sources, and print one line for the agent's context.
//!                                Exit 1 with the rebuild command when a binary is missing or stale.
//!   subtask [--sh "<cmd>" | --http [--method M] [--save-dir D] | --grep <regex> | --hash | --lines]
//!           [--tasks <jsonl or item list>] [--out <jsonl>] [--keep-going] [--timeout-ms N] [--per-host N]
//!           [--retries N] [--mem-mb N] [--cpu N] [--net N] [--file N] [--show N]
//!                                Run one task per stdin item for a template flag, or one per JSONL line
//!                                ({"sh": ..}, {"argv": [..]}, {"browser": {"session": "name",
//!                                "commands": [["open", "URL"], ["snapshot", "-i"]]}},
//!                                {"http": {"url": ..}}, {"grep": {"path": .., "pattern": ..}},
//!                                {"hash": ..}, {"lines": ..}) otherwise.
//!                                Browser commands run sequentially, stopping on failure; each action
//!                                has the normal timeout. Same-session tasks in this run never overlap,
//!                                and a failed task prevents later reuse in the run. Session state is not
//!                                coordinated across processes; timed-out daemon actions may continue.
//!                                Browser sessions persist. Browser tasks share the CPU pool and --cpu cap.
//!                                A --sh command with {} runs once per item; without it, 32 items per process.
//!                                CPU, HTTP, and file tasks have separate pools. The cores are split evenly
//!                                with the verification worker and every other subtask run; the HTTP and file
//!                                pools are split across subtask runs. Results go to a JSONL file; stdout
//!                                prints only the summary and the first failures.
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use xstack_subtasks::engine::{self, Http, SubOptions, Task, Widths};
use xstack_subtasks::Result;
use xstack_worker::probe::Meter;
use xstack_worker::share::{Kind, Share};

#[derive(Default)]
struct Args {
    timeout_ms: Option<u64>,
    keep_going: bool,
    sh: Option<String>,
    http: bool,
    method: Option<String>,
    save_dir: Option<PathBuf>,
    grep: Option<String>,
    hash: bool,
    lines: bool,
    tasks: Option<PathBuf>,
    out: Option<PathBuf>,
    per_host: Option<usize>,
    retries: Option<u32>,
    mem_mb: Option<u64>,
    cpu: Option<usize>,
    net: Option<usize>,
    file: Option<usize>,
    show: Option<usize>,
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
            "--timeout-ms" => args.timeout_ms = Some(number("--timeout-ms", it.next())?),
            "--keep-going" => args.keep_going = true,
            "--sh" => args.sh = Some(it.next().ok_or("--sh needs a command")?),
            "--http" => args.http = true,
            "--method" => args.method = Some(it.next().ok_or("--method needs a method")?),
            "--save-dir" => {
                args.save_dir = Some(PathBuf::from(it.next().ok_or("--save-dir needs a path")?))
            }
            "--grep" => args.grep = Some(it.next().ok_or("--grep needs a pattern")?),
            "--hash" => args.hash = true,
            "--lines" => args.lines = true,
            "--tasks" => args.tasks = Some(PathBuf::from(it.next().ok_or("--tasks needs a path")?)),
            "--out" => args.out = Some(PathBuf::from(it.next().ok_or("--out needs a path")?)),
            "--per-host" => args.per_host = Some(number("--per-host", it.next())?),
            "--retries" => args.retries = Some(number("--retries", it.next())?),
            "--mem-mb" => args.mem_mb = Some(number("--mem-mb", it.next())?),
            "--cpu" => args.cpu = Some(number("--cpu", it.next())?),
            "--net" => args.net = Some(number("--net", it.next())?),
            "--file" => args.file = Some(number("--file", it.next())?),
            "--show" => args.show = Some(number("--show", it.next())?),
            _ => {
                return Err(format!(
                    "unknown argument {a} (see the header of src/main.rs)"
                ))
            }
        }
    }
    Ok(args)
}

fn main() -> ExitCode {
    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    let command = match argv.first().map(String::as_str) {
        Some("probe" | "hot") => Some(argv.remove(0)),
        _ => None,
    };
    let outcome = parse(argv).and_then(|args| match command.as_deref() {
        Some("probe") => self::probe(&args),
        Some("hot") => hot(),
        _ => run(args),
    });
    match outcome {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("subtask: {message}");
            ExitCode::from(2)
        }
    }
}

/// The xstack checkout this binary was built from.
/// The xstack checkout this binary belongs to: the one it sits in (`<root>/subtasks/target/release`), else the one it was built from.
fn root() -> PathBuf {
    let sits_in = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.parent()?.parent()?.to_path_buf()))
        .filter(|root| root.join("subtasks/Cargo.toml").is_file());
    sits_in.unwrap_or_else(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    })
}

/// A fresh results file in the temp dir, else in the working directory, else stdout above the summary when both are read-only.
fn default_out() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = format!("xstack-subtask-{}-{nanos}.jsonl", std::process::id());
    let temp = std::env::temp_dir().join(&name);
    if std::fs::File::create(&temp).is_ok() {
        return temp;
    }
    let here = PathBuf::from(format!(".{name}"));
    if std::fs::File::create(&here).is_ok() {
        return here;
    }
    PathBuf::from("/dev/stdout")
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    if !path.is_dir() {
        return path.metadata().and_then(|m| m.modified()).ok();
    }
    std::fs::read_dir(path)
        .ok()?
        .flatten()
        .filter_map(|entry| modified(&entry.path()))
        .max()
}

/// Why a binary needs a build, or `None` when it is newer than every source file of the crate.
fn stale(binary: &Path, krate: &Path) -> Option<&'static str> {
    let Some(built) = modified(binary) else {
        return Some("missing");
    };
    ["src", "Cargo.toml", "Cargo.lock"]
        .iter()
        .filter_map(|part| modified(&krate.join(part)))
        .any(|source| source > built)
        .then_some("stale")
}

fn hot() -> Result<u8> {
    let started = std::time::Instant::now();
    let root = root();
    let me = std::env::current_exe().map_err(|e| format!("cannot find own binary: {e}"))?;
    let worker = root.join("worker/target/release/worker");
    // A subtask build compiles the worker library too, so the worker sources count toward its freshness.
    let checks = [
        ("subtask", me.as_path(), root.join("subtasks")),
        ("subtask", me.as_path(), root.join("worker")),
        ("worker", worker.as_path(), root.join("worker")),
    ];
    let mut problems: Vec<String> = checks
        .iter()
        .filter_map(|(name, binary, krate)| stale(binary, krate).map(|why| format!("{name} {why}")))
        .collect();
    problems.dedup();
    if !problems.is_empty() {
        println!(
            "SUBTASK COLD {}: run `cargo build --release --manifest-path {} && cargo build --release --manifest-path {}` in the background, then `subtask hot` again",
            problems.join(", "),
            root.join("worker/Cargo.toml").display(),
            root.join("subtasks/Cargo.toml").display()
        );
        return Ok(1);
    }
    let (_, power) = Meter::quick();
    let runs = Share::join(Kind::Subtask).runs();
    let w = engine::widths(
        &power,
        runs,
        0,
        engine::DEFAULT_SH_MEM_MB,
        engine::raise_nofile(),
    );
    println!(
        "SUBTASK HOT {} (worker {}): the default for mechanical fan-out this session, per Subtasks in poteto-mode; per run now cpu {}, net {}, file {} on {} core(s), {} run(s) live; ready in {} ms",
        me.display(),
        worker.display(),
        w.cpu,
        w.net,
        w.file,
        power.cores,
        runs.all,
        started.elapsed().as_millis()
    );
    Ok(0)
}

fn probe(args: &Args) -> Result<u8> {
    let started = std::time::Instant::now();
    let (_, power) = Meter::quick();
    let runs = Share::join(Kind::Subtask).runs();
    let w = engine::widths(
        &power,
        runs,
        0,
        args.mem_mb.unwrap_or(engine::DEFAULT_SH_MEM_MB),
        engine::raise_nofile(),
    );
    println!(
        "SUBTASK per run: cpu {}, net {}, file {} ({} run(s) live, {} of them subtask runs; probed in {} ms)",
        w.cpu,
        w.net,
        w.file,
        runs.all,
        runs.same_kind,
        started.elapsed().as_millis()
    );
    Ok(0)
}

fn input_reader(source: Option<&Path>) -> Result<Box<dyn BufRead + Send>> {
    Ok(match source {
        Some(path) => Box::new(io::BufReader::new(
            std::fs::File::open(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?,
        )),
        None => Box::new(io::BufReader::new(io::stdin())),
    })
}

/// Streams the non-empty lines of the task file, or of stdin, without holding them.
fn read_lines(source: Option<&Path>) -> Result<Box<dyn Iterator<Item = String> + Send>> {
    let reader = input_reader(source)?;
    Ok(Box::new(
        reader
            .lines()
            .map_while(|l| l.ok())
            .filter(|l| !l.trim().is_empty()),
    ))
}

fn sub_tasks(args: &Args) -> Result<Box<dyn Iterator<Item = engine::Input> + Send>> {
    let templates = [
        args.sh.is_some(),
        args.http,
        args.grep.is_some(),
        args.hash,
        args.lines,
    ];
    if templates.iter().filter(|&&t| t).count() > 1 {
        return Err("subtask takes one of --sh, --http, --grep, --hash, --lines".into());
    }
    if !templates.iter().any(|&t| t) {
        let mut lines = input_reader(args.tasks.as_deref())?.lines().enumerate();
        let mut stop = false;
        return Ok(Box::new(std::iter::from_fn(move || {
            if stop {
                return None;
            }
            loop {
                let (i, line) = lines.next()?;
                match line {
                    Ok(line) if line.trim().is_empty() => continue,
                    Ok(line) => {
                        return Some(
                            serde_json::from_str(&line).map_err(|_| {
                                format!("task {}: invalid JSON or task schema", i + 1)
                            }),
                        )
                    }
                    Err(error) => {
                        stop = error.kind() != io::ErrorKind::InvalidData;
                        return Some(Err(format!("task {}: cannot read input", i + 1)));
                    }
                }
            }
        })));
    }
    let items = read_lines(args.tasks.as_deref())?;
    if let Some(template) = &args.sh {
        return Ok(Box::new(engine::shell_tasks(template, items).map(Ok)));
    }
    let method = args.method.clone().unwrap_or_else(|| "GET".into());
    let (http, grep, hash) = (args.http, args.grep.clone(), args.hash);
    let save_dir = args.save_dir.clone();
    let task = move |(i, item): (usize, String)| -> engine::Input {
        Ok(if http {
            Task::Http(Http {
                url: item,
                method: method.clone(),
                headers: Default::default(),
                body: None,
                save: save_dir.as_ref().map(|dir| dir.join(i.to_string())),
            })
        } else if let Some(pattern) = &grep {
            Task::Grep {
                path: item,
                pattern: pattern.clone(),
            }
        } else if hash {
            Task::Hash(item)
        } else {
            Task::Lines(item)
        })
    };
    Ok(Box::new(items.enumerate().map(task)))
}

fn run(args: Args) -> Result<u8> {
    let tasks = sub_tasks(&args)?;
    if let Some(dir) = &args.save_dir {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let out = args.out.clone().unwrap_or_else(default_out);
    let options = SubOptions {
        cwd: std::env::current_dir().map_err(|e| e.to_string())?,
        out: out.clone(),
        fail_fast: !args.keep_going,
        timeout_ms: args.timeout_ms.unwrap_or(engine::DEFAULT_TIMEOUT_MS),
        per_host: args.per_host.unwrap_or(engine::DEFAULT_PER_HOST),
        retries: args.retries.unwrap_or(engine::DEFAULT_RETRIES),
        sh_mem_mb: args.mem_mb.unwrap_or(engine::DEFAULT_SH_MEM_MB),
        cap: Widths {
            cpu: args.cpu.unwrap_or(Widths::UNCAPPED.cpu),
            net: args.net.unwrap_or(Widths::UNCAPPED.net),
            file: args.file.unwrap_or(Widths::UNCAPPED.file),
        },
        show: args.show.unwrap_or(3),
    };
    let summary = engine::run(tasks, &options)?;
    if summary.total == 0 {
        let _ = std::fs::remove_file(&out);
        println!("SUBTASK OK nothing to run");
        return Ok(0);
    }
    let w = summary.widths;
    let pools = format!(
        "peak cpu {}, net {}, file {}; {} run(s) sharing",
        w.cpu, w.net, w.file, summary.runs
    );
    if summary.failed == 0 && summary.cancelled == 0 && summary.skipped == 0 {
        println!(
            "SUBTASK OK {} task(s) in {} ms ({pools}); results {}",
            summary.total,
            summary.ms,
            out.display()
        );
        return Ok(0);
    }
    println!(
        "SUBTASK FAIL {} failed, {} cancelled, {} skipped of {} task(s) in {} ms ({pools}); results {}",
        summary.failed,
        summary.cancelled,
        summary.skipped,
        summary.total,
        summary.ms,
        out.display()
    );
    for failure in &summary.failures {
        println!(
            "    FAIL #{} {} {} (code {})",
            failure.i, failure.kind, failure.task, failure.code
        );
        let lines: Vec<&str> = failure.output.lines().collect();
        for line in &lines[lines.len().saturating_sub(20)..] {
            println!("      {line}");
        }
    }
    Ok(1)
}
