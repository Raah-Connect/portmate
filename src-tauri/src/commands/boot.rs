use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use tauri::{AppHandle, Emitter, State};

use crate::ShipState;

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

    let dest = std::path::PathBuf::from(&dest_dir);
    extract_urbit(&bytes, &dest)
}

fn extract_urbit(bytes: &[u8], dest_dir: &std::path::Path) -> Result<String, String> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let gz = GzDecoder::new(std::io::Cursor::new(bytes));
    let mut arc = Archive::new(gz);

    for entry in arc.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?;
        let path = path.to_path_buf();

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
    if state.process.lock().unwrap().is_some() {
        return Err("A ship is already running".to_string());
    }

    let pier_path = format!("{}/{}", pier_dir, comet_name);

    let mut child = Command::new(&binary_path)
        .args(["-c", &pier_path, "--loom", "34", "-t"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to launch urbit: {e}"))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut stdin = child.stdin.take().unwrap();

    let (tx, rx) = mpsc::channel::<String>();
    *state.stdin_tx.lock().unwrap() = Some(tx);

    // stdout thread
    let app_out = app.clone();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut code_asked = false;
        let mut loopback_port: Option<u16> = None;

        for line in reader.lines() {
            while let Ok(cmd) = rx.try_recv() {
                let _ = writeln!(stdin, "{}", cmd);
            }

            match line {
                Ok(line) => {
                    let _ = app_out.emit("ship-log", serde_json::json!({ "line": line }));

                    // Capture loopback port
                    if line.contains("loopback live on") {
                        loopback_port = parse_port(&line);
                    }

                    // Detect web interface port
                    if line.contains("web interface live on") {
                        let port = parse_port(&line).unwrap_or(8080);
                        let _ = app_out.emit(
                            "ship-ready",
                            serde_json::json!({
                                "port": port,
                                "url": format!("http://localhost:{}", port)
                            }),
                        );
                    }

                    // Request code via lens when ship is live
                    if line.contains("pier (34): live") && !code_asked {
                        code_asked = true;
                        let port = loopback_port.unwrap_or(12321);
                        let app_lens = app_out.clone();
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
                                    let _ = app_lens.emit(
                                        "ship-log",
                                        serde_json::json!({
                                            "line": format!("[lens] {:?}", text)
                                        }),
                                    );
                                    // Emit code directly without validation
                                    if !code.is_empty() {
                                        let _ = app_lens.emit(
                                            "ship-code",
                                            serde_json::json!({
                                                "code": code
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

        let _ = app_out.emit("ship-exited", serde_json::json!({}));
    });

    // stderr thread
    let app_err = app.clone();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().flatten() {
            let _ = app_err.emit(
                "ship-log",
                serde_json::json!({
                    "line": format!("[stderr] {:?}", line)
                }),
            );
        }
    });

    *state.process.lock().unwrap() = Some(child);
    Ok(())
}

/// Send any dojo command from the frontend
#[tauri::command]
pub fn send_dojo(command: String, state: State<'_, ShipState>) -> Result<(), String> {
    let guard = state.stdin_tx.lock().unwrap();
    guard
        .as_ref()
        .ok_or_else(|| "No ship running".to_string())?
        .send(command)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn stop_ship(state: State<'_, ShipState>) -> Result<(), String> {
    if let Some(mut child) = state.process.lock().unwrap().take() {
        *state.stdin_tx.lock().unwrap() = None;
        child.kill().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn is_ship_running(state: State<'_, ShipState>) -> bool {
    state.process.lock().unwrap().is_some()
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

fn is_access_code(line: &str) -> bool {
    let t = line.trim().trim_start_matches('>').trim();
    let parts: Vec<&str> = t.split('-').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| (4..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_lowercase()))
}
