use reqwest::blocking::Client;
use std::io::Write;
use std::process::Stdio;
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// Copy exit-hook.hoon into the pier and activate it via lens.
/// Safe to call on every boot — skips if already installed.
pub fn install_exit_hook(app: &AppHandle, pier_path: &str, loopback_port: u16) {
    let dest = std::path::Path::new(pier_path).join("base/ted/exit-hook.hoon");

    if !dest.exists() {
        if let Some(parent) = dest.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("[exit-hook] could not create {}: {e}", parent.display());
                return;
            }
        }

        let resource_path = match app.path().resolve(
            "resources/hoon/exit-hook.hoon",
            tauri::path::BaseDirectory::Resource,
        ) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[exit-hook] could not resolve resource: {e}");
                return;
            }
        };

        if let Err(e) = std::fs::copy(&resource_path, &dest) {
            eprintln!("[exit-hook] copy failed: {e}");
            return;
        }
        eprintln!("[exit-hook] installed to {}", dest.display());
    }

    // Only |commit %base — no |rein needed for threads
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let base = format!("http://localhost:{}", loopback_port);
    let body = serde_json::json!({
        "source": { "dojo": "|commit %base" },
        "sink":   { "stdout": null }
    });

    match client
        .post(&base)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
    {
        Ok(_) => eprintln!("[exit-hook] committed base"),
        Err(e) => eprintln!("[exit-hook] lens error: {e}"),
    }
}

/// Gracefully stop a ship via the exit-hook thread over conn.sock.
pub fn stop_ship_graceful(binary_path: &str, pier_path: &str) -> Result<(), String> {
    let conn_sock = format!("{}/.urb/conn.sock", pier_path);

    // Step 1: jam the noun
    let mut jam_proc = std::process::Command::new(binary_path)
        .args(["eval", "-jn"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("jam spawn failed: {e}"))?;

    jam_proc
        .stdin
        .take()
        .unwrap()
        .write_all(b"[0 %fyrd %base %exit-hook [%noun %noun ~]]")
        .map_err(|e| format!("jam write failed: {e}"))?;

    let jammed = jam_proc
        .wait_with_output()
        .map_err(|e| format!("jam failed: {e}"))?
        .stdout;

    // Step 2: send to conn.sock
    let mut nc = std::process::Command::new("nc")
        .args(["-U", "-w", "1", &conn_sock])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("nc spawn failed: {e}"))?;

    nc.stdin
        .take()
        .unwrap()
        .write_all(&jammed)
        .map_err(|e| format!("nc write failed: {e}"))?;

    nc.wait().map_err(|e| format!("nc wait failed: {e}"))?;

    Ok(())
}
