//! Queues commits for verification and looks up their results.
use crate::git::{git, is_ancestor, lines, resolve_commit, short, tree_of};
use crate::run::{cores, run_command, RunOptions, Slots, Spec, DEFAULT_TIMEOUT_MS};
use crate::state::{
    alive, config_key, home, now_ms, read_state, with_state, CheckResult, Config, Entry, State,
    Status, Worker,
};
use crate::Result;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(500);
const MAX_DEPTH_SCAN: usize = 200;

/// Runs a check containing `{shard}` once per slot.
/// Replaces `{shard}` with the index and `{shards}` with the total slot count.
pub fn expand_checks(checks: &[String], parallel: usize) -> Vec<String> {
    checks
        .iter()
        .flat_map(|cmd| {
            if cmd.contains("{shard}") {
                (1..=parallel)
                    .map(|i| {
                        cmd.replace("{shard}", &i.to_string())
                            .replace("{shards}", &parallel.to_string())
                    })
                    .collect()
            } else {
                vec![cmd.clone()]
            }
        })
        .collect()
}

#[derive(Default)]
pub struct InitOptions {
    pub checks: Vec<String>,
    pub copy: Vec<String>,
    pub timeout_ms: Option<u64>,
    pub base: Option<String>,
    pub parallel: Option<usize>,
    pub commits: Option<usize>,
    pub job_mem_mb: Option<u64>,
}

/// Records the checks and queues the base commit to identify preexisting failures.
pub fn init(repo: &Path, options: InitOptions) -> Result<Entry> {
    if options.checks.is_empty() {
        return Err("init needs at least one --check".into());
    }
    let config = Config {
        checks: options.checks,
        copy: options.copy,
        timeout_ms: options.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS),
        parallel: options.parallel.map(|n| n.max(1)),
        commits: options.commits.unwrap_or(2).max(1),
        job_mem_mb: options
            .job_mem_mb
            .unwrap_or(crate::probe::DEFAULT_JOB_MEM_MB),
    };
    with_state(repo, |state| {
        state.config = Some(config);
        Ok(())
    })?;
    enqueue(repo, options.base.as_deref().unwrap_or("HEAD"), false, true)
}

/// Queues a commit. With `coalesce`, skips queued ancestors in favor of this commit.
/// Leaves running checks alone so new commits cannot repeatedly interrupt them.
pub fn enqueue(repo: &Path, reference: &str, coalesce: bool, baseline: bool) -> Result<Entry> {
    let commit = resolve_commit(repo, reference)?;
    let ancestors: HashSet<String> = if coalesce {
        read_state(repo)?
            .entries
            .values()
            .filter(|e| {
                e.status == Status::Queued
                    && e.sha != commit.sha
                    && is_ancestor(repo, &e.sha, &commit.sha)
            })
            .map(|e| e.sha.clone())
            .collect()
    } else {
        HashSet::new()
    };
    with_state(repo, |state| {
        let key = config_key(
            state
                .config
                .as_ref()
                .ok_or("not initialized: run worker init first")?,
        );
        if let Some(existing) = state.entries.get_mut(&commit.sha) {
            if existing.status != Status::Skipped
                && (existing.config_key == key || !existing.status.is_final())
            {
                if !coalesce {
                    existing.explicit = true;
                }
                return Ok(existing.clone());
            }
        }
        let reused = state.own_result(&commit.sha, &commit.tree).cloned();
        let entry = match &reused {
            Some(done) => Entry {
                sha: commit.sha.clone(),
                tree: commit.tree.clone(),
                status: done.status,
                checks: done.checks.clone(),
                config_key: key,
                reused_from: Some(done.sha.clone()),
                baseline,
                explicit: !coalesce,
                queued_at: now_ms(),
                started_at: None,
                ended_at: Some(now_ms()),
                pid: None,
                superseded_by: None,
            },
            None => Entry {
                sha: commit.sha.clone(),
                tree: commit.tree.clone(),
                status: Status::Queued,
                checks: Vec::new(),
                config_key: key,
                reused_from: None,
                baseline,
                explicit: !coalesce,
                queued_at: now_ms(),
                started_at: None,
                ended_at: None,
                pid: None,
                superseded_by: None,
            },
        };
        state.entries.insert(commit.sha.clone(), entry.clone());
        if !state.order.contains(&commit.sha) {
            state.order.push(commit.sha.clone());
        }
        if reused.is_none() {
            for other in state.entries.values_mut() {
                if ancestors.contains(&other.sha)
                    && other.status == Status::Queued
                    && !other.baseline
                    && !other.explicit
                {
                    other.status = Status::Skipped;
                    other.superseded_by = Some(commit.sha.clone());
                }
            }
        }
        Ok(entry)
    })
}

fn has_work(state: &State) -> bool {
    state.entries.values().any(|e| {
        e.status == Status::Queued || (e.status == Status::Running && !alive(e.pid.unwrap_or(0)))
    })
}

/// Starts a detached worker running `exe drain --repo <repo>` unless one is alive or nothing waits.
pub fn start_worker(repo: &Path, exe: &Path) -> Result<bool> {
    let log_path = home(repo)?.join("worker.log");
    with_state(repo, |state| {
        if state.worker.as_ref().is_some_and(|w| alive(w.pid)) || !has_work(state) {
            return Ok(false);
        }
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| e.to_string())?;
        let log_err = log.try_clone().map_err(|e| e.to_string())?;
        let mut command = Command::new(exe);
        command
            .arg("drain")
            .arg("--repo")
            .arg(repo)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(log_err);
        // SAFETY: setsid detaches the worker from this terminal's session so it outlives the caller.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let child = command
            .spawn()
            .map_err(|e| format!("cannot start worker: {e}"))?;
        state.worker = Some(Worker {
            pid: child.id() as i32,
        });
        Ok(true)
    })
}

pub fn slot_lock_base(repo: &Path) -> Result<PathBuf> {
    let root = git(repo, &["rev-parse", "--show-toplevel"])?;
    let name = Path::new(&root)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".into());
    Ok(std::env::temp_dir().join(format!("{name}-tests.slot")))
}

/// Runs a foreground command, trying higher-numbered slots first to reduce contention with background checks.
pub fn run_in_slot(repo: &Path, cmd: &str) -> Result<i32> {
    let total = cores();
    let slots = Slots {
        lock_base: slot_lock_base(repo)?,
        order: (0..total).rev().collect(),
    };
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let options = RunOptions {
        slots: Some(&slots),
        inherit: true,
        ..RunOptions::new(u64::MAX / 2)
    };
    Ok(run_command(&cwd, &Spec::Shell(cmd.into()), &options).exit)
}

/// Waits for the commit's result, an identical tree's result, or its superseding descendant's result.
pub fn wait_for(
    repo: &Path,
    reference: &str,
    timeout: Option<Duration>,
    exe: Option<&Path>,
) -> Result<Option<Entry>> {
    let commit = resolve_commit(repo, reference)?;
    let started = Instant::now();
    loop {
        let state = read_state(repo)?;
        if let Some(result) = state.result_for(&commit.sha, &commit.tree, false) {
            return Ok(Some(result.clone()));
        }
        if !state.entries.contains_key(&commit.sha) {
            return Err(format!(
                "{} is not queued: run worker enqueue {}",
                short(&commit.sha),
                short(&commit.sha)
            ));
        }
        if let Some(exe) = exe {
            start_worker(repo, exe)?;
        }
        if timeout.is_some_and(|t| started.elapsed() > t) {
            return Ok(None);
        }
        sleep(POLL);
    }
}

/// Commits on HEAD's first-parent line since the newest commit that passed.
pub fn unverified_depth(repo: &Path) -> Result<usize> {
    let state = read_state(repo)?;
    let history = git(
        repo,
        &[
            "rev-list",
            "--first-parent",
            &format!("--max-count={MAX_DEPTH_SCAN}"),
            "HEAD",
        ],
    )?;
    let mut depth = 0;
    for sha in lines(&history) {
        let tree = tree_of(repo, &sha)?;
        if state
            .result_for(&sha, &tree, false)
            .is_some_and(|e| e.status == Status::Pass)
        {
            return Ok(depth);
        }
        if state.entries.get(&sha).is_some_and(|e| e.baseline) {
            return Ok(depth);
        }
        depth += 1;
    }
    Ok(depth)
}

pub struct Row {
    pub sha: String,
    pub subject: String,
    pub status: String,
    pub covered_by: Option<(String, Status)>,
    pub checks: Vec<CheckResult>,
}

pub fn status(repo: &Path, base: &str) -> Result<Vec<Row>> {
    let state = read_state(repo)?;
    lines(&git(
        repo,
        &["rev-list", "--reverse", &format!("{base}..HEAD")],
    )?)
    .into_iter()
    .map(|sha| {
        let tree = tree_of(repo, &sha)?;
        let own = state.result_for(&sha, &tree, true);
        let covered = if own.is_some() {
            None
        } else {
            state.result_for(&sha, &tree, false)
        };
        let entry = state.entries.get(&sha);
        let status = match (own, entry) {
            (Some(own), _) => own.status.as_str().to_string(),
            (None, Some(e)) if !e.status.is_final() => e.status.as_str().to_string(),
            _ => "none".to_string(),
        };
        Ok(Row {
            subject: git(repo, &["log", "-1", "--format=%s", &sha])?,
            status,
            covered_by: covered.map(|c| (c.sha.clone(), c.status)),
            checks: own
                .or(covered)
                .map(|e| e.checks.clone())
                .unwrap_or_default(),
            sha,
        })
    })
    .collect()
}

pub struct GateResult {
    pub ok: bool,
    pub problems: Vec<String>,
    pub queued: Vec<String>,
}

fn failed_commands(entry: &Entry) -> String {
    entry
        .checks
        .iter()
        .filter(|c| c.exit != 0)
        .map(|c| c.cmd.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

/// Requires HEAD to have passed, or every commit since base with `each`.
/// Queues unverified commits without coalescing them.
pub fn gate(repo: &Path, base: &str, each: bool) -> Result<GateResult> {
    let state = read_state(repo)?;
    let mut problems = Vec::new();
    let mut queued = Vec::new();
    if let Some(baseline) = state.entries.values().find(|e| e.baseline) {
        if let Some(result) = state
            .result_for(&baseline.sha, &baseline.tree, true)
            .filter(|r| r.status == Status::Fail)
        {
            problems.push(format!(
                "baseline {} was already red before this work ({}). Fix it or drop that check, or the gate proves nothing",
                short(&baseline.sha),
                failed_commands(result)
            ));
        }
    }
    let targets = if each {
        lines(&git(
            repo,
            &["rev-list", "--reverse", &format!("{base}..HEAD")],
        )?)
    } else {
        vec![resolve_commit(repo, "HEAD")?.sha]
    };
    for sha in targets {
        let tree = tree_of(repo, &sha)?;
        match state.result_for(&sha, &tree, each) {
            Some(result) if result.status == Status::Pass => {}
            Some(result) => problems.push(format!(
                "{} failed: {}",
                short(&sha),
                failed_commands(result)
            )),
            None => {
                enqueue(repo, &sha, false, false)?;
                problems.push(format!("{} is not verified yet, queued", short(&sha)));
                queued.push(sha);
            }
        }
    }
    Ok(GateResult {
        ok: problems.is_empty(),
        problems,
        queued,
    })
}
