use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn skill_entry_reads_workspace_files_and_rejects_shell_tasks() {
    let dir = std::env::temp_dir().join(format!("xstack-skill-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("example.txt"), "first\nsecond\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&dir)
        .args(["skill", "--skill", "judgment"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        r#"{{"id":"lines","operation":"lines","path":"example.txt"}}"#
    )
    .unwrap();
    writeln!(
        stdin,
        r#"{{"id":"unsafe","operation":"sh","command":"touch forbidden"}}"#
    )
    .unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let rows: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows[0]["skill"], "judgment");
    assert_eq!(rows[0]["id"], "lines");
    assert_eq!(rows[0]["status"], "ok");
    assert_eq!(rows[0]["count"], 2);
    assert_eq!(rows[0]["completed_count"], 1);
    assert_eq!(rows[0]["failed_count"], 1);
    assert_eq!(rows[1]["id"], "unsafe");
    assert_eq!(rows[1]["operation"], "sh");
    assert_eq!(rows[1]["status"], "unsupported");
    assert!(!dir.join("forbidden").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn skill_entry_rejects_workspace_escapes_and_symlinks() {
    use std::os::unix::fs::symlink;
    let root = std::env::temp_dir().join(format!("xstack-skill-boundary-{}", std::process::id()));
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    fs::write(root.join("secret.txt"), "outside\n").unwrap();
    symlink(root.join("secret.txt"), workspace.join("link")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&workspace)
        .args(["skill", "--skill", "judgment"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(
        input,
        r#"{{"id":"parent","operation":"lines","path":"../secret.txt"}}"#
    )
    .unwrap();
    writeln!(
        input,
        r#"{{"id":"link","operation":"lines","path":"link"}}"#
    )
    .unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let rows: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows[0]["status"], "denied");
    assert_eq!(rows[1]["status"], "denied");
    assert!(!rows
        .iter()
        .any(|row| row["output"].as_str().unwrap_or("").contains("outside")));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn skill_writes_need_explicit_authorization_and_matching_content() {
    let dir = std::env::temp_dir().join(format!("xstack-skill-write-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("example.txt");
    fs::write(&path, "old value\n").unwrap();
    let hash = blake3::hash(b"old value\n").to_hex().to_string();
    let request = serde_json::json!({"id":"edit","operation":"replace","path":"example.txt","old":"old","new":"new","expected_hash":hash,"authorized":true});
    let run = |allow: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_subtask"));
        command
            .current_dir(&dir)
            .args(["skill", "--skill", "implementation"]);
        if allow {
            command.arg("--allow-write");
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(child.stdin.take().unwrap(), "{request}").unwrap();
        child.wait_with_output().unwrap()
    };
    let denied = run(false);
    assert!(!denied.status.success());
    assert_eq!(fs::read_to_string(&path).unwrap(), "old value\n");
    let allowed = run(true);
    assert!(
        allowed.status.success(),
        "{}",
        String::from_utf8_lossy(&allowed.stderr)
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "new value\n");
    let stale = run(true);
    assert!(!stale.status.success());
    assert_eq!(fs::read_to_string(&path).unwrap(), "new value\n");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn skill_deadline_expires_when_input_stays_open() {
    let dir = std::env::temp_dir().join(format!("xstack-skill-timeout-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&dir)
        .args(["skill", "--skill", "judgment", "--timeout-ms", "100"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = child.stdin.take().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if std::time::Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    drop(input);
    if status.is_none() {
        child.kill().unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        status.and_then(|status| status.code()),
        Some(124),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn skill_rejects_unsupported_network_operations_explicitly() {
    let dir = std::env::temp_dir().join(format!("xstack-skill-network-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&dir)
        .args(["skill", "--skill", "judgment"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        r#"{{"id":"network","operation":"http","url":"https://example.com"}}"#
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    let row: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!output.status.success());
    assert_eq!(row["status"], "unsupported");
    assert_eq!(row["id"], "network");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn skill_read_reports_bounded_output_with_evidence() {
    let dir = std::env::temp_dir().join(format!("xstack-skill-read-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("long.txt"), "x".repeat(5000)).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&dir)
        .args(["skill", "--skill", "documentation"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        r#"{{"id":"read","operation":"read","path":"long.txt"}}"#
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let row: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(row["status"], "ok");
    assert_eq!(row["evidence"], "long.txt");
    assert_eq!(row["output"].as_str().unwrap().len(), 4000);
    assert_eq!(row["truncated"], true);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn competing_writes_do_not_both_succeed() {
    let dir = std::env::temp_dir().join(format!("xstack-skill-race-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("file.txt");
    fs::write(&path, "original").unwrap();
    let hash = blake3::hash(b"original").to_hex().to_string();
    let mut children = Vec::new();
    for replacement in ["first", "second"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_subtask"))
            .current_dir(&dir)
            .args(["skill", "--skill", "implementation", "--allow-write"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let request = serde_json::json!({"id":replacement,"operation":"replace","path":"file.txt","old":"original","new":replacement,"expected_hash":hash,"authorized":true});
        writeln!(child.stdin.take().unwrap(), "{request}").unwrap();
        children.push(child);
    }
    let outputs: Vec<_> = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect();
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.status.success())
            .count(),
        1
    );
    assert!(["first", "second"].contains(&fs::read_to_string(path).unwrap().as_str()));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn shared_entry_point_is_discoverable_without_configuration() {
    let output = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .args(["skill", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("--skill"));
    assert!(help.contains("--allow-write"));
    assert!(help.contains("Landlock"));
}

#[test]
fn read_only_mode_does_not_create_registry_inside_workspace() {
    let dir =
        std::env::temp_dir().join(format!("xstack-skill-temp-boundary-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&dir)
        .env("TMPDIR", &dir)
        .args(["skill", "--skill", "judgment"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        r#"{{"id":"missing","operation":"lines","path":"missing"}}"#
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(
        fs::read_dir(&dir).unwrap().next().is_none(),
        "read-only mode changed the workspace"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_search_does_not_echo_sensitive_pattern() {
    let dir = std::env::temp_dir().join(format!("xstack-skill-secret-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("file"), "content").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&dir)
        .args(["skill", "--skill", "judgment"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        r#"{{"id":"search","operation":"grep","path":"file","pattern":"private-secret-782("}}"#
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("private-secret-782"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("private-secret-782"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn write_flag_does_not_authorize_an_unapproved_operation() {
    let dir = std::env::temp_dir().join(format!("xstack-skill-unapproved-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("file");
    fs::write(&path, "before").unwrap();
    let request = serde_json::json!({"id":"edit","operation":"replace","path":"file","old":"before","new":"after","expected_hash":blake3::hash(b"before").to_hex().to_string()});
    let mut child = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&dir)
        .args(["skill", "--skill", "judgment", "--allow-write"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "{request}").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let row: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(row["status"], "denied");
    assert_eq!(fs::read_to_string(path).unwrap(), "before");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn skill_argument_errors_are_structured() {
    let output = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .args(["skill", "--unsupported"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["status"], "failed");
    assert_eq!(error["error"], "unsupported skill argument");
}

#[test]
fn empty_skill_batch_is_an_explicit_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .args(["skill", "--skill", "judgment"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["status"], "failed");
    assert_eq!(error["error"], "skill batch is empty");
}

#[test]
fn oversized_batch_cannot_apply_earlier_writes() {
    let dir = std::env::temp_dir().join(format!("xstack-skill-limit-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("file");
    fs::write(&path, "before").unwrap();
    let request = serde_json::json!({"id":"edit","operation":"replace","path":"file","old":"before","new":"after","expected_hash":blake3::hash(b"before").to_hex().to_string(),"authorized":true});
    let mut child = Command::new(env!("CARGO_BIN_EXE_subtask"))
        .current_dir(&dir)
        .args(["skill", "--skill", "implementation", "--allow-write"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{request}").unwrap();
    for index in 0..64 {
        writeln!(
            stdin,
            r#"{{"id":"{index}","operation":"lines","path":"file"}}"#
        )
        .unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"], "skill batch exceeds 64 operations");
    assert_eq!(fs::read_to_string(path).unwrap(), "before");
    fs::remove_dir_all(dir).unwrap();
}
