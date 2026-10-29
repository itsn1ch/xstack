use crate::Result;
use std::fs::{self, File};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

#[cfg(target_os = "linux")]
pub fn restrict(workspace: &File, allow_write: bool) -> Result<()> {
    const WRITE_FILE: u64 = 1 << 1;
    const READ_FILE: u64 = 1 << 2;
    const READ_DIRECTORY: u64 = 1 << 3;
    const REMOVE_FILE: u64 = 1 << 5;
    const MAKE_REGULAR_FILE: u64 = 1 << 8;
    const REFER: u64 = 1 << 13;
    const TRUNCATE_FILE: u64 = 1 << 14;
    const READ: u64 = READ_FILE | READ_DIRECTORY;
    const WRITE: u64 = WRITE_FILE | REMOVE_FILE | MAKE_REGULAR_FILE | TRUNCATE_FILE;
    const ALL: u64 = (TRUNCATE_FILE << 1) - 1;
    #[repr(C)]
    struct Ruleset {
        handled_access_fs: u64,
    }
    #[repr(C, packed)]
    struct PathRule {
        allowed_access: u64,
        parent_fd: i32,
    }

    let abi = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            std::ptr::null::<u8>(),
            0,
            1,
        )
    };
    if abi < 3 {
        return Err("skill mode requires Linux Landlock ABI 3 or newer".into());
    }
    let temp = fs::canonicalize(std::env::temp_dir())
        .map_err(|e| format!("cannot resolve temporary directory: {e}"))?;
    let workspace_path =
        fs::canonicalize(".").map_err(|e| format!("cannot resolve workspace: {e}"))?;
    let share = temp.join(format!("xstack-share-{}", unsafe { libc::getuid() }));
    if share.starts_with(&workspace_path) || workspace_path.starts_with(&share) {
        return Err("shared resource directory must be outside the workspace".into());
    }
    match fs::DirBuilder::new().mode(0o700).create(&share) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(format!("cannot prepare shared resource directory: {e}")),
    }
    let share = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&share)
        .map_err(|e| format!("cannot open shared resource directory: {e}"))?;
    let meta = share.metadata().map_err(|e| e.to_string())?;
    if meta.uid() != unsafe { libc::getuid() } || meta.mode() & 0o022 != 0 {
        return Err("unsafe shared resource directory permissions".into());
    }
    let proc = File::open(Path::new("/proc")).map_err(|e| e.to_string())?;
    let rules = Ruleset {
        handled_access_fs: ALL,
    };
    let fd = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            &rules,
            std::mem::size_of::<Ruleset>(),
            0,
        )
    };
    if fd < 0 {
        return Err(format!(
            "cannot create workspace sandbox: {}",
            std::io::Error::last_os_error()
        ));
    }
    let ruleset = unsafe { File::from_raw_fd(fd as i32) };
    for (path, rights) in [
        (workspace, READ | if allow_write { WRITE } else { 0 }),
        (&share, READ | WRITE | REFER),
        (&proc, READ),
    ] {
        let rule = PathRule {
            allowed_access: rights,
            parent_fd: path.as_raw_fd(),
        };
        if unsafe {
            libc::syscall(
                libc::SYS_landlock_add_rule,
                ruleset.as_raw_fd(),
                1,
                &rule,
                0,
            )
        } < 0
        {
            return Err(format!(
                "cannot add workspace sandbox rule: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(format!(
            "cannot restrict process privileges: {}",
            std::io::Error::last_os_error()
        ));
    }
    if unsafe { libc::syscall(libc::SYS_landlock_restrict_self, ruleset.as_raw_fd(), 0) } < 0 {
        return Err(format!(
            "cannot enforce workspace sandbox: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn restrict(_workspace: &File, _allow_write: bool) -> Result<()> {
    Err("skill mode is unavailable without Linux Landlock".into())
}
