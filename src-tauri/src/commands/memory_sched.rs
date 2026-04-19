use std::path::PathBuf;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, Manager, State};

use super::memory::run_scheduled_memory_op;
use crate::ShipState;

const DAY_SECONDS: i64 = 24 * 60 * 60;
const SCHEDULER_POLL_INTERVAL: Duration = Duration::from_secs(30);

// ── Schedule model ────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MemorySchedule {
    pub pier_path: String,
    pub op: String,
    pub interval_days: u32,
    pub enabled: bool,
    #[serde(default)]
    pub last_run_at: Option<i64>,
    #[serde(default)]
    pub next_run_at: Option<i64>,
    #[serde(default)]
    pub last_status: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub running: bool,
}

// ── Disk persistence ─────────────────────────────────────────────────────────

fn schedules_file(data_dir: &PathBuf) -> PathBuf {
    data_dir.join("portmate_schedules.json")
}

fn load_from_disk(data_dir: &PathBuf) -> Option<Vec<MemorySchedule>> {
    let path = schedules_file(data_dir);
    if !path.exists() {
        return Some(Vec::new());
    }

    let json = std::fs::read_to_string(path).ok()?;
    let schedules: Vec<MemorySchedule> = serde_json::from_str(&json).ok()?;
    Some(
        schedules
            .into_iter()
            .filter_map(normalize_loaded_schedule)
            .collect(),
    )
}

fn save_to_disk(data_dir: &PathBuf, schedules: &[MemorySchedule]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(schedules).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    std::fs::write(schedules_file(data_dir), json).map_err(|e| e.to_string())
}

fn persist_schedules(state: &ShipState, schedules: &[MemorySchedule]) -> Result<(), String> {
    save_to_disk(&state.data_dir, schedules)?;
    let _ = state.save();
    Ok(())
}

// ── Shared helpers ────────────────────────────────────────────────────────────

fn normalize_op(op: &str) -> Option<String> {
    match op.to_lowercase().as_str() {
        "pack" | "meld" | "roll" | "chop" => Some(op.to_lowercase()),
        _ => None,
    }
}

fn normalize_loaded_schedule(mut schedule: MemorySchedule) -> Option<MemorySchedule> {
    schedule.op = normalize_op(&schedule.op)?;

    if schedule.last_status.as_deref() == Some("running") {
        schedule.last_status = Some("error".to_string());
        if schedule.last_error.is_none() {
            schedule.last_error = Some("Portmate restarted while maintenance was running.".to_string());
        }
    }

    schedule.running = false;
    if schedule.enabled {
        if schedule.next_run_at.is_none() {
            schedule.next_run_at = compute_next_run_at(schedule.last_run_at, schedule.interval_days, true);
        }
    } else {
        schedule.next_run_at = None;
    }

    Some(schedule)
}

fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn compute_next_run_at(last_run_at: Option<i64>, interval_days: u32, enabled: bool) -> Option<i64> {
    if !enabled {
        return None;
    }

    let base = last_run_at.unwrap_or_else(now_timestamp);
    Some(base + i64::from(interval_days) * DAY_SECONDS)
}

fn get_schedule_index(schedules: &[MemorySchedule], pier_path: &str, op: &str) -> Option<usize> {
    schedules
        .iter()
        .position(|s| s.pier_path == pier_path && s.op == op)
}

fn ship_exists(state: &State<'_, ShipState>, pier_path: &str) -> bool {
    let ships = state.ships.lock().unwrap();
    ships.iter().any(|s| s.pier_path == pier_path)
}

fn emit_schedule_updated(app: &AppHandle, pier_path: &str, op: Option<&str>) {
    let _ = app.emit(
        "memory-schedule-updated",
        serde_json::json!({
            "pierPath": pier_path,
            "op": op,
            "success": true,
        }),
    );
}

fn emit_ship_log(app: &AppHandle, pier_path: &str, line: &str) {
    let _ = app.emit(
        "ship-log",
        serde_json::json!({ "line": line, "pier_path": pier_path }),
    );
}

// ── Scheduler runtime ─────────────────────────────────────────────────────────

pub fn start_memory_scheduler_loop(app: AppHandle) {
    thread::spawn(move || loop {
        if let Err(error) = run_scheduler_tick(&app) {
            eprintln!("[portmate] Memory scheduler tick failed: {error}");
        }
        thread::sleep(SCHEDULER_POLL_INTERVAL);
    });
}

fn run_scheduler_tick(app: &AppHandle) -> Result<(), String> {
    let due_schedules = claim_due_schedules(app)?;

    for schedule in due_schedules {
        emit_ship_log(
            app,
            &schedule.pier_path,
            &format!(
                "[portmate] Scheduled maintenance due: {} every {} day(s)",
                schedule.op, schedule.interval_days
            ),
        );

        if let Err(error) = run_scheduled_memory_op(
            schedule.op.clone(),
            schedule.pier_path.clone(),
            app.clone(),
            app.state(),
        ) {
            emit_ship_log(
                app,
                &schedule.pier_path,
                &format!("[portmate] Failed to start scheduled {}: {error}", schedule.op),
            );
            record_schedule_result(app, &schedule.pier_path, &schedule.op, false, Some(error));
        }
    }

    Ok(())
}

fn claim_due_schedules(app: &AppHandle) -> Result<Vec<MemorySchedule>, String> {
    let state = app.state::<ShipState>();
    let now = now_timestamp();
    let active_memory_ops = state.active_memory_ops.lock().unwrap().clone();
    let mut schedules = state.memory_schedules.lock().unwrap();
    let mut due_schedules = Vec::new();
    let mut changed = false;

    for schedule in schedules.iter_mut() {
        if !schedule.enabled || schedule.running {
            continue;
        }

        let Some(next_run_at) = schedule.next_run_at else {
            continue;
        };

        if next_run_at > now || active_memory_ops.contains(&schedule.pier_path) {
            continue;
        }

        schedule.running = true;
        schedule.last_status = Some("running".to_string());
        schedule.last_error = None;
        due_schedules.push(schedule.clone());
        changed = true;
    }

    if changed {
        persist_schedules(&state, &schedules)?;
    }

    drop(schedules);

    for schedule in &due_schedules {
        emit_schedule_updated(app, &schedule.pier_path, Some(&schedule.op));
    }

    Ok(due_schedules)
}

pub fn mark_schedule_running(app: &AppHandle, pier_path: &str, op: &str) {
    let Some(op) = normalize_op(op) else {
        return;
    };

    let state = app.state::<ShipState>();
    let mut schedules = state.memory_schedules.lock().unwrap();
    let Some(index) = get_schedule_index(&schedules, pier_path, &op) else {
        return;
    };

    schedules[index].running = true;
    schedules[index].last_status = Some("running".to_string());
    schedules[index].last_error = None;

    if persist_schedules(&state, &schedules).is_ok() {
        drop(schedules);
        emit_schedule_updated(app, pier_path, Some(&op));
    }
}

pub fn record_schedule_result(
    app: &AppHandle,
    pier_path: &str,
    op: &str,
    success: bool,
    error: Option<String>,
) {
    let Some(op) = normalize_op(op) else {
        return;
    };

    let state = app.state::<ShipState>();
    let mut schedules = state.memory_schedules.lock().unwrap();
    let Some(index) = get_schedule_index(&schedules, pier_path, &op) else {
        return;
    };

    let schedule = &mut schedules[index];
    let completed_at = now_timestamp();
    schedule.running = false;
    schedule.last_run_at = Some(completed_at);
    schedule.last_status = Some(if success { "success" } else { "error" }.to_string());
    schedule.last_error = if success { None } else { error };
    schedule.next_run_at = compute_next_run_at(Some(completed_at), schedule.interval_days, schedule.enabled);

    if persist_schedules(&state, &schedules).is_ok() {
        drop(schedules);
        emit_schedule_updated(app, pier_path, Some(&op));
    }
}

// ── Tauri commands ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_memory_schedules(
    state: State<'_, ShipState>,
) -> Result<Vec<MemorySchedule>, String> {
    let schedules = state.memory_schedules.lock().unwrap();
    Ok(schedules.clone())
}

#[tauri::command]
pub fn get_memory_schedules_for_ship(
    pier_path: String,
    state: State<'_, ShipState>,
) -> Result<Vec<MemorySchedule>, String> {
    if !ship_exists(&state, &pier_path) {
        return Err(format!("No ship found at {pier_path}"));
    }

    let schedules = state.memory_schedules.lock().unwrap();
    Ok(schedules
        .iter()
        .filter(|s| s.pier_path == pier_path)
        .cloned()
        .collect())
}

#[tauri::command]
pub fn get_memory_schedule(
    pier_path: String,
    op: String,
    state: State<'_, ShipState>,
) -> Result<Option<MemorySchedule>, String> {
    let op = normalize_op(&op).ok_or_else(|| format!("Invalid memory op: {}", op))?;

    if !ship_exists(&state, &pier_path) {
        return Err(format!("No ship found at {pier_path}"));
    }

    let schedules = state.memory_schedules.lock().unwrap();
    Ok(schedules
        .iter()
        .find(|s| s.pier_path == pier_path && s.op == op)
        .cloned())
}

#[tauri::command]
pub fn set_memory_schedule(
    schedule: MemorySchedule,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    let op = normalize_op(&schedule.op)
        .ok_or_else(|| format!("Invalid memory op: {}", schedule.op))?;

    if schedule.interval_days == 0 {
        return Err("interval_days must be greater than 0".to_string());
    }

    if !ship_exists(&state, &schedule.pier_path) {
        return Err(format!("No ship found at {}", schedule.pier_path));
    }

    let mut schedules = state.memory_schedules.lock().unwrap();
    let existing = get_schedule_index(&schedules, &schedule.pier_path, &op)
        .map(|index| schedules[index].clone());

    let next_run_at = if existing.as_ref().is_some_and(|item| item.running) {
        existing.as_ref().and_then(|item| item.next_run_at)
    } else {
        compute_next_run_at(
            existing.as_ref().and_then(|item| item.last_run_at),
            schedule.interval_days,
            schedule.enabled,
        )
    };

    let new_schedule = MemorySchedule {
        pier_path: schedule.pier_path.clone(),
        op: op.clone(),
        interval_days: schedule.interval_days,
        enabled: schedule.enabled,
        last_run_at: existing.as_ref().and_then(|item| item.last_run_at),
        next_run_at,
        last_status: existing.as_ref().and_then(|item| item.last_status.clone()),
        last_error: existing.as_ref().and_then(|item| item.last_error.clone()),
        running: existing.as_ref().is_some_and(|item| item.running),
    };

    match get_schedule_index(&schedules, &schedule.pier_path, &op) {
        Some(index) => schedules[index] = new_schedule,
        None => schedules.push(new_schedule),
    }

    persist_schedules(&state, &schedules)?;
    drop(schedules);
    emit_schedule_updated(&app, &schedule.pier_path, Some(&op));

    Ok(())
}

#[tauri::command]
pub fn clear_memory_schedule(
    pier_path: String,
    op: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    let op = normalize_op(&op).ok_or_else(|| format!("Invalid memory op: {}", op))?;

    if !ship_exists(&state, &pier_path) {
        return Err(format!("No ship found at {pier_path}"));
    }

    let mut schedules = state.memory_schedules.lock().unwrap();
    let before = schedules.len();
    schedules.retain(|s| !(s.pier_path == pier_path && s.op == op));

    if schedules.len() == before {
        return Err(format!("No schedule found for {pier_path} / {op}"));
    }

    persist_schedules(&state, &schedules)?;
    drop(schedules);

    let _ = app.emit(
        "memory-schedule-updated",
        serde_json::json!({
            "pierPath": pier_path,
            "op": op,
            "success": true,
            "deleted": true,
        }),
    );

    Ok(())
}

#[tauri::command]
pub fn clear_all_memory_schedules_for_ship(
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    if !ship_exists(&state, &pier_path) {
        return Err(format!("No ship found at {pier_path}"));
    }

    let mut schedules = state.memory_schedules.lock().unwrap();
    let before = schedules.len();
    schedules.retain(|s| s.pier_path != pier_path);

    if schedules.len() == before {
        return Err(format!("No schedules found for {pier_path}"));
    }

    persist_schedules(&state, &schedules)?;
    drop(schedules);

    let _ = app.emit(
        "memory-schedule-updated",
        serde_json::json!({
            "pierPath": pier_path,
            "success": true,
            "deletedAll": true,
        }),
    );

    Ok(())
}

// ── Startup helper ────────────────────────────────────────────────────────────

pub fn load_schedules_into_state(state: &ShipState) -> Result<(), String> {
    let loaded = load_from_disk(&state.data_dir).unwrap_or_default();
    let mut schedules = state.memory_schedules.lock().unwrap();
    *schedules = loaded;
    Ok(())
}