//! Runs many small subtasks for one agent without any model call: shell commands, HTTP requests, and file reads.
//! Each kind has its own pool, so slow requests never hold back commands. A pool's width comes from the machine,
//! is split evenly across every `worker sub` running at once, and is read again every `RETUNE`.
//! Results stream to a JSONL file, so a run of hundreds of thousands of tasks keeps memory flat.
use crate::Result;
use regex::bytes::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
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

/// One subtask. In JSONL each line is an object with one key, such as `{"sh": "cmd"}` or `{"hash": "path"}`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Task {
    Sh(String),
    Argv(Vec<String>),
    Http(Http),
    Grep { path: String, pattern: String },
    Hash(String),
    Lines(String),
}

#[derive(Clone, Debug, Deserialize)]
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
            Task::Sh(_) | Task::Argv(_) => Pool::Cpu,
            Task::Http(_) => Pool::Net,
            Task::Grep { .. } | Task::Hash(_) | Task::Lines(_) => Pool::File,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Task::Sh(_) => "sh",
            Task::Argv(_) => "argv",
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
    running: AtomicUsize,
    width: AtomicUsize,
    peak: AtomicUsize,
    freed: Notify,
}

impl Gate {
    fn new(width: usize) -> Gate {
        Gate {
            running: AtomicUsize::new(0),
            width: AtomicUsize::new(width),
            peak: AtomicUsize::new(0),
            freed: Notify::new(),
        }
    }

    async fn enter(&self) {
        while self.running.load(SeqCst) >= self.width.load(SeqCst) {
            self.freed.notified().await;
        }
        let now = self.running.fetch_add(1, SeqCst) + 1;
        self.peak.fetch_max(now, SeqCst);
    }

    fn leave(&self) {
        self.running.fetch_sub(1, SeqCst);
        self.freed.notify_one();
    }

    fn resize(&self, width: usize) {
        if self.width.swap(width, SeqCst) != width {
            self.freed.notify_one();
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
    patterns: HashMap<String, std::result::Result<Regex, String>>,
    cancel: watch::Receiver<bool>,
}

impl Ctx {
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
}

/// Quotes one item for `sh -c`.
pub fn quote(item: &str) -> String {
    format!("'{}'", item.replace('\'', r"'\''"))
}

/// Builds one task per item when the template has `{}`, otherwise one per `BATCH_ITEMS` items appended to it.
pub fn shell_tasks(template: &str, items: &[String]) -> Vec<Task> {
    if template.contains("{}") {
        return items
            .iter()
            .map(|item| Task::Sh(template.replace("{}", &quote(item))))
            .collect();
    }
    items
        .chunks(BATCH_ITEMS)
        .map(|batch| {
            let args: Vec<String> = batch.iter().map(|item| quote(item)).collect();
            Task::Sh(format!("{template} {}", args.join(" ")))
        })
        .collect()
}

pub fn run(tasks: Vec<Task>, options: &SubOptions) -> Result<Summary> {
    let started = Instant::now();
    let nofile = raise_nofile();
    let share = Share::join(Kind::Subtask)?;
    let (meter, power) = Meter::quick();
    let first = widths(&power, share.runs(), 0, options.sh_mem_mb, nofile).min(options.cap);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(power.cores.max(1))
        .max_blocking_threads(FILE_MAX + 16)
        .enable_all()
        .build()
        .map_err(|e| format!("cannot start runtime: {e}"))?;
    let mut summary = runtime.block_on(drive(tasks, options, share, meter, nofile, first))?;
    summary.ms = started.elapsed().as_millis() as u64;
    Ok(summary)
}

async fn drive(
    tasks: Vec<Task>,
    options: &SubOptions,
    share: Share,
    mut meter: Meter,
    nofile: u64,
    first: Widths,
) -> Result<Summary> {
    let total = tasks.len();
    let mut patterns = HashMap::new();
    for task in &tasks {
        if let Task::Grep { pattern, .. } = task {
            patterns
                .entry(pattern.clone())
                .or_insert_with(|| Regex::new(pattern).map_err(|e| e.to_string()));
        }
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(options.timeout_ms))
        .pool_max_idle_per_host(options.per_host)
        .user_agent("xstack-worker")
        .build()
        .map_err(|e| format!("cannot build HTTP client: {e}"))?;
    let (cancel_tx, cancel) = watch::channel(false);
    let ctx = Arc::new(Ctx {
        cwd: options.cwd.clone(),
        timeout: Duration::from_millis(options.timeout_ms),
        retries: options.retries,
        per_host: options.per_host.max(1),
        client,
        hosts: Mutex::new(HashMap::new()),
        patterns,
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
                let ours = gates[0].running.load(SeqCst);
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
    let writer = tokio::task::spawn_blocking(move || -> Result<Tally> {
        let mut out = BufWriter::new(file);
        let mut tally = Tally::default();
        while let Some(outcome) = results.blocking_recv() {
            tally.done += 1;
            serde_json::to_writer(&mut out, &outcome).map_err(|e| e.to_string())?;
            out.write_all(b"\n").map_err(|e| e.to_string())?;
            if outcome.cancelled {
                tally.cancelled += 1;
            } else if !outcome.ok {
                tally.failed += 1;
                if fail_fast {
                    let _ = cancel_tx.send(true);
                }
                if tally.failures.len() < show {
                    tally.failures.push(outcome);
                }
            }
        }
        out.flush().map_err(|e| e.to_string())?;
        Ok(tally)
    });

    let mut queues: [Vec<(usize, Task)>; 3] = Default::default();
    for (i, task) in tasks.into_iter().enumerate() {
        let pool = match task.pool() {
            Pool::Cpu => 0,
            Pool::Net => 1,
            Pool::File => 2,
        };
        queues[pool].push((i, task));
    }
    let dispatchers: Vec<_> = queues
        .into_iter()
        .zip(gates.iter().cloned())
        .map(|(queue, gate)| {
            tokio::spawn(dispatch(queue, gate, Arc::clone(&ctx), results_tx.clone()))
        })
        .collect();
    drop(results_tx);
    for dispatcher in dispatchers {
        dispatcher.await.map_err(|e| e.to_string())?;
    }
    let tally = writer.await.map_err(|e| e.to_string())??;
    retune.abort();
    let peak = |g: &Gate| g.peak.load(SeqCst);
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

#[derive(Default)]
struct Tally {
    done: usize,
    failed: usize,
    cancelled: usize,
    failures: Vec<Outcome>,
}

async fn dispatch(
    queue: Vec<(usize, Task)>,
    gate: Arc<Gate>,
    ctx: Arc<Ctx>,
    results: mpsc::Sender<Outcome>,
) {
    for (i, task) in queue {
        if *ctx.cancel.borrow() {
            return;
        }
        gate.enter().await;
        if *ctx.cancel.borrow() {
            gate.leave();
            return;
        }
        let (gate, ctx, results) = (Arc::clone(&gate), Arc::clone(&ctx), results.clone());
        tokio::spawn(async move {
            let outcome = execute(i, task, &ctx).await;
            gate.leave();
            let _ = results.send(outcome).await;
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
        Task::Http(request) => http(request, ctx).await,
        Task::Grep { path, pattern } => match ctx.patterns.get(&pattern) {
            Some(Ok(re)) => {
                let re = re.clone();
                blocking(move || grep(Path::new(&path), &re)).await
            }
            Some(Err(e)) => (false, -1, format!("bad pattern: {e}")),
            None => (false, -1, "pattern was not compiled".into()),
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

async fn shell(mut command: tokio::process::Command, ctx: &Ctx) -> Done {
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
    let pgid = child.id().map_or(0, |id| id as i32);
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
        kill_group(pgid);
    }
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
