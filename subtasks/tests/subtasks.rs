use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
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
        r#"{"http": {"url": "http://127.0.0.1:9/"}}"#,
        r#"{"grep": {"path": "f", "pattern": "x"}}"#,
        r#"{"hash": "f"}"#,
        r#"{"lines": "f"}"#,
    ];
    for line in lines {
        serde_json::from_str::<Task>(line).unwrap_or_else(|e| panic!("{line}: {e}"));
    }
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
