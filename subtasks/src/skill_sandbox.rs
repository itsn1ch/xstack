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
    let workspace_path =
        fs::canonicalize(".").map_err(|e| format!("cannot resolve workspace: {e}"))?;
    let share = open_share(&workspace_path)?;
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

/// Creates (or reuses) the per-user shared resource directory outside the workspace and opens it
/// without following symlinks, refusing it unless this user owns it and no one else can write it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn open_share(workspace_path: &Path) -> Result<File> {
    let temp = fs::canonicalize(std::env::temp_dir())
        .map_err(|e| format!("cannot resolve temporary directory: {e}"))?;
    let share = temp.join(format!("xstack-share-{}", unsafe { libc::getuid() }));
    if share.starts_with(workspace_path) || workspace_path.starts_with(&share) {
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
    Ok(share)
}

/// The path the kernel currently records for an open directory, after all symlinks.
#[cfg(target_os = "macos")]
fn descriptor_path(file: &File) -> Result<std::path::PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut buffer = [0u8; libc::PATH_MAX as usize];
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, buffer.as_mut_ptr()) } != 0 {
        return Err(format!(
            "cannot resolve sandbox path: {}",
            std::io::Error::last_os_error()
        ));
    }
    let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    Ok(std::ffi::OsStr::from_bytes(&buffer[..end]).into())
}

/// Quotes a path as an SBPL string literal, refusing anything that is not a plain absolute path.
#[cfg(target_os = "macos")]
fn sbpl_path(path: &Path) -> Result<String> {
    let text = path.to_str().ok_or("sandbox path is not UTF-8")?;
    if !text.starts_with('/')
        || text.len() < 2
        || text.contains(['"', '\\'])
        || text.chars().any(char::is_control)
    {
        return Err("sandbox path cannot be expressed safely".into());
    }
    Ok(format!("\"{text}\""))
}

/// The Seatbelt profile for one skill run. Like the Landlock ruleset it governs the filesystem
/// only: every file read, write and exec is denied, then reads (and with `allow_write`, writes)
/// are allowed beneath the workspace, reads and writes beneath the shared resource directory,
/// and the process's own descriptors through `/dev/fd`. Seatbelt resolves symlinks and `..`
/// before matching, so neither can leave an allowed subtree.
#[cfg(target_os = "macos")]
pub(crate) fn profile(workspace: &Path, share: &Path, allow_write: bool) -> Result<String> {
    let workspace = sbpl_path(workspace)?;
    let share = sbpl_path(share)?;
    let write = if allow_write {
        format!("(allow file-write* (subpath {workspace}))\n")
    } else {
        String::new()
    };
    Ok(format!(
        "(version 1)\n\
         (allow default)\n\
         (deny file-read* file-write* file-ioctl process-exec*)\n\
         (allow file-read* (subpath {workspace}))\n\
         {write}\
         (allow file-read* file-write* (subpath {share}))\n\
         (allow file-read* file-write-data (literal \"/dev/null\") (subpath \"/dev/fd\"))\n"
    ))
}

#[cfg(target_os = "macos")]
static SHARE: std::sync::OnceLock<File> = std::sync::OnceLock::new();

/// Confines this process to the workspace with Seatbelt through `sandbox_init`, which Apple
/// deprecated but still ships; a profile the kernel rejects, or any unsafe path, fails closed.
#[cfg(target_os = "macos")]
pub fn restrict(workspace: &File, allow_write: bool) -> Result<()> {
    extern "C" {
        fn sandbox_init(
            profile: *const libc::c_char,
            flags: u64,
            errorbuf: *mut *mut libc::c_char,
        ) -> libc::c_int;
        fn sandbox_free_error(errorbuf: *mut libc::c_char);
    }
    if !workspace.metadata().map_err(|e| e.to_string())?.is_dir() {
        return Err("workspace must be a directory".into());
    }
    let workspace_path = descriptor_path(workspace)?;
    let share = open_share(&workspace_path)?;
    let share_path = descriptor_path(&share)?;
    if share_path.starts_with(&workspace_path) || workspace_path.starts_with(&share_path) {
        return Err("shared resource directory must be outside the workspace".into());
    }
    let profile = std::ffi::CString::new(profile(&workspace_path, &share_path, allow_write)?)
        .map_err(|_| "invalid sandbox profile")?;
    if SHARE.set(share).is_err() {
        return Err("workspace sandbox is already active".into());
    }
    let mut error = std::ptr::null_mut();
    // SAFETY: the profile is a NUL-terminated string and `error` receives an allocation freed below.
    if unsafe { sandbox_init(profile.as_ptr(), 0, &mut error) } != 0 {
        let message = if error.is_null() {
            "unknown error".to_string()
        } else {
            // SAFETY: sandbox_init set `error` to a NUL-terminated message it allocated.
            let text = unsafe { std::ffi::CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned();
            unsafe { sandbox_free_error(error) };
            text
        };
        return Err(format!("cannot enforce workspace sandbox: {message}"));
    }
    Ok(())
}

/// An anonymous read-write file in the shared resource directory, unlinked before it is returned.
/// macOS has no memfd, so this stands in for it; it only works after `restrict` succeeded.
#[cfg(target_os = "macos")]
pub fn temporary_file() -> Result<File> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let share = SHARE.get().ok_or("workspace sandbox is not active")?;
    let name = std::ffi::CString::new(format!(
        ".xstack-skill-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
    .map_err(|_| "invalid temporary name")?;
    // SAFETY: `name` is NUL-terminated and the share descriptor stays open for the process lifetime.
    let fd = unsafe {
        libc::openat(
            share.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(format!(
            "cannot create temporary file: {}",
            std::io::Error::last_os_error()
        ));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    if unsafe { libc::unlinkat(share.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err("cannot detach temporary file".into());
    }
    Ok(file)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn restrict(_workspace: &File, _allow_write: bool) -> Result<()> {
    Err("skill mode requires Linux Landlock or macOS Seatbelt".into())
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use super::*;
    use std::io::ErrorKind;
    use std::process::Command;

    const CHILD_ROOT: &str = "XSTACK_SANDBOX_CHILD_ROOT";
    const CHILD_WRITE: &str = "XSTACK_SANDBOX_CHILD_WRITE";

    #[test]
    fn profile_refuses_paths_that_could_alter_the_rules() {
        let share = Path::new("/private/tmp/share");
        for bad in [
            "relative/ws",
            "/",
            "/ws\")(allow file-read* (subpath \"/\")",
            "/ws\\x",
            "/ws\nline",
        ] {
            assert!(profile(Path::new(bad), share, false).is_err(), "{bad:?}");
        }
        let read_only = profile(Path::new("/private/tmp/ws"), share, false).unwrap();
        assert!(read_only.contains("(deny file-read* file-write* file-ioctl process-exec*)"));
        assert!(!read_only.contains("(allow file-write* (subpath \"/private/tmp/ws\"))"));
        let writable = profile(Path::new("/private/tmp/ws"), share, true).unwrap();
        assert!(writable.contains("(allow file-write* (subpath \"/private/tmp/ws\"))"));
    }

    /// Applies the sandbox in a fresh copy of this test binary, because a sandbox cannot be
    /// lifted, and checks from inside that the kernel itself refuses escapes the skill code
    /// would never attempt: absolute paths, `..`, symlinks, hard links, renames and exec.
    #[test]
    fn seatbelt_confines_files_to_the_workspace() {
        for allow_write in [false, true] {
            let root = fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!(
                    "xstack-seatbelt-{}-{allow_write}",
                    std::process::id()
                ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("ws")).unwrap();
            fs::write(root.join("ws/inside.txt"), "inside\n").unwrap();
            fs::write(root.join("outside.txt"), "outside\n").unwrap();
            std::os::unix::fs::symlink(root.join("outside.txt"), root.join("ws/link")).unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "skill_sandbox::macos_tests::sandboxed_child",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(CHILD_ROOT, &root)
                .env(CHILD_WRITE, if allow_write { "1" } else { "0" })
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && stdout.contains("SANDBOX CHILD OK"),
                "allow_write={allow_write}\n{stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!root.join("escaped.txt").exists());
            assert!(!root.join("moved.txt").exists());
            assert_eq!(
                fs::read_to_string(root.join("outside.txt")).unwrap(),
                "outside\n"
            );
            assert_eq!(
                root.join("ws/created.txt").exists(),
                allow_write,
                "workspace writes must follow --allow-write"
            );
            fs::remove_dir_all(&root).unwrap();
        }
    }

    #[test]
    fn sandboxed_child() {
        let Some(root) = std::env::var_os(CHILD_ROOT).map(std::path::PathBuf::from) else {
            return;
        };
        let allow_write = std::env::var(CHILD_WRITE).as_deref() == Ok("1");
        let ws = root.join("ws");
        let workspace = File::open(&ws).unwrap();
        restrict(&workspace, allow_write).unwrap();
        let denied = |result: std::io::Result<()>, what: &str| match result {
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {}
            other => panic!("{what}: expected the sandbox to deny it, got {other:?}"),
        };

        assert_eq!(
            fs::read_to_string(ws.join("inside.txt")).unwrap(),
            "inside\n"
        );
        denied(
            fs::read(root.join("outside.txt")).map(drop),
            "absolute read outside",
        );
        denied(
            fs::read(ws.join("../outside.txt")).map(drop),
            "read through ..",
        );
        denied(fs::read(ws.join("link")).map(drop), "read through symlink");
        denied(fs::read("/etc/hosts").map(drop), "read system file");
        denied(fs::write(root.join("escaped.txt"), "x"), "write outside");
        denied(
            fs::write(root.join("outside.txt"), "changed"),
            "overwrite outside",
        );
        denied(
            fs::hard_link(root.join("outside.txt"), ws.join("hard")),
            "hard link into workspace",
        );
        denied(
            fs::rename(ws.join("inside.txt"), root.join("moved.txt")),
            "rename out of workspace",
        );
        denied(
            Command::new("/bin/echo").arg("escaped").output().map(drop),
            "exec",
        );
        let created = fs::write(ws.join("created.txt"), "x");
        if allow_write {
            created.unwrap();
        } else {
            denied(created, "write in read-only workspace");
        }

        let mut temp = temporary_file().unwrap();
        use std::io::{Read, Seek, Write};
        temp.write_all(b"snapshot").unwrap();
        temp.rewind().unwrap();
        let mut back = String::new();
        temp.read_to_string(&mut back).unwrap();
        assert_eq!(back, "snapshot");
        assert!(
            restrict(&workspace, allow_write).is_err(),
            "sandbox applied twice"
        );
        println!("SANDBOX CHILD OK");
    }
}
