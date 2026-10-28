//! Counts changed lines for comparison with the review budget.
use crate::git::{git, lines};
use crate::Result;
use std::fs;
use std::path::Path;

pub const DEFAULT_MAX_LINES: u64 = 400;
pub const DEFAULT_EXCLUDES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "bun.lock",
    "bun.lockb",
    "yarn.lock",
    "pnpm-lock.yaml",
    "poetry.lock",
    "go.sum",
    ".snap",
];

pub struct Budget {
    pub total: u64,
    /// Changed lines per file, largest first.
    pub files: Vec<(String, u64)>,
}

fn excluded(path: &str, excludes: &[String]) -> bool {
    excludes.iter().any(|p| path == p || path.ends_with(p.as_str()) || (p.ends_with('/') && path.starts_with(p.as_str())))
}

/// Counts added and removed lines between `base` and the working tree.
/// Includes untracked files and skips files that match the exclusion list.
pub fn measure(repo: &Path, base: &str, extra_excludes: &[String]) -> Result<Budget> {
    let excludes: Vec<String> = DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).chain(extra_excludes.iter().cloned()).collect();
    let mut files = Vec::new();
    for line in lines(&git(repo, &["diff", "--numstat", base])?) {
        let mut parts = line.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if excluded(path, &excludes) {
            continue;
        }
        let count = added.parse::<u64>().unwrap_or(0) + removed.parse::<u64>().unwrap_or(0);
        files.push((path.to_string(), count));
    }
    let root = git(repo, &["rev-parse", "--show-toplevel"])?;
    for path in lines(&git(repo, &["ls-files", "--others", "--exclude-standard"])?) {
        if excluded(&path, &excludes) {
            continue;
        }
        let count = fs::read(Path::new(&root).join(&path))
            .ok()
            .filter(|bytes| !bytes.contains(&0))
            .map(|bytes| bytes.iter().filter(|b| **b == b'\n').count() as u64)
            .unwrap_or(0);
        files.push((path, count));
    }
    files.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(Budget { total: files.iter().map(|f| f.1).sum(), files })
}
