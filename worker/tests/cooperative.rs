use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("xstack-cooperative-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn idle_worker_executes_a_busy_workers_unstarted_items() {
    let workspace = Workspace::new();
    let input: String = (0..96).map(|i| format!("item-{i:03}\n")).collect();
    let script = r#"
test "$#" -le 32 || { echo 'batch exceeds donation boundary' >&2; exit 8; }
printf '%s\n' "$#" >> batch-sizes
for item; do
    if [ "$item" = item-000 ]; then
        touch first-started
        while [ ! -f helped ]; do sleep 0.01; done
        touch first-finished
    fi
    printf '%s\n' "$item" >> completed
    if [ "$item" = item-094 ]; then touch helped; fi
done
"#;
    let mut child = Command::new(env!("CARGO_BIN_EXE_worker"))
        .args([
            "split",
            "-j",
            "2",
            "--timeout-ms",
            "10000",
            "--",
            "sh",
            "-c",
            script,
            "_",
        ])
        .current_dir(&workspace.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let completed = fs::read_to_string(workspace.0.join("completed")).unwrap();
    let mut items: Vec<_> = completed.lines().collect();
    items.sort_unstable();
    assert_eq!(items, input.lines().collect::<Vec<_>>());
    assert!(workspace.0.join("first-started").exists());
    assert!(workspace.0.join("first-finished").exists());
    assert!(workspace.0.join("helped").exists());
}

fn execute(
    workspace: &Workspace,
    input: &str,
    flags: &[&str],
    script: &str,
) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_worker"))
        .args(["split", "-j", "2", "--timeout-ms", "10000"])
        .args(flags)
        .args(["--", "sh", "-c", script, "_"])
        .current_dir(&workspace.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn shared_batches_preserve_duplicate_occurrences_and_literal_arguments() {
    let workspace = Workspace::new();
    let values = [
        "same",
        "space name",
        "quote'\"",
        "$(touch injected)",
        "*",
        "{}",
    ];
    let input: String = values
        .iter()
        .cycle()
        .take(192)
        .map(|item| format!("{item}\n"))
        .collect();
    let output = execute(
        &workspace,
        &input,
        &["-j", "4"],
        r#"printf '%s\0' "$@" >> arguments"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = fs::read(workspace.0.join("arguments")).unwrap();
    let mut actual: Vec<_> = actual
        .split(|&byte| byte == 0)
        .filter(|arg| !arg.is_empty())
        .collect();
    let mut expected: Vec<_> = input.lines().map(str::as_bytes).collect();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);
    assert!(!workspace.0.join("injected").exists());
}

#[test]
fn a_failed_batch_cancels_running_siblings_and_leaves_tails_unstarted() {
    let workspace = Workspace::new();
    let input: String = (0..96).map(|i| format!("item-{i:03}\n")).collect();
    let output = execute(
        &workspace,
        &input,
        &[],
        r#"
case "$1" in
    item-000)
        while [ ! -f sibling-started ]; do sleep 0.01; done
        echo 'original batch failure' >&2
        exit 7
        ;;
    item-001)
        echo $$ > sibling-pid
        touch sibling-started
        sleep 30
        touch leaked
        ;;
    *) touch unexpected-tail ;;
esac
"#,
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("original batch failure"), "{stderr}");
    assert!(!workspace.0.join("unexpected-tail").exists());
    assert!(!workspace.0.join("leaked").exists());
    let pid: i32 = fs::read_to_string(workspace.0.join("sibling-pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

#[test]
fn keep_going_processes_every_pending_occurrence_after_a_batch_failure() {
    let workspace = Workspace::new();
    let input: String = (0..192).map(|i| format!("item-{i:03}\n")).collect();
    let output = execute(
        &workspace,
        &input,
        &["--keep-going"],
        r#"
printf '%s\n' "$@" >> completed
case "$1" in
    item-000) echo first-failure >&2; exit 7 ;;
    item-001) echo second-failure >&2; exit 8 ;;
esac
"#,
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("first-failure") && stderr.contains("second-failure"),
        "{stderr}"
    );
    let completed = fs::read_to_string(workspace.0.join("completed")).unwrap();
    let mut actual: Vec<_> = completed.lines().collect();
    actual.sort_unstable();
    assert_eq!(actual, input.lines().collect::<Vec<_>>());
}

#[test]
fn timed_out_batches_do_not_retry_and_keep_going_finishes_the_tail() {
    let workspace = Workspace::new();
    let input: String = (0..96).map(|i| format!("item-{i:03}\n")).collect();
    let output = execute(
        &workspace,
        &input,
        &["--keep-going", "--timeout-ms", "300"],
        r#"
if [ "$1" = item-000 ]; then
    echo started >> attempts
    exec sleep 30
fi
printf '%s\n' "$@" >> completed
"#,
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("exit 124") && stderr.contains("timed out"),
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(workspace.0.join("attempts")).unwrap(),
        "started\n"
    );
    let completed = fs::read_to_string(workspace.0.join("completed")).unwrap();
    assert!(completed.lines().any(|item| item == "item-094"));
}
