use std::process::Stdio;
use tauri::{AppHandle, Manager};

pub fn stop_ship_graceful(pier_path: &str, app: &AppHandle) -> Result<(), String> {
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
    if let Ok(p) = app
        .path()
        .resolve("resources/click", tauri::path::BaseDirectory::Resource)
    {
        if p.exists() {
            return Ok(p);
        }
    }

    Err(format!(
        "click binary not found. Expected at src-tauri/resources/click (dev) \
         or app bundle resources/click (production). \
         Resolved path: {:?}",
        app.path()
            .resolve("resources/click", tauri::path::BaseDirectory::Resource)
            .ok()
    ))
}
