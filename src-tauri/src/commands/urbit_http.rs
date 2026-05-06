use reqwest::blocking::Client;
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// Copy exit-hook.hoon into the pier and activate it via lens.
/// Safe to call on every boot — skips if already installed.
pub fn install_exit_hook(
    app: &AppHandle,
    pier_path: &str,
    loopback_port: u16,
) {
    // Destination
    let dest = std::path::Path::new(pier_path)
        .join("base/app/exit-hook.hoon");

    // Copy from bundled resources if not already there
    if !dest.exists() {
        if let Some(parent) = dest.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("[exit-hook] could not create {}: {e}", parent.display());
                return;
            }
        }

        let resource_path = match app
            .path()
            .resolve("resources/hoon/exit-hook.hoon", tauri::path::BaseDirectory::Resource)
        {
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

    // Send |commit %base and |rein %base [& %exit-hook] via lens
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    let base = format!("http://localhost:{}", loopback_port);

    for cmd in &[
        "|commit %base",
        "|rein %base [& %exit-hook]",
    ] {
        let body = serde_json::json!({
            "source": { "dojo": cmd },
            "sink":   { "stdout": null }
        });

        match client.post(&base)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
        {
            Ok(_)  => eprintln!("[exit-hook] sent: {cmd}"),
            Err(e) => eprintln!("[exit-hook] lens error for `{cmd}`: {e}"),
        }

        std::thread::sleep(Duration::from_secs(2));
    }
}

/// Gracefully stop a ship via the exit-hook agent over HTTP.
/// Falls back to kill if HTTP fails.
pub fn stop_ship_graceful(
    _pier_path: &str,
    port: u16,
    access_code: &str,
    ship_name: &str,  // full comet name e.g. "livdec-rovmep-..."
) -> Result<(), String> {
    let base = format!("http://localhost:{}", port);
    let channel = format!("{}/~/channel/portmate-exit", base);
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    // Login
    let login = client
        .post(format!("{}/~/login", base))
        .form(&[("password", access_code)])
        .send()
        .map_err(|e| format!("login failed: {e}"))?;

    if !login.status().is_success() {
        return Err(format!("login failed with HTTP {}", login.status()));
    }

    // Extract cookie
    let cookie = login
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or("").to_string())
        .ok_or("no cookie in login response")?;

    // Poke exit-hook
    let body = serde_json::json!([{
        "id": 1,
        "action": "poke",
        "ship": ship_name,
        "app": "exit-hook",
        "mark": "json",
        "json": null
    }]);

    let poke = client
        .post(&channel)
        .header("Content-Type", "application/json")
        .header("Cookie", &cookie)
        .body(body.to_string())
        .send()
        .map_err(|e| format!("poke failed: {e}"))?;

    if !poke.status().is_success() {
        return Err(format!("poke failed with HTTP {}", poke.status()));
    }

    Ok(())
}
