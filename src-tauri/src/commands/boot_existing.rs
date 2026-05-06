use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use tauri::{AppHandle, Emitter, Manager};

use crate::ShipState;
use crate::ShipInfo;
use super::boot::spawn_access_code_fetch;

// ── Boot Existing ─────────────────────────────────────────────────────────────
//
// Boots any existing pier that already has a data directory on disk.
// Works for moons, planets, stars, galaxies, and comets.
// Command: urbit <pier_path> --loom 34 -t
// On Windows, existing piers omit the loom flag.
//
// The urbit binary is auto-detected from the parent directory of the pier.
// If not found, it is automatically downloaded for the current OS/arch.

fn download_url(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("https://urbit.org/install/macos-aarch64/latest"),
        ("macos", "x86_64")  => Some("https://urbit.org/install/macos-x86_64/latest"),
        ("linux", "x86_64")  => Some("https://urbit.org/install/linux-x86_64/latest"),
        ("linux", "aarch64") => Some("https://urbit.org/install/linux-aarch64/latest"),
        // Note: no trailing space
        ("windows", "x86_64") => {
            Some("https://github.com/urbit/vere/releases/latest/download/windows-x86_64.tgz")
        }
        _ => None,
    }
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

async fn find_or_download_binary(pier_path: &str, app: &AppHandle) -> Result<String, String> {
    let parent = std::path::Path::new(pier_path)
        .parent()
        .ok_or("Could not determine parent directory of pier")?
        .to_path_buf();

    // Check if binary already exists
    let binary_name = if cfg!(windows) { "urbit.exe" } else { "urbit" };
    let binary_path = parent.join(binary_name);
    if binary_path.exists() {
        return Ok(binary_path.to_string_lossy().to_string());
    }

    // Binary not found — download it
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let url = download_url(os, arch).ok_or_else(|| {
        format!("Unsupported platform: {os}/{arch} — cannot auto-download urbit binary")
    })?;

    let _ = app.emit(
        "ship-log",
        serde_json::json!({
            "line": format!(
                "[portmate] urbit binary not found in {}, downloading for {os}/{arch}…",
                parent.display()
            ),
            "pier_path": pier_path,
        }),
    );

    let client = reqwest::Client::new();
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to download urbit binary: HTTP {}",
            response.status()
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| e.to_string())?
        .to_vec();

    let result = extract_urbit(&bytes, &parent)?;

    let _ = app.emit(
        "ship-log",
        serde_json::json!({
            "line": "[portmate] urbit binary downloaded and ready.",
            "pier_path": pier_path,
        }),
    );

    Ok(result)
}

// ── Boot Existing Command ─────────────────────────────────────────────────────

#[tauri::command]
pub async fn boot_existing(
    pier_path: String,
    app: AppHandle,
    state: tauri::State<'_, ShipState>,
) -> Result<(), String> {
    // Verify the pier actually exists on disk
    if !std::path::Path::new(&pier_path).exists() {
        return Err(format!("Pier not found at {}", pier_path));
    }

    // Auto-detect or download the binary
    let binary_path = find_or_download_binary(&pier_path, &app).await?;

    // Derive the ship name from the last component of the pier path
    let ship_name = std::path::Path::new(&pier_path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Could not determine ship name from pier path")?
        .to_string();

    // Don't boot if already running
    if state
        .processes
        .lock()
        .unwrap()
        .iter()
        .any(|(p, _)| p == &pier_path)
    {
        return Err(format!("{} is already running", ship_name));
    }

    // On Windows, run from parent directory with just the pier name to match terminal behavior
    let pier_parent = Path::new(&pier_path)
        .parent()
        .ok_or("Could not determine parent directory")?
        .to_string_lossy()
        .to_string();
    let pier_name_only = Path::new(&pier_path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Could not extract pier name")?;

    let (cwd_dir, args): (Option<String>, Vec<&str>) = if cfg!(target_os = "windows") {
        (Some(pier_parent), vec![pier_name_only, "-t"])
    } else {
        (None, vec![&pier_path, "--loom", "34", "-t"])
    };

    let mut cmd = Command::new(&binary_path);
    if let Some(dir) = cwd_dir {
        cmd.current_dir(&dir);
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
    {
        let mut ships = state.ships.lock().unwrap();
        if let Some(existing) = ships.iter_mut().find(|s| s.pier_path == pier_path) {
            existing.status = "booting".to_string();
            existing.pid = Some(pid);
            existing.url = String::new();
            existing.loopback_port = None;
            // access_code preserved — it never changes for a ship
        } else {
            ships.push(ShipInfo {
                name: ship_name.clone(),
                pier_path: pier_path.clone(),
                url: String::new(),
                access_code: String::new(),
                status: "booting".to_string(),
                binary_path: binary_path.clone(),
                pid: Some(pid),
                loopback_port: None,
                pier_size_bytes: None,
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

                        let _ = app_out.emit(
                            "ship-log",
                            serde_json::json!({
                                "line": format!("[portmate] Ship URL {}", url),
                                "pier_path": pier_path_out,
                            }),
                        );

                        let _ = app_out.emit(
                            "ship-ready",
                            serde_json::json!({
                                "pier_path": pier_path_out,
                                "port":      port,
                                "url":       url,
                            }),
                        );
                    }

                    // Match any pier live line e.g. "pier (34): live" or "pier (4661): live"
                    if line.contains("pier (") && line.contains("): live") && !code_asked {
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
            ship.url = String::new();
            ship.loopback_port = None;
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

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_port(line: &str) -> Option<u16> {
    line.split("localhost:")
        .nth(1)?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}