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
