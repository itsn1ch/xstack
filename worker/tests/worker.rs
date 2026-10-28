use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use std::collections::BTreeMap;
use xstack_worker::budget;
use xstack_worker::probe::Power;
use xstack_worker::queue::{self, InitOptions};
use xstack_worker::run::{self, partition_by_size};
use xstack_worker::scheduler::{self, rank};
use xstack_worker::state::{read_state, with_state, Entry, History, Status};

const EXE: &str = env!("CARGO_BIN_EXE_worker");
static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("xstack-worker-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn sh(cwd: &Path, cmd: &str) -> String {
    let out = Command::new("sh").arg("-c").arg(cmd).current_dir(cwd).output().unwrap();
    assert!(out.status.success(), "{cmd}\n{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn make_repo() -> PathBuf {
    let dir = temp_dir("repo");
    sh(&dir, "git init -q -b main && git config user.email t@t && git config user.name t && git config commit.gpgsign false");
    fs::write(dir.join("README.md"), "base\n").unwrap();
    sh(&dir, "git add -A && git commit -qm base");
    dir
}

fn commit(dir: &Path, file: &str) -> String {
    fs::write(dir.join(file), "x\n").unwrap();
    sh(dir, &format!("git add -A && git commit -qm {file}"));
    head(dir)
}

fn head(dir: &Path) -> String {
    sh(dir, "git rev-parse HEAD")
}

fn init(dir: &Path, checks: &[&str]) -> Entry {
    init_with(dir, checks, |_| {})
}

fn init_with(dir: &Path, checks: &[&str], tweak: impl FnOnce(&mut InitOptions)) -> Entry {
    let mut options = InitOptions { checks: checks.iter().map(|c| c.to_string()).collect(), ..Default::default() };
    tweak(&mut options);
    queue::init(dir, options).unwrap()
}

fn enqueue(dir: &Path) -> Entry {
    queue::enqueue(dir, "HEAD", true, false).unwrap()
}

fn entry(dir: &Path, sha: &str) -> Entry {
    read_state(dir).unwrap().entries[sha].clone()
}

fn set_entry(dir: &Path, sha: &str, change: impl FnOnce(&mut Entry)) {
    with_state(dir, |state| {
        change(state.entries.get_mut(sha).unwrap());
        Ok(())
    })
    .unwrap();
}

/// A log file outside the repo; a check appends to it so tests can prove what ran and what was skipped.
fn log_file() -> PathBuf {
    temp_dir("log").join("ran")
}

fn ran(log: &Path) -> Vec<String> {
    fs::read_to_string(log).unwrap_or_default().lines().map(str::to_string).collect()
}

fn cli(dir: &Path, args: &[&str]) -> Output {
    Command::new(EXE).args(args).arg("--repo").arg(dir).output().unwrap()
}

fn cli_stdin(cwd: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(EXE).args(args).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn text(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[test]
fn a_passing_commit_is_recorded_and_opens_the_gate() {
    let dir = make_repo();
    let base = head(&dir);
    init(&dir, &["test -f README.md"]);
    let a = commit(&dir, "a.txt");
    enqueue(&dir);
    scheduler::drain(&dir).unwrap();
    assert_eq!(entry(&dir, &a).status, Status::Pass);
    assert!(queue::gate(&dir, &base, false).unwrap().ok);
}

#[test]
fn a_failing_commit_keeps_its_output_and_blocks_the_gate() {
    let dir = make_repo();
    let base = head(&dir);
    init(&dir, &["test ! -f broken.txt || { echo broken found; exit 3; }"]);
    let a = commit(&dir, "broken.txt");
    enqueue(&dir);
    scheduler::drain(&dir).unwrap();
    let e = entry(&dir, &a);
    assert_eq!(e.status, Status::Fail);
    assert_eq!(e.checks[0].exit, 3);
    assert!(e.checks[0].output.contains("broken found"));
    let gate = queue::gate(&dir, &base, false).unwrap();
    assert!(!gate.ok);
    assert!(gate.problems.join("\n").contains(&format!("{} failed", &a[..7])));
}

#[test]
fn a_check_that_fails_once_then_passes_is_flaky_not_red() {
    let dir = make_repo();
    let counter = temp_dir("count").join("n");
    let flaky = format!("n=$(cat {c} 2>/dev/null || echo 0); n=$((n+1)); echo $n > {c}; [ $n -ge 2 ]", c = counter.display());
    let baseline = init(&dir, &[&flaky]);
    scheduler::drain(&dir).unwrap();
    let e = entry(&dir, &baseline.sha);
    assert_eq!(e.status, Status::Pass);
    assert!(e.checks[0].flaky);
    assert!(e.checks[0].first_output.is_some());
}

#[test]
fn waiting_commits_coalesce_into_the_newest_and_the_skipped_one_is_covered() {
    let dir = make_repo();
    let log = log_file();
    let baseline = init(&dir, &[&format!("git rev-parse HEAD >> {}", log.display())]);
    let a = commit(&dir, "a.txt");
    enqueue(&dir);
    let b = commit(&dir, "b.txt");
    enqueue(&dir);
    assert_eq!(entry(&dir, &a).status, Status::Skipped);
    scheduler::drain(&dir).unwrap();
    assert_eq!(ran(&log), vec![baseline.sha, b.clone()]);
    let covered = queue::wait_for(&dir, &a, Some(Duration::from_secs(5)), None).unwrap().unwrap();
    assert_eq!(covered.sha, b);
    assert_eq!(covered.status, Status::Pass);
}

#[test]
fn a_running_check_is_never_cancelled_by_a_newer_commit() {
    let dir = make_repo();
    init(&dir, &["true"]);
    let a = commit(&dir, "a.txt");
    enqueue(&dir);
    set_entry(&dir, &a, |e| {
        e.status = Status::Running;
        e.pid = Some(std::process::id() as i32);
    });
    commit(&dir, "b.txt");
    enqueue(&dir);
    assert_eq!(entry(&dir, &a).status, Status::Running);
}

#[test]
fn an_amend_that_keeps_the_tree_reuses_the_result_without_running_again() {
    let dir = make_repo();
    let log = log_file();
    init(&dir, &[&format!("git rev-parse HEAD >> {}", log.display())]);
    let a = commit(&dir, "a.txt");
    enqueue(&dir);
    scheduler::drain(&dir).unwrap();
    let runs = ran(&log).len();
    sh(&dir, "git commit -q --amend -m renamed");
    assert_ne!(head(&dir), a);
    let e = enqueue(&dir);
    assert_eq!(e.status, Status::Pass);
    assert_eq!(e.reused_from.as_deref(), Some(a.as_str()));
    assert_eq!(ran(&log).len(), runs);
}

#[test]
fn changing_the_checks_invalidates_earlier_results() {
    let dir = make_repo();
    init(&dir, &["true"]);
    commit(&dir, "a.txt");
    enqueue(&dir);
    scheduler::drain(&dir).unwrap();
    init(&dir, &["true", "test -f a.txt"]);
    assert_eq!(enqueue(&dir).status, Status::Queued);
}

#[test]
fn uncommitted_work_in_the_main_tree_never_reaches_the_check() {
    let dir = make_repo();
    init(&dir, &["test ! -e dirty.txt"]);
    let a = commit(&dir, "a.txt");
    fs::write(dir.join("dirty.txt"), "not committed\n").unwrap();
    enqueue(&dir);
    scheduler::drain(&dir).unwrap();
    assert_eq!(entry(&dir, &a).status, Status::Pass);
    assert!(dir.join("dirty.txt").exists());
}

#[test]
fn untracked_files_named_in_copy_reach_the_worktree() {
    let dir = make_repo();
    fs::write(dir.join(".gitignore"), ".env\n").unwrap();
    sh(&dir, "git add -A && git commit -qm ignore");
    fs::write(dir.join(".env"), "SECRET=1\n").unwrap();
    init_with(&dir, &["grep -q SECRET=1 .env"], |o| o.copy = vec![".env".into()]);
    let a = commit(&dir, "a.txt");
    enqueue(&dir);
    scheduler::drain(&dir).unwrap();
    assert_eq!(entry(&dir, &a).status, Status::Pass);
}

#[test]
fn a_check_already_red_at_the_baseline_is_preexisting_and_the_gate_says_so() {
    let dir = make_repo();
    let base = head(&dir);
    init(&dir, &["test -f never.txt"]);
    let a = commit(&dir, "a.txt");
    enqueue(&dir);
    scheduler::drain(&dir).unwrap();
    assert!(entry(&dir, &a).checks[0].preexisting);
    assert!(queue::gate(&dir, &base, false).unwrap().problems.join("\n").contains("was already red before this work"));
}

#[test]
fn a_hung_check_is_killed_at_its_timeout_and_not_rerun() {
    let dir = make_repo();
    let baseline = init_with(&dir, &["sleep 5"], |o| o.timeout_ms = Some(300));
    let started = Instant::now();
    scheduler::drain(&dir).unwrap();
    let check = entry(&dir, &baseline.sha).checks[0].clone();
    assert!(check.timed_out);
    assert_eq!(check.exit, 124);
    assert!(started.elapsed() < Duration::from_millis(2500));
}

#[test]
fn gate_each_queues_skipped_commits_and_they_are_never_coalesced_away() {
    let dir = make_repo();
    let base = head(&dir);
    init(&dir, &["true"]);
    let a = commit(&dir, "a.txt");
    enqueue(&dir);
    commit(&dir, "b.txt");
    enqueue(&dir);
    scheduler::drain(&dir).unwrap();
    assert!(queue::gate(&dir, &base, false).unwrap().ok);
    let first = queue::gate(&dir, &base, true).unwrap();
    assert!(!first.ok);
    assert_eq!(first.queued, vec![a.clone()]);
    commit(&dir, "c.txt");
    enqueue(&dir);
    assert_eq!(entry(&dir, &a).status, Status::Queued);
    scheduler::drain(&dir).unwrap();
    assert!(queue::gate(&dir, &base, true).unwrap().ok);
}

#[test]
fn unverified_depth_counts_commits_since_the_newest_pass() {
    let dir = make_repo();
    init(&dir, &["true"]);
    scheduler::drain(&dir).unwrap();
    commit(&dir, "a.txt");
    enqueue(&dir);
    commit(&dir, "b.txt");
    enqueue(&dir);
    assert_eq!(queue::unverified_depth(&dir).unwrap(), 2);
    scheduler::drain(&dir).unwrap();
    assert_eq!(queue::unverified_depth(&dir).unwrap(), 0);
}

#[test]
fn a_run_left_behind_by_a_dead_worker_is_picked_up_again() {
    let dir = make_repo();
    init(&dir, &["true"]);
    let a = commit(&dir, "a.txt");
    enqueue(&dir);
    set_entry(&dir, &a, |e| {
        e.status = Status::Running;
        e.pid = Some(4_206_649);
    });
    scheduler::drain(&dir).unwrap();
    assert_eq!(entry(&dir, &a).status, Status::Pass);
}

#[test]
fn status_shows_a_skipped_commit_as_covered_by_its_descendant() {
    let dir = make_repo();
    let base = head(&dir);
    init(&dir, &["true"]);
    commit(&dir, "a.txt");
    enqueue(&dir);
    let b = commit(&dir, "b.txt");
    enqueue(&dir);
    scheduler::drain(&dir).unwrap();
    let rows = queue::status(&dir, &base).unwrap();
    assert_eq!(rows[0].status, "skipped");
    assert_eq!(rows[0].covered_by, Some((b, Status::Pass)));
    assert_eq!(rows[1].status, "pass");
}

#[test]
fn the_cli_runs_the_worker_in_the_background_and_wait_reports_the_result() {
    let dir = make_repo();
    assert!(cli(&dir, &["init", "--check", "test -f README.md"]).status.success());
    commit(&dir, "a.txt");
    let queued = cli(&dir, &["enqueue"]);
    assert!(queued.status.success(), "{}", text(&queued));
    assert!(text(&queued).contains("unverified depth"));
    let waited = cli(&dir, &["wait"]);
    assert!(text(&waited).contains("pass"), "{}", text(&waited));
    assert!(waited.status.success());
    assert!(text(&cli(&dir, &["gate", "HEAD~1"])).contains("GATE OK"));
}

#[test]
fn checks_run_side_by_side_up_to_the_slot_count() {
    let dir = make_repo();
    init_with(&dir, &["sleep 1", "sleep 1"], |o| o.parallel = Some(2));
    let started = Instant::now();
    scheduler::drain(&dir).unwrap();
    assert!(started.elapsed() < Duration::from_millis(1800));
}

#[test]
fn one_slot_runs_checks_one_after_another() {
    let dir = make_repo();
    init_with(&dir, &["sleep 1", "sleep 1"], |o| o.parallel = Some(1));
    let started = Instant::now();
    scheduler::drain(&dir).unwrap();
    assert!(started.elapsed() >= Duration::from_millis(2000));
}

#[test]
fn a_shard_check_is_split_across_every_slot() {
    let dir = make_repo();
    let log = log_file();
    let baseline = init_with(&dir, &[&format!("echo {{shard}}/{{shards}} >> {}", log.display())], |o| o.parallel = Some(3));
    scheduler::drain(&dir).unwrap();
    let mut lines = ran(&log);
    lines.sort();
    assert_eq!(lines, vec!["1/3", "2/3", "3/3"]);
    assert_eq!(entry(&dir, &baseline.sha).checks.len(), 3);
    assert_eq!(
        queue::expand_checks(&["lint".into(), "test --shard={shard}/{shards}".into()], 2),
        vec!["lint", "test --shard=1/2", "test --shard=2/2"]
    );
}

#[test]
fn the_fast_gate_stops_at_the_first_failure_instead_of_waiting_for_the_slowest() {
    let started = Instant::now();
    let result = run::fast(&std::env::temp_dir(), &["sleep 5".into(), "echo type error; exit 4".into()], 60_000);
    let failed = result.results[result.first_failed.expect("a failure")].as_ref().unwrap();
    assert_eq!(failed.exit, 4);
    assert!(failed.output.contains("type error"));
    assert!(started.elapsed() < Duration::from_millis(2000));
}

#[test]
fn the_fast_gate_passes_when_every_check_passes() {
    let result = run::fast(&std::env::temp_dir(), &["true".into(), "sleep 0.2".into()], 60_000);
    assert!(result.first_failed.is_none());
}

#[test]
fn a_busy_slot_held_by_another_process_makes_the_background_check_wait() {
    let dir = make_repo();
    let lock = format!("{}.0", queue::slot_lock_base(&dir).unwrap().display());
    let mut holder = Command::new("flock").args([lock.as_str(), "sleep", "1.5"]).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    init_with(&dir, &["true"], |o| o.parallel = Some(1));
    let started = Instant::now();
    scheduler::drain(&dir).unwrap();
    assert!(started.elapsed() >= Duration::from_millis(1100));
    let _ = holder.kill();
    let _ = holder.wait();
}

#[test]
fn partition_by_size_balances_the_work_across_groups() {
    let items: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
    let size = |item: &str| match item {
        "a" => 100,
        "b" => 90,
        "c" => 10,
        _ => 5,
    };
    let groups = partition_by_size(&items, 2, size);
    assert_eq!(groups.len(), 2);
    let loads: Vec<u64> = groups.iter().map(|g| g.iter().map(|i| size(i)).sum()).collect();
    assert!(loads.iter().max().unwrap() - loads.iter().min().unwrap() <= 10, "{loads:?}");
    assert_eq!(partition_by_size(&items[..1], 4, size).len(), 1);
}

#[test]
fn split_runs_one_job_per_item_across_cores_at_once() {
    let cwd = temp_dir("split");
    let started = Instant::now();
    let out = cli_stdin(&cwd, &["split", "-j", "4", "--each", "--", "sh", "-c", "sleep 1"], "a\nb\nc\nd\n");
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("SPLIT OK 4 job(s)"));
    assert!(started.elapsed() < Duration::from_millis(1800));
}

#[test]
fn split_each_replaces_the_item_placeholder() {
    let cwd = temp_dir("split");
    let log = log_file();
    let out = cli_stdin(&cwd, &["split", "--each", "--", "sh", "-c", &format!("echo {{}} >> {}", log.display())], "a\nb\nc\n");
    assert!(out.status.success(), "{}", text(&out));
    let mut lines = ran(&log);
    lines.sort();
    assert_eq!(lines, vec!["a", "b", "c"]);
}

#[test]
fn split_groups_append_the_items_to_the_command() {
    let cwd = temp_dir("split");
    for name in ["x1", "x2", "x3"] {
        fs::write(cwd.join(name), "content\n").unwrap();
    }
    let log = log_file();
    let out = cli_stdin(&cwd, &["split", "-j", "2", "--", "sh", "-c", &format!("for f; do echo $f >> {}; done", log.display()), "_"], "x1\nx2\nx3\n");
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("SPLIT OK 2 job(s) over 3 item(s)"));
    let mut lines = ran(&log);
    lines.sort();
    assert_eq!(lines, vec!["x1", "x2", "x3"]);
}

#[test]
fn split_stops_at_the_first_failure() {
    let cwd = temp_dir("split");
    let started = Instant::now();
    let out = cli_stdin(&cwd, &["split", "--each", "--", "sh", "-c", "if [ {} = bad ]; then echo broke; exit 3; fi; sleep 5"], "slow\nbad\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("SPLIT FAIL"));
    assert!(text(&out).contains("broke"));
    assert!(started.elapsed() < Duration::from_millis(2000));
}

#[test]
fn the_diff_budget_counts_changed_and_untracked_lines_but_not_lockfiles() {
    let dir = make_repo();
    fs::write(dir.join("big.txt"), "line\n".repeat(450)).unwrap();
    fs::write(dir.join("Cargo.lock"), "lock\n".repeat(1000)).unwrap();
    let measured = budget::measure(&dir, "HEAD", &[]).unwrap();
    assert_eq!(measured.total, 450);
    assert_eq!(measured.files[0], ("big.txt".to_string(), 450));
    let over = cli(&dir, &["diff-budget"]);
    assert_eq!(over.status.code(), Some(1));
    assert!(text(&over).contains("DIFF OVER 450"));
    assert!(cli(&dir, &["diff-budget", "--max", "500"]).status.success());
}

fn jobs(dir: &Path, stage: &str) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir.join(".git/xstack-worker/jobs").join(stage))
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn every_check_is_its_own_job_file_and_the_folder_is_empty_after() {
    let dir = make_repo();
    init_with(&dir, &["sleep 1", "sleep 1 # second"], |o| o.parallel = Some(2));
    let worker = {
        let dir = dir.clone();
        std::thread::spawn(move || scheduler::drain(&dir).unwrap())
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    while jobs(&dir, "running").len() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let running = jobs(&dir, "running");
    assert_eq!(running.len(), 2, "{running:?}");
    assert!(running[0].starts_with("000000-000-") && running[1].starts_with("000000-001-"), "{running:?}");
    worker.join().unwrap();
    for stage in ["queued", "running", "done"] {
        assert!(jobs(&dir, stage).is_empty(), "{stage} left {:?}", jobs(&dir, stage));
    }
}

#[test]
fn the_check_that_failed_last_time_runs_first_then_the_slowest() {
    let cmds: Vec<String> = ["lint", "unit", "e2e", "new"].iter().map(|s| s.to_string()).collect();
    let mut history = BTreeMap::new();
    history.insert("lint".to_string(), History { ms: 100, failed: false });
    history.insert("unit".to_string(), History { ms: 2_000, failed: true });
    history.insert("e2e".to_string(), History { ms: 60_000, failed: false });
    assert_eq!(rank(&cmds, &history), vec![1, 3, 2, 0]);
}

#[test]
fn the_burst_width_follows_idle_cores_and_free_memory() {
    let idle = Power { cores: 8, busy: 0.0, mem_free_mb: Some(64_000) };
    assert_eq!(idle.width(1, 0, 768), 7);
    assert_eq!(idle.width(0, 0, 768), 8);
    let loaded = Power { cores: 8, busy: 5.0, mem_free_mb: Some(64_000) };
    assert_eq!(loaded.width(1, 0, 768), 2);
    assert_eq!(loaded.width(1, 4, 768), 6, "our own running jobs count as capacity");
    let tight = Power { cores: 8, busy: 0.0, mem_free_mb: Some(1_600) };
    assert_eq!(tight.width(1, 0, 768), 2);
    let saturated = Power { cores: 2, busy: 2.0, mem_free_mb: None };
    assert_eq!(saturated.width(1, 0, 768), 1, "always at least one job");
}

#[test]
fn the_first_real_failure_stops_the_commits_other_jobs() {
    let dir = make_repo();
    init_with(&dir, &["[ -f slow.txt ] && sleep 5 || true", "test ! -f bad.txt || { echo bad; exit 3; }"], |o| o.parallel = Some(2));
    scheduler::drain(&dir).unwrap();
    commit(&dir, "slow.txt");
    let a = commit(&dir, "bad.txt");
    enqueue(&dir);
    let started = Instant::now();
    scheduler::drain(&dir).unwrap();
    assert!(started.elapsed() < Duration::from_millis(2500), "{:?}", started.elapsed());
    let e = entry(&dir, &a);
    assert_eq!(e.status, Status::Fail);
    assert_eq!(e.checks.len(), 1, "the stopped job is left out: {:?}", e.checks);
    assert_eq!(e.checks[0].exit, 3);
}

#[test]
fn a_check_red_at_the_baseline_never_stops_the_others() {
    let dir = make_repo();
    let log = log_file();
    init_with(&dir, &["test -f never.txt", &format!("sleep 0.3; echo ran >> {}", log.display())], |o| o.parallel = Some(2));
    let a = commit(&dir, "a.txt");
    enqueue(&dir);
    scheduler::drain(&dir).unwrap();
    assert_eq!(ran(&log).len(), 2);
    assert_eq!(entry(&dir, &a).checks.len(), 2);
}

#[test]
fn two_commits_verify_at_once_in_their_own_worktrees() {
    let dir = make_repo();
    let base = head(&dir);
    init_with(&dir, &["[ -f slow.txt ] && sleep 1 || true"], |o| {
        o.parallel = Some(2);
        o.commits = Some(2);
    });
    scheduler::drain(&dir).unwrap();
    commit(&dir, "slow.txt");
    commit(&dir, "b.txt");
    let started = Instant::now();
    assert!(!queue::gate(&dir, &base, true).unwrap().ok);
    scheduler::drain(&dir).unwrap();
    assert!(started.elapsed() < Duration::from_millis(1900), "{:?}", started.elapsed());
    assert!(queue::gate(&dir, &base, true).unwrap().ok);
}

#[test]
fn probe_reports_the_power_and_the_burst() {
    let out = Command::new(EXE).arg("probe").output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("POWER ") && text(&out).contains("BURST background"), "{}", text(&out));
}
