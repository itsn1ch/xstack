//! Creates one job file per check for each claimed commit.
//! The worker samples CPU load and available memory each tick to adjust concurrency.
//! A new check failure stops the commit's remaining jobs.
use crate::git::git;
use crate::probe::{Meter, RESERVE};
use crate::queue::{expand_checks, slot_lock_base};
use crate::run::{run_command, RunOptions, Slots, Spec};
use crate::spool::{job_name, short_sha, Job, Spool, Stage};
use crate::state::{alive, config_key, home, now_ms, read_state, with_state, CheckResult, Config, History, Status};
use crate::Result;
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

const TICK: Duration = Duration::from_millis(100);

/// A commit this worker claimed. Its worktree stays locked until every one of its jobs reports.
struct Active {
    fail_fast: bool,
    cancel: Arc<AtomicBool>,
    worktree: usize,
    _lock: File,
}

struct Claimed {
    sha: String,
    seq: usize,
    baseline: bool,
    red_at_baseline: HashSet<String>,
    history: BTreeMap<String, History>,
}

/// Orders checks by previous failure first, then longest duration. Treats unknown durations as longest.
pub fn rank(cmds: &[String], history: &BTreeMap<String, History>) -> Vec<usize> {
    let mut order: Vec<usize> = (0..cmds.len()).collect();
    order.sort_by_key(|&i| {
        let last = history.get(&cmds[i]);
        (!last.is_some_and(|h| h.failed), Reverse(last.map_or(u64::MAX, |h| h.ms)), i)
    });
    order
}

/// Requeues commits whose worker died and clears job files that no live worker owns.
fn recover(repo: &Path, spool: &Spool) -> Result<()> {
    with_state(repo, |state| {
        let mut live = HashSet::new();
        for entry in state.entries.values_mut().filter(|e| e.status == Status::Running) {
            if alive(entry.pid.unwrap_or(0)) {
                live.insert(short_sha(&entry.sha).to_string());
            } else {
                entry.status = Status::Queued;
                entry.pid = None;
            }
        }
        for sha in spool.shas().difference(&live) {
            spool.purge(sha);
        }
        Ok(())
    })
}

/// Claims the oldest queued commit. Runs the baseline alone before other commits.
/// Later runs do not cancel other jobs when a check repeats a baseline failure.
fn claim_commit(repo: &Path, me: i32) -> Result<Option<Claimed>> {
    with_state(repo, |state| {
        let Some(seq) = state.order.iter().position(|sha| state.entries.get(sha).is_some_and(|e| e.status == Status::Queued)) else {
            return Ok(None);
        };
        let sha = state.order[seq].clone();
        let baseline_pending = state.entries.values().any(|e| e.baseline && e.status == Status::Running);
        if baseline_pending || (!state.entries[&sha].baseline && state.entries.values().any(|e| e.baseline && e.status == Status::Queued)) {
            return Ok(None);
        }
        let red_at_baseline = state
            .entries
            .values()
            .filter(|e| e.baseline && e.status.is_final() && e.sha != sha)
            .flat_map(|e| e.checks.iter().filter(|c| c.exit != 0).map(|c| c.cmd.clone()))
            .collect();
        let history = state.history.clone();
        let entry = state.entries.get_mut(&sha).expect("queued entry exists");
        entry.status = Status::Running;
        entry.pid = Some(me);
        entry.started_at = Some(now_ms());
        Ok(Some(Claimed { baseline: entry.baseline, sha, seq, red_at_baseline, history }))
    })
}

fn lock_worktree(dir: &Path, k: usize) -> Option<File> {
    let file = OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(format!("{k}.lock"))).ok()?;
    // SAFETY: flock on a descriptor this function owns.
    (unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0).then_some(file)
}

fn copy_into(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for item in fs::read_dir(from)? {
            let item = item?;
            copy_into(&item.path(), &to.join(item.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(from, to).map(|_| ())
    }
}

fn prepare_worktree(repo: &Path, worktree: &Path, sha: &str, copy: &[String]) -> Result<()> {
    let root = PathBuf::from(git(repo, &["rev-parse", "--show-toplevel"])?);
    if worktree.join(".git").exists() {
        git(worktree, &["checkout", "--detach", "--force", sha])?;
        // -fd without -x keeps ignored files such as installed dependencies, so warm caches survive between runs.
        git(worktree, &["clean", "-fdq"])?;
    } else {
        let _ = fs::remove_dir_all(worktree);
        git(repo, &["worktree", "prune"])?;
        git(repo, &["worktree", "add", "--detach", "--force", &worktree.to_string_lossy(), sha])?;
    }
    for rel in copy {
        let from = root.join(rel);
        if from.exists() {
            copy_into(&from, &worktree.join(rel)).map_err(|e| format!("cannot copy {rel}: {e}"))?;
        }
    }
    Ok(())
}

/// Checks out the commit and writes one job file per check, in run order.
fn start_commit(repo: &Path, spool: &Spool, claimed: &Claimed, worktree: &Path, config: &Config) -> Result<()> {
    prepare_worktree(repo, worktree, &claimed.sha, &config.copy)?;
    let cmds = expand_checks(&config.checks, config.shards());
    for (position, index) in rank(&cmds, &claimed.history).into_iter().enumerate() {
        let job = Job {
            sha: claimed.sha.clone(),
            index,
            cmd: cmds[index].clone(),
            worktree: worktree.to_path_buf(),
            timeout_ms: config.timeout_ms,
            preexisting: claimed.red_at_baseline.contains(&cmds[index]),
            result: None,
        };
        spool.push(&job_name(claimed.seq, position, &claimed.sha), &job)?;
    }
    Ok(())
}

/// Saves a commit's completed results in state, then clears its job files.
fn finish_commit(repo: &Path, spool: &Spool, sha: &str, config: &Config, extra: Vec<CheckResult>) -> Result<()> {
    let mut jobs: Vec<Job> = spool.list(Stage::Done, Some(sha)).iter().filter_map(|n| spool.read(Stage::Done, n)).collect();
    jobs.sort_by_key(|j| j.index);
    let mut checks: Vec<CheckResult> = jobs.into_iter().filter_map(|j| j.result).filter(|r| !r.cancelled).collect();
    checks.extend(extra);
    spool.purge(sha);
    with_state(repo, |state| {
        let red_at_baseline: HashSet<String> = state
            .entries
            .values()
            .filter(|e| e.baseline && e.status.is_final() && e.sha != sha)
            .flat_map(|e| e.checks.iter().filter(|c| c.exit != 0).map(|c| c.cmd.clone()))
            .collect();
        for check in &mut checks {
            check.preexisting = check.exit != 0 && red_at_baseline.contains(&check.cmd);
            state.history.insert(check.cmd.clone(), History { ms: check.ms, failed: check.exit != 0 && !check.preexisting });
        }
        if let Some(entry) = state.entries.get_mut(sha) {
            entry.status = if checks.iter().all(|c| c.exit == 0) { Status::Pass } else { Status::Fail };
            entry.checks = checks;
            entry.config_key = config_key(config);
            entry.ended_at = Some(now_ms());
            entry.pid = None;
        }
        Ok(())
    })
}

/// Runs a job at low priority in a shared slot. Retries a failed check once unless it timed out.
fn run_job(job: &Job, slots: &Slots, cancel: &AtomicBool) -> CheckResult {
    let options = RunOptions { slots: Some(slots), nice: true, cancel: Some(cancel), ..RunOptions::new(job.timeout_ms) };
    let spec = Spec::Shell(job.cmd.clone());
    let stopped = |result: CheckResult| CheckResult { cancelled: true, ..result };
    let first = run_command(&job.worktree, &spec, &options);
    if first.exit != 0 && cancel.load(SeqCst) {
        return stopped(first);
    }
    if first.exit == 0 || first.timed_out {
        return first;
    }
    let second = run_command(&job.worktree, &spec, &options);
    if second.exit == 0 {
        CheckResult { flaky: true, first_output: Some(first.output), ..second }
    } else if cancel.load(SeqCst) {
        stopped(second)
    } else {
        second
    }
}

/// Drains the queue. Multiple workers can run at once, each processing only the commits it claimed.
pub fn drain(repo: &Path) -> Result<()> {
    let me = std::process::id() as i32;
    let spool = Spool::open(repo)?;
    recover(repo, &spool)?;
    let config = read_state(repo)?.config.ok_or("not initialized: run worker init first")?;
    let worktrees = home(repo)?.join("worktrees");
    fs::create_dir_all(&worktrees).map_err(|e| format!("cannot create {}: {e}", worktrees.display()))?;
    let slots = Slots { lock_base: slot_lock_base(repo)?, order: (0..config.shards()).collect() };
    let (mut meter, mut power) = Meter::start();
    let (tx, rx) = mpsc::channel::<(String, Job)>();
    let mut active: HashMap<String, Active> = HashMap::new();
    let mut running = 0usize;

    thread::scope(|scope| -> Result<()> {
        loop {
            while active.len() < config.commits {
                let used: HashSet<usize> = active.values().map(|a| a.worktree).collect();
                let free = (0..config.commits).filter(|k| !used.contains(k)).find_map(|k| lock_worktree(&worktrees, k).map(|lock| (k, lock)));
                let Some((k, lock)) = free else { break };
                let Some(claimed) = claim_commit(repo, me)? else { break };
                match start_commit(repo, &spool, &claimed, &worktrees.join(k.to_string()), &config) {
                    Ok(()) => {
                        let cancel = Arc::new(AtomicBool::new(false));
                        active.insert(claimed.sha.clone(), Active { fail_fast: !claimed.baseline, cancel, worktree: k, _lock: lock });
                    }
                    Err(message) => {
                        spool.purge(&claimed.sha);
                        let failure = CheckResult { cmd: "prepare worktree".into(), exit: 1, output: message, ..Default::default() };
                        finish_commit(repo, &spool, &claimed.sha, &config, vec![failure])?;
                    }
                }
            }

            let width = config.parallel.unwrap_or_else(|| power.width(RESERVE, running, config.job_mem_mb));
            let mut queued: Vec<String> = active.keys().flat_map(|sha| spool.list(Stage::Queued, Some(sha))).collect();
            queued.sort();
            for name in queued {
                if running >= width {
                    break;
                }
                let Some(job) = spool.claim(&name) else { continue };
                let cancel = Arc::clone(&active[&job.sha].cancel);
                let (tx, slots) = (tx.clone(), &slots);
                running += 1;
                scope.spawn(move || {
                    let result = run_job(&job, slots, &cancel);
                    let _ = tx.send((name, Job { result: Some(result), ..job }));
                });
            }

            if running == 0 && active.is_empty() {
                let idle = with_state(repo, |state| {
                    let waiting = state.entries.values().any(|e| e.status == Status::Queued);
                    if !waiting && state.worker.as_ref().is_some_and(|w| w.pid == me) {
                        state.worker = None;
                    }
                    Ok(!waiting)
                })?;
                if idle {
                    return Ok(());
                }
                thread::sleep(TICK);
                power = meter.read();
                continue;
            }

            let mut finished: Vec<(String, Job)> = rx.recv_timeout(TICK).into_iter().collect();
            finished.extend(rx.try_iter());
            for (name, job) in finished {
                running -= 1;
                spool.finish(&name, &job)?;
                let failed = job.result.as_ref().is_some_and(|r| r.exit != 0 && !r.cancelled) && !job.preexisting;
                if let Some(commit) = active.get(&job.sha) {
                    if failed && commit.fail_fast && !commit.cancel.swap(true, SeqCst) {
                        for sibling in spool.list(Stage::Queued, Some(&job.sha)) {
                            spool.remove(Stage::Queued, &sibling);
                        }
                    }
                }
                if spool.list(Stage::Queued, Some(&job.sha)).is_empty() && spool.list(Stage::Running, Some(&job.sha)).is_empty() {
                    finish_commit(repo, &spool, &job.sha, &config, Vec::new())?;
                    active.remove(&job.sha);
                }
            }
            power = meter.read();
        }
    })
}
