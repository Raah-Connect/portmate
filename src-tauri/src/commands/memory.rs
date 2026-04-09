use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::thread;
use tauri::{AppHandle, Emitter, State};

use crate::ShipState;

// ── All four ops are offline: stop → run binary → emit memory-op-done ─────────

#[tauri::command]
pub fn pack_ship(
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    run_offline_op("pack", pier_path, app, state)
}

#[tauri::command]
pub fn meld_ship(
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    run_offline_op("meld", pier_path, app, state)
}

#[tauri::command]
pub fn roll_ship(
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    run_offline_op("roll", pier_path, app, state)
}

#[tauri::command]
pub fn chop_ship(
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    run_offline_op("chop", pier_path, app, state)
}

// ── Shared implementation ─────────────────────────────────────────────────────

fn run_offline_op(
    op: &'static str,
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    let binary = binary_for(&pier_path, &state)?;

    let pier_path_obj = std::path::Path::new(&pier_path);
    let work_dir = pier_path_obj
        .parent()
        .ok_or("Could not determine pier parent directory")?
        .to_path_buf();
    let pier_name = pier_path_obj
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Could not determine pier name")?
        .to_string();

    stop_for_maintenance(&pier_path, &app, &state);

    let app2 = app.clone();
    let path2 = pier_path.clone();
    thread::spawn(move || {
        emit_log(
            &app2,
            &path2,
            &format!("[portmate] Starting {op} on '{pier_name}' — this may take a few minutes…"),
        );
        match run_op(op, &binary, &pier_name, &work_dir, &path2, &app2) {
            Ok(()) => {
                emit_log(&app2, &path2, &format!("[portmate] {op} complete ✓"));
                let _ = app2.emit(
                    "memory-op-done",
                    serde_json::json!({
                        "pier_path": path2,
                        "op":        op,
                        "success":   true,
                    }),
                );
            }
            Err(e) => {
                emit_log(&app2, &path2, &format!("[portmate] {op} failed: {e}"));
                let _ = app2.emit(
                    "memory-op-done",
                    serde_json::json!({
                        "pier_path": path2,
                        "op":        op,
                        "success":   false,
                        "error":     e,
                    }),
                );
            }
        }
    });

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn binary_for(pier_path: &str, state: &State<'_, ShipState>) -> Result<String, String> {
    state
        .ships
        .lock()
        .unwrap()
        .iter()
        .find(|s| s.pier_path == pier_path)
        .map(|s| s.binary_path.clone())
        .ok_or_else(|| format!("No ship found at {pier_path}"))
}

/// Returns true if any live process has `pier_path` in its command line.
/// This catches BOTH the urbit launcher and the worker process.
#[cfg(unix)]
fn any_urbit_process_alive(pier_path: &str) -> bool {
    // `pgrep -f <pattern>` exits 0 if at least one match, 1 if none.
    std::process::Command::new("pgrep")
        .args(["-f", pier_path])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn any_urbit_process_alive(pier_path: &str) -> bool {
    // On Windows use WMIC to search for the pier path in process command lines.
    std::process::Command::new("wmic")
        .args([
            "process",
            "where",
            &format!("CommandLine like '%{}%'", pier_path),
            "get",
            "ProcessId",
        ])
        .output()
        .map(|o| {
            let out = String::from_utf8_lossy(&o.stdout);
            // Output has a header line + one line per match; if only header, no processes.
            out.lines()
                .filter(|l| !l.trim().is_empty() && !l.contains("ProcessId"))
                .count()
                > 0
        })
        .unwrap_or(false)
}

/// Gracefully shut down the ship before a maintenance op.
///
/// Urbit spawns TWO processes per ship:
///   - launcher:  urbit /path/to/pier --loom 34 -t
///   - worker:    urbit work --snap-dir /path/to/pier ...
///
/// The lock file only tracks the worker PID. We must wait for BOTH to exit
/// before running roll/chop/pack/meld, otherwise urbit sees a live process
/// and aborts with "shenanigans!".
///
/// Shutdown order:
///   1. Confirm the ship is actually running.
///   2. Send |exit via dojo stdin.
///   3. Wait up to 30 s for the lock file to clear (worker exited).
///   4. Wait a further 10 s for the launcher to also fully exit.
///   5. Only if processes are still alive after all that: SIGTERM then SIGKILL.
fn stop_for_maintenance(pier_path: &str, app: &AppHandle, state: &State<'_, ShipState>) {
    let lock = std::path::Path::new(pier_path).join(".urb").join("lock");

    // ── Step 1: confirm the ship is actually running ──────────────────────────
    let ship_was_running = {
        let in_table = state
            .processes
            .lock()
            .unwrap()
            .iter()
            .any(|(p, _)| p == pier_path);
        let lock_alive = lock.exists() && {
            std::fs::read_to_string(&lock)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .map(is_pid_alive)
                .unwrap_or(false)
        };
        in_table || lock_alive
    };

    if !ship_was_running {
        emit_log(
            app,
            pier_path,
            "[portmate] Warning: ship does not appear to be running. \
             If it was previously hard-killed (SIGKILL), the pier state may be \
             dirty and roll/chop may fail with 'shenanigans!'. \
             To recover: boot the ship, wait for it to fully start \
             (pier live message), then stop it cleanly and retry.",
        );
    } else {
        // ── Step 2: send |exit ────────────────────────────────────────────────
        emit_log(
            app,
            pier_path,
            "[portmate] Requesting clean shutdown via |exit…",
        );
        {
            let txs = state.stdin_txs.lock().unwrap();
            if let Some((_, tx)) = txs.iter().find(|(p, _)| p == pier_path) {
                let _ = tx.send("|exit".to_string());
            }
        }

        // ── Step 3: wait for worker lock to clear (up to 30 s) ───────────────
        let mut worker_gone = false;
        for i in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if !lock.exists() {
                emit_log(
                    app,
                    pier_path,
                    &format!(
                        "[portmate] Worker exited after ~{}ms, waiting for launcher…",
                        (i + 1) * 500
                    ),
                );
                worker_gone = true;
                break;
            }
        }

        if !worker_gone {
            emit_log(
                app,
                pier_path,
                "[portmate] Worker did not exit after 30s — escalating…",
            );
        }

        // ── Step 4: wait for launcher + any other urbit process to also exit ──
        // The launcher process outlives the worker briefly. Give it up to 10 s.
        let mut all_gone = false;
        for i in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if !any_urbit_process_alive(pier_path) {
                emit_log(
                    app,
                    pier_path,
                    &format!(
                        "[portmate] All urbit processes exited after ~{}ms ✓",
                        (i + 1) * 500
                    ),
                );
                all_gone = true;
                break;
            }
        }

        // ── Step 5: escalate if any process is still alive ───────────────────
        if !all_gone {
            emit_log(
                app,
                pier_path,
                "[portmate] Processes still alive after 10s — sending SIGTERM…",
            );

            #[cfg(unix)]
            {
                let _ = std::process::Command::new("pkill")
                    .args(["-TERM", "-f", pier_path])
                    .output();
                std::thread::sleep(std::time::Duration::from_secs(3));
            }

            // Kill tracked child.
            {
                let mut processes = state.processes.lock().unwrap();
                if let Some(pos) = processes.iter().position(|(p, _)| p == pier_path) {
                    let (_, mut child) = processes.remove(pos);
                    let _ = child.kill();
                }
            }

            // Last resort SIGKILL.
            #[cfg(unix)]
            {
                let _ = std::process::Command::new("pkill")
                    .args(["-9", "-f", pier_path])
                    .output();
                std::thread::sleep(std::time::Duration::from_secs(1));
            }

            if any_urbit_process_alive(pier_path) {
                emit_log(app, pier_path,
                    "[portmate] Warning: could not kill all urbit processes — op may fail with 'shenanigans!'"
                );
            }

            if lock.exists() {
                emit_log(
                    app,
                    pier_path,
                    "[portmate] Forcibly removing stale lock file…",
                );
                let _ = std::fs::remove_file(&lock);
            }
        }
    }

    // ── Clean up portmate state ───────────────────────────────────────────────
    state
        .stdin_txs
        .lock()
        .unwrap()
        .retain(|(p, _)| p != pier_path);
    {
        let mut processes = state.processes.lock().unwrap();
        if let Some(pos) = processes.iter().position(|(p, _)| p == pier_path) {
            let (_, mut child) = processes.remove(pos);
            let _ = child.kill();
        }
    }
    {
        let mut ships = state.ships.lock().unwrap();
        if let Some(ship) = ships.iter_mut().find(|s| s.pier_path == pier_path) {
            ship.status = "stopped".to_string();
            ship.pid = None;
        }
    }
    let _ = state.save();

    emit_log(
        app,
        pier_path,
        "[portmate] All processes clear. Proceeding with maintenance op…",
    );
}

/// Spawn `urbit <op> <pier_name> --loom 34` from the pier's parent directory,
/// stream stdout/stderr back as `ship-log` events, and wait for the process.
fn run_op(
    op: &str,
    binary: &str,
    pier_name: &str,
    work_dir: &std::path::Path,
    pier_path: &str,
    app: &AppHandle,
) -> Result<(), String> {
    emit_log(
        app,
        pier_path,
        &format!(
            "[portmate] Running: {} {} {} --loom 34  (cwd: {})",
            binary,
            op,
            pier_name,
            work_dir.display()
        ),
    );

    let mut child = std::process::Command::new(binary)
        .args([op, pier_name, "--loom", "34"])
        .current_dir(work_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start `urbit {op}`: {e}"))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let app_out = app.clone();
    let pier_out = pier_path.to_string();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().flatten() {
            emit_log(&app_out, &pier_out, &line);
        }
    });

    let app_err = app.clone();
    let pier_err = pier_path.to_string();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().flatten() {
            emit_log(&app_err, &pier_err, &format!("[stderr] {line}"));
        }
    });

    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`urbit {op}` exited with {status}"))
    }
}

fn emit_log(app: &AppHandle, pier_path: &str, line: &str) {
    let _ = app.emit(
        "ship-log",
        serde_json::json!({ "line": line, "pier_path": pier_path }),
    );
}

#[cfg(unix)]
fn is_pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
fn is_pid_alive(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid), "/NH"])
        .output()
        .map_or(false, |o| {
            String::from_utf8_lossy(&o.stdout).contains(&pid.to_string())
        })
}
