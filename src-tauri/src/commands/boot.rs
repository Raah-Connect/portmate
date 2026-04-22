use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{ShipInfo, ShipState};
use crate::commands::ship_stats::refresh_ship_size;
use super::memory_sched::ensure_default_memory_schedules_for_ship;

pub(crate) fn spawn_access_code_fetch(app: AppHandle, pier_path: String, loopback_port: u16) {
    thread::spawn(move || {
        let client = reqwest::blocking::Client::new();
        let mut code = String::new();

        for attempt in 1..=40 {
            std::thread::sleep(std::time::Duration::from_secs(3));

            let response = client
                .post(format!("http://localhost:{}", loopback_port))
                .header("Content-Type", "application/json")
                .body(r#"{"source":{"dojo":"+code"},"sink":{"stdout":null}}"#)
                .timeout(std::time::Duration::from_secs(5))
                .send();

            match response {
                Ok(resp) => {
                    let text = resp.text().unwrap_or_default();
                    let candidate = text
                        .trim()
                        .trim_matches('"')
                        .replace("\\n", "")
                        .trim()
                        .to_string();

                    if !candidate.is_empty() {
                        code = candidate;
                        break;
                    }

                    let _ = app.emit(
                        "ship-log",
                        serde_json::json!({
                            "line": format!("[lens] attempt {} — empty response, retrying…", attempt),
                            "pier_path": &pier_path,
                        }),
                    );
                }
                Err(error) => {
                    let _ = app.emit(
                        "ship-log",
                        serde_json::json!({
                            "line": format!("[lens] attempt {} — {}, retrying…", attempt, error),
                            "pier_path": &pier_path,
                        }),
                    );
                }
            }
        }

        if !code.is_empty() {
            let state = app.state::<ShipState>();
            let mut ships = state.ships.lock().unwrap();
            if let Some(ship) = ships.iter_mut().find(|s| s.pier_path == pier_path) {
                ship.access_code = code.clone();
            }
            drop(ships);
            let _ = state.save();
            let _ = app.emit(
                "ship-code",
                serde_json::json!({
                    "pier_path": &pier_path,
                    "code": code,
                }),
            );
        } else {
            let _ = app.emit(
                "ship-log",
                serde_json::json!({
                    "line": "[lens] gave up after 40 attempts — could not retrieve access code",
                    "pier_path": &pier_path,
                }),
            );
        }
    });
}

#[tauri::command]
pub fn request_access_code(
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    let ship = state
        .ships
        .lock()
        .unwrap()
        .iter()
        .find(|ship| ship.pier_path == pier_path)
        .cloned()
        .ok_or_else(|| "No ship found at that path".to_string())?;

    if ship.status != "running" {
        return Err("Ship must be running to request an access code".to_string());
    }

    if !ship.access_code.trim().is_empty() {
        let _ = app.emit(
            "ship-code",
            serde_json::json!({
                "pier_path": pier_path,
                "code": ship.access_code,
            }),
        );
        return Ok(());
    }

    let loopback_port = ship
        .loopback_port
        .ok_or_else(|| "Loopback dojo port is not available for this ship yet".to_string())?;

    let _ = app.emit(
        "ship-log",
        serde_json::json!({
            "line": "[portmate] Requesting access code via +code…",
            "pier_path": &pier_path,
        }),
    );

    spawn_access_code_fetch(app, pier_path, loopback_port);
    Ok(())
}

// ── Platform ──────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
pub struct PlatformInfo {
    pub os: String,
    pub arch: String,
    pub supported: bool,
}

#[tauri::command]
pub fn get_platform_info() -> PlatformInfo {
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let supported = download_url(&os, &arch).is_some();
    PlatformInfo { os, arch, supported }
}

fn download_url(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("https://urbit.org/install/macos-aarch64/latest"),
        ("macos", "x86_64") => Some("https://urbit.org/install/macos-x86_64/latest"),
        ("linux", "x86_64") => Some("https://urbit.org/install/linux-x86_64/latest"),
        ("linux", "aarch64") => Some("https://urbit.org/install/linux-aarch64/latest"),
        // Note: no trailing space
        ("windows", "x86_64") => {
            Some("https://github.com/urbit/vere/releases/latest/download/windows-x86_64.tgz")
        }
        _ => None,
    }
}

// ── Download ──────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
struct DownloadProgress {
    percent: f32,
    downloaded: u64,
    total: u64,
}

#[tauri::command]
pub async fn download_urbit(dest_dir: String, app: AppHandle) -> Result<String, String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let url = download_url(os, arch).ok_or_else(|| format!("Unsupported platform: {os}/{arch}"))?;

    let client = reqwest::Client::new();
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let total = response.content_length().unwrap_or(0);
    let mut downloaded = 0u64;
    let mut bytes: Vec<u8> = Vec::with_capacity(total as usize);

    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;
        bytes.extend_from_slice(&chunk);
        let percent = if total > 0 {
            (downloaded as f32 / total as f32) * 100.0
        } else {
            0.0
        };
        let _ = app.emit(
            "download-progress",
            DownloadProgress { percent, downloaded, total },
        );
    }

    extract_urbit(&bytes, &std::path::PathBuf::from(&dest_dir))
}

fn extract_urbit(bytes: &[u8], dest_dir: &std::path::Path) -> Result<String, String> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    // On Windows the binary is named urbit.exe / vere.exe
    let binary_out_name = if cfg!(windows) { "urbit.exe" } else { "urbit" };

    let gz = GzDecoder::new(std::io::Cursor::new(bytes));
    let mut arc = Archive::new(gz);

    for entry in arc.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.to_path_buf();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        let is_urbit = name == "urbit"
            || name == "urbit.exe"
            || name.starts_with("urbit-")
            || name.starts_with("vere-");

        if is_urbit {
            let out = dest_dir.join(binary_out_name);
            entry.unpack(&out).map_err(|e| e.to_string())?;
            make_executable(&out)?;
            return Ok(out.to_string_lossy().to_string());
        }
    }

    Err("urbit binary not found in archive".to_string())
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .map_err(|e| e.to_string())?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn make_executable(_: &std::path::Path) -> Result<(), String> {
    Ok(())
}

// ── Boot ──────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn boot_comet(
    binary_path: String,
    pier_dir: String,
    comet_name: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    // Use Path::join so the separator is correct on every OS
    let pier_path = Path::new(&pier_dir)
        .join(&comet_name)
        .to_string_lossy()
        .to_string();

    // Don't boot if already running
    if state
        .processes
        .lock()
        .unwrap()
        .iter()
        .any(|(p, _)| p == &pier_path)
    {
        return Err(format!("{} is already running", comet_name));
    }

    // On Windows, run from parent directory with just the pier name to match terminal behavior
    let (cwd, args): (Option<&str>, Vec<&str>) = if cfg!(target_os = "windows") {
        if Path::new(&pier_path).exists() {
            // Existing: run from parent with just name
            (Some(&pier_dir), vec![&comet_name, "-t"])
        } else {
            // New: run from parent with just name
            (Some(&pier_dir), vec!["-c", &comet_name, "-t"])
        }
    } else {
        // Unix: use full path as before
        if Path::new(&pier_path).exists() {
            (None, vec![&pier_path, "--loom", "34", "-t"])
        } else {
            (None, vec!["-c", &pier_path, "--loom", "34", "-t"])
        }
    };

    let mut cmd = Command::new(&binary_path);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let mut child = cmd.args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to launch urbit: {e}"))?;

    let pid = child.id();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let stdin = child.stdin.take().unwrap();

    // Add or update ship in the list
    let created_ship = {
        let mut ships = state.ships.lock().unwrap();
        if let Some(existing) = ships.iter_mut().find(|s| s.pier_path == pier_path) {
            existing.status = "booting".to_string();
            existing.pid = Some(pid);
            existing.url = String::new();
            existing.loopback_port = None;
            false
        } else {
            ships.push(ShipInfo {
                name: comet_name.clone(),
                pier_path: pier_path.clone(),
                url: String::new(),
                access_code: String::new(),
                status: "booting".to_string(),
                binary_path: binary_path.clone(),
                pid: Some(pid),
                loopback_port: None,
                pier_size_bytes: None,
            });
            true
        }
    };
    let _ = state.save();
    let _ = refresh_ship_size(&pier_path, &app, &state);

    if created_ship {
        if let Err(error) = ensure_default_memory_schedules_for_ship(&app, &state, &pier_path) {
            eprintln!(
                "[portmate] Failed to seed default maintenance schedules for {}: {}",
                pier_path, error
            );
        }
    }

    // Stdin writer thread
    let (tx, rx) = mpsc::channel::<String>();
    state
        .stdin_txs
        .lock()
        .unwrap()
        .push((pier_path.clone(), tx));

    thread::spawn(move || {
        let mut stdin = stdin;
        for cmd in rx {
            let _ = writeln!(stdin, "{}", cmd);
        }
    });

    // stdout thread
    let app_out = app.clone();
    let pier_path_out = pier_path.clone();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut code_asked = false;
        let mut loopback_port: Option<u16> = None;

        for line in reader.lines() {
            match line {
                Ok(line) => {
                    let _ = app_out.emit(
                        "ship-log",
                        serde_json::json!({
                            "line":      line,
                            "pier_path": pier_path_out,
                        }),
                    );

                    if line.contains("loopback live on") {
                        loopback_port = parse_port(&line);
                        if let Some(port) = loopback_port {
                            let state = app_out.state::<ShipState>();
                            let mut ships = state.ships.lock().unwrap();
                            if let Some(ship) = ships.iter_mut().find(|s| s.pier_path == pier_path_out)
                            {
                                ship.loopback_port = Some(port);
                            }
                            drop(ships);
                            let _ = state.save();
                        }
                    }

                    if line.contains("web interface live on") {
                        let port = parse_port(&line).unwrap_or(8080);
                        let url = format!("http://localhost:{}", port);

                        let state = app_out.state::<ShipState>();
                        let mut ships = state.ships.lock().unwrap();
                        if let Some(ship) = ships.iter_mut().find(|s| s.pier_path == pier_path_out)
                        {
                            ship.url = url.clone();
                            ship.status = "running".to_string();
                        }
                        drop(ships);
                        let _ = state.save();
                        let _ = refresh_ship_size(&pier_path_out, &app_out, &state);

                        let _ = app_out.emit(
                            "ship-ready",
                            serde_json::json!({
                                "pier_path": pier_path_out,
                                "port":      port,
                                "url":       url,
                            }),
                        );
                    }

                    if line.contains("pier (34): live") && !code_asked {
                        code_asked = true;
                        let port = loopback_port.unwrap_or_else(|| {
                            let _ = app_out.emit(
                                "ship-log",
                                serde_json::json!({
                                    "line": "[portmate] Warning: loopback port unknown, falling back to 12321",
                                    "pier_path": pier_path_out,
                                }),
                            );
                            12321
                        });

                        spawn_access_code_fetch(app_out.clone(), pier_path_out.clone(), port);
                    }
                }
                Err(_) => break,
            }
        }

        // Mark stopped when stdout closes
        let state = app_out.state::<ShipState>();
        let mut ships = state.ships.lock().unwrap();
        if let Some(ship) = ships.iter_mut().find(|s| s.pier_path == pier_path_out) {
            ship.status = "stopped".to_string();
            ship.pid = None;
        }
        drop(ships);
        let _ = state.save();
        let _ = refresh_ship_size(&pier_path_out, &app_out, &state);
        let _ = app_out.emit(
            "ship-exited",
            serde_json::json!({ "pier_path": pier_path_out }),
        );
    });

    // stderr thread
    let app_err = app.clone();
    let pier_path_err = pier_path.clone();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().flatten() {
            let _ = app_err.emit(
                "ship-log",
                serde_json::json!({
                    "line": format!("[stderr] {}", line),
                    "pier_path": pier_path_err,
                }),
            );
        }
    });

    state
        .processes
        .lock()
        .unwrap()
        .push((pier_path.clone(), child));
    Ok(())
}

// ── Ship queries ──────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_running_ships(state: State<'_, ShipState>) -> Vec<ShipInfo> {
    state.ships.lock().unwrap().clone()
}

#[tauri::command]
pub fn send_dojo(
    pier_path: String,
    command: String,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    let guard = state.stdin_txs.lock().unwrap();
    guard
        .iter()
        .find(|(p, _)| p == &pier_path)
        .ok_or_else(|| "No ship running at that path".to_string())?
        .1
        .send(command)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn stop_ship(
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    let mut processes = state.processes.lock().unwrap();
    if let Some(pos) = processes.iter().position(|(p, _)| p == &pier_path) {
        let (_, mut child) = processes.remove(pos);
        child.kill().map_err(|e| e.to_string())?;
    }
    drop(processes);

    state.stdin_txs.lock().unwrap().retain(|(p, _)| p != &pier_path);

    let mut ships = state.ships.lock().unwrap();
    if let Some(ship) = ships.iter_mut().find(|s| s.pier_path == pier_path) {
        ship.status = "stopped".to_string();
        ship.pid = None;
    }
    drop(ships);
    let _ = state.save();
    let _ = refresh_ship_size(&pier_path, &app, &state);

    Ok(())
}

#[tauri::command]
pub fn restart_ship(
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    restart_ship_internal(pier_path, app, state)
}

pub(crate) fn restart_ship_internal(
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    let ship_info = state
        .ships
        .lock()
        .unwrap()
        .iter()
        .find(|s| s.pier_path == pier_path)
        .cloned()
        .ok_or("No ship found at that path")?;

    // Use Path::parent() — works correctly on both Unix and Windows
    let pier_dir = Path::new(&pier_path)
        .parent()
        .ok_or("Could not determine pier directory")?
        .to_string_lossy()
        .to_string();

    // Force-kill the process by PID, platform-appropriately
    kill_ship_process(&ship_info, &pier_path);

    // Also remove from our tracked process list if present
    {
        let mut processes = state.processes.lock().unwrap();
        if let Some(pos) = processes.iter().position(|(p, _)| p == &pier_path) {
            let (_, mut child) = processes.remove(pos);
            let _ = child.kill();
        }
    }

    state
        .stdin_txs
        .lock()
        .unwrap()
        .retain(|(p, _)| p != &pier_path);

    std::thread::sleep(std::time::Duration::from_secs(2));
    boot_comet(ship_info.binary_path, pier_dir, ship_info.name, app, state)
}

/// Kill the OS-level process for a ship, cross-platform.
fn kill_ship_process(ship_info: &ShipInfo, pier_path: &str) {
    // Prefer killing by PID when we have one — works the same everywhere.
    if let Some(pid) = ship_info.pid {
        #[cfg(unix)]
        {
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
            std::thread::sleep(std::time::Duration::from_millis(800));
        }
        #[cfg(windows)]
        {
            // /F force-kills, /T also terminates child processes of that PID
            let _ = Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .output();
            std::thread::sleep(std::time::Duration::from_millis(800));
        }
    } else {
        // Fallback: match by pier path in the process name
        #[cfg(unix)]
        {
            let _ = Command::new("pkill").args(["-9", "-f", pier_path]).output();
            std::thread::sleep(std::time::Duration::from_millis(800));
        }
        #[cfg(windows)]
        {
            // WMIC lets us kill by command-line substring when we have no PID
            let _ = Command::new("wmic")
                .args([
                    "process",
                    "where",
                    &format!("CommandLine like '%{}%'", pier_path),
                    "call",
                    "terminate",
                ])
                .output();
            std::thread::sleep(std::time::Duration::from_millis(800));
        }
    }
}

#[tauri::command]
pub fn delete_ship(pier_path: String, state: State<'_, ShipState>) -> Result<(), String> {
    {
        let mut processes = state.processes.lock().unwrap();
        if let Some(pos) = processes.iter().position(|(p, _)| p == &pier_path) {
            let (_, mut child) = processes.remove(pos);
            let _ = child.kill();
        }
    }
    state
        .stdin_txs
        .lock()
        .unwrap()
        .retain(|(p, _)| p != &pier_path);

    state
        .ships
        .lock()
        .unwrap()
        .retain(|s| s.pier_path != pier_path);
    let _ = state.save();

    if Path::new(&pier_path).exists() {
        std::fs::remove_dir_all(&pier_path)
            .map_err(|e| format!("Failed to delete pier: {}", e))?;
    }

    Ok(())
}

#[tauri::command]
pub fn is_ship_running(pier_path: String, state: State<'_, ShipState>) -> bool {
    state
        .processes
        .lock()
        .unwrap()
        .iter()
        .any(|(p, _)| p == &pier_path)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_port(line: &str) -> Option<u16> {
    line.split("localhost:")
        .nth(1)?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}