use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use xstack_subtasks::engine::{self as sub, Http, SubOptions, Summary, Task, Widths};
use xstack_worker::probe::Power;
use xstack_worker::share::{Kind, Runs, Share};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir =
        std::env::temp_dir().join(format!("xstack-subtasks-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn assert_no_temporary_files(dir: &Path) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        assert!(
            !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".xstack-subtasks-"),
            "task spool remains: {:?}",
            entry.path()
        );
    }
}

fn options(dir: &Path) -> SubOptions {
    SubOptions {
        cwd: dir.to_path_buf(),
        out: dir.join("results.jsonl"),
        fail_fast: false,
        timeout_ms: 30_000,
        per_host: sub::DEFAULT_PER_HOST,
        retries: sub::DEFAULT_RETRIES,
        sh_mem_mb: sub::DEFAULT_SH_MEM_MB,
        cap: Widths::UNCAPPED,
        show: 100,
    }
}

fn results(dir: &Path) -> Vec<serde_json::Value> {
    let mut rows: Vec<serde_json::Value> = fs::read_to_string(dir.join("results.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    rows.sort_by_key(|r| r["i"].as_u64());
    rows
}

fn run(tasks: Vec<Task>, options: &SubOptions) -> Summary {
    sub::run(tasks.into_iter().map(Ok), options).unwrap()
}

#[test]
fn a_template_with_braces_runs_once_per_quoted_item() {
    let items = vec!["a b".to_string(), "it's".to_string()];
    let tasks: Vec<Task> = sub::shell_tasks("printf '[%s]' {}", items).collect();
    assert_eq!(tasks.len(), 2);
    let dir = temp_dir("each");
    let summary = run(tasks, &options(&dir));
    assert_eq!((summary.total, summary.failed), (2, 0));
    let rows = results(&dir);
    assert_eq!(rows[0]["output"], "[a b]");
    assert_eq!(rows[1]["output"], "[it's]");
}

#[test]
fn a_template_without_braces_batches_items_per_process() {
    let items: Vec<String> = (0..100).map(|i| format!("item {i}")).collect();
    let tasks: Vec<Task> = sub::shell_tasks("printf '%s\\n'", items).collect();
    assert_eq!(tasks.len(), 100usize.div_ceil(sub::BATCH_ITEMS));
    let dir = temp_dir("batch");
    run(tasks, &options(&dir));
    let lines: usize = results(&dir)
        .iter()
        .map(|r| r["output"].as_str().unwrap().lines().count())
        .sum();
    assert_eq!(lines, 100);
}

#[test]
fn fail_fast_reports_the_failure_and_cancels_or_skips_the_rest() {
    let dir = temp_dir("failfast");
    let mut tasks = vec![Task::Sh("exit 3".into())];
    tasks.extend((0..200).map(|_| Task::Sh("sleep 5".into())));
    let started = Instant::now();
    let summary = run(
        tasks,
        &SubOptions {
            fail_fast: true,
            ..options(&dir)
        },
    );
    assert!(
        started.elapsed().as_secs() < 4,
        "running commands were not killed"
    );
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.failures[0].code, 3);
    assert!(summary.skipped > 0);
    assert_eq!(summary.cancelled + summary.skipped, 200);
}

#[test]
fn a_hung_command_is_killed_at_the_timeout() {
    let dir = temp_dir("timeout");
    let summary = run(
        vec![Task::Sh("sleep 30".into())],
        &SubOptions {
            timeout_ms: 200,
            ..options(&dir)
        },
    );
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.failures[0].code, 124);
}

#[test]
fn file_tasks_count_lines_find_matches_and_hash() {
    let dir = temp_dir("files");
    fs::write(dir.join("a.txt"), "one\nTODO two\nthree\nTODO four\n").unwrap();
    let path = dir.join("a.txt").to_string_lossy().into_owned();
    let tasks = vec![
        Task::Lines(path.clone()),
        Task::Grep {
            path: path.clone(),
            pattern: "^TODO".into(),
        },
        Task::Hash(path.clone()),
        Task::Lines(dir.join("missing").to_string_lossy().into_owned()),
    ];
    let summary = run(tasks, &options(&dir));
    assert_eq!(summary.failed, 1, "only the missing file fails");
    let rows = results(&dir);
    assert_eq!(rows[0]["code"], 4);
    assert_eq!(rows[1]["code"], 2);
    assert_eq!(rows[1]["output"], "2:TODO two\n4:TODO four\n");
    assert_eq!(
        rows[2]["output"],
        blake3::hash(b"one\nTODO two\nthree\nTODO four\n")
            .to_hex()
            .as_str()
    );
    assert_eq!(rows[3]["ok"], false);
}

#[test]
fn jsonl_lines_parse_into_every_task_kind() {
    let lines = [
        r#"{"sh": "echo hi"}"#,
        r#"{"argv": ["echo", "a"]}"#,
        r#"{"browser": {"session": "research", "commands": [["open", "https://example.com"], ["snapshot", "-i"]]}}"#,
        r#"{"http": {"url": "http://127.0.0.1:9/"}}"#,
        r#"{"grep": {"path": "f", "pattern": "x"}}"#,
        r#"{"hash": "f"}"#,
        r#"{"lines": "f"}"#,
    ];
    for line in lines {
        serde_json::from_str::<Task>(line).unwrap_or_else(|e| panic!("{line}: {e}"));
    }
}

// Exercise the public JSONL/CLI interface with a deterministic agent-browser executable.
fn browser_cli(dir: &Path, tasks: &[serde_json::Value], extra: &[&str]) -> std::process::Output {
    let tmp = dir.join("tmp");
    fs::create_dir_all(&tmp).unwrap();
    browser_cli_in_tmp(dir, tasks, extra, &tmp)
}

fn browser_cli_in_tmp(
    dir: &Path,
    tasks: &[serde_json::Value],
    extra: &[&str],
    tmp: &Path,
) -> std::process::Output {
    let script = dir.join("agent-browser");
    fs::write(
        &script,
        r#"#!/bin/sh
if [ "$1" != "--session" ]; then exit 70; fi
session=$2
shift 2
action=$1
shift
case "$action" in
  echo) printf 'arg:%s\n' "$@" ;;
  fail) echo 'browser error' >&2; exit 9 ;;
  after) touch after ;;
  hold)
    if ! mkdir "$session.lock" 2>/dev/null; then echo overlap >&2; exit 31; fi
    sleep 0.3
    rmdir "$session.lock"
    echo held
    ;;
  meet)
    touch "$session.started"
    n=0
    while [ ! -e "$1.started" ] && [ "$n" -lt 40 ]; do
      sleep 0.05
      n=$((n + 1))
    done
    if [ ! -e "$1.started" ]; then echo 'peer not observed' >&2; exit 32; fi
    echo met
    ;;
  hang) echo started; sleep 30 ;;
  *) exit 71 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let input = tasks
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(dir.join("tasks.jsonl"), input).unwrap();
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(dir)
        .env("PATH", path)
        .env("TMPDIR", tmp)
        .args([
            "--tasks",
            "tasks.jsonl",
            "--out",
            "results.jsonl",
            "--cpu",
            "4",
        ])
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn browser_commands_preserve_order_arguments_and_failure_output() {
    let dir = temp_dir("browser-actions");
    let tasks = vec![
        serde_json::json!({"browser": {"session": "ordered", "commands": [
            ["echo", "hello world", "; touch injected"], ["echo", "second"], ["fail"], ["after"]
        ]}}),
    ];
    let output = browser_cli(&dir, &tasks, &["--keep-going"]);
    assert!(!output.status.success());
    let rows = results(&dir);
    assert_eq!(rows[0]["kind"], "browser");
    assert_eq!(rows[0]["code"], 9);
    let text = rows[0]["output"].as_str().unwrap();
    assert!(text.find("arg:hello world").unwrap() < text.find("arg:second").unwrap());
    assert!(text.find("arg:second").unwrap() < text.find("browser error").unwrap());
    assert!(text.contains("arg:; touch injected"));
    assert!(!dir.join("injected").exists());
    assert!(!dir.join("after").exists());
}

#[test]
fn browser_rejects_session_overrides_and_does_not_repeat_action_arguments() {
    let dir = temp_dir("browser-arguments");
    let tasks = vec![
        serde_json::json!({"browser": {"session": "a", "commands": [["--session", "other", "after"]]}}),
        serde_json::json!({"browser": {"session": "b", "commands": [["--session=other", "after"]]}}),
        serde_json::json!({"browser": {"session": "c", "commands": [["hold", "private-value"]]}}),
    ];
    let output = browser_cli(&dir, &tasks, &["--keep-going"]);
    assert!(!output.status.success());
    let rows = results(&dir);
    assert_eq!(rows[0]["code"], -1);
    assert_eq!(rows[1]["code"], -1);
    assert_eq!(rows[2]["code"], 0);
    assert!(!rows[2]["output"]
        .as_str()
        .unwrap()
        .contains("private-value"));
    assert!(!dir.join("after").exists());
}

#[test]
fn invalid_browser_input_does_not_disclose_command_arguments() {
    let dir = temp_dir("browser-invalid-secret");
    let secret = "private-browser-token-7092";
    fs::write(
        dir.join("tasks.jsonl"),
        format!(
            "{{\"browser\":{{\"session\":\"one\",\"commands\":[[\"fill\",\"{secret}\"]]}}\n\
             {{\"browser\":{{\"session\":\"two\",\"commands\":\"{secret}\"}}}}\n"
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&dir)
        .args([
            "--tasks",
            "tasks.jsonl",
            "--out",
            "results.jsonl",
            "--keep-going",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
    assert!(!fs::read_to_string(dir.join("results.jsonl"))
        .unwrap()
        .contains(secret));
    assert_eq!(results(&dir).len(), 2);
}

#[test]
fn invalid_utf8_input_fails_without_losing_later_tasks() {
    let dir = temp_dir("invalid-input-bytes");
    let mut input = fs::File::create(dir.join("tasks.jsonl")).unwrap();
    input
        .write_all(b"{\"sh\":\"printf first\"}\n\xff\n{\"sh\":\"printf second\"}\n")
        .unwrap();
    drop(input);
    let output = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&dir)
        .args([
            "--tasks",
            "tasks.jsonl",
            "--out",
            "results.jsonl",
            "--keep-going",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let rows = results(&dir);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["output"], "first");
    assert_eq!(rows[1]["ok"], false);
    assert_eq!(rows[2]["output"], "second");
}

#[cfg(target_os = "linux")]
#[test]
fn output_failure_does_not_wait_for_open_input() {
    let dir = temp_dir("full-output-open-input");
    let mut child = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&dir)
        .args(["--out", "/dev/full", "--keep-going", "--cpu", "1"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(input, r#"{{"sh":"printf ok"}}"#).unwrap();
    input.flush().unwrap();
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    drop(input);
    if status.is_none() {
        child.kill().unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(status.is_some(), "output error waited for input EOF");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("space"));
}

#[cfg(target_os = "linux")]
#[test]
fn output_failure_interrupts_a_dispatcher_waiting_for_a_file_slot() {
    use std::os::unix::ffi::OsStrExt;
    let dir = temp_dir("full-output-blocked-file");
    let fifo = dir.join("blocked");
    let path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let mut child = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&dir)
        .args([
            "--out",
            "/dev/full",
            "--keep-going",
            "--cpu",
            "1",
            "--file",
            "1",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(input, "{}", serde_json::json!({"lines": fifo})).unwrap();
    writeln!(input, r#"{{"lines":"/dev/null"}}"#).unwrap();
    for _ in 0..3 {
        writeln!(input, r#"{{"sh":"printf '%04000d' 1"}}"#).unwrap();
    }
    drop(input);
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    if status.is_none() {
        child.kill().unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        status.is_some(),
        "output error waited for a blocked file task"
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("space"));
}

#[test]
fn browser_rejects_path_aliases_for_session_names() {
    let dir = temp_dir("browser-session-alias");
    let tasks = vec![
        serde_json::json!({"browser": {"session": "./same", "commands": [["after"]]}}),
        serde_json::json!({"browser": {"session": "../same", "commands": [["after"]]}}),
        serde_json::json!({"browser": {"session": "same\\other", "commands": [["after"]]}}),
    ];
    let output = browser_cli(&dir, &tasks, &["--keep-going"]);
    assert!(!output.status.success());
    let rows = results(&dir);
    assert!(rows.iter().all(|row| row["code"] == -1));
    assert!(!dir.join("after").exists());
}

#[test]
fn browser_rejects_shared_browser_connection_options() {
    let dir = temp_dir("browser-shared-connection");
    let tasks = vec![
        serde_json::json!({"browser": {"session": "one", "commands": [["--cdp", "9222", "after"]]}}),
        serde_json::json!({"browser": {"session": "two", "commands": [["--auto-connect", "after"]]}}),
        serde_json::json!({"browser": {"session": "three", "commands": [["--profile", "shared", "after"]]}}),
    ];
    let output = browser_cli(&dir, &tasks, &["--keep-going"]);
    assert!(!output.status.success());
    let rows = results(&dir);
    assert!(rows.iter().all(|row| row["code"] == -1));
    assert!(!dir.join("after").exists());
}

#[test]
fn browser_rejects_shared_project_connection_configuration() {
    let dir = temp_dir("browser-config-connection");
    fs::write(dir.join("agent-browser.json"), r#"{"cdp":"9222"}"#).unwrap();
    let tasks = vec![serde_json::json!({"browser": {"session": "one", "commands": [["after"]]}})];
    let output = browser_cli(&dir, &tasks, &["--keep-going"]);
    assert!(!output.status.success());
    let rows = results(&dir);
    assert_eq!(rows[0]["code"], -1);
    assert!(!dir.join("after").exists());
}

#[test]
fn browser_rejects_shared_connection_environment() {
    let dir = temp_dir("browser-env-connection");
    let tasks =
        vec![serde_json::json!({"browser": {"session": "one", "commands": [["echo", "safe"]]}})];
    let setup = browser_cli(&dir, &tasks, &["--keep-going"]);
    assert!(setup.status.success());
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&dir)
        .env("PATH", path)
        .env("AGENT_BROWSER_CDP", "9222")
        .args([
            "--tasks",
            "tasks.jsonl",
            "--out",
            "results.jsonl",
            "--keep-going",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let rows = results(&dir);
    assert_eq!(rows[0]["code"], -1);
    assert!(!rows[0]["output"].as_str().unwrap().contains("arg:safe"));
}

#[test]
fn browser_tasks_in_the_same_session_do_not_overlap() {
    let dir = temp_dir("browser-lock");
    let tasks: Vec<_> = (0..8)
        .map(|_| serde_json::json!({"browser": {"session": "same", "commands": [["hold"]]}}))
        .collect();
    let output = browser_cli(&dir, &tasks, &["--keep-going"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let rows = results(&dir);
    assert_eq!(rows.len(), 8);
    assert!(rows.iter().all(|row| row["ok"] == true));
}

#[test]
fn browser_independent_sessions_can_run_concurrently() {
    let dir = temp_dir("browser-concurrent");
    let tasks = vec![
        serde_json::json!({"browser": {"session": "left", "commands": [["meet", "right"]]}}),
        serde_json::json!({"browser": {"session": "left", "commands": [["hold"]]}}),
        serde_json::json!({"browser": {"session": "right", "commands": [["meet", "left"]]}}),
    ];
    let output = browser_cli(&dir, &tasks, &["--keep-going"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let rows = results(&dir);
    // A machine whose shared CPU allocation is one cannot run either task concurrently.
    if stdout.contains("peak cpu 1,") {
        assert!(rows.iter().any(|row| row["output"]
            .as_str()
            .unwrap_or("")
            .contains("peer not observed")));
    } else {
        assert!(output.status.success(), "{stdout}");
        assert!(rows[0]["output"].as_str().unwrap().contains("met"));
        assert!(rows[2]["output"].as_str().unwrap().contains("met"));
    }
}

#[test]
fn browser_backlog_does_not_block_an_independent_session() {
    let dir = temp_dir("browser-spooled-backlog");
    let mut tasks =
        vec![serde_json::json!({"browser": {"session": "left", "commands": [["meet", "right"]]}})];
    tasks.extend((0..1030).map(
        |_| serde_json::json!({"browser": {"session": "left", "commands": [["echo", "queued"]]}}),
    ));
    tasks
        .push(serde_json::json!({"browser": {"session": "right", "commands": [["meet", "left"]]}}));
    let output = browser_cli(&dir, &tasks, &["--keep-going"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("peak cpu 1,") {
        return;
    }
    assert!(output.status.success(), "{stdout}");
    let rows = results(&dir);
    assert_eq!(rows.len(), tasks.len());
    assert!(rows.iter().all(|row| row["ok"] == true));
}

#[test]
fn browser_backlog_keeps_arguments_out_of_stdout_and_cleans_temporary_files() {
    let dir = temp_dir("browser-spool-private");
    let secret = "private-browser-argument-not-for-stdout";
    let mut tasks =
        vec![serde_json::json!({"browser": {"session": "same", "commands": [["hold"]]}})];
    tasks.extend((0..64).map(
        |_| serde_json::json!({"browser": {"session": "same", "commands": [["echo", secret]]}}),
    ));
    let output = browser_cli(&dir, &tasks, &["--keep-going"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(!stdout.contains(secret));
    let rows = results(&dir);
    assert_eq!(rows.len(), tasks.len());
    assert!(rows.iter().all(|row| row["ok"] == true));
    assert_no_temporary_files(&dir.join("tmp"));
}

#[test]
fn browser_input_error_does_not_lose_other_queued_tasks() {
    let dir = temp_dir("browser-spool-input-error");
    let mut tasks =
        vec![serde_json::json!({"browser": {"session": "same", "commands": [["hold"]]}})];
    tasks.extend((0..64).map(|i| {
        serde_json::json!({"browser": {"session": "same", "commands": [["echo", i.to_string()]]}})
    }));
    tasks.insert(32, serde_json::json!({"not-a-task": true}));
    let output = browser_cli(&dir, &tasks, &["--keep-going"]);
    assert!(!output.status.success());
    let rows = results(&dir);
    assert_eq!(rows.len(), tasks.len());
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(row["i"], i);
        assert_eq!(row["ok"], i != 32);
    }
    assert_no_temporary_files(&dir.join("tmp"));
}

#[test]
fn browser_spool_creation_failure_is_not_reported_as_success() {
    let dir = temp_dir("browser-spool-unavailable");
    let tmp = dir.join("not-a-directory");
    fs::write(&tmp, "untouched").unwrap();
    let tasks = vec![serde_json::json!({"browser": {"session": "same", "commands": [["after"]]}})];
    let output = browser_cli_in_tmp(&dir, &tasks, &["--keep-going"], &tmp);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("SUBTASK OK"));
    assert!(!dir.join("after").exists());
    assert_eq!(fs::read_to_string(tmp).unwrap(), "untouched");
    if dir.join("results.jsonl").exists() {
        assert!(results(&dir).iter().all(|row| row["ok"] != true));
    }
}

#[test]
fn browser_rejects_empty_tasks_and_reports_timeouts() {
    let dir = temp_dir("browser-invalid");
    let tasks = vec![
        serde_json::json!({"browser": {"session": "", "commands": [["echo"]]}}),
        serde_json::json!({"browser": {"session": "a", "commands": []}}),
        serde_json::json!({"browser": {"session": "b", "commands": [[]]}}),
        serde_json::json!({"browser": {"session": "c", "commands": [["hang"], ["after"]]}}),
    ];
    let output = browser_cli(&dir, &tasks, &["--keep-going", "--timeout-ms", "200"]);
    assert!(!output.status.success());
    let rows = results(&dir);
    assert!(rows[..3].iter().all(|row| row["code"] == -1));
    assert_eq!(rows[3]["code"], 124);
    assert!(rows[3]["output"].as_str().unwrap().contains("timed out"));
    assert!(!dir.join("after").exists());
}

#[test]
fn browser_timeout_blocks_later_tasks_in_the_same_session() {
    let dir = temp_dir("browser-timeout-session");
    let script = dir.join("agent-browser");
    fs::write(
        &script,
        "#!/bin/sh\nshift 2\ncase \"$1\" in hang) touch started; sleep 30;; after) touch after;; esac\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&dir)
        .env("PATH", path)
        .args([
            "--out",
            "results.jsonl",
            "--keep-going",
            "--timeout-ms",
            "1000",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(
        input,
        "{}",
        serde_json::json!({"browser": {"session": "same", "commands": [["hang"]]}})
    )
    .unwrap();
    input.flush().unwrap();
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    while !dir.join("started").exists() && Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        dir.join("started").exists(),
        "first browser action did not start"
    );
    writeln!(
        input,
        "{}",
        serde_json::json!({"browser": {"session": "same", "commands": [["after"]]}})
    )
    .unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let rows = results(&dir);
    assert_eq!(rows[0]["code"], 124);
    assert!(!rows[1]["ok"].as_bool().unwrap());
    assert!(!dir.join("after").exists());
}

#[test]
fn browser_reports_a_missing_executable() {
    let dir = temp_dir("browser-missing");
    fs::write(
        dir.join("tasks.jsonl"),
        r#"{"browser":{"session":"s","commands":[["open","https://example.com"],["after"]]}}"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&dir)
        .env("PATH", &dir)
        .args([
            "--tasks",
            "tasks.jsonl",
            "--out",
            "results.jsonl",
            "--keep-going",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let rows = results(&dir);
    assert_eq!(rows[0]["code"], 127);
    assert!(rows[0]["output"].as_str().unwrap().contains("cannot start"));
    assert!(!dir.join("after").exists());
}

#[test]
fn browser_waiters_and_running_actions_obey_fail_fast() {
    let dir = temp_dir("browser-cancel");
    let mut tasks = vec![serde_json::json!({"sh": "sleep 0.2; exit 8"})];
    tasks.extend((0..12).map(
        |_| serde_json::json!({"browser": {"session": "same", "commands": [["hang"], ["after"]]}}),
    ));
    let started = Instant::now();
    let output = browser_cli(&dir, &tasks, &[]);
    assert!(!output.status.success());
    assert!(started.elapsed().as_secs() < 10);
    let rows = results(&dir);
    assert_eq!(rows[0]["code"], 8);
    assert!(rows.iter().skip(1).all(|row| row["cancelled"] == true));
    assert!(!dir.join("after").exists());
    assert_no_temporary_files(&dir.join("tmp"));
}

/// Serves `/flaky` as 503 twice then 200, `/missing` as 404, and anything else as 200 with the path as body.
fn serve() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let flaky = Arc::new(AtomicUsize::new(0));
    let hits = Arc::clone(&flaky);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let hits = Arc::clone(&hits);
            std::thread::spawn(move || {
                let mut request = Vec::new();
                let mut byte = [0u8; 1];
                while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                    request.push(byte[0]);
                }
                let line = String::from_utf8_lossy(&request);
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, body) = match path.as_str() {
                    "/flaky" if hits.fetch_add(1, Ordering::SeqCst) < 2 => {
                        ("503 Busy", String::new())
                    }
                    "/missing" => ("404 Not Found", "nope".into()),
                    _ => ("200 OK", path.clone()),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nRetry-After: 0\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    (base, flaky)
}

fn get(url: String) -> Task {
    Task::Http(Http {
        url,
        method: "GET".into(),
        headers: Default::default(),
        body: None,
        save: None,
    })
}

#[test]
fn http_tasks_retry_busy_responses_report_errors_and_save_bodies() {
    let (base, flaky) = serve();
    let dir = temp_dir("http");
    let saved = dir.join("saved");
    let tasks = vec![
        get(format!("{base}/flaky")),
        get(format!("{base}/missing")),
        Task::Http(Http {
            url: format!("{base}/keep"),
            method: "GET".into(),
            headers: Default::default(),
            body: None,
            save: Some(saved.clone()),
        }),
    ];
    let summary = run(tasks, &options(&dir));
    let rows = results(&dir);
    assert_eq!(rows[0]["code"], 200);
    assert_eq!(flaky.load(Ordering::SeqCst), 3, "two retries, then success");
    assert_eq!(
        (rows[1]["code"].as_i64(), rows[1]["ok"].as_bool()),
        (Some(404), Some(false))
    );
    assert_eq!(fs::read_to_string(saved).unwrap(), "/keep");
    assert_eq!(summary.failed, 1);
}

#[test]
fn thousands_of_http_requests_run_at_once_and_all_finish() {
    let (base, _) = serve();
    let dir = temp_dir("http-many");
    let tasks: Vec<Task> = (0..3000).map(|i| get(format!("{base}/item/{i}"))).collect();
    let summary = run(tasks, &options(&dir));
    assert_eq!(
        (summary.total, summary.failed, summary.skipped),
        (3000, 0, 0)
    );
    assert!(
        summary.widths.net > 16,
        "requests ran {} at a time",
        summary.widths.net
    );
    let rows = results(&dir);
    assert_eq!(rows[2999]["output"], "/item/2999");
}

#[test]
fn a_hundred_thousand_file_tasks_stream_to_the_results_file() {
    let dir = temp_dir("scale");
    fs::write(dir.join("f"), "a\nb\n").unwrap();
    let path = dir.join("f").to_string_lossy().into_owned();
    let tasks: Vec<Task> = (0..100_000).map(|_| Task::Lines(path.clone())).collect();
    let summary = run(tasks, &options(&dir));
    assert_eq!((summary.total, summary.failed), (100_000, 0));
    assert_eq!(
        fs::read_to_string(dir.join("results.jsonl"))
            .unwrap()
            .lines()
            .count(),
        100_000
    );
}

#[test]
fn concurrent_runs_split_every_pool_evenly() {
    let power = Power {
        cores: 8,
        busy: 0.0,
        mem_free_mb: Some(64 * 1024),
    };
    let runs = |all, same_kind| Runs { all, same_kind };
    let one = sub::widths(&power, Runs::ALONE, 0, 256, 1 << 20);
    let two = sub::widths(&power, runs(2, 2), 0, 256, 1 << 20);
    assert_eq!((one.cpu, two.cpu), (8, 4));
    assert_eq!(two.net, one.net / 2);
    assert_eq!(two.file, one.file / 2);
    let tight = sub::widths(&power, Runs::ALONE, 0, 256, 1024);
    assert_eq!(
        tight.net,
        (1024 - 256) / 2,
        "descriptors bound the network pool"
    );
}

#[test]
fn a_live_verification_worker_takes_its_share_of_the_cores_only() {
    let power = Power {
        cores: 8,
        busy: 0.0,
        mem_free_mb: Some(64 * 1024),
    };
    let alone = sub::widths(&power, Runs::ALONE, 0, 256, 1 << 20);
    let beside = sub::widths(
        &power,
        Runs {
            all: 2,
            same_kind: 1,
        },
        0,
        256,
        1 << 20,
    );
    assert_eq!(beside.cpu, 4);
    assert_eq!((beside.net, beside.file), (alone.net, alone.file));
}

#[test]
fn every_live_run_is_counted_by_kind_and_dead_ones_are_cleared() {
    let subtask = Share::join(Kind::Subtask);
    let _verification = Share::join(Kind::Verification);
    let runs = subtask.runs();
    assert!(
        runs.all > runs.same_kind,
        "the verification run counts toward the cores only"
    );
    let dir = std::env::temp_dir().join(format!("xstack-share-{}", unsafe { libc::getuid() }));
    let dead = dir.join("subtask-1-dead");
    fs::write(&dead, "").unwrap();
    subtask.runs();
    assert!(!dead.exists(), "an unlocked file belongs to a dead run");
}

#[test]
fn a_bad_input_line_fails_alone_and_the_rest_still_run() {
    let dir = temp_dir("input");
    let inputs = vec![
        Ok(Task::Sh("echo one".into())),
        Err("task 2: expected value: {nope".to_string()),
        Ok(Task::Sh("echo three".into())),
    ];
    let summary = sub::run(inputs, &options(&dir)).unwrap();
    assert_eq!((summary.total, summary.failed), (3, 1));
    assert_eq!(summary.failures[0].kind, "input");
    let rows = results(&dir);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2]["output"], "three\n");
}

#[test]
fn fail_fast_still_counts_every_unread_task_as_skipped() {
    let dir = temp_dir("count");
    let inputs = std::iter::once(Ok(Task::Sh("exit 1".into())))
        .chain((0..50_000).map(|_| Ok(Task::Sh("sleep 5".into()))));
    let summary = sub::run(
        inputs,
        &SubOptions {
            fail_fast: true,
            ..options(&dir)
        },
    )
    .unwrap();
    assert_eq!(summary.total, 50_001);
    assert_eq!(summary.failed + summary.cancelled + summary.skipped, 50_001);
}

#[test]
fn a_read_only_temp_dir_runs_alone_and_writes_results_to_the_working_directory() {
    let dir = temp_dir("readonly");
    let locked = dir.join("tmp");
    fs::create_dir(&locked).unwrap();
    let mut permissions = fs::metadata(&locked).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o500);
    fs::set_permissions(&locked, permissions).unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_subtask"))
        .args(["--sh", "echo {}"])
        .env("TMPDIR", &locked)
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"a\nb\n").unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("SUBTASK OK 2 task(s)"), "{stdout}");
    assert!(stdout.contains("1 run(s) sharing"), "{stdout}");
    let written: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".xstack-subtask-")
        })
        .collect();
    assert_eq!(written.len(), 1, "{stdout}");
    assert_eq!(
        fs::read_to_string(written[0].path())
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn a_run_with_no_writable_directory_prints_its_results_above_the_summary() {
    let dir = temp_dir("nowhere");
    let mut permissions = fs::metadata(&dir).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o500);
    fs::set_permissions(&dir, permissions).unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_subtask"))
        .args(["--sh", "test {} = a"])
        .env("TMPDIR", &dir)
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"a\nb\n").unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("SUBTASK FAIL 1 failed"), "{stdout}");
    assert!(stdout.contains("results /dev/stdout"), "{stdout}");
    assert_eq!(
        stdout.lines().filter(|l| l.starts_with("{\"i\":")).count(),
        2,
        "{stdout}"
    );
}
