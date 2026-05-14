use std::process::Stdio;

/// Gracefully stop a ship via click.
pub fn stop_ship_graceful(pier_path: &str) -> Result<(), String> {
    // Use click binary in resources or current directory
    let click_path = if std::path::Path::new("./click").exists() {
        "./click"
    } else if std::path::Path::new("resources/click").exists() {
        "resources/click"
    } else {
        return Err("click binary not found in ./ or ./resources".to_string());
    };

    // Hoon code for graceful shutdown
    let hoon = "=/  m  (strand ,vase)  ;<  ~  bind:m  (poke-our %hood %drum-exit !>(~))  (pure:m !>(~))";

    let output = std::process::Command::new(click_path)
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
