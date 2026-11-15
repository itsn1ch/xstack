use std::process::Command;

#[derive(Clone, Copy, Default)]
pub struct Spawn {
    pub own_group: bool,
    pub low_priority: bool,
    pub detached: bool,
}

#[cfg(unix)]
pub fn configure(command: &mut Command, spawn: Spawn) {
    use std::os::unix::process::CommandExt;
    if spawn.own_group && !spawn.detached {
        command.process_group(0);
    }
    let (low_priority, detached) = (spawn.low_priority, spawn.detached);
    if low_priority || detached {
        unsafe {
            command.pre_exec(move || {
                if detached {
                    libc::setsid();
                }
                if low_priority {
                    libc::setpriority(libc::PRIO_PROCESS, 0, 10);
                }
                Ok(())
            });
        }
    }
}

#[cfg(windows)]
pub fn configure(command: &mut Command, spawn: Spawn) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{
        BELOW_NORMAL_PRIORITY_CLASS, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS,
    };
    let mut flags = 0;
    if spawn.own_group || spawn.detached {
        flags |= CREATE_NEW_PROCESS_GROUP;
    }
    if spawn.detached {
        flags |= DETACHED_PROCESS;
    }
    if spawn.low_priority {
        flags |= BELOW_NORMAL_PRIORITY_CLASS;
    }
    command.creation_flags(flags);
}

#[cfg(unix)]
pub fn kill_tree(pid: i32) {
    if pid > 0 {
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
}

#[cfg(windows)]
pub fn kill_tree(pid: i32) {
    if pid > 0 {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

#[cfg(unix)]
pub fn alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
pub fn alive(pid: i32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    const STILL_ACTIVE: u32 = 259;
    if pid <= 0 {
        return false;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32) };
    if handle.is_null() {
        return unsafe { GetLastError() } == ERROR_ACCESS_DENIED;
    }
    let mut code = 0u32;
    let ok = unsafe { GetExitCodeProcess(handle, &mut code) } != 0;
    unsafe { CloseHandle(handle) };
    ok && code == STILL_ACTIVE
}

#[cfg(unix)]
pub fn user_tag() -> String {
    unsafe { libc::getuid() }.to_string()
}

#[cfg(windows)]
pub fn user_tag() -> String {
    "user".into()
}
