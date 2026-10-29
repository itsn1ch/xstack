//! Runs many small subtasks for one agent without any model call: commands, browser actions, HTTP requests, and file reads.
//! Each kind has its own pool, with browser actions sharing the CPU pool. A pool's width comes from the machine,
//! is split evenly across every `worker sub` running at once, and is read again every `RETUNE`.
//! Tasks stream through bounded queues; browser tasks use a private disk spool.
//! Results stream to a JSONL file, so memory stays flat as task count grows.
#[path = "spool.rs"]
mod spool;

use crate::Result;
use regex::bytes::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, watch, Notify, Semaphore};
use xstack_worker::probe::{Meter, Power};
use xstack_worker::run::{keep_tail, kill_group, OUTPUT_TAIL_BYTES};
use xstack_worker::share::{Kind, Runs, Share};

pub const DEFAULT_TIMEOUT_MS: u64 = 5 * 60_000;
pub const DEFAULT_PER_HOST: usize = 128;
pub const DEFAULT_RETRIES: u32 = 2;
/// Most shell subtasks are small tools, not test suites, so they get less memory than a background check.
pub const DEFAULT_SH_MEM_MB: u64 = 256;
/// A command template without `{}` receives this many items per process.
pub const BATCH_ITEMS: usize = 32;
const RETUNE: Duration = Duration::from_millis(250);
const NET_MAX: usize = 10_000;
/// Memory one open request may need, with its buffers and TLS state.
const NET_TASK_KB: u64 = 256;
/// Descriptors kept free for the runtime, the results file, and shell pipes.
const FD_HEADROOM: u64 = 256;
const FILE_PER_CORE: usize = 16;
const FILE_MAX: usize = 512;
const MAX_RETRY_WAIT: Duration = Duration::from_secs(30);
/// Bounded read-ahead for network and file tasks, and CPU completion notifications.
const QUEUE_DEPTH: usize = 1024;

/// One input line: a task, or why the line is not one.
pub type Input = std::result::Result<Task, String>;

/// One subtask. In JSONL each line is an object with one key, such as `{"sh": "cmd"}` or `{"hash": "path"}`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Task {
    Sh(String),
    Argv(Vec<String>),
    Browser(Browser),
    Http(Http),
    Grep { path: String, pattern: String },
    Hash(String),
    Lines(String),
}

/// Commands are argument arrays passed directly to agent-browser, without a shell.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Browser {
    pub session: String,
    pub commands: Vec<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Http {
    pub url: String,
    #[serde(default = "get")]
    pub method: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
    /// Streams the response body to this file instead of keeping its tail.
    #[serde(default)]
    pub save: Option<PathBuf>,
}

fn get() -> String {
    "GET".into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pool {
    Cpu,
    Net,
    File,
}

impl Task {
    fn pool(&self) -> Pool {
        match self {
            Task::Sh(_) | Task::Argv(_) | Task::Browser(_) => Pool::Cpu,
            Task::Http(_) => Pool::Net,
            Task::Grep { .. } | Task::Hash(_) | Task::Lines(_) => Pool::File,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Task::Sh(_) => "sh",
            Task::Argv(_) => "argv",
            Task::Browser(_) => "browser",
            Task::Http(_) => "http",
            Task::Grep { .. } => "grep",
            Task::Hash(_) => "hash",
            Task::Lines(_) => "lines",
        }
    }

    fn label(&self) -> String {
        match self {
            Task::Sh(cmd) => cmd.clone(),
            Task::Argv(argv) => argv.join(" "),
            Task::Browser(browser) => browser.session.clone(),
            Task::Http(h) => format!("{} {}", h.method, h.url),
            Task::Grep { path, pattern } => format!("{pattern} {path}"),
            Task::Hash(path) | Task::Lines(path) => path.clone(),
        }
    }
}

/// The code of a command that fail-fast killed after another task failed.
pub const CANCELLED: i64 = -130;

/// One finished subtask. `code` is the exit code, the HTTP status, the match count, or the line count.
#[derive(Clone, Debug, Serialize)]
pub struct Outcome {
    pub i: usize,
    pub kind: &'static str,
    pub task: String,
    pub ok: bool,
    /// Fail-fast stopped it after another task failed, so it is neither a pass nor a failure.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cancelled: bool,
    pub code: i64,
    pub ms: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub output: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Widths {
    pub cpu: usize,
    pub net: usize,
    pub file: usize,
}

impl Widths {
    pub const UNCAPPED: Widths = Widths {
        cpu: usize::MAX,
        net: usize::MAX,
        file: usize::MAX,
    };

    fn min(self, cap: Widths) -> Widths {
        Widths {
            cpu: self.cpu.min(cap.cpu).max(1),
            net: self.net.min(cap.net).max(1),
            file: self.file.min(cap.file).max(1),
        }
    }
}

/// Splits the cores across every live run, the verification worker included, and the network and file
/// pools across subtask runs only. `ours_cpu` running commands count as reusable, because the CPU sample
/// already includes their load.
pub fn widths(power: &Power, runs: Runs, ours_cpu: usize, sh_mem_mb: u64, nofile: u64) -> Widths {
    let share = |total: usize| (total / runs.same_kind.max(1)).max(1);
    let cores = (power.cores / runs.all.max(1)).max(1);
    let mem_kb = power
        .mem_free_mb
        .map_or(u64::MAX, |mb| mb.saturating_mul(1024));
    let net_total = NET_MAX
        .min(usize::try_from(nofile.saturating_sub(FD_HEADROOM) / 2).unwrap_or(usize::MAX))
        .min(usize::try_from(mem_kb / NET_TASK_KB).unwrap_or(usize::MAX));
    Widths {
        cpu: power.width(0, ours_cpu, sh_mem_mb).min(cores),
        net: share(net_total),
        file: share((power.cores * FILE_PER_CORE).clamp(FILE_PER_CORE, FILE_MAX)),
    }
}

/// Raises the soft descriptor limit to the hard one and returns the new soft limit.
pub fn raise_nofile() -> u64 {
    // SAFETY: getrlimit and setrlimit only read and write the struct passed to them.
    unsafe {
        let mut limit: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) != 0 {
            return 1024;
        }
        if limit.rlim_cur < limit.rlim_max {
            let raised = libc::rlimit {
                rlim_cur: limit.rlim_max,
                rlim_max: limit.rlim_max,
            };
            if libc::setrlimit(libc::RLIMIT_NOFILE, &raised) == 0 {
                limit.rlim_cur = limit.rlim_max;
            }
        }
        limit.rlim_cur as u64
    }
}

pub struct SubOptions {
    pub cwd: PathBuf,
    pub out: PathBuf,
    pub fail_fast: bool,
    pub timeout_ms: u64,
    pub per_host: usize,
    pub retries: u32,
    pub sh_mem_mb: u64,
    pub cap: Widths,
    /// How many failures the summary keeps; the results file keeps all of them.
    pub show: usize,
}

pub struct Summary {
    pub total: usize,
    pub failed: usize,
    /// Tasks fail-fast stopped while they ran.
    pub cancelled: usize,
    /// Tasks never started because fail-fast stopped the run.
    pub skipped: usize,
    pub ms: u64,
    /// The widest each pool ran during the run.
    pub widths: Widths,
    pub runs: usize,
    pub failures: Vec<Outcome>,
}

/// Lets one dispatcher start a task only while fewer than `width` of its pool are running.
struct Gate {
    state: Mutex<GateState>,
    freed: Notify,
}

struct GateState {
    running: usize,
    width: usize,
    peak: usize,
}

impl Gate {
    fn new(width: usize) -> Gate {
        Gate {
            state: Mutex::new(GateState {
                running: 0,
                width,
                peak: 0,
            }),
            freed: Notify::new(),
        }
    }

    fn try_enter(&self) -> bool {
        let mut state = self.state.lock().expect("gate state");
        if state.running >= state.width {
            return false;
        }
        state.running += 1;
        state.peak = state.peak.max(state.running);
        true
    }

    async fn enter(&self) {
        loop {
            let notified = self.freed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut state = self.state.lock().expect("gate state");
                if state.running < state.width {
                    state.running += 1;
                    state.peak = state.peak.max(state.running);
                    return;
                }
            }
            notified.await;
        }
    }

    fn leave(&self) {
        self.state.lock().expect("gate state").running -= 1;
        self.freed.notify_waiters();
    }

    fn resize(&self, width: usize) {
        let mut state = self.state.lock().expect("gate state");
        let increased = width > state.width;
        state.width = width;
        drop(state);
        if increased {
            self.freed.notify_waiters();
        }
    }
}

struct Ctx {
    cwd: PathBuf,
    timeout: Duration,
    retries: u32,
    per_host: usize,
    client: reqwest::Client,
    hosts: Mutex<HashMap<String, Arc<Semaphore>>>,
    patterns: Mutex<HashMap<String, Arc<std::result::Result<Regex, String>>>>,
    sessions: Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>,
    uncertain_sessions: Mutex<UncertainSessions>,
    cancel: watch::Receiver<bool>,
}

#[derive(Default)]
struct UncertainSessions {
    names: HashSet<String>,
    overflowed: bool,
}

impl UncertainSessions {
    fn contains(&self, name: &str) -> bool {
        self.overflowed || self.names.contains(name)
    }

    fn insert(&mut self, name: String) {
        if !self.overflowed {
            self.names.insert(name);
            self.overflowed = self.names.len() >= QUEUE_DEPTH;
        }
    }
}

impl Ctx {
    fn session(&self, name: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut sessions = self.sessions.lock().expect("browser sessions");
        if let Some(session) = sessions.get(name).and_then(Weak::upgrade) {
            return session;
        }
        let session = Arc::new(tokio::sync::Mutex::new(()));
        sessions.insert(name.to_string(), Arc::downgrade(&session));
        session
    }

    fn forget_session(&self, name: &str, session: &Arc<tokio::sync::Mutex<()>>) {
        let mut sessions = self.sessions.lock().expect("browser sessions");
        if Arc::strong_count(session) == 1 {
            sessions.remove(name);
        }
    }

    fn host(&self, url: &str) -> Arc<Semaphore> {
        let host = reqwest::Url::parse(url)
            .ok()
            .and_then(|u| {
                u.host_str()
                    .map(|h| format!("{h}:{}", u.port_or_known_default().unwrap_or(0)))
            })
            .unwrap_or_default();
        let mut hosts = self.hosts.lock().expect("host limits");
        Arc::clone(
            hosts
                .entry(host)
                .or_insert_with(|| Arc::new(Semaphore::new(self.per_host))),
        )
    }

    /// Compiles each pattern once, on first use.
    fn pattern(&self, pattern: &str) -> Arc<std::result::Result<Regex, String>> {
        let mut patterns = self.patterns.lock().expect("patterns");
        Arc::clone(
            patterns
                .entry(pattern.to_string())
                .or_insert_with(|| Arc::new(Regex::new(pattern).map_err(|e| e.to_string()))),
        )
    }
}

/// Quotes one item for `sh -c`.
pub fn quote(item: &str) -> String {
    format!("'{}'", item.replace('\'', r"'\''"))
}

/// Builds one task per item when the template has `{}`, otherwise one per `BATCH_ITEMS` items appended to it.
/// Reads `items` lazily, so a template over millions of items never holds them all.
pub fn shell_tasks<I>(template: &str, items: I) -> Box<dyn Iterator<Item = Task> + Send>
where
    I: IntoIterator<Item = String>,
    I::IntoIter: Send + 'static,
{
    let template = template.to_string();
    let mut items = items.into_iter();
    if template.contains("{}") {
        return Box::new(items.map(move |item| Task::Sh(template.replace("{}", &quote(&item)))));
    }
    Box::new(std::iter::from_fn(move || {
        let batch: Vec<String> = items
            .by_ref()
            .take(BATCH_ITEMS)
            .map(|item| quote(&item))
            .collect();
        (!batch.is_empty()).then(|| Task::Sh(format!("{template} {}", batch.join(" "))))
    }))
}

pub fn run<I>(tasks: I, options: &SubOptions) -> Result<Summary>
where
    I: IntoIterator<Item = Input>,
    I::IntoIter: Send + 'static,
{
    let started = Instant::now();
    let nofile = raise_nofile();
    let share = Share::join(Kind::Subtask);
    let (meter, power) = Meter::quick();
    let first = widths(&power, share.runs(), 0, options.sh_mem_mb, nofile).min(options.cap);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(power.cores.max(1))
        .max_blocking_threads(FILE_MAX + 16)
        .enable_all()
        .build()
        .map_err(|e| format!("cannot start runtime: {e}"))?;
    let tasks = tasks.into_iter();
    let result = runtime.block_on(drive(tasks, options, share, meter, nofile, first));
    if result.is_err() {
        runtime.shutdown_timeout(Duration::from_millis(10));
    }
    let mut summary = result?;
    summary.ms = started.elapsed().as_millis() as u64;
    Ok(summary)
}

async fn drive(
    tasks: impl Iterator<Item = Input> + Send + 'static,
    options: &SubOptions,
    share: Share,
    mut meter: Meter,
    nofile: u64,
    first: Widths,
) -> Result<Summary> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(options.timeout_ms))
        .pool_max_idle_per_host(options.per_host)
        .user_agent("xstack-worker")
        .build()
        .map_err(|e| format!("cannot build HTTP client: {e}"))?;
    let spool = spool::Spool::new();
    let (cancel_tx, cancel) = watch::channel(false);
    let ctx = Arc::new(Ctx {
        cwd: options.cwd.clone(),
        timeout: Duration::from_millis(options.timeout_ms),
        retries: options.retries,
        per_host: options.per_host.max(1),
        client,
        hosts: Mutex::new(HashMap::new()),
        patterns: Mutex::new(HashMap::new()),
        sessions: Mutex::new(HashMap::new()),
        uncertain_sessions: Mutex::new(UncertainSessions::default()),
        cancel,
    });
    let gates = [
        Arc::new(Gate::new(first.cpu)),
        Arc::new(Gate::new(first.net)),
        Arc::new(Gate::new(first.file)),
    ];

    let (sh_mem_mb, cap) = (options.sh_mem_mb, options.cap);
    let runs_seen = Arc::new(AtomicUsize::new(1));
    let retune = {
        let (gates, runs_seen) = (gates.clone(), Arc::clone(&runs_seen));
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(RETUNE).await;
                let runs = share.runs();
                runs_seen.fetch_max(runs.all, SeqCst);
                let ours = gates[0].state.lock().expect("gate state").running;
                let w = widths(&meter.read(), runs, ours, sh_mem_mb, nofile).min(cap);
                gates[0].resize(w.cpu);
                gates[1].resize(w.net);
                gates[2].resize(w.file);
            }
        })
    };

    let (results_tx, mut results) = mpsc::channel::<Outcome>(4096);
    let file = File::create(&options.out)
        .map_err(|e| format!("cannot create {}: {e}", options.out.display()))?;
    let (fail_fast, show) = (options.fail_fast, options.show);
    let fatal = Arc::new(AtomicBool::new(false));
    let (fatal_tx, mut fatal_rx) = watch::channel::<Option<String>>(None);
    let writer_fatal = Arc::clone(&fatal);
    let writer_cancel = cancel_tx.clone();
    let writer_error = fatal_tx.clone();
    let writer = tokio::task::spawn_blocking(move || -> Result<Tally> {
        let result: Result<Tally> = (|| {
            let mut out = BufWriter::new(file);
            let mut tally = Tally::default();
            while let Some(outcome) = results.blocking_recv() {
                tally.done += 1;
                serde_json::to_writer(&mut out, &outcome).map_err(|e| e.to_string())?;
                out.write_all(b"\n").map_err(|e| e.to_string())?;
                out.flush().map_err(|e| e.to_string())?;
                if outcome.cancelled {
                    tally.cancelled += 1;
                } else if !outcome.ok {
                    tally.failed += 1;
                    if fail_fast {
                        let _ = writer_cancel.send(true);
                    }
                    if tally.failures.len() < show {
                        tally.failures.push(outcome);
                    }
                }
            }
            out.flush().map_err(|e| e.to_string())?;
            Ok(tally)
        })();
        if let Err(error) = &result {
            writer_fatal.store(true, SeqCst);
            let _ = writer_error.send(Some(error.clone()));
            let _ = writer_cancel.send(true);
        }
        result
    });

    let mut queues = Vec::new();
    let dispatchers: Vec<_> = gates
        .iter()
        .map(|gate| {
            let (tx, rx) = mpsc::channel::<(usize, Task)>(QUEUE_DEPTH);
            queues.push(tx);
            tokio::spawn(dispatch(
                rx,
                Arc::clone(gate),
                Arc::clone(&ctx),
                results_tx.clone(),
            ))
        })
        .collect();
    let cpu_dispatcher = {
        let (spool, gate, ctx, results, cancel) = (
            Arc::clone(&spool),
            Arc::clone(&gates[0]),
            Arc::clone(&ctx),
            results_tx.clone(),
            cancel_tx.clone(),
        );
        let fatal = Arc::clone(&fatal);
        let dispatcher_error = fatal_tx.clone();
        tokio::spawn(async move {
            let result = dispatch_cpu(spool, gate, ctx, results).await;
            if let Err(error) = &result {
                fatal.store(true, SeqCst);
                let _ = dispatcher_error.send(Some(error.clone()));
                let _ = cancel.send(true);
            }
            result
        })
    };
    // Reads the input on its own thread. After fail-fast it keeps counting without sending, so the
    // summary still reports how many tasks never started.
    let reader = {
        let (cancel, results, error_cancel) =
            (ctx.cancel.clone(), results_tx.clone(), cancel_tx.clone());
        let fatal = Arc::clone(&fatal);
        let reader_error = fatal_tx.clone();
        let spool = spool::Writer(spool);
        tokio::task::spawn_blocking(move || -> Result<usize> {
            let mut read = 0;
            let mut tasks = tasks;
            while !fatal.load(SeqCst) {
                let Some(input) = tasks.next() else { break };
                let i = read;
                read += 1;
                if *cancel.borrow() {
                    continue;
                }
                match input {
                    Ok(task) => match &task {
                        Task::Browser(_) => {
                            if let Err(error) = spool.append(i, &task) {
                                fatal.store(true, SeqCst);
                                let _ = reader_error.send(Some(error.clone()));
                                let _ = error_cancel.send(true);
                                return Err(error);
                            }
                        }
                        _ => {
                            let pool = match task.pool() {
                                Pool::Cpu => 0,
                                Pool::Net => 1,
                                Pool::File => 2,
                            };
                            let _ = queues[pool].blocking_send((i, task));
                        }
                    },
                    Err(problem) => {
                        let _ = results.blocking_send(Outcome {
                            i,
                            kind: "input",
                            task: problem.clone(),
                            ok: false,
                            cancelled: false,
                            code: -1,
                            ms: 0,
                            output: problem,
                        });
                    }
                }
            }
            Ok(read)
        })
    };
    drop(results_tx);
    let (total, cpu_result) = match await_input_and_cpu(reader, cpu_dispatcher, &mut fatal_rx).await
    {
        Ok(results) => results,
        Err(error) => {
            retune.abort();
            return Err(error);
        }
    };
    let mut dispatch_result = Ok(());
    for dispatcher in dispatchers {
        tokio::select! {
            result = dispatcher => {
                if let Err(error) = result {
                    dispatch_result = Err(error.to_string());
                }
            }
            result = fatal_rx.wait_for(|error| error.is_some()) => {
                retune.abort();
                return Err(result.ok().and_then(|error| error.clone()).unwrap_or_else(|| "fatal task error".into()));
            }
        }
    }
    let tally = writer
        .await
        .map_err(|e| e.to_string())
        .and_then(|result| result);
    retune.abort();
    let total = total?;
    cpu_result?;
    dispatch_result?;
    let tally = tally?;
    let peak = |g: &Gate| g.state.lock().expect("gate state").peak;
    Ok(Summary {
        total,
        failed: tally.failed,
        cancelled: tally.cancelled,
        skipped: total - tally.done,
        ms: 0,
        widths: Widths {
            cpu: peak(&gates[0]),
            net: peak(&gates[1]),
            file: peak(&gates[2]),
        },
        runs: runs_seen.load(SeqCst),
        failures: tally.failures,
    })
}

async fn await_input_and_cpu(
    reader: tokio::task::JoinHandle<Result<usize>>,
    cpu: tokio::task::JoinHandle<Result<()>>,
    fatal: &mut watch::Receiver<Option<String>>,
) -> Result<(Result<usize>, Result<()>)> {
    let completed = async {
        let total = reader
            .await
            .map_err(|error| error.to_string())
            .and_then(|result| result);
        let cpu_result = cpu
            .await
            .map_err(|error| error.to_string())
            .and_then(|result| result);
        (total, cpu_result)
    };
    tokio::select! {
        result = completed => Ok(result),
        _ = fatal.changed() => {
            Err(fatal.borrow().clone().unwrap_or_else(|| "fatal task error".into()))
        }
    }
}

#[derive(Default)]
struct Tally {
    done: usize,
    failed: usize,
    cancelled: usize,
    failures: Vec<Outcome>,
}

#[derive(Default)]
struct SessionQueue {
    head: Option<u64>,
    tail: Option<u64>,
}

fn cpu_finished(
    name: Option<String>,
    sessions: &mut HashMap<String, SessionQueue>,
    ready: &mut VecDeque<String>,
) {
    if let Some(name) = name {
        if sessions
            .get(&name)
            .expect("active browser session")
            .head
            .is_some()
        {
            ready.push_back(name);
        } else {
            sessions.remove(&name);
        }
    }
}

fn start_cpu_task(
    i: usize,
    task: Task,
    name: Option<String>,
    gate: Arc<Gate>,
    ctx: Arc<Ctx>,
    results: mpsc::Sender<Outcome>,
    finished: mpsc::Sender<Option<String>>,
) {
    tokio::spawn(async move {
        let outcome = if let Some(name) = &name {
            execute_browser(i, task, name, &ctx).await
        } else {
            execute(i, task, &ctx).await
        };
        let _ = results.send(outcome).await;
        // Keep the gate slot until completion has entered the bounded channel.
        let _ = finished.send(name).await;
        gate.leave();
    });
}

/// Scan each input record once. Blocked sessions have linked FIFO records on
/// disk; only running/ready sessions have an in-memory head and tail.
async fn dispatch_cpu(
    spool: Arc<spool::Spool>,
    gate: Arc<Gate>,
    ctx: Arc<Ctx>,
    results: mpsc::Sender<Outcome>,
) -> Result<()> {
    let (finished_tx, mut finished) = mpsc::channel::<Option<String>>(QUEUE_DEPTH);
    let mut sessions: HashMap<String, SessionQueue> = HashMap::new();
    let mut ready = VecDeque::new();
    let mut cursor = 0;
    let mut deferred = 0usize;
    let mut deferred_bytes = 0u64;
    let mut scanned = 0usize;
    let mut cancel = ctx.cancel.clone();
    loop {
        if *cancel.borrow() || results.is_closed() {
            return Ok(());
        }
        while let Ok(name) = finished.try_recv() {
            cpu_finished(name, &mut sessions, &mut ready);
        }
        if spool.should_reclaim(cursor, deferred_bytes)? {
            let compact_spool = Arc::clone(&spool);
            let compact_cancel = ctx.cancel.clone();
            let mut compact_sessions = std::mem::take(&mut sessions);
            let (updated, result) = tokio::task::spawn_blocking(move || {
                let result = compact_spool.reclaim_with_sessions(
                    cursor,
                    deferred_bytes,
                    &mut compact_sessions,
                    &compact_cancel,
                );
                (compact_sessions, result)
            })
            .await
            .map_err(|error| error.to_string())?;
            sessions = updated;
            if let Some(new_cursor) = result? {
                cursor = new_cursor;
            }
            if *cancel.borrow() {
                return Ok(());
            }
        }

        // Register before examining the log and gate, to avoid losing a wakeup.
        let changed = spool.changed.notified();
        let freed = gate.freed.notified();
        tokio::pin!(changed, freed);
        changed.as_mut().enable();
        freed.as_mut().enable();
        let (record, closed) = spool.read(cursor)?;

        // Alternate a scan step with each ready session. This bounds ready
        // session bookkeeping even if distinct sessions continuously finish,
        // while the scan still progresses past a busy session's backlog.
        if !ready.is_empty() && (record.is_none() || scanned >= 1) && gate.try_enter() {
            let name = ready.pop_front().expect("ready session");
            let queue = sessions.get_mut(&name).expect("active browser session");
            let head = queue.head.expect("ready session task");
            let pending = match spool.read(head) {
                Ok((Some(record), _)) => record,
                Ok((None, _)) => {
                    gate.leave();
                    return Err("cannot read private task spool: missing queued task".into());
                }
                Err(error) => {
                    gate.leave();
                    return Err(error);
                }
            };
            queue.head = pending.link;
            if queue.head.is_none() {
                queue.tail = None;
            }
            deferred -= 1;
            deferred_bytes -= pending.next - head;
            start_cpu_task(
                pending.task.0,
                pending.task.1,
                Some(name),
                Arc::clone(&gate),
                Arc::clone(&ctx),
                results.clone(),
                finished_tx.clone(),
            );
            scanned = 0;
            continue;
        }

        let mut wait = true;
        if let Some(record) = record {
            let (i, task) = record.task;
            let name = match &task {
                Task::Browser(browser) => Some(browser.session.clone()),
                _ => None,
            };
            if let Some(queue) = name.as_ref().and_then(|name| sessions.get_mut(name)) {
                if let Some(tail) = queue.tail {
                    spool.link(tail, cursor)?;
                } else {
                    queue.head = Some(cursor);
                }
                queue.tail = Some(cursor);
                deferred += 1;
                deferred_bytes += record.next - cursor;
                cursor = record.next;
                wait = false;
            } else if gate.try_enter() {
                if let Some(name) = &name {
                    sessions.insert(name.clone(), SessionQueue::default());
                }
                cursor = record.next;
                start_cpu_task(
                    i,
                    task,
                    name,
                    Arc::clone(&gate),
                    Arc::clone(&ctx),
                    results.clone(),
                    finished_tx.clone(),
                );
                wait = false;
            }
        } else if closed && deferred == 0 {
            return Ok(());
        }
        if !wait {
            scanned += 1;
            if scanned % 256 == 0 {
                tokio::task::yield_now().await;
            }
            continue;
        }
        tokio::select! {
            _ = &mut changed => {}
            _ = &mut freed => {}
            name = finished.recv() => {
                if let Some(name) = name {
                    cpu_finished(name, &mut sessions, &mut ready);
                }
            }
            _ = cancel.wait_for(|cancelled| *cancelled) => return Ok(()),
            _ = results.closed() => return Ok(()),
        }
    }
}

async fn execute_browser(i: usize, task: Task, name: &str, ctx: &Ctx) -> Outcome {
    let session = ctx.session(name);
    let mut cancel = ctx.cancel.clone();
    let guard = tokio::select! {
        guard = session.lock() => Some(guard),
        _ = cancel.wait_for(|cancelled| *cancelled) => None,
    };
    let uncertain = guard.is_some()
        && ctx
            .uncertain_sessions
            .lock()
            .expect("uncertain sessions")
            .contains(name);
    let outcome = if uncertain {
        Outcome {
            i, kind: task.kind(), task: task.label(), ok: false, cancelled: false, code: -1, ms: 0,
            output: "browser session cannot run after a failed action or safety limit; confirm sessions are safe before reuse".into(),
        }
    } else if guard.is_some() && !*cancel.borrow() {
        let outcome = execute(i, task, ctx).await;
        if !outcome.ok {
            ctx.uncertain_sessions
                .lock()
                .expect("uncertain sessions")
                .insert(name.to_string());
        }
        outcome
    } else {
        Outcome {
            i,
            kind: task.kind(),
            task: task.label(),
            ok: false,
            cancelled: true,
            code: CANCELLED,
            ms: 0,
            output: "[cancelled by an earlier failure]".into(),
        }
    };
    drop(guard);
    ctx.forget_session(name, &session);
    outcome
}

async fn dispatch(
    mut queue: mpsc::Receiver<(usize, Task)>,
    gate: Arc<Gate>,
    ctx: Arc<Ctx>,
    results: mpsc::Sender<Outcome>,
) {
    let mut cancel = ctx.cancel.clone();
    while let Some((i, task)) = queue.recv().await {
        if *ctx.cancel.borrow() {
            return;
        }
        tokio::select! {
            _ = gate.enter() => {}
            _ = cancel.wait_for(|cancelled| *cancelled) => return,
        }
        if *ctx.cancel.borrow() {
            gate.leave();
            return;
        }
        let (gate, ctx, results) = (Arc::clone(&gate), Arc::clone(&ctx), results.clone());
        tokio::spawn(async move {
            let outcome = execute(i, task, &ctx).await;
            let _ = results.send(outcome).await;
            gate.leave();
        });
    }
}

async fn execute(i: usize, task: Task, ctx: &Ctx) -> Outcome {
    let started = Instant::now();
    let (kind, label) = (task.kind(), task.label());
    let (ok, code, output) = match task {
        Task::Sh(cmd) => {
            let mut command = tokio::process::Command::new("sh");
            command.arg("-c").arg(cmd);
            shell(command, ctx).await
        }
        Task::Argv(argv) => match argv.split_first() {
            Some((program, args)) => {
                let mut command = tokio::process::Command::new(program);
                command.args(args);
                shell(command, ctx).await
            }
            None => (false, 127, "empty argv".into()),
        },
        Task::Browser(browser) => browser_actions(browser, ctx).await,
        Task::Http(request) => http(request, ctx).await,
        Task::Grep { path, pattern } => match &*ctx.pattern(&pattern) {
            Ok(re) => {
                let re = re.clone();
                blocking(move || grep(Path::new(&path), &re)).await
            }
            Err(e) => (false, -1, format!("bad pattern: {e}")),
        },
        Task::Hash(path) => blocking(move || hash(Path::new(&path))).await,
        Task::Lines(path) => blocking(move || lines(Path::new(&path))).await,
    };
    Outcome {
        i,
        kind,
        task: label,
        ok,
        cancelled: code == CANCELLED,
        code,
        ms: started.elapsed().as_millis() as u64,
        output,
    }
}

type Done = (bool, i64, String);

async fn browser_actions(browser: Browser, ctx: &Ctx) -> Done {
    if browser.session.trim().is_empty()
        || browser.session.contains(['/', '\\', '\0'])
        || browser.session == "."
        || browser.session == ".."
    {
        return (
            false,
            -1,
            "browser session must be a nonempty name without path separators".into(),
        );
    }
    if browser.commands.is_empty()
        || browser
            .commands
            .iter()
            .any(|args| args.is_empty() || args[0].trim().is_empty())
    {
        return (
            false,
            -1,
            "browser commands must contain nonempty command arrays".into(),
        );
    }
    let unsafe_flags = [
        "--session",
        "--cdp",
        "--auto-connect",
        "--profile",
        "--config",
        "--provider",
        "--session-name",
        "-p",
    ];
    if browser.commands.iter().flatten().any(|arg| {
        unsafe_flags.iter().any(|flag| {
            arg == flag
                || arg
                    .strip_prefix(flag)
                    .is_some_and(|rest| rest.starts_with('='))
        })
    }) {
        return (
            false,
            -1,
            "browser commands cannot override session or shared browser connection settings".into(),
        );
    }
    if let Err(message) = browser_connection_settings(&ctx.cwd) {
        return (false, -1, message);
    }
    let buffer = Mutex::new(Vec::new());
    for (i, args) in browser.commands.iter().enumerate() {
        let mut command = tokio::process::Command::new("agent-browser");
        command.arg("--session").arg(&browser.session).args(args);
        let (ok, code, output) = shell(command, ctx).await;
        let header = format!("[action {} (code {})]\n", i + 1, code);
        keep_tail(&buffer, header.as_bytes());
        keep_tail(&buffer, output.as_bytes());
        if !output.ends_with('\n') {
            keep_tail(&buffer, b"\n");
        }
        if !ok {
            return (
                false,
                code,
                String::from_utf8_lossy(&buffer.lock().expect("output buffer")).into_owned(),
            );
        }
    }
    let output = String::from_utf8_lossy(&buffer.lock().expect("output buffer")).into_owned();
    (true, 0, output)
}

fn browser_connection_settings(cwd: &Path) -> std::result::Result<(), String> {
    let vars = [
        "AGENT_BROWSER_CDP",
        "AGENT_BROWSER_AUTO_CONNECT",
        "AGENT_BROWSER_PROFILE",
        "AGENT_BROWSER_SESSION_NAME",
        "AGENT_BROWSER_PROVIDER",
    ];
    for key in vars {
        if std::env::var(key).ok().is_some_and(|value| {
            !value.is_empty()
                && (key != "AGENT_BROWSER_AUTO_CONNECT"
                    || (value != "0"
                        && !value.eq_ignore_ascii_case("false")
                        && !value.eq_ignore_ascii_case("no")))
        }) {
            return Err(format!(
                "browser task cannot use shared connection setting {key}"
            ));
        }
    }
    let paths = if let Some(path) = std::env::var_os("AGENT_BROWSER_CONFIG") {
        vec![PathBuf::from(path)]
    } else {
        let home = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .ok_or_else(|| {
                "cannot inspect agent-browser user configuration without HOME".to_string()
            })?;
        let mut paths = vec![PathBuf::from(home).join(".agent-browser/config.json")];
        paths.push(cwd.join("agent-browser.json"));
        paths
    };
    for path in paths {
        let contents = match std::fs::read(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                return Err(
                    "cannot inspect agent-browser configuration for shared connections".into(),
                )
            }
        };
        let config: serde_json::Value = serde_json::from_slice(&contents).map_err(|_| {
            "cannot inspect agent-browser configuration for shared connections".to_string()
        })?;
        for key in ["cdp", "autoConnect", "profile", "provider", "sessionName"] {
            if config
                .get(key)
                .is_some_and(|value| !value.is_null() && value != false && value != "")
            {
                return Err(format!(
                    "browser task cannot use shared connection setting {key}"
                ));
            }
        }
    }
    Ok(())
}
async fn blocking(work: impl FnOnce() -> std::io::Result<(i64, String)> + Send + 'static) -> Done {
    match tokio::task::spawn_blocking(work).await {
        Ok(Ok((code, output))) => (true, code, output),
        Ok(Err(e)) => (false, -1, e.to_string()),
        Err(e) => (false, -1, e.to_string()),
    }
}

async fn pump(source: Option<impl AsyncRead + Unpin>, buffer: &Mutex<Vec<u8>>) {
    let Some(mut source) = source else {
        return;
    };
    let mut chunk = [0u8; 8192];
    while let Ok(n) = source.read(&mut chunk).await {
        if n == 0 {
            break;
        }
        keep_tail(buffer, &chunk[..n]);
    }
}

struct CommandGroup(i32);

impl Drop for CommandGroup {
    fn drop(&mut self) {
        kill_group(self.0);
    }
}

async fn shell(mut command: tokio::process::Command, ctx: &Ctx) -> Done {
    if *ctx.cancel.borrow() {
        return (false, CANCELLED, "[cancelled by an earlier failure]".into());
    }
    command
        .current_dir(&ctx.cwd)
        .env("XSTACK_WORKER", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return (false, 127, format!("cannot start: {e}")),
    };
    let mut group = CommandGroup(child.id().map_or(0, |id| id as i32));
    let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
    let buffer = Mutex::new(Vec::new());
    let finished = async {
        let (status, _, _) =
            tokio::join!(child.wait(), pump(stdout, &buffer), pump(stderr, &buffer));
        status
    };
    let mut cancel = ctx.cancel.clone();
    let (code, note) = tokio::select! {
        outcome = tokio::time::timeout(ctx.timeout, finished) => match outcome {
            Ok(Ok(status)) => (status.code().unwrap_or(1) as i64, None),
            Ok(Err(e)) => (1, Some(format!("\n[wait failed: {e}]"))),
            Err(_) => (124, Some(format!("\n[timed out after {} ms]", ctx.timeout.as_millis()))),
        },
        _ = cancel.wait_for(|cancelled| *cancelled) => (CANCELLED, Some("\n[cancelled by an earlier failure]".to_string())),
    };
    if note.is_some() {
        kill_group(group.0);
        // Do not release a browser session while its CLI process is still alive.
        let _ = child.wait().await;
    }
    group.0 = 0;
    let mut output = String::from_utf8_lossy(&buffer.lock().expect("output buffer")).into_owned();
    output.push_str(note.as_deref().unwrap_or(""));
    (code == 0, code, output)
}

fn retryable(status: u16) -> bool {
    matches!(status, 429 | 502 | 503 | 504)
}

fn backoff(attempt: u32, retry_after: Option<Duration>) -> Duration {
    retry_after
        .unwrap_or(Duration::from_millis(200 << attempt.min(8)))
        .min(MAX_RETRY_WAIT)
}

async fn http(request: Http, ctx: &Ctx) -> Done {
    let method = match reqwest::Method::from_bytes(request.method.to_uppercase().as_bytes()) {
        Ok(method) => method,
        Err(e) => return (false, -1, format!("bad method: {e}")),
    };
    let host = ctx.host(&request.url);
    let Ok(_permit) = host.acquire().await else {
        return (false, -1, "host limit closed".into());
    };
    let mut attempt = 0;
    loop {
        let mut builder = ctx.client.request(method.clone(), &request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = &request.body {
            builder = builder.body(body.clone());
        }
        match builder.send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                if retryable(status) && attempt < ctx.retries {
                    let after = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok()?.parse().ok())
                        .map(Duration::from_secs);
                    tokio::time::sleep(backoff(attempt, after)).await;
                    attempt += 1;
                    continue;
                }
                return match body(response, request.save.as_deref()).await {
                    Ok(output) => (status < 400, status as i64, output),
                    Err(e) => (false, status as i64, e),
                };
            }
            Err(e) if (e.is_connect() || e.is_timeout()) && attempt < ctx.retries => {
                tokio::time::sleep(backoff(attempt, None)).await;
                attempt += 1;
            }
            Err(e) => return (false, 0, e.to_string()),
        }
    }
}

async fn body(
    mut response: reqwest::Response,
    save: Option<&Path>,
) -> std::result::Result<String, String> {
    let mut file = match save {
        Some(path) => Some(
            tokio::fs::File::create(path)
                .await
                .map_err(|e| format!("cannot create {}: {e}", path.display()))?,
        ),
        None => None,
    };
    let tail = Mutex::new(Vec::new());
    let mut size = 0usize;
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        size += chunk.len();
        match file.as_mut() {
            Some(file) => file.write_all(&chunk).await.map_err(|e| e.to_string())?,
            None => keep_tail(&tail, &chunk),
        }
    }
    if let (Some(mut file), Some(path)) = (file, save) {
        file.flush().await.map_err(|e| e.to_string())?;
        return Ok(format!("{size} bytes saved to {}", path.display()));
    }
    Ok(String::from_utf8_lossy(&tail.into_inner().expect("body tail")).into_owned())
}

/// Returns the match count and the matching lines as `line:text`, keeping the last `OUTPUT_TAIL_BYTES`.
fn grep(path: &Path, re: &Regex) -> std::io::Result<(i64, String)> {
    let data = std::fs::read(path)?;
    let tail = Mutex::new(Vec::new());
    let mut count = 0;
    for (n, line) in data.split(|&b| b == b'\n').enumerate() {
        if re.is_match(line) {
            count += 1;
            keep_tail(&tail, format!("{}:", n + 1).as_bytes());
            keep_tail(&tail, &line[..line.len().min(OUTPUT_TAIL_BYTES)]);
            keep_tail(&tail, b"\n");
        }
    }
    Ok((
        count,
        String::from_utf8_lossy(&tail.into_inner().expect("grep tail")).into_owned(),
    ))
}

fn hash(path: &Path) -> std::io::Result<(i64, String)> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(File::open(path)?)?;
    Ok((0, hasher.finalize().to_hex().to_string()))
}

fn lines(path: &Path) -> std::io::Result<(i64, String)> {
    let mut file = File::open(path)?;
    let mut chunk = vec![0u8; 64 * 1024];
    let mut count = 0i64;
    loop {
        let n = file.read(&mut chunk)?;
        if n == 0 {
            return Ok((count, String::new()));
        }
        count += chunk[..n].iter().filter(|&&b| b == b'\n').count() as i64;
    }
}

#[cfg(test)]
mod gate_tests {
    use super::*;
    #[tokio::test]
    async fn aborted_command_terminates_its_process_group() {
        let dir = std::env::temp_dir().join(format!("xstack-group-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join("started"));
        let _ = std::fs::remove_file(dir.join("orphan"));
        let (_cancel_tx, cancel) = watch::channel(false);
        let ctx = Arc::new(Ctx {
            cwd: dir.clone(),
            timeout: Duration::from_secs(10),
            retries: 0,
            per_host: 1,
            client: reqwest::Client::new(),
            hosts: Mutex::new(HashMap::new()),
            patterns: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            uncertain_sessions: Mutex::new(UncertainSessions::default()),
            cancel,
        });
        let task = tokio::spawn(async move {
            let mut command = tokio::process::Command::new("sh");
            command.args(["-c", "(sleep 1; touch orphan) & touch started; wait"]);
            shell(command, &ctx).await
        });
        tokio::time::timeout(Duration::from_secs(3), async {
            while !dir.join("started").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        task.abort();
        let _ = task.await;
        tokio::time::sleep(Duration::from_millis(1300)).await;
        assert!(
            !dir.join("orphan").exists(),
            "task descendant survived cancellation"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn dispatcher_failure_does_not_wait_for_blocked_input() {
        let reader = tokio::spawn(std::future::pending::<Result<usize>>());
        let (fatal_tx, mut fatal_rx) = watch::channel(None);
        let dispatcher = tokio::spawn(async move {
            fatal_tx
                .send(Some("spool read failed".to_string()))
                .unwrap();
            Err("spool read failed".to_string())
        });
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            await_input_and_cpu(reader, dispatcher, &mut fatal_rx),
        )
        .await
        .expect("fatal scheduler errors must not wait for input");
        assert_eq!(result.unwrap_err(), "spool read failed");
    }

    #[tokio::test]
    async fn increasing_capacity_releases_all_newly_available_slots() {
        let gate = Arc::new(Gate::new(1));
        gate.enter().await;
        let (ready, mut entered) = mpsc::unbounded_channel();
        let (waiting, mut waiters) = mpsc::unbounded_channel();
        for _ in 0..2 {
            let gate = Arc::clone(&gate);
            let ready = ready.clone();
            let waiting = waiting.clone();
            tokio::spawn(async move {
                waiting.send(()).unwrap();
                gate.enter().await;
                ready.send(()).unwrap();
            });
        }
        waiters.recv().await.unwrap();
        waiters.recv().await.unwrap();
        tokio::task::yield_now().await;
        gate.resize(3);
        tokio::time::timeout(Duration::from_secs(2), async {
            entered.recv().await.unwrap();
            entered.recv().await.unwrap();
        })
        .await
        .expect("newly available slots must be admitted without another completion");
    }
}
