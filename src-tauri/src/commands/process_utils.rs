// Cross-platform process liveness check for a given PID
#[cfg(unix)]
pub fn is_pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(windows)]
pub fn is_pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::System::Threading::{OpenProcess, GetExitCodeProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut exit_code = 0u32;
        let alive =
            GetExitCodeProcess(handle, &mut exit_code as *mut u32) != 0 && exit_code == STILL_ACTIVE as u32;
        CloseHandle(handle);
        alive
    }
}
