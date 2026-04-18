use std::path::PathBuf;

use tauri::{AppHandle, Emitter, State};

use crate::ShipState;

// ── Schedule model ────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MemorySchedule {
    pub pier_path: String,
    pub op: String,          // "pack" | "meld" | "roll" | "chop"
    pub interval_days: u32,  // run every N days
    pub enabled: bool,
}

// ── New state storage ─────────────────────────────────────────────────────────
// NOTE: This is a first draft. You may also choose to embed this inside
// ShipState in lib.rs instead of keeping separate storage here.
//
// For now, we store schedules in a JSON file under the app data dir.

fn schedules_file(data_dir: &PathBuf) -> PathBuf {
    data_dir.join("portmate_schedules.json")
}

fn load_from_disk(data_dir: &PathBuf) -> Option<Vec<MemorySchedule>> {
    let path = schedules_file(data_dir);
    if !path.exists() {
        return Some(Vec::new());
    }

    let json = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&json).ok()
}

fn save_to_disk(data_dir: &PathBuf, schedules: &[MemorySchedule]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(schedules).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    std::fs::write(schedules_file(data_dir), json).map_err(|e| e.to_string())
}

// ── Shared access helpers ─────────────────────────────────────────────────────

fn normalize_op(op: &str) -> Option<String> {
    match op.to_lowercase().as_str() {
        "pack" | "meld" | "roll" | "chop" => Some(op.to_lowercase()),
        _ => None,
    }
}

fn get_schedule_index(schedules: &[MemorySchedule], pier_path: &str) -> Option<usize> {
    schedules.iter().position(|s| s.pier_path == pier_path)
}

// ── Tauri commands ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_memory_schedule(
    pier_path: String,
    state: State<'_, ShipState>,
) -> Result<Option<MemorySchedule>, String> {
    let ships = state.ships.lock().unwrap();
    let ship_exists = ships.iter().any(|s| s.pier_path == pier_path);
    drop(ships);

    if !ship_exists {
        return Err(format!("No ship found at {pier_path}"));
    }

    let schedules = state.memory_schedules.lock().unwrap();
    Ok(schedules.iter().find(|s| s.pier_path == pier_path).cloned())
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

    let ship_exists = {
        let ships = state.ships.lock().unwrap();
        ships.iter().any(|s| s.pier_path == schedule.pier_path)
    };

    if !ship_exists {
        return Err(format!("No ship found at {}", schedule.pier_path));
    }

    let mut schedules = state.memory_schedules.lock().unwrap();
    let new_schedule = MemorySchedule {
        pier_path: schedule.pier_path.clone(),
        op,
        interval_days: schedule.interval_days,
        enabled: schedule.enabled,
    };

    match get_schedule_index(&schedules, &schedule.pier_path) {
        Some(idx) => schedules[idx] = new_schedule,
        None => schedules.push(new_schedule),
    }

    save_to_disk(&state.data_dir, &schedules)?;
    let _ = state.save();

    let _ = app.emit(
        "memory-schedule-updated",
        serde_json::json!({
            "pierPath": schedule.pier_path,
            "success": true
        }),
    );

    Ok(())
}

#[tauri::command]
pub fn clear_memory_schedule(
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    let mut schedules = state.memory_schedules.lock().unwrap();
    let before = schedules.len();
    schedules.retain(|s| s.pier_path != pier_path);

    if schedules.len() == before {
        return Err(format!("No schedule found for {pier_path}"));
    }

    save_to_disk(&state.data_dir, &schedules)?;
    let _ = state.save();

    let _ = app.emit(
        "memory-schedule-updated",
        serde_json::json!({
            "pierPath": pier_path,
            "success": true,
            "deleted": true
        }),
    );

    Ok(())
}

#[tauri::command]
pub fn list_memory_schedules(
    state: State<'_, ShipState>,
) -> Result<Vec<MemorySchedule>, String> {
    let schedules = state.memory_schedules.lock().unwrap();
    Ok(schedules.clone())
}

// ── Startup helper ────────────────────────────────────────────────────────────
// Call this from lib.rs when ShipState is created or loaded.

pub fn load_schedules_into_state(state: &ShipState) -> Result<(), String> {
    let loaded = load_from_disk(&state.data_dir).unwrap_or_default();
    let mut schedules = state.memory_schedules.lock().unwrap();
    *schedules = loaded;
    Ok(())
}