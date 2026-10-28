use std::fs;
use std::path::Path;
use std::process::Command;

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Audit test")
        .env("GIT_AUTHOR_EMAIL", "audit@example.invalid")
        .env("GIT_COMMITTER_NAME", "Audit test")
        .env("GIT_COMMITTER_EMAIL", "audit@example.invalid")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg(unix)]
fn audit_holds_tracked_edits_and_recent_transcripts_without_deleting_files() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("main repo");
    let worktree = root.path().join("linked worktree");
    let home = root.path().join("home");
    let bin = root.path().join("bin");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let git_path = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join("git"))
        .find(|path| path.is_file())
        .unwrap();
    std::os::unix::fs::symlink(git_path, bin.join("git")).unwrap();
    git(&repo, &["init", "--quiet"]);
    fs::write(repo.join("tracked.txt"), "original\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "--quiet", "-m", "initial"]);
    git(
        &repo,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "audit-linked",
            worktree.to_str().unwrap(),
        ],
    );
    fs::write(worktree.join("tracked.txt"), "changed\n").unwrap();
    fs::write(worktree.join("scratch.txt"), "scratch\n").unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_worktree-audit"))
            .args(["-b", "HEAD"])
            .arg(&repo)
            .env("HOME", &home)
            .env("PATH", &bin)
            .output()
            .unwrap()
    };
    let first = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let stdout = String::from_utf8(first.stdout).unwrap();
    assert!(
        stdout.contains("\tYES\twip:1\tno-remote\t-\t-\thold-wip\t"),
        "{stdout}"
    );
    assert!(stdout.contains(worktree.to_str().unwrap()));
    fs::write(worktree.join("tracked.txt"), "original\n").unwrap();
    let slug = worktree
        .to_string_lossy()
        .trim_start_matches('/')
        .replace('/', "-");
    let transcripts = home
        .join(".cursor/projects")
        .join(slug)
        .join("agent-transcripts");
    fs::create_dir_all(&transcripts).unwrap();
    fs::write(
        transcripts.join("session.jsonl"),
        format!("{{\"cwd\":\"{}\"}}", worktree.display()),
    )
    .unwrap();
    let second = run();
    assert!(second.status.success());
    let stdout = String::from_utf8(second.stdout).unwrap();
    assert!(
        stdout.contains("\tYES\tscratch:1\tno-remote\t-\t"),
        "{stdout}"
    );
    assert!(stdout.contains("\tverify-recent-chat\t"), "{stdout}");
    assert_eq!(
        fs::read_to_string(worktree.join("scratch.txt")).unwrap(),
        "scratch\n"
    );
}
