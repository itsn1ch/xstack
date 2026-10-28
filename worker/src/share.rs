//! Registers every run that wants cores, so the verification worker and each `subtask` run split the machine evenly.
//! Each run holds an exclusive flock on its own file. A file nobody holds belongs to a dead run.
//! Where the temp dir is read-only (some sandboxes), a run goes unregistered and counts as alone instead of failing.
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
    /// The registered file and the lock that marks this run alive; `None` when the registry could not be written.
    entry: Option<(PathBuf, File)>,
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
    /// Registers this run, or runs unregistered when the registry cannot be written. Never fails.
    pub fn join(kind: Kind) -> Share {
        Share {
            kind,
            entry: register(kind).ok(),
        }
    }

    /// Whether other runs can see this one. An unregistered run still sees the registered ones.
    pub fn registered(&self) -> bool {
        self.entry.is_some()
    }

    /// Counts live runs and removes the files of dead ones.
    pub fn runs(&self) -> Runs {
        let Ok(entries) = fs::read_dir(dir()) else {
            return Runs::ALONE;
        };
        let mine = format!("{}-", self.kind.prefix());
        // An unregistered run has no file, so it counts itself.
        let me = usize::from(!self.registered());
        let (mut all, mut same_kind) = (me, me);
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

/// Locks the file under a hidden name first, so a scan never mistakes a starting run for a dead one.
fn register(kind: Kind) -> Result<(PathBuf, File)> {
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
        let _ = fs::remove_file(&staging);
        return Err(format!("cannot lock {}", staging.display()));
    }
    let path = dir.join(name);
    if let Err(e) = fs::rename(&staging, &path) {
        let _ = fs::remove_file(&staging);
        return Err(format!("cannot register {}: {e}", path.display()));
    }
    Ok((path, lock))
}

impl Drop for Share {
    fn drop(&mut self) {
        if let Some((path, _)) = &self.entry {
            let _ = fs::remove_file(path);
        }
    }
}
