use std::process::{Command, Stdio};
use tauri::{AppHandle, Manager};

// ── Graceful shutdown ─────────────────────────────────────────────────────────

pub fn stop_ship_graceful(pier_path: &str, app: &AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        stop_ship_graceful_macos(pier_path, app)
    }
    #[cfg(target_os = "linux")]
    {
        stop_ship_graceful_linux(pier_path, app)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err("Graceful ship shutdown not supported on this platform".to_string())
    }
}

/// macOS: Use click with `-kp` flags
#[cfg(target_os = "macos")]
fn stop_ship_graceful_macos(pier_path: &str, app: &AppHandle) -> Result<(), String> {
    let click_path = find_click_binary(app)?;

    let hoon =
        "=/  m  (strand ,vase)  ;<  ~  bind:m  (poke-our %hood %drum-exit !>(~))  (pure:m !>(~))";

    let output = std::process::Command::new(&click_path)
        .args(["-kp", pier_path, hoon])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("failed to launch click: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("click failed: {}", stderr))
    }
}

/// Linux: Use click with the working command pattern
#[cfg(target_os = "linux")]
fn stop_ship_graceful_linux(pier_path: &str, app: &AppHandle) -> Result<(), String> {
    let click_path = find_click_binary(app)?;

    let hoon =
        "=/  m  (strand ,vase)  ;<  ~  bind:m  (poke-our %hood %drum-exit !>(~))  (pure:m !>(~))";

    let output = std::process::Command::new(&click_path)
        .args(["-kp", pier_path, hoon])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("failed to launch click: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("click failed: {}", stderr))
    }
}

fn find_click_binary(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let mut attempted: Vec<std::path::PathBuf> = Vec::new();

    if let Ok(p) = app
        .path()
        .resolve("resources/click", tauri::path::BaseDirectory::Resource)
    {
        attempted.push(p.clone());
        if p.exists() {
            return Ok(p);
        }
    }

    // Dev fallbacks for local runs (Linux/macOS/Windows):
    // - ./resources/click when cwd is src-tauri
    // - ../resources/click from target/{debug,release}
    // - ../../resources/click from target/{debug,release}/<bin parent>
    let mut candidates = vec![
        std::path::PathBuf::from("resources/click"),
        std::path::PathBuf::from("src-tauri/resources/click"),
    ];

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("resources/click"));
            if let Some(p2) = parent.parent() {
                candidates.push(p2.join("resources/click"));
                if let Some(p3) = p2.parent() {
                    candidates.push(p3.join("resources/click"));
                }
            }
        }
    }

    for candidate in candidates {
        attempted.push(candidate.clone());
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(format!(
        "click binary not found. Tried paths: {:?}",
        attempted
    ))
}

// ── Force-kill operations (cross-platform) ────────────────────────────────────

/// Force-kill a ship process by PID (cross-platform).
/// Uses `kill -9 [PID]` on Unix and `taskkill /F /T /PID [PID]` on Windows.
pub fn kill_ship_by_pid(pid: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output()
            .map_err(|e| format!("Failed to kill process {}: {}", pid, e))?;
    }
    #[cfg(windows)]
    {
        Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output()
            .map_err(|e| format!("Failed to kill process {}: {}", pid, e))?;
    }
    std::thread::sleep(std::time::Duration::from_millis(800));
    Ok(())
}

/// Force-kill all urbit/vere processes matching a pier path pattern.
/// Uses `pkill -9 -f [pier_path]` on Unix and PowerShell on Windows.
pub fn kill_ships_by_pier_path(pier_path: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        Command::new("pkill")
            .args(["-9", "-f", pier_path])
            .output()
            .map_err(|e| format!("Failed to kill ships at {}: {}", pier_path, e))?;
    }
    #[cfg(windows)]
    {
        let pier_name = std::path::Path::new(pier_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let script = format!(
            "Get-CimInstance Win32_Process \
             | Where-Object {{ \
                 $_.ProcessId -ne $PID -and \
                 $_.CommandLine -and \
                 $_.Name -match '^(urbit|vere)(\\.exe)?$' -and \
                 ($_.CommandLine -like '*{}*' -or $_.CommandLine -like '*{}*') \
             }} \
             | ForEach-Object {{ taskkill /F /T /PID $_.ProcessId }}",
            pier_path.replace('\'', "''"),
            pier_name.replace('\'', "''")
        );
        Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .output()
            .map_err(|e| format!("Failed to kill ships at {} via PowerShell: {}", pier_path, e))?;
    }
    std::thread::sleep(std::time::Duration::from_millis(800));
    Ok(())
}

/// Force-kill a child process (cross-platform).
pub fn force_kill_child(child: &mut std::process::Child) -> Result<(), String> {
    child.kill().map_err(|e| format!("Failed to kill child process: {}", e))
}
