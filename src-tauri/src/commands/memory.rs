use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::thread;
use tauri::{AppHandle, Emitter, Manager, State};

use super::boot::restart_ship_internal;
use super::memory_sched::{mark_schedule_running, record_schedule_result};
use crate::ShipState;

// ── Memory operations entry points ────────────────────────────────────────────

#[tauri::command]
pub fn pack_ship(
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
	run_memory_op("pack".to_string(), pier_path, app, state, true)
}

#[tauri::command]
pub fn meld_ship(
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
	run_memory_op("meld".to_string(), pier_path, app, state, true)
}

#[tauri::command]
pub fn roll_ship(
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
	run_memory_op("roll".to_string(), pier_path, app, state, true)
}

#[tauri::command]
pub fn chop_ship(
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    run_memory_op("chop".to_string(), pier_path, app, state, true)
}

// ── Shared implementation ─────────────────────────────────────────────────────

pub fn run_scheduled_memory_op(
    op: String,
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    run_memory_op(op, pier_path, app, state, true)
}

fn run_memory_op(
    op: String,
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
    restart_after: bool,
) -> Result<(), String> {
    let op = normalize_memory_op(&op)?;
    begin_active_memory_op(&state, &pier_path)?;

    let setup = (|| -> Result<(String, std::path::PathBuf, String), String> {
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

        Ok((binary, work_dir, pier_name))
    })();

    let (binary, work_dir, pier_name) = match setup {
        Ok(values) => values,
        Err(error) => {
            finish_active_memory_op(&state, &pier_path);
            return Err(error);
        }
    };

    mark_schedule_running(&app, &pier_path, &op);

    let run_online = use_online_pack_meld_path(&op, &pier_path, &state);
    if !run_online {
        stop_for_maintenance(&pier_path, &app, &state);
    }

    let app2 = app.clone();
    let path2 = pier_path.clone();
    thread::spawn(move || {
        let mut success = false;
        let mut error_message: Option<String> = None;

        if run_online {
            emit_log(
                &app2,
                &path2,
                &format!("[portmate] Starting online {op} on '{pier_name}' via conn.sock…"),
            );
            match run_online_pack_meld(&op, &binary, &path2, &app2) {
                Ok(()) => {
                    emit_log(&app2, &path2, &format!("[portmate] online {op} complete ✓"));
                    success = true;
                }
                Err(e) => {
                    emit_log(&app2, &path2, &format!("[portmate] online {op} failed: {e}"));
                    emit_log(
                        &app2,
                        &path2,
                        &format!("[portmate] online {op} failed; falling back to offline maintenance mode…"),
                    );

                    let state2 = app2.state::<ShipState>();
                    stop_for_maintenance(&path2, &app2, &state2);

                    match run_op(&op, &binary, &pier_name, &work_dir, &path2, &app2) {
                        Ok(()) => {
                            emit_log(&app2, &path2, &format!("[portmate] offline {op} complete ✓"));
                            success = true;

                            if restart_after {
                                emit_log(&app2, &path2, "[portmate] Restarting ship after maintenance…");
                                if let Err(error) = restart_ship_internal(path2.clone(), app2.clone(), app2.state()) {
                                    emit_log(&app2, &path2, &format!("[portmate] restart failed: {error}"));
                                    success = false;
                                    error_message = Some(format!("Maintenance succeeded but restart failed: {error}"));
                                }
                            }
                        }
                        Err(fallback_error) => {
                            emit_log(
                                &app2,
                                &path2,
                                &format!("[portmate] offline fallback {op} failed: {fallback_error}"),
                            );
                            error_message = Some(format!(
                                "Online {op} failed: {e}; offline fallback failed: {fallback_error}"
                            ));
                        }
                    }
                }
            }
        } else {
            emit_log(
                &app2,
                &path2,
                &format!("[portmate] Starting {op} on '{pier_name}' — this may take a few minutes…"),
            );
            match run_op(&op, &binary, &pier_name, &work_dir, &path2, &app2) {
                Ok(()) => {
                    emit_log(&app2, &path2, &format!("[portmate] {op} complete ✓"));
                    success = true;

                    if restart_after {
                        emit_log(&app2, &path2, "[portmate] Restarting ship after maintenance…");
                        if let Err(error) = restart_ship_internal(path2.clone(), app2.clone(), app2.state()) {
                            emit_log(&app2, &path2, &format!("[portmate] restart failed: {error}"));
                            success = false;
                            error_message = Some(format!("Maintenance succeeded but restart failed: {error}"));
                        }
                    }
                }
                Err(e) => {
                    emit_log(&app2, &path2, &format!("[portmate] {op} failed: {e}"));
                    error_message = Some(e);
                }
            }
        }

        record_schedule_result(&app2, &path2, &op, success, error_message.clone());
        finish_active_memory_op(&app2.state::<ShipState>(), &path2);

        let _ = app2.emit(
            "memory-op-done",
            serde_json::json!({
                "pier_path": path2,
                "op":        op,
                "success":   success,
                "error":     error_message,
            }),
        );
    });

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn normalize_memory_op(op: &str) -> Result<String, String> {
    match op.to_lowercase().as_str() {
        "pack" => Ok("pack".to_string()),
        "meld" => Ok("meld".to_string()),
        "roll" => Ok("roll".to_string()),
        "chop" => Ok("chop".to_string()),
        _ => Err(format!("Invalid memory op: {op}")),
    }
}

fn begin_active_memory_op(state: &State<'_, ShipState>, pier_path: &str) -> Result<(), String> {
    let mut active_memory_ops = state.active_memory_ops.lock().unwrap();
    if !active_memory_ops.insert(pier_path.to_string()) {
        return Err(format!("Maintenance already running for {pier_path}"));
    }
    Ok(())
}

fn finish_active_memory_op(state: &State<'_, ShipState>, pier_path: &str) {
    state
        .active_memory_ops
        .lock()
        .unwrap()
        .remove(pier_path);
}

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

#[cfg(unix)]
fn ship_looks_running(pier_path: &str, state: &State<'_, ShipState>) -> bool {
    let lock = std::path::Path::new(pier_path).join(".urb").join("lock");
    let in_table = state
        .processes
        .lock()
        .unwrap()
        .iter()
        .any(|(p, _)| p == pier_path);
    let has_stdin_tx = state
        .stdin_txs
        .lock()
        .unwrap()
        .iter()
        .any(|(p, _)| p == pier_path);
    let lock_alive = lock.exists()
        && std::fs::read_to_string(&lock)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .map(is_pid_alive)
            .unwrap_or(false);

    in_table || has_stdin_tx || lock_alive
}

#[cfg(unix)]
fn use_online_pack_meld_path(op: &str, pier_path: &str, state: &State<'_, ShipState>) -> bool {
    matches!(op, "pack" | "meld") && ship_looks_running(pier_path, state)
}

#[cfg(not(unix))]
fn use_online_pack_meld_path(_op: &str, _pier_path: &str, _state: &State<'_, ShipState>) -> bool {
    false
}

#[cfg(unix)]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(unix)]
fn run_online_pack_meld(
    op: &str,
    binary: &str,
    pier_path: &str,
    app: &AppHandle,
) -> Result<(), String> {
    let conn_sock = std::path::Path::new(pier_path).join(".urb").join("conn.sock");
    if !conn_sock.exists() {
        return Err(format!(
            "No conn.sock found at {}. Is the ship running?",
            conn_sock.display()
        ));
    }

    let payload = format!("[0 %urth %{op}]");
    let binary_q = shell_single_quote(binary);
    let sock_q = shell_single_quote(&conn_sock.to_string_lossy());
    let payload_q = shell_single_quote(&payload);
    let pipeline = format!(
        "echo {payload_q} | {binary_q} eval -jn | nc -U -w 1 {sock_q} | {binary_q} eval -cn"
    );

    emit_log(app, pier_path, &format!("[portmate] Running: {pipeline}"));

    let mut child = std::process::Command::new("/bin/sh")
        .args(["-lc", &pipeline])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start online `{op}` pipeline: {e}"))?;

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
        Err(format!("online `{op}` pipeline exited with {status}"))
    }
}

#[cfg(not(unix))]
fn run_online_pack_meld(
    _op: &str,
    _binary: &str,
    _pier_path: &str,
    _app: &AppHandle,
) -> Result<(), String> {
    Err("online pack/meld only supported on Unix".to_string())
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

#[cfg(windows)]
fn any_urbit_process_alive(pier_path: &str) -> bool {
    // Use PowerShell to search for the pier path in process command lines.
    // wmic is deprecated in Windows 10 21H1+ and removed from Windows 11.
    let script = format!(
        "(Get-CimInstance Win32_Process | Where-Object {{ $_.CommandLine -like '*{}*' }} | Measure-Object).Count",
        pier_path.replace('\'', "''")  // escape single quotes for PowerShell
    );
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u32>()
                .unwrap_or(0)
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
///   5. Only if processes are still alive after all that: SIGTERM then SIGKILL
///      (Unix), or taskkill /F /T (Windows).
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
             If it was previously hard-killed, the pier state may be \
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
                "[portmate] Processes still alive after 10s — force-killing…",
            );

            // Unix: SIGTERM first, then SIGKILL
            #[cfg(unix)]
            {
                let _ = std::process::Command::new("pkill")
                    .args(["-TERM", "-f", pier_path])
                    .output();
                std::thread::sleep(std::time::Duration::from_secs(3));
            }

            // Kill the tracked child (cross-platform — Tauri's Child::kill()
            // calls TerminateProcess on Windows and SIGKILL on Unix)
            {
                let mut processes = state.processes.lock().unwrap();
                if let Some(pos) = processes.iter().position(|(p, _)| p == pier_path) {
                    let (_, mut child) = processes.remove(pos);
                    let _ = child.kill();
                }
            }

            // Unix last-resort SIGKILL (catches the worker and any stragglers)
            #[cfg(unix)]
            {
                let _ = std::process::Command::new("pkill")
                    .args(["-9", "-f", pier_path])
                    .output();
                std::thread::sleep(std::time::Duration::from_secs(1));
            }

            // Windows: taskkill /F /T kills the target and all its children.
            // Run it once for TERM-equivalent and once more as last resort.
            // We use PowerShell to find PIDs by command-line then taskkill each,
            // because taskkill /FI "COMMANDLINE like ..." is not reliable.
            #[cfg(windows)]
            {
                let script = format!(
                    "Get-CimInstance Win32_Process \
                     | Where-Object {{ $_.CommandLine -like '*{}*' }} \
                     | ForEach-Object {{ taskkill /F /T /PID $_.ProcessId }}",
                    pier_path.replace('\'', "''")
                );
                let _ = std::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", &script])
                    .output();
                std::thread::sleep(std::time::Duration::from_secs(2));
            }

            if any_urbit_process_alive(pier_path) {
                emit_log(
                    app,
                    pier_path,
                    "[portmate] Warning: could not kill all urbit processes — \
                     op may fail with 'shenanigans!'",
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

#[cfg(windows)]
fn is_pid_alive(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid), "/NH"])
        .output()
        .map_or(false, |o| {
            String::from_utf8_lossy(&o.stdout).contains(&pid.to_string())
        })
}