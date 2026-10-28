//! Registers every run that wants cores, so the verification worker and each `subtask` run split the machine evenly.
//! Each run holds an exclusive flock on its own file. A file nobody holds belongs to a dead run.
use crate::Result;
use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Who registered a run. Every kind shares the cores; only subtask runs share the network and file pools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Verification,
    Subtask,
}

impl Kind {
    fn prefix(self) -> &'static str {
        match self {
            Kind::Verification => "verification",
            Kind::Subtask => "subtask",
        }
    }
}

/// Live runs, this one included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Runs {
    pub all: usize,
    pub same_kind: usize,
}

impl Runs {
    pub const ALONE: Runs = Runs {
        all: 1,
        same_kind: 1,
    };
}

pub struct Share {
    kind: Kind,
    path: PathBuf,
    _lock: File,
}

fn dir() -> PathBuf {
    // SAFETY: getuid has no preconditions.
    std::env::temp_dir().join(format!("xstack-share-{}", unsafe { libc::getuid() }))
}

fn try_lock(file: &File) -> bool {
    // SAFETY: flock on a descriptor the caller owns.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
}

impl Share {
    /// Locks the file under a hidden name first, so a scan never mistakes a starting run for a dead one.
    pub fn join(kind: Kind) -> Result<Share> {
        let dir = dir();
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let name = format!("{}-{}-{nanos}", kind.prefix(), std::process::id());
        let staging = dir.join(format!(".{name}"));
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&staging)
            .map_err(|e| format!("cannot create {}: {e}", staging.display()))?;
        if !try_lock(&lock) {
            return Err(format!("cannot lock {}", staging.display()));
        }
        let path = dir.join(name);
        fs::rename(&staging, &path)
            .map_err(|e| format!("cannot register {}: {e}", path.display()))?;
        Ok(Share {
            kind,
            path,
            _lock: lock,
        })
    }

    /// Counts live runs and removes the files of dead ones.
    pub fn runs(&self) -> Runs {
        let Ok(entries) = fs::read_dir(dir()) else {
            return Runs::ALONE;
        };
        let mine = format!("{}-", self.kind.prefix());
        let (mut all, mut same_kind) = (0, 0);
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(file) = File::open(entry.path()) else {
                continue;
            };
            if try_lock(&file) {
                let _ = fs::remove_file(entry.path());
                continue;
            }
            all += 1;
            if name.starts_with(&mine) {
                same_kind += 1;
            }
        }
        Runs {
            all: all.max(1),
            same_kind: same_kind.max(1),
        }
    }
}

impl Drop for Share {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
