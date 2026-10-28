use crate::Result;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn lines(text: &str) -> Vec<String> {
    text.lines().filter(|l| !l.is_empty()).map(str::to_string).collect()
}

pub fn is_ancestor(repo: &Path, older: &str, newer: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["merge-base", "--is-ancestor", older, newer])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub struct Commit {
    pub sha: String,
    pub tree: String,
}

pub fn resolve_commit(repo: &Path, reference: &str) -> Result<Commit> {
    let sha = git(repo, &["rev-parse", &format!("{reference}^{{commit}}")])?;
    let tree = tree_of(repo, &sha)?;
    Ok(Commit { sha, tree })
}

pub fn tree_of(repo: &Path, sha: &str) -> Result<String> {
    git(repo, &["rev-parse", &format!("{sha}^{{tree}}")])
}

pub fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}
