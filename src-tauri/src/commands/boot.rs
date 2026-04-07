use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{ShipInfo, ShipState};

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
    PlatformInfo {
        os,
        arch,
        supported,
    }
}

fn download_url(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("https://urbit.org/install/macos-aarch64/latest"),
        ("macos", "x86_64") => Some("https://urbit.org/install/macos-x86_64/latest"),
        ("linux", "x86_64") => Some("https://urbit.org/install/linux-x86_64/latest"),
        ("linux", "aarch64") => Some("https://urbit.org/install/linux-aarch64/latest"),
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
            DownloadProgress {
                percent,
                downloaded,
                total,
            },
        );
    }

    extract_urbit(&bytes, &std::path::PathBuf::from(&dest_dir))
}

fn extract_urbit(bytes: &[u8], dest_dir: &std::path::Path) -> Result<String, String> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let gz = GzDecoder::new(std::io::Cursor::new(bytes));
    let mut arc = Archive::new(gz);

    for entry in arc.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.to_path_buf();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if name == "urbit" || name.starts_with("urbit-") || name.starts_with("vere-") {
            let out = dest_dir.join("urbit");
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
    let pier_path = format!("{}/{}", pier_dir, comet_name);

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

    // Use -c only for new piers, existing piers just get the path
    let args: Vec<&str> = if std::path::Path::new(&pier_path).exists() {
        vec![&pier_path, "--loom", "34", "-t"]
    } else {
        vec!["-c", &pier_path, "--loom", "34", "-t"]
    };

    let mut child = Command::new(&binary_path)
        .args(&args)
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
    {
        let mut ships = state.ships.lock().unwrap();
        if let Some(existing) = ships.iter_mut().find(|s| s.pier_path == pier_path) {
            existing.status = "booting".to_string();
            existing.pid = Some(pid);
            existing.url = String::new();
            //existing.access_code = String::new();  access_code preserved — it never changes for a ship
        } else {
            ships.push(ShipInfo {
                name: comet_name.clone(),
                pier_path: pier_path.clone(),
                url: String::new(),
                access_code: String::new(),
                status: "booting".to_string(),
                binary_path: binary_path.clone(),
                pid: Some(pid),
            });
        }
    }
    let _ = state.save();

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
                            let _ = app_out.emit("ship-log", serde_json::json!({
                                "line": "[portmate] Warning: loopback port unknown, falling back to 12321"
                            }));
                            12321
                        });

                        let app_lens = app_out.clone();
                        let pier_path_lens = pier_path_out.clone();
                        thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_secs(5));
                            let client = reqwest::blocking::Client::new();
                            let res = client
                                .post(format!("http://localhost:{}", port))
                                .header("Content-Type", "application/json")
                                .body(r#"{"source":{"dojo":"+code"},"sink":{"stdout":null}}"#)
                                .send();

                            match res {
                                Ok(resp) => {
                                    let text = resp.text().unwrap_or_default();
                                    let code = text
                                        .trim()
                                        .trim_matches('"')
                                        .replace("\\n", "")
                                        .trim()
                                        .to_string();

                                    if !code.is_empty() {
                                        let state = app_lens.state::<ShipState>();
                                        let mut ships = state.ships.lock().unwrap();
                                        if let Some(ship) =
                                            ships.iter_mut().find(|s| s.pier_path == pier_path_lens)
                                        {
                                            ship.access_code = code.clone();
                                        }
                                        drop(ships);
                                        let _ = state.save();
                                        let _ = app_lens.emit(
                                            "ship-code",
                                            serde_json::json!({
                                                "pier_path": pier_path_lens,
                                                "code":      code,
                                            }),
                                        );
                                    } else {
                                        let _ = app_lens.emit(
                                            "ship-log",
                                            serde_json::json!({
                                                "line": "[lens] received empty code response"
                                            }),
                                        );
                                    }
                                }
                                Err(e) => {
                                    let _ = app_lens.emit(
                                        "ship-log",
                                        serde_json::json!({
                                            "line": format!("[lens error] {}", e)
                                        }),
                                    );
                                }
                            }
                        });
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
        let _ = app_out.emit(
            "ship-exited",
            serde_json::json!({ "pier_path": pier_path_out }),
        );
    });

    // stderr thread
    let app_err = app.clone();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().flatten() {
            let _ = app_err.emit(
                "ship-log",
                serde_json::json!({
                    "line": format!("[stderr] {}", line)
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
pub fn stop_ship(pier_path: String, state: State<'_, ShipState>) -> Result<(), String> {
    // Kill process
    let mut processes = state.processes.lock().unwrap();
    if let Some(pos) = processes.iter().position(|(p, _)| p == &pier_path) {
        let (_, mut child) = processes.remove(pos);
        child.kill().map_err(|e| e.to_string())?;
    }
    drop(processes);

    // Remove stdin channel
    let mut txs = state.stdin_txs.lock().unwrap();
    txs.retain(|(p, _)| p != &pier_path);
    drop(txs);

    // Update status
    let mut ships = state.ships.lock().unwrap();
    if let Some(ship) = ships.iter_mut().find(|s| s.pier_path == pier_path) {
        ship.status = "stopped".to_string();
        ship.pid = None;
    }
    drop(ships);
    let _ = state.save();

    Ok(())
}

#[tauri::command]
pub fn restart_ship(
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

    let pier_dir = ship_info
        .pier_path
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .ok_or("Could not determine pier directory")?;

    // Stop the process and its worker children
    // Kill all urbit processes for this pier by path
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("pkill")
            .args(["-9", "-f", &pier_path])
            .output();
        std::thread::sleep(std::time::Duration::from_millis(800));
    }

    // Also remove from our tracked processes if present
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

#[tauri::command]
pub fn delete_ship(pier_path: String, state: State<'_, ShipState>) -> Result<(), String> {
    // Stop first if running
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

    // Remove from ship list
    state
        .ships
        .lock()
        .unwrap()
        .retain(|s| s.pier_path != pier_path);
    let _ = state.save();

    // Delete pier directory
    if std::path::Path::new(&pier_path).exists() {
        std::fs::remove_dir_all(&pier_path).map_err(|e| format!("Failed to delete pier: {}", e))?;
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
