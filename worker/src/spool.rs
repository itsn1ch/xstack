//! Stores each verification job in a file whose name determines its run order.
//!
//!   jobs/queued/<commit seq>-<rank>-<sha12>.json   waiting, lowest name first
//!   jobs/running/<same name>                       claimed through an atomic rename
//!   jobs/done/<same name>                          finished, with its result
//!
//! A commit's jobs share its sequence number, so older commits run first.
//! Within each commit, checks that failed last time run first, followed by the slowest checks.
use crate::state::{home, CheckResult};
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub sha: String,
    /// Results use this index to follow the configured check order.
    pub index: usize,
    pub cmd: String,
    pub worktree: PathBuf,
    pub timeout_ms: u64,
    /// Unique among the jobs that can run at once, across every commit in flight.
    /// The worker replaces `{slot}` with it and exports it as `XSTACK_WORKER_SLOT`.
    #[serde(default)]
    pub slot: usize,
    /// This check failed at the baseline. Its failure does not stop the commit's other jobs.
    #[serde(default)]
    pub preexisting: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<CheckResult>,
}

#[derive(Clone, Copy)]
pub enum Stage {
    Queued,
    Running,
    Done,
}

impl Stage {
    fn dir(self) -> &'static str {
        match self {
            Stage::Queued => "queued",
            Stage::Running => "running",
            Stage::Done => "done",
        }
    }
}

const STAGES: [Stage; 3] = [Stage::Queued, Stage::Running, Stage::Done];

pub fn short_sha(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

pub fn job_name(seq: usize, rank: usize, sha: &str) -> String {
    format!("{seq:06}-{rank:03}-{}.json", short_sha(sha))
}

fn sha_of(name: &str) -> &str {
    name.trim_end_matches(".json")
        .rsplit('-')
        .next()
        .unwrap_or("")
}

pub struct Spool {
    root: PathBuf,
}

impl Spool {
    pub fn open(repo: &Path) -> Result<Spool> {
        let root = home(repo)?.join("jobs");
        for stage in STAGES {
            fs::create_dir_all(root.join(stage.dir()))
                .map_err(|e| format!("cannot create {}: {e}", root.display()))?;
        }
        Ok(Spool { root })
    }

    pub fn dir(&self, stage: Stage) -> PathBuf {
        self.root.join(stage.dir())
    }

    fn write(&self, stage: Stage, name: &str, job: &Job) -> Result<()> {
        let tmp = self.dir(stage).join(format!(".{name}.tmp"));
        let bytes = serde_json::to_vec_pretty(job).map_err(|e| e.to_string())?;
        fs::write(&tmp, bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        fs::rename(&tmp, self.dir(stage).join(name))
            .map_err(|e| format!("cannot publish job {name}: {e}"))
    }

    pub fn push(&self, name: &str, job: &Job) -> Result<()> {
        self.write(Stage::Queued, name, job)
    }

    pub fn read(&self, stage: Stage, name: &str) -> Option<Job> {
        serde_json::from_slice(&fs::read(self.dir(stage).join(name)).ok()?).ok()
    }

    /// Job names in a stage, in run order. With `sha`, only that commit's jobs.
    pub fn list(&self, stage: Stage, sha: Option<&str>) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.dir(stage))
            .map(|dir| {
                dir.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.retain(|n| {
            !n.starts_with('.')
                && n.ends_with(".json")
                && sha.is_none_or(|s| sha_of(n) == short_sha(s))
        });
        names.sort();
        names
    }

    /// Moves a queued job to running. Only one caller wins the rename, the others get `None`.
    pub fn claim(&self, name: &str) -> Option<Job> {
        fs::rename(
            self.dir(Stage::Queued).join(name),
            self.dir(Stage::Running).join(name),
        )
        .ok()?;
        self.read(Stage::Running, name)
    }

    pub fn finish(&self, name: &str, job: &Job) -> Result<()> {
        self.write(Stage::Done, name, job)?;
        let _ = fs::remove_file(self.dir(Stage::Running).join(name));
        Ok(())
    }

    pub fn remove(&self, stage: Stage, name: &str) {
        let _ = fs::remove_file(self.dir(stage).join(name));
    }

    /// Removes every file of a commit, in every stage.
    pub fn purge(&self, sha: &str) {
        for stage in STAGES {
            for name in self.list(stage, Some(sha)) {
                self.remove(stage, &name);
            }
        }
    }

    /// The short SHAs that have a file in any stage.
    pub fn shas(&self) -> HashSet<String> {
        STAGES
            .iter()
            .flat_map(|s| self.list(*s, None))
            .map(|n| sha_of(&n).to_string())
            .collect()
    }
}
