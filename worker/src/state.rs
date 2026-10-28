use crate::git::git;
use crate::probe::{DEFAULT_JOB_MEM_MB, RESERVE};
use crate::run::cores;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const LOCK_STALE: Duration = Duration::from_secs(60);
const LOCK_WAIT: Duration = Duration::from_secs(30);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub checks: Vec<String>,
    pub copy: Vec<String>,
    pub timeout_ms: u64,
    /// Sets concurrency and shard count. With `None`, the worker adjusts concurrency to available CPU and memory.
    #[serde(default)]
    pub parallel: Option<usize>,
    /// Sets how many commits to verify at once, each in a separate worktree.
    #[serde(default = "default_commits")]
    pub commits: usize,
    /// Estimates memory use per job in MB to limit concurrency.
    #[serde(default = "default_job_mem_mb")]
    pub job_mem_mb: u64,
}

fn default_commits() -> usize {
    2
}

fn default_job_mem_mb() -> u64 {
    DEFAULT_JOB_MEM_MB
}

impl Config {
    /// Uses the fixed shard count, or the available core count minus `RESERVE`, with a minimum of one.
    pub fn shards(&self) -> usize {
        self.parallel
            .unwrap_or_else(|| cores().saturating_sub(RESERVE))
            .max(1)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub cmd: String,
    pub exit: i32,
    pub ms: u64,
    pub output: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub timed_out: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub flaky: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_output: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub preexisting: bool,
    /// Stopped because another check of the same commit failed first.
    #[serde(default, skip_serializing_if = "is_false")]
    pub cancelled: bool,
}

/// Records the last run of a check to determine its scheduling order.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default)]
pub struct History {
    pub ms: u64,
    pub failed: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Running,
    Skipped,
    Pass,
    Fail,
}

impl Status {
    pub fn is_final(self) -> bool {
        matches!(self, Status::Pass | Status::Fail)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Queued => "queued",
            Status::Running => "running",
            Status::Skipped => "skipped",
            Status::Pass => "pass",
            Status::Fail => "fail",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub sha: String,
    pub tree: String,
    pub status: Status,
    #[serde(default)]
    pub checks: Vec<CheckResult>,
    pub config_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reused_from: Option<String>,
    #[serde(default)]
    pub baseline: bool,
    #[serde(default)]
    pub explicit: bool,
    pub queued_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Worker {
    pub pid: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct State {
    pub config: Option<Config>,
    #[serde(default)]
    pub entries: BTreeMap<String, Entry>,
    #[serde(default)]
    pub order: Vec<String>,
    #[serde(default)]
    pub worker: Option<Worker>,
    #[serde(default)]
    pub history: BTreeMap<String, History>,
}

/// Identifies the configured checks and copy paths. Changing either invalidates previous results.
pub fn config_key(config: &Config) -> String {
    serde_json::to_string(&(&config.checks, &config.copy)).expect("config serializes")
}

impl State {
    fn valid(&self, entry: &Entry) -> bool {
        entry.status.is_final()
            && self
                .config
                .as_ref()
                .is_some_and(|c| config_key(c) == entry.config_key)
    }

    /// Returns the commit's result or a result for an identical tree, such as after a content-preserving amend or rebase.
    pub fn own_result(&self, sha: &str, tree: &str) -> Option<&Entry> {
        if let Some(entry) = self.entries.get(sha).filter(|e| self.valid(e)) {
            return Some(entry);
        }
        self.entries
            .values()
            .find(|e| e.tree == tree && self.valid(e))
    }

    /// Without `own`, a commit skipped in the queue is covered by the descendant that superseded it.
    pub fn result_for(&self, sha: &str, tree: &str, own: bool) -> Option<&Entry> {
        let mine = self.own_result(sha, tree);
        if mine.is_some() || own {
            return mine;
        }
        let mut seen = HashSet::new();
        let mut entry = self.entries.get(sha);
        while let Some(current) = entry {
            if current.status != Status::Skipped || !seen.insert(current.sha.clone()) {
                break;
            }
            entry = current
                .superseded_by
                .as_ref()
                .and_then(|s| self.entries.get(s));
            if let Some(next) = entry.filter(|e| self.valid(e)) {
                return Some(next);
            }
        }
        None
    }
}

pub fn home(repo: &Path) -> Result<PathBuf> {
    let common = git(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let dir = PathBuf::from(common).join("xstack-worker");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    Ok(dir)
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 only checks that the process exists; it delivers nothing.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn load(file: &Path) -> Result<State> {
    match fs::read(file) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|e| format!("corrupt {}: {e}", file.display()))
        }
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(format!("cannot read {}: {e}", file.display())),
    }
}

pub fn read_state(repo: &Path) -> Result<State> {
    load(&home(repo)?.join("state.json"))
}

struct LockGuard(PathBuf);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Holds a directory lock while updating state and writes the result atomically.
/// Writes nothing if `change` returns an error.
pub fn with_state<T>(repo: &Path, change: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
    let dir = home(repo)?;
    let lock = dir.join("lock");
    let started = Instant::now();
    loop {
        match fs::create_dir(&lock) {
            Ok(()) => break,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                let stale = fs::metadata(&lock)
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t.elapsed().unwrap_or_default() > LOCK_STALE);
                if stale {
                    let _ = fs::remove_dir_all(&lock);
                }
                if started.elapsed() > LOCK_WAIT {
                    return Err(format!(
                        "state lock stayed busy for {}s: {}",
                        LOCK_WAIT.as_secs(),
                        lock.display()
                    ));
                }
                sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("cannot lock {}: {e}", lock.display())),
        }
    }
    let _guard = LockGuard(lock);
    let file = dir.join("state.json");
    let mut state = load(&file)?;
    let out = change(&mut state)?;
    let tmp = dir.join(format!("state.json.{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(&state).map_err(|e| e.to_string())?;
    fs::write(&tmp, bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, &file).map_err(|e| format!("cannot replace {}: {e}", file.display()))?;
    Ok(out)
}
