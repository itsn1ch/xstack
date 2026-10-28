use chrono::{DateTime, Local};
use clap::Parser;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser)]
#[command(
    name = "worktree-audit",
    about = "Audit worktrees without deleting files. Buckets are advice, not permission to delete."
)]
struct Args {
    /// Ref used to check whether a worktree has merged. Defaults to origin/HEAD, origin/main, then origin/master.
    #[arg(short = 'b', long)]
    base: Option<String>,
    /// Repository path. Defaults to the current repository.
    repo: Option<PathBuf>,
}

fn output(repo: &Path, program: &str, args: &[&str]) -> Option<Vec<u8>> {
    let result = Command::new(program)
        .args(args)
        .current_dir(repo)
        .output()
        .ok()?;
    result.status.success().then_some(result.stdout)
}

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    output(repo, "git", args).map(|bytes| String::from_utf8_lossy(&bytes).trim_end().to_string())
}

fn paths(porcelain: &[u8]) -> Vec<PathBuf> {
    porcelain
        .split(|b| *b == 0)
        .filter_map(|field| {
            field.strip_prefix(b"worktree ").map(|name| {
                #[cfg(unix)]
                {
                    use std::os::unix::ffi::OsStrExt;
                    PathBuf::from(std::ffi::OsStr::from_bytes(name))
                }
                #[cfg(not(unix))]
                {
                    PathBuf::from(String::from_utf8_lossy(name).into_owned())
                }
            })
        })
        .collect()
}

fn dirty_status(bytes: &[u8]) -> String {
    let mut tracked = 0;
    let mut scratch = 0;
    let mut fields = bytes.split(|b| *b == 0).filter(|field| !field.is_empty());
    while let Some(field) = fields.next() {
        if field.starts_with(b"??") {
            scratch += 1;
        } else {
            tracked += 1;
            if field
                .get(..2)
                .is_some_and(|s| s.contains(&b'R') || s.contains(&b'C'))
            {
                fields.next();
            }
        }
    }
    if tracked > 0 {
        format!("wip:{tracked}")
    } else if scratch > 0 {
        format!("scratch:{scratch}")
    } else {
        "clean".into()
    }
}

fn bucket(dirty: &str, pr: &str, recent: bool, merged: bool) -> &'static str {
    if dirty.starts_with("wip:") {
        "hold-wip"
    } else if pr.ends_with("/open") || pr.ends_with("/draft") {
        "hold-open-pr"
    } else if recent {
        "verify-recent-chat"
    } else if merged || pr != "-" {
        "safe"
    } else {
        "review"
    }
}

fn disk_bytes(path: &Path, seen: &mut HashSet<(u64, u64)>) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    #[cfg(unix)]
    let own = {
        use std::os::unix::fs::MetadataExt;
        if !seen.insert((meta.dev(), meta.ino())) {
            return 0;
        }
        meta.blocks().saturating_mul(512)
    };
    #[cfg(not(unix))]
    let own = meta.len();
    if !meta.is_dir() {
        return own;
    }
    fs::read_dir(path)
        .map(|entries| {
            entries.flatten().fold(own, |size, entry| {
                size.saturating_add(disk_bytes(&entry.path(), seen))
            })
        })
        .unwrap_or(own)
}

fn human_size(bytes: u64) -> String {
    let units = ["", "K", "M", "G", "T", "P", "E"];
    let mut value = bytes as f64;
    let mut index = 0;
    while value >= 1024.0 && index + 1 < units.len() {
        value /= 1024.0;
        index += 1;
    }
    if index == 0 {
        return bytes.to_string();
    }
    if value < 10.0 {
        format!("{:.1}{}", (value * 10.0).ceil() / 10.0, units[index])
    } else {
        format!("{:.0}{}", value.ceil(), units[index])
    }
}

fn transcript_dirs(home: &Path, project: &Path) -> Vec<PathBuf> {
    let name = project.to_string_lossy();
    let slash = name.trim_start_matches('/').replace('/', "-");
    let claude: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    [
        home.join(".cursor/projects")
            .join(&slash)
            .join("agent-transcripts"),
        home.join(".claude/projects").join(claude),
        home.join(".pi/agent/sessions").join(format!("--{slash}--")),
    ]
    .into_iter()
    .filter(|p| p.is_dir())
    .collect()
}

fn mentions_worktree(path: &Path, needles: &[String]) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return false,
            Ok(_) => {
                if needles.iter().any(|needle| {
                    line.windows(needle.len())
                        .any(|bytes| bytes == needle.as_bytes())
                }) {
                    return true;
                }
            }
        }
    }
}

fn latest_transcript(dir: &Path, needles: &[String], latest: &mut Option<SystemTime>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            latest_transcript(&path, needles, latest);
        } else if meta.is_file() {
            let Ok(modified) = meta.modified() else {
                continue;
            };
            if latest.is_some_and(|last| modified <= last) {
                continue;
            }
            if mentions_worktree(&path, needles) {
                *latest = Some(modified);
            }
        }
    }
}

fn prs(repo: &Path) -> Vec<(String, String)> {
    let origin = output(
        repo,
        "origin",
        &[
            "pr",
            "list",
            "--author",
            "@me",
            "--state",
            "all",
            "--limit",
            "1000",
            "--json",
            "number,status,headRef",
        ],
    );
    let (bytes, is_origin) = match origin {
        Some(bytes) => (bytes, true),
        None => (
            output(
                repo,
                "gh",
                &[
                    "pr",
                    "list",
                    "--author",
                    "@me",
                    "--state",
                    "all",
                    "--limit",
                    "1000",
                    "--json",
                    "number,state,headRefName",
                ],
            )
            .unwrap_or_default(),
            false,
        ),
    };
    let rows: Vec<Value> = serde_json::from_slice(&bytes).unwrap_or_default();
    rows.into_iter()
        .filter_map(|row| {
            let head = row[if is_origin { "headRef" } else { "headRefName" }]
                .as_str()?
                .to_owned();
            let state = row[if is_origin { "status" } else { "state" }].as_str()?;
            Some((
                head,
                format!(
                    "#{}/{}",
                    row["number"].as_u64()?,
                    if is_origin {
                        state.to_owned()
                    } else {
                        state.to_lowercase()
                    }
                ),
            ))
        })
        .collect()
}

fn audit(args: Args) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let repo = match args.repo {
        Some(path) => path,
        None => PathBuf::from(
            git(&cwd, &["rev-parse", "--show-toplevel"])
                .ok_or("not in a git repo; pass a repo path")?,
        ),
    };
    if git(&repo, &["rev-parse", "--git-dir"]).is_none() {
        return Err(format!("not a git repo: {}", repo.display()));
    }
    let automatic = args.base.is_none();
    let base = match args.base {
        Some(base) => base,
        None => {
            let head = git(
                &repo,
                &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
            )
            .unwrap_or_default();
            [head, "origin/main".into(), "origin/master".into()].into_iter()
                .find(|r| !r.is_empty() && git(&repo, &["rev-parse", "--verify", "--quiet", &format!("{r}^{{commit}}")]).is_some())
                .ok_or("no base ref (origin/HEAD, origin/main, origin/master all missing); pass -b <ref>")?
        }
    };
    if automatic {
        eprintln!("warn: no -b given; MERGED checks against {base}");
    }
    if let Some(branch) = base.strip_prefix("origin/") {
        if git(
            &repo,
            &[
                "fetch",
                "origin",
                &format!("+refs/heads/{branch}:refs/remotes/{base}"),
                "--quiet",
            ],
        )
        .is_none()
        {
            eprintln!("warn: could not fetch {base}; merged column may be stale");
        }
    }
    if git(
        &repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{base}^{{commit}}"),
        ],
    )
    .is_none()
    {
        return Err(format!("base ref not found: {base}"));
    }
    let worktrees = paths(
        &output(&repo, "git", &["worktree", "list", "--porcelain", "-z"])
            .ok_or("cannot list worktrees")?,
    );
    let main = worktrees.first().ok_or("no worktrees found")?;
    let prs = prs(&repo);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let now = SystemTime::now();
    let now_secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    let mut rows = Vec::new();
    for wt in worktrees.iter().skip(1) {
        let size = disk_bytes(wt, &mut HashSet::new());
        let head = git(wt, &["rev-parse", "HEAD"]).unwrap_or_default();
        let age = git(wt, &["log", "-1", "--format=%ct", "HEAD"])
            .and_then(|s| s.parse::<i64>().ok())
            .filter(|n| *n > 0)
            .map(|ts| format!("{}d", (now_secs - ts) / 86400))
            .unwrap_or_else(|| "?".into());
        let merged = git(&repo, &["merge-base", "--is-ancestor", &head, &base]).is_some();
        let dirty =
            dirty_status(&output(wt, "git", &["status", "--porcelain", "-z"]).unwrap_or_default());
        let branch = git(wt, &["symbolic-ref", "--quiet", "--short", "HEAD"]).unwrap_or_default();
        let remote = if branch.is_empty() {
            "detached".into()
        } else if git(
            wt,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/remotes/origin/{branch}"),
            ],
        )
        .is_some()
        {
            if git(wt, &["rev-parse", &format!("origin/{branch}")]).as_deref() == Some(&head) {
                "pushed".into()
            } else {
                format!(
                    "ahead{}",
                    git(
                        wt,
                        &["rev-list", "--count", &format!("origin/{branch}..HEAD")]
                    )
                    .unwrap_or_default()
                )
            }
        } else {
            "no-remote".into()
        };
        let pr = prs
            .iter()
            .find(|(head, _)| !branch.is_empty() && head == &branch)
            .map(|(_, pr)| pr.as_str())
            .unwrap_or("-");
        let mut latest = None;
        if let Some(home) = &home {
            let dirs: HashSet<PathBuf> = transcript_dirs(home, main)
                .into_iter()
                .chain(transcript_dirs(home, wt))
                .collect();
            let needles = [format!("{}/", wt.display()), format!("{}\"", wt.display())];
            for dir in dirs {
                latest_transcript(&dir, &needles, &mut latest);
            }
        }
        let recent = latest.is_some_and(|last| {
            now_secs
                - (last
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64)
                < 5 * 86400
        });
        let last = latest
            .map(|time| DateTime::<Local>::from(time).format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "-".into());
        rows.push((
            size,
            format!(
                "{}\t{age}\t{}\t{dirty}\t{remote}\t{pr}\t{last}\t{}\t{}",
                human_size(size),
                if merged { "YES" } else { "no" },
                bucket(&dirty, pr, recent, merged),
                wt.display()
            ),
        ));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    println!("# base: {base}");
    println!("SIZE\tAGE\tMERGED\tDIRTY\tREMOTE\tPR\tLAST_CHAT\tBUCKET\tWORKTREE");
    for (_, row) in rows {
        println!("{row}");
    }
    Ok(())
}

pub fn main() -> i32 {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(error) => {
            let code = error.exit_code();
            let _ = error.print();
            return code;
        }
    };
    match audit(args) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("error: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracked_edits_take_precedence_over_scratch_and_rename_counts_once() {
        assert_eq!(
            dirty_status(b"?? scratch\0 M tracked\0R  new\0old\0"),
            "wip:2"
        );
        assert_eq!(dirty_status(b"?? a\nb\0?? c\0"), "scratch:2");
        assert_eq!(dirty_status(b""), "clean");
    }

    #[test]
    fn buckets_preserve_hold_and_recent_chat_precedence() {
        assert_eq!(bucket("wip:1", "#2/open", true, true), "hold-wip");
        assert_eq!(bucket("clean", "#2/draft", true, true), "hold-open-pr");
        assert_eq!(
            bucket("scratch:2", "#2/merged", true, true),
            "verify-recent-chat"
        );
        assert_eq!(bucket("clean", "#2/closed", false, false), "safe");
        assert_eq!(bucket("clean", "-", false, true), "safe");
        assert_eq!(bucket("clean", "-", false, false), "review");
    }

    #[test]
    fn worktree_paths_preserve_spaces_and_newlines() {
        assert_eq!(
            paths(b"worktree /tmp/main repo\0HEAD abc\0\0worktree /tmp/other\nrepo\0HEAD def\0\0"),
            vec![
                PathBuf::from("/tmp/main repo"),
                PathBuf::from("/tmp/other\nrepo")
            ]
        );
    }

    #[test]
    fn transcript_match_requires_a_path_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("chat.jsonl");
        let needles = ["/repo/wt/".into(), "/repo/wt\"".into()];
        fs::write(&file, "\"/repo/wt-other/file\"").unwrap();
        let mut latest = None;
        latest_transcript(dir.path(), &needles, &mut latest);
        assert!(latest.is_none());
        fs::write(&file, "\"/repo/wt/file\"").unwrap();
        latest_transcript(dir.path(), &needles, &mut latest);
        assert!(latest.is_some());
    }
}
