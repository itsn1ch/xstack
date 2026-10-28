use crate::engine::{self, SubOptions, Task, Widths};
use crate::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::ffi::CString;
use std::fs::File;
use std::io::{self, BufRead, Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_TASKS: usize = 64;
const MAX_INPUT: usize = 1024 * 1024;
const MAX_FILE: u64 = 8 * 1024 * 1024;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: String,
    operation: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    pattern: String,
    #[serde(default)]
    command: String,
    #[serde(default)]
    old: String,
    #[serde(default)]
    new: String,
    #[serde(default)]
    expected_hash: String,
    #[serde(default)]
    authorized: bool,
}

#[cfg(target_os = "linux")]
fn temporary_file() -> Result<File> {
    let fd = unsafe { libc::memfd_create(c"xstack-skill".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(format!(
            "cannot create temporary file: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(not(target_os = "linux"))]
fn temporary_file() -> Result<File> {
    Err("skill mode is unavailable without Linux Landlock".into())
}

fn open_workspace_file(root: &File, path: &str) -> Result<File> {
    let parts: Vec<_> = Path::new(path).components().collect();
    if parts.is_empty() || parts.iter().any(|c| !matches!(c, Component::Normal(_))) {
        return Err("path must be relative to the workspace".into());
    }
    let mut directory = root.try_clone().map_err(|e| e.to_string())?;
    for (index, part) in parts.iter().enumerate() {
        let Component::Normal(name) = part else {
            unreachable!()
        };
        let name = CString::new(name.as_bytes()).map_err(|_| "invalid path")?;
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if index + 1 < parts.len() {
                libc::O_DIRECTORY
            } else {
                0
            };
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(format!(
                "cannot open workspace file: {}",
                io::Error::last_os_error()
            ));
        }
        directory = unsafe { File::from_raw_fd(fd) };
    }
    let metadata = directory.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_FILE {
        return Err("workspace input must be a regular file of at most 8 MiB".into());
    }
    Ok(directory)
}

fn replace_workspace_file(root: &File, request: &Request) -> Result<()> {
    let path = Path::new(&request.path);
    let name = path.file_name().ok_or("invalid workspace path")?;
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("path must be relative to the workspace".into());
    }
    let parent = match path.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(parent) => {
            let mut dir = root.try_clone().map_err(|e| e.to_string())?;
            for part in parent.components() {
                let Component::Normal(name) = part else {
                    return Err("invalid workspace path".into());
                };
                let name = CString::new(name.as_bytes()).map_err(|_| "invalid path")?;
                let fd = unsafe {
                    libc::openat(
                        dir.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
                if fd < 0 {
                    return Err("cannot open workspace directory".into());
                }
                dir = unsafe { File::from_raw_fd(fd) };
            }
            dir
        }
        None => root.try_clone().map_err(|e| e.to_string())?,
    };
    let name = CString::new(name.as_bytes()).map_err(|_| "invalid path")?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err("cannot open workspace file".into());
    }
    let mut original = unsafe { File::from_raw_fd(fd) };
    if unsafe { libc::flock(original.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("workspace file is locked by another operation".into());
    }
    let before = original.metadata().map_err(|e| e.to_string())?;
    if !before.is_file() || before.len() > MAX_FILE {
        return Err("workspace input must be a regular file of at most 8 MiB".into());
    }
    if before.mode() & 0o7000 != 0 {
        return Err("files with special mode bits cannot be replaced".into());
    }
    #[cfg(target_os = "linux")]
    if unsafe { libc::flistxattr(original.as_raw_fd(), std::ptr::null_mut(), 0) } != 0 {
        return Err("files with extended attributes cannot be replaced".into());
    }
    #[cfg(not(target_os = "linux"))]
    return Err("replacement is unsupported on this operating system".into());
    let mut bytes = Vec::new();
    Read::by_ref(&mut original)
        .take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FILE
        || blake3::hash(&bytes).to_hex().as_str() != request.expected_hash
    {
        return Err("workspace file does not match expected hash".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "workspace file is not UTF-8")?;
    if request.old.is_empty() || text.matches(&request.old).count() != 1 {
        return Err("replacement must match exactly once".into());
    }
    let updated = text.replacen(&request.old, &request.new, 1);
    if updated.len() as u64 > MAX_FILE {
        return Err("replacement exceeds 8 MiB".into());
    }
    let mut current: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            &mut current,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
        || current.st_dev as u64 != before.dev()
        || current.st_ino as u64 != before.ino()
    {
        return Err("workspace file changed while waiting for the lock".into());
    }
    let temp_name = CString::new(format!(
        ".xstack-write-{}-{}",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ))
    .unwrap();
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            temp_name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err("cannot create replacement file".into());
    }
    let mut temp = unsafe { File::from_raw_fd(fd) };
    let result = (|| -> Result<()> {
        let new_meta = temp.metadata().map_err(|e| e.to_string())?;
        if new_meta.uid() != before.uid() || new_meta.gid() != before.gid() {
            return Err("file ownership cannot be preserved".into());
        }
        if unsafe { libc::fchmod(temp.as_raw_fd(), before.mode() & 0o7777) } != 0 {
            return Err("cannot preserve file mode".into());
        }
        temp.write_all(updated.as_bytes())
            .map_err(|e| e.to_string())?;
        temp.sync_all().map_err(|e| e.to_string())?;
        let after = original.metadata().map_err(|e| e.to_string())?;
        if after.size() != before.size()
            || after.mtime() != before.mtime()
            || after.mtime_nsec() != before.mtime_nsec()
        {
            return Err("workspace file changed during replacement".into());
        }
        if unsafe {
            libc::fstatat(
                parent.as_raw_fd(),
                name.as_ptr(),
                &mut current,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
            || current.st_dev as u64 != before.dev()
            || current.st_ino as u64 != before.ino()
        {
            return Err("workspace file changed during replacement".into());
        }
        if unsafe {
            libc::renameat(
                parent.as_raw_fd(),
                temp_name.as_ptr(),
                parent.as_raw_fd(),
                name.as_ptr(),
            )
        } != 0
        {
            return Err("cannot commit workspace replacement".into());
        }
        Ok(())
    })();
    unsafe {
        libc::unlinkat(parent.as_raw_fd(), temp_name.as_ptr(), 0);
    }
    result
}

#[cfg(target_os = "linux")]
extern "C" fn deadline_expired(_: libc::c_int) {
    let message = b"{\"status\":\"timed_out\",\"error\":\"skill deadline exceeded\"}\n";
    unsafe {
        libc::write(libc::STDERR_FILENO, message.as_ptr().cast(), message.len());
        libc::_exit(124);
    }
}

#[cfg(target_os = "linux")]
fn arm_deadline(timeout_ms: u64) -> Result<()> {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = deadline_expired as *const () as usize;
    unsafe {
        libc::sigemptyset(&mut action.sa_mask);
    }
    if unsafe { libc::sigaction(libc::SIGALRM, &action, std::ptr::null_mut()) } != 0 {
        return Err("cannot configure skill deadline".into());
    }
    let timer = libc::itimerval {
        it_interval: libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        it_value: libc::timeval {
            tv_sec: (timeout_ms / 1000) as libc::time_t,
            tv_usec: ((timeout_ms % 1000) * 1000) as libc::suseconds_t,
        },
    };
    if unsafe { libc::setitimer(libc::ITIMER_REAL, &timer, std::ptr::null_mut()) } != 0 {
        return Err("cannot configure skill deadline".into());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn arm_deadline(_: u64) -> Result<()> {
    Err("skill mode is unavailable without Linux Landlock".into())
}

pub fn run(args: &[String]) -> Result<u8> {
    if args == ["--help"] {
        println!("subtask skill --skill <name> [--timeout-ms <1..300000>] [--allow-write]");
        println!("Read up to 64 JSONL operations from stdin, relative to the current workspace.");
        println!(
            "Operations: read, lines, hash, grep, replace. Unsupported operations fail explicitly."
        );
        println!(r#"Read: {{"id":"source","operation":"read","path":"README.md"}}"#);
        println!(
            r#"Search: {{"id":"search","operation":"grep","path":"README.md","pattern":"text"}}"#
        );
        println!(
            r#"Replace: {{"id":"edit","operation":"replace","path":"file","old":"before","new":"after","expected_hash":"<blake3>","authorized":true}}"#
        );
        println!("Replace requires --allow-write, authorized=true, and an expected BLAKE3 hash.");
        println!(
            "The host must authorize invocation. This mode requires Linux Landlock ABI 3 or newer."
        );
        return Ok(0);
    }
    let mut skill = None;
    let mut allow_write = false;
    let mut timeout_ms = 30_000;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--skill" if skill.is_none() => skill = args.next(),
            "--allow-write" if !allow_write => allow_write = true,
            "--timeout-ms" => {
                timeout_ms = args
                    .next()
                    .and_then(|v| v.parse::<u64>().ok())
                    .filter(|v| (1..=300_000).contains(v))
                    .ok_or("--timeout-ms must be between 1 and 300000")?;
            }
            _ => return Err("unsupported skill argument".into()),
        }
    }
    let skill = skill
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        .ok_or("skill requires --skill <name>")?;
    arm_deadline(timeout_ms)?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = File::open(&cwd).map_err(|e| e.to_string())?;
    crate::skill_sandbox::restrict(&root, allow_write)?;
    let mut input = Vec::new();
    io::stdin()
        .take((MAX_INPUT + 1) as u64)
        .read_to_end(&mut input)
        .map_err(|e| e.to_string())?;
    if input.len() > MAX_INPUT {
        return Err("skill input exceeds 1 MiB".into());
    }
    let mut rows: Vec<Option<Value>> = Vec::new();
    let mut pending = Vec::new();
    let mut snapshots = Vec::new();
    let mut snapshot_bytes = 0u64;
    let mut metadata = Vec::new();
    let mut ids = HashSet::new();
    let input = std::str::from_utf8(&input).map_err(|_| "skill input is not UTF-8")?;
    if input.trim().is_empty() {
        return Err("skill batch is empty".into());
    }
    if input.lines().filter(|line| !line.trim().is_empty()).count() > MAX_TASKS {
        return Err("skill batch exceeds 64 operations".into());
    }
    for line in input.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let position = rows.len();
        rows.push(None);
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => {
                rows[position] =
                    Some(json!({"skill":skill,"status":"invalid","error":"invalid operation"}));
                continue;
            }
        };
        let id = value.get("id").and_then(Value::as_str).unwrap_or("");
        let operation = value.get("operation").and_then(Value::as_str).unwrap_or("");
        if id.is_empty()
            || id.len() > 128
            || operation.is_empty()
            || operation.len() > 64
            || !operation
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            rows[position] =
                Some(json!({"skill":skill,"status":"invalid","error":"invalid operation"}));
            continue;
        }
        if !ids.insert(id.to_string()) {
            rows[position] = Some(
                json!({"skill":skill,"id":id,"operation":operation,"status":"invalid","error":"duplicate operation ID"}),
            );
            continue;
        }
        if !matches!(operation, "read" | "lines" | "hash" | "grep" | "replace") {
            rows[position] = Some(
                json!({"skill":skill,"id":id,"operation":operation,"status":"unsupported","error":"operation is not supported"}),
            );
            continue;
        }
        let request: Request = match Request::deserialize(&value) {
            Ok(request) => request,
            Err(_) => {
                rows[position] = Some(
                    json!({"skill":skill,"id":id,"operation":operation,"status":"invalid","error":"invalid operation"}),
                );
                continue;
            }
        };
        if !request.command.is_empty()
            || request.pattern.len() > 4096
            || (request.operation != "grep" && !request.pattern.is_empty())
            || (request.operation != "replace"
                && (!request.old.is_empty()
                    || !request.new.is_empty()
                    || !request.expected_hash.is_empty()
                    || request.authorized))
        {
            rows[position] = Some(
                json!({"skill":skill,"id":request.id,"operation":request.operation,"status":"invalid","error":"invalid operation arguments"}),
            );
            continue;
        }
        if request.operation == "replace" {
            if !allow_write || !request.authorized {
                rows[position] = Some(
                    json!({"skill":skill,"id":request.id,"operation":"replace","status":"denied","error":"replace requires --allow-write and operation authorization"}),
                );
            } else {
                let started = std::time::Instant::now();
                let result = replace_workspace_file(&root, &request);
                rows[position] = Some(match result {
                    Ok(()) => {
                        json!({"skill":skill,"id":request.id,"operation":"replace","status":"ok","evidence":request.path,"duration_ms":started.elapsed().as_millis()})
                    }
                    Err(error) => {
                        json!({"skill":skill,"id":request.id,"operation":"replace","status":"failed","error":error,"duration_ms":started.elapsed().as_millis()})
                    }
                });
            }
            continue;
        }
        let started = std::time::Instant::now();
        let mut source = match open_workspace_file(&root, &request.path) {
            Ok(file) => file,
            Err(error) => {
                rows[position] =
                    Some(json!({"skill":skill,"id":request.id,"status":"denied","error":error}));
                continue;
            }
        };
        let mut snapshot = match temporary_file() {
            Ok(file) => file,
            Err(error) => {
                rows[position] = Some(
                    json!({"skill":skill,"id":request.id,"operation":request.operation,"status":"failed","error":error}),
                );
                continue;
            }
        };
        let copied = match io::copy(
            &mut Read::by_ref(&mut source).take(MAX_FILE + 1),
            &mut snapshot,
        ) {
            Ok(copied) => copied,
            Err(_) => {
                rows[position] = Some(
                    json!({"skill":skill,"id":request.id,"operation":request.operation,"status":"failed","error":"cannot snapshot workspace file"}),
                );
                continue;
            }
        };
        if copied > MAX_FILE {
            rows[position] = Some(
                json!({"skill":skill,"id":request.id,"status":"denied","error":"workspace file exceeds 8 MiB"}),
            );
            continue;
        }
        if snapshot_bytes.saturating_add(copied) > 32 * 1024 * 1024 {
            rows[position] = Some(
                json!({"skill":skill,"id":request.id,"status":"denied","error":"skill snapshot budget exceeded"}),
            );
            continue;
        }
        snapshot_bytes += copied;
        if snapshot.seek(SeekFrom::Start(0)).is_err() {
            rows[position] = Some(
                json!({"skill":skill,"id":request.id,"operation":request.operation,"status":"failed","error":"cannot read snapshot"}),
            );
            continue;
        }
        if request.operation == "read" {
            let mut bytes = Vec::new();
            if Read::by_ref(&mut snapshot)
                .take(4000)
                .read_to_end(&mut bytes)
                .is_err()
            {
                rows[position] = Some(
                    json!({"skill":skill,"id":request.id,"operation":"read","status":"failed","error":"cannot read snapshot"}),
                );
                continue;
            }
            let truncated = copied > bytes.len() as u64;
            let text = match std::str::from_utf8(&bytes) {
                Ok(text) => text,
                Err(error) if error.error_len().is_none() && truncated => {
                    std::str::from_utf8(&bytes[..error.valid_up_to()]).expect("valid UTF-8 prefix")
                }
                Err(_) => {
                    rows[position] = Some(
                        json!({"skill":skill,"id":request.id,"operation":"read","status":"failed","error":"workspace file is not UTF-8"}),
                    );
                    continue;
                }
            };
            rows[position] = Some(
                json!({"skill":skill,"id":request.id,"operation":"read","status":"ok","evidence":request.path,"output":text,"truncated":truncated,"duration_ms":started.elapsed().as_millis()}),
            );
            continue;
        }
        let path = format!("/dev/fd/{}", snapshot.as_raw_fd());
        let task = match request.operation.as_str() {
            "lines" => Task::Lines(path),
            "hash" => Task::Hash(path),
            "grep" => Task::Grep {
                path,
                pattern: request.pattern.clone(),
            },
            _ => unreachable!(),
        };
        metadata.push((position, request.id, request.path, request.operation));
        pending.push(Ok(task));
        snapshots.push(snapshot);
    }
    if !pending.is_empty() {
        let execution = (|| -> Result<()> {
            let mut output = temporary_file()?;
            let options = SubOptions {
                cwd,
                out: format!("/dev/fd/{}", output.as_raw_fd()).into(),
                fail_fast: false,
                timeout_ms: engine::DEFAULT_TIMEOUT_MS,
                per_host: engine::DEFAULT_PER_HOST,
                retries: 0,
                sh_mem_mb: engine::DEFAULT_SH_MEM_MB,
                cap: Widths {
                    cpu: 4,
                    net: 1,
                    file: 4,
                },
                show: 0,
            };
            engine::run(pending, &options)?;
            output.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            for line in io::BufReader::new(output).lines() {
                let outcome: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                let index = outcome["i"].as_u64().ok_or("invalid task result")? as usize;
                let (position, id, path, operation) =
                    metadata.get(index).ok_or("invalid task result")?;
                let ok = outcome["ok"].as_bool().unwrap_or(false);
                let truncated = if operation == "grep"
                    && outcome["output"].as_str().is_some_and(|s| s.len() >= 4000)
                {
                    Value::Null
                } else {
                    json!(false)
                };
                rows[*position] = Some(
                    json!({"skill":skill,"id":id,"operation":operation,"status":if ok {"ok"} else {"failed"},"count":outcome["code"],"output":if ok {outcome["output"].clone()} else {Value::Null},"error":if ok {Value::Null} else {json!("operation failed")},"duration_ms":outcome["ms"],"evidence":path,"truncated":truncated}),
                );
            }
            Ok(())
        })();
        for (position, id, path, operation) in &metadata {
            if execution.is_err() || rows[*position].is_none() {
                rows[*position] = Some(
                    json!({"skill":skill,"id":id,"operation":operation,"status":"failed","error":"shared executor failed","evidence":path}),
                );
            }
        }
    }
    drop(snapshots);
    let completed = rows
        .iter()
        .filter(|row| row.as_ref().is_some_and(|row| row["status"] == "ok"))
        .count();
    let failed = rows.len() - completed;
    let mut stdout = io::stdout().lock();
    for row in rows {
        let mut row = row.ok_or("missing task result")?;
        let object = row.as_object_mut().ok_or("invalid task result")?;
        object.insert("completed_count".into(), json!(completed));
        object.insert("failed_count".into(), json!(failed));
        object.entry("id").or_insert(Value::Null);
        object.entry("operation").or_insert(Value::Null);
        object.entry("duration_ms").or_insert(Value::Null);
        object.entry("evidence").or_insert(Value::Null);
        object.entry("truncated").or_insert(Value::Null);
        object.entry("error").or_insert(Value::Null);
        serde_json::to_writer(&mut stdout, &row).map_err(|e| e.to_string())?;
        writeln!(stdout).map_err(|e| e.to_string())?;
    }
    Ok(u8::from(failed != 0))
}
