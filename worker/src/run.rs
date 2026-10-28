//! Runs commands in parallel, each in a separate process group so cancellation can kill the whole group.
//! Optional per-core slot locks limit concurrency across worker processes on the machine.
use crate::state::CheckResult;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering::SeqCst};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

pub const OUTPUT_TAIL_BYTES: usize = 4000;
pub const DEFAULT_TIMEOUT_MS: u64 = 30 * 60_000;
/// How often a running command checks for cancellation and timeout; its exit is noticed immediately.
const CANCEL_POLL: Duration = Duration::from_millis(50);
const SLOT_RETRY_MIN: Duration = Duration::from_millis(5);
const SLOT_RETRY_MAX: Duration = Duration::from_millis(50);

/// Runs a shell command through `sh -c` or passes an argument vector directly without shell quoting.
#[derive(Clone, Debug)]
pub enum Spec {
    Shell(String),
    Argv(Vec<String>),
}

impl Spec {
    pub fn label(&self) -> String {
        match self {
            Spec::Shell(cmd) => cmd.clone(),
            Spec::Argv(argv) => argv.join(" "),
        }
    }
}

/// Per-core slot locks: `<lock_base>.<i>` for each i in `order`, tried in that order.
pub struct Slots {
    pub lock_base: PathBuf,
    pub order: Vec<usize>,
}

pub struct RunOptions<'a> {
    pub timeout_ms: u64,
    pub slots: Option<&'a Slots>,
    pub nice: bool,
    pub inherit: bool,
    pub cancel: Option<&'a AtomicBool>,
    pub pgid: Option<&'a AtomicI32>,
}

impl RunOptions<'_> {
    pub fn new(timeout_ms: u64) -> Self {
        RunOptions {
            timeout_ms,
            slots: None,
            nice: false,
            inherit: false,
            cancel: None,
            pgid: None,
        }
    }
}

pub fn cores() -> usize {
    thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
}

pub fn kill_group(pgid: i32) {
    if pgid > 0 {
        // SAFETY: a negative pid targets the process group this tool created for one command.
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
    }
}

/// Blocks until one slot is free and returns its locked file; the lock is released when the file drops.
/// Retries with a growing delay so a slot freed soon after the first attempt is taken quickly.
fn acquire_slot(slots: &Slots) -> Option<File> {
    let mut files = slots
        .order
        .iter()
        .map(|i| {
            OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(format!("{}.{i}", slots.lock_base.display()))
                .ok()
        })
        .collect::<Option<Vec<File>>>()?;
    let mut delay = SLOT_RETRY_MIN;
    loop {
        // SAFETY: flock on descriptors this function owns.
        let free = files.iter().position(
            |file| unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        );
        if let Some(i) = free {
            return Some(files.swap_remove(i));
        }
        sleep(delay);
        delay = (delay * 2).min(SLOT_RETRY_MAX);
    }
}

pub fn keep_tail(buffer: &Mutex<Vec<u8>>, chunk: &[u8]) {
    let mut out = buffer.lock().expect("output buffer");
    out.extend_from_slice(chunk);
    if out.len() > OUTPUT_TAIL_BYTES {
        let cut = out.len() - OUTPUT_TAIL_BYTES;
        out.drain(..cut);
    }
}

fn pump(
    mut source: impl Read + Send + 'static,
    buffer: Arc<Mutex<Vec<u8>>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        while let Ok(n) = source.read(&mut chunk) {
            if n == 0 {
                break;
            }
            keep_tail(&buffer, &chunk[..n]);
        }
    })
}

pub fn run_command(cwd: &Path, spec: &Spec, options: &RunOptions) -> CheckResult {
    let label = spec.label();
    let _slot = options.slots.and_then(acquire_slot);
    if options.cancel.is_some_and(|c| c.load(SeqCst)) {
        return CheckResult {
            cmd: label,
            exit: 130,
            output: "[cancelled before start]".into(),
            ..Default::default()
        };
    }
    let mut command = match spec {
        Spec::Shell(cmd) => {
            let mut c = Command::new("sh");
            c.arg("-c").arg(cmd);
            c
        }
        Spec::Argv(argv) => {
            let mut c = Command::new(&argv[0]);
            c.args(&argv[1..]);
            c
        }
    };
    command
        .current_dir(cwd)
        .env("XSTACK_WORKER", "1")
        .process_group(0);
    if options.nice {
        // SAFETY: setpriority is async-signal-safe and only lowers this child's own priority.
        unsafe {
            command.pre_exec(|| {
                libc::setpriority(libc::PRIO_PROCESS, 0, 10);
                Ok(())
            });
        }
    }
    if options.inherit {
        command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
    } else {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    }

    let started = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            return CheckResult {
                cmd: label,
                exit: 127,
                output: format!("cannot start: {e}"),
                ..Default::default()
            };
        }
    };
    let pgid = child.id() as i32;
    if let Some(slot) = options.pgid {
        slot.store(pgid, SeqCst);
    }
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let readers: Vec<_> = [
        child.stdout.take().map(|s| pump(s, Arc::clone(&buffer))),
        child.stderr.take().map(|s| pump(s, Arc::clone(&buffer))),
    ]
    .into_iter()
    .flatten()
    .collect();

    let (exited, exit_status) = mpsc::channel();
    let waiter = thread::spawn(move || {
        let _ = exited.send(child.wait());
    });

    let timeout = Duration::from_millis(options.timeout_ms);
    let mut timed_out = false;
    let mut killed = false;
    let status = loop {
        match exit_status.recv_timeout(CANCEL_POLL) {
            Ok(status) => break status.ok(),
            Err(RecvTimeoutError::Timeout) => {
                let cancelled = options.cancel.is_some_and(|c| c.load(SeqCst));
                if !killed && (cancelled || started.elapsed() > timeout) {
                    timed_out = !cancelled;
                    killed = true;
                    kill_group(pgid);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break None,
        }
    };
    let _ = waiter.join();
    for reader in readers {
        let _ = reader.join();
    }
    let mut output = String::from_utf8_lossy(&buffer.lock().expect("output buffer")).into_owned();
    let exit = if timed_out {
        output.push_str(&format!("\n[timed out after {} ms]", options.timeout_ms));
        124
    } else {
        status.and_then(|s| s.code()).unwrap_or(1)
    };
    CheckResult {
        cmd: label,
        exit,
        ms: started.elapsed().as_millis() as u64,
        output,
        timed_out,
        ..Default::default()
    }
}

/// Runs `work` over every item on at most `limit` threads, keeping results in input order.
pub fn pool<T: Sync, R: Send>(items: &[T], limit: usize, work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    thread::scope(|scope| {
        for _ in 0..limit.max(1).min(items.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, SeqCst);
                if i >= items.len() {
                    break;
                }
                let result = work(&items[i]);
                *results[i].lock().expect("result slot") = Some(result);
            });
        }
    });
    results
        .into_iter()
        .map(|m| {
            m.into_inner()
                .expect("result slot")
                .expect("every item ran")
        })
        .collect()
}

pub struct RunAll {
    /// `None` for a job cancelled before it started.
    pub results: Vec<Option<CheckResult>>,
    pub first_failed: Option<usize>,
    pub ms: u64,
}

/// Runs up to `limit` jobs at once. With `fail_fast`, the first failure stops the remaining jobs.
pub fn run_all(
    cwd: &Path,
    jobs: &[Spec],
    limit: usize,
    fail_fast: bool,
    timeout_ms: u64,
) -> RunAll {
    let started = Instant::now();
    let cancel = AtomicBool::new(false);
    let pgids: Vec<AtomicI32> = jobs.iter().map(|_| AtomicI32::new(0)).collect();
    let first_failed = Mutex::new(None);
    let indices: Vec<usize> = (0..jobs.len()).collect();
    let results = pool(&indices, limit, |&i| {
        if cancel.load(SeqCst) {
            return None;
        }
        let options = RunOptions {
            cancel: Some(&cancel),
            pgid: Some(&pgids[i]),
            ..RunOptions::new(timeout_ms)
        };
        let result = run_command(cwd, &jobs[i], &options);
        if result.exit != 0 {
            if fail_fast {
                if !cancel.swap(true, SeqCst) {
                    *first_failed.lock().expect("first failure") = Some(i);
                    for (j, pgid) in pgids.iter().enumerate() {
                        if j != i {
                            kill_group(pgid.load(SeqCst));
                        }
                    }
                }
            } else {
                first_failed.lock().expect("first failure").get_or_insert(i);
            }
        }
        Some(result)
    });
    RunAll {
        results,
        first_failed: first_failed.into_inner().expect("first failure"),
        ms: started.elapsed().as_millis() as u64,
    }
}

/// Runs all checks in the foreground without waiting for slots. The first failure stops the rest.
pub fn fast(cwd: &Path, checks: &[String], timeout_ms: u64) -> RunAll {
    let jobs: Vec<Spec> = checks.iter().cloned().map(Spec::Shell).collect();
    run_all(cwd, &jobs, jobs.len(), true, timeout_ms)
}

/// Assigns input occurrences to size-balanced queues, starting with the largest.
pub fn partition_indices_by_size(
    items: &[String],
    jobs: usize,
    size: impl Fn(&str) -> u64,
) -> Vec<Vec<usize>> {
    let mut sized: Vec<(u64, usize)> = items
        .iter()
        .enumerate()
        .map(|(index, item)| (size(item), index))
        .collect();
    sized.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| items[a.1].cmp(&items[b.1]))
            .then_with(|| a.1.cmp(&b.1))
    });
    let mut groups: Vec<(u64, Vec<usize>)> =
        vec![(0, Vec::new()); jobs.max(1).min(items.len().max(1))];
    for (weight, index) in sized {
        let lightest = groups
            .iter_mut()
            .min_by_key(|g| g.0)
            .expect("at least one group");
        lightest.0 += weight.max(1);
        lightest.1.push(index);
    }
    groups
        .into_iter()
        .map(|g| g.1)
        .filter(|g| !g.is_empty())
        .collect()
}

pub fn file_size(path: &str) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

pub(crate) fn item_jobs(items: &[String], command: &[String]) -> Vec<Spec> {
    let has_slot = command.iter().any(|arg| arg.contains("{}"));
    items
        .iter()
        .map(|item| {
            let mut argv: Vec<String> = command.iter().map(|arg| arg.replace("{}", item)).collect();
            if !has_slot {
                argv.push(item.clone());
            }
            Spec::Argv(argv)
        })
        .collect()
}
