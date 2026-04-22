use chrono::{DateTime, Duration as ChronoDuration, Local, LocalResult, TimeZone, Timelike};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, Manager, State};

use super::memory::run_scheduled_memory_op;
use crate::ShipState;

const SCHEDULER_POLL_INTERVAL: Duration = Duration::from_secs(30);
const BUSY_RETRY_MIN_SECONDS: i64 = 90;
const BUSY_RETRY_JITTER_SECONDS: u64 = 30;
const DEFAULT_START_TIME: &str = "03:00";
const DEFAULT_SHIP_SCHEDULES: [(&str, &str); 4] = [
    ("pack", "00:00"),
    ("meld", "01:00"),
    ("roll", "02:00"),
    ("chop", "03:00"),
];

// ── Schedule model ────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MemorySchedule {
    pub pier_path: String,
    pub op: String,
    pub interval_days: u32,
    pub enabled: bool,
    #[serde(default)]
    pub start_time: String,
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
    schedule.start_time = normalize_start_time(&schedule.start_time)
        .or_else(|| schedule.next_run_at.and_then(start_time_from_timestamp))
        .or_else(|| schedule.last_run_at.and_then(start_time_from_timestamp))
        .unwrap_or_else(default_schedule_start_time);

    if schedule.last_status.as_deref() == Some("running") {
        schedule.last_status = Some("error".to_string());
        if schedule.last_error.is_none() {
            schedule.last_error = Some("Portmate restarted while maintenance was running.".to_string());
        }
    }

    schedule.running = false;
    if schedule.enabled {
        if schedule.next_run_at.is_none() {
            schedule.next_run_at = compute_next_run_at(
                schedule.last_run_at,
                schedule.interval_days,
                true,
                &schedule.start_time,
            );
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

fn default_schedule_start_time() -> String {
    DEFAULT_START_TIME.to_string()
}

fn parse_start_time(start_time: &str) -> Option<(u32, u32)> {
    let mut parts = start_time.split(':');
    let hour = parts.next()?.parse::<u32>().ok()?;
    let minute = parts.next()?.parse::<u32>().ok()?;

    if parts.next().is_some() || hour > 23 || minute > 59 {
        return None;
    }

    Some((hour, minute))
}

fn normalize_start_time(start_time: &str) -> Option<String> {
    let (hour, minute) = parse_start_time(start_time)?;
    Some(format!("{hour:02}:{minute:02}"))
}

fn local_datetime_from_timestamp(timestamp: i64) -> Option<DateTime<Local>> {
    match Local.timestamp_opt(timestamp, 0) {
        LocalResult::Single(datetime) => Some(datetime),
        LocalResult::Ambiguous(earliest, _) => Some(earliest),
        LocalResult::None => None,
    }
}

fn start_time_from_timestamp(timestamp: i64) -> Option<String> {
    let datetime = local_datetime_from_timestamp(timestamp)?;
    Some(format!("{:02}:{:02}", datetime.hour(), datetime.minute()))
}

fn resolve_local_timestamp(date: chrono::NaiveDate, hour: u32, minute: u32) -> Option<i64> {
    let local_time = date.and_hms_opt(hour, minute, 0)?;

    for offset_minutes in 0..=120 {
        let candidate = local_time + ChronoDuration::minutes(i64::from(offset_minutes));
        match Local.from_local_datetime(&candidate) {
            LocalResult::Single(datetime) => return Some(datetime.timestamp()),
            LocalResult::Ambiguous(earliest, _) => return Some(earliest.timestamp()),
            LocalResult::None => continue,
        }
    }

    None
}

fn compute_next_run_at(
    last_run_at: Option<i64>,
    interval_days: u32,
    enabled: bool,
    start_time: &str,
) -> Option<i64> {
    if !enabled {
        return None;
    }

    let normalized_start_time = normalize_start_time(start_time).unwrap_or_else(default_schedule_start_time);
    let (hour, minute) = parse_start_time(&normalized_start_time)?;

    if let Some(last_run_at) = last_run_at {
        let last_run = local_datetime_from_timestamp(last_run_at)?;
        let next_date = last_run
            .date_naive()
            .checked_add_signed(ChronoDuration::days(i64::from(interval_days)))?;

        return resolve_local_timestamp(next_date, hour, minute);
    }

    let now = Local::now();
    let today = now.date_naive();
    let today_at_start_time = resolve_local_timestamp(today, hour, minute)?;

    if today_at_start_time >= now.timestamp() {
        return Some(today_at_start_time);
    }

    let next_date = today.checked_add_signed(ChronoDuration::days(1))?;
    resolve_local_timestamp(next_date, hour, minute)
}

fn compute_busy_retry_at(now: i64, pier_path: &str, op: &str) -> i64 {
    let mut hasher = DefaultHasher::new();
    pier_path.hash(&mut hasher);
    op.hash(&mut hasher);
    now.hash(&mut hasher);

    let jitter = (hasher.finish() % (BUSY_RETRY_JITTER_SECONDS + 1)) as i64;
    now + BUSY_RETRY_MIN_SECONDS + jitter
}

fn is_ship_busy_error(error: &str) -> bool {
    let error = error.to_lowercase();
    error.contains("maintenance already running")
        || error.contains("already running")
        || error.contains("already busy")
        || error.contains("in progress")
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

pub fn ensure_default_memory_schedules_for_ship(
    app: &AppHandle,
    state: &State<'_, ShipState>,
    pier_path: &str,
) -> Result<(), String> {
    let mut schedules = state.memory_schedules.lock().unwrap();
    let mut added_ops = Vec::new();

    for (op, start_time) in DEFAULT_SHIP_SCHEDULES {
        if get_schedule_index(&schedules, pier_path, op).is_some() {
            continue;
        }

        schedules.push(MemorySchedule {
            pier_path: pier_path.to_string(),
            op: op.to_string(),
            interval_days: 1,
            enabled: true,
            start_time: start_time.to_string(),
            last_run_at: None,
            next_run_at: compute_next_run_at(None, 1, true, start_time),
            last_status: None,
            last_error: None,
            running: false,
        });
        added_ops.push(op.to_string());
    }

    if added_ops.is_empty() {
        return Ok(());
    }

    persist_schedules(state, &schedules)?;
    drop(schedules);

    for op in added_ops {
        emit_schedule_updated(app, pier_path, Some(&op));
    }

    Ok(())
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

fn mark_schedule_waiting(
    app: &AppHandle,
    pier_path: &str,
    op: &str,
    retry_at: i64,
) -> Result<(), String> {
    let Some(op) = normalize_op(op) else {
        return Ok(());
    };

    let state = app.state::<ShipState>();
    let mut schedules = state.memory_schedules.lock().unwrap();
    let Some(index) = get_schedule_index(&schedules, pier_path, &op) else {
        return Ok(());
    };

    let schedule = &mut schedules[index];
    schedule.running = false;
    schedule.last_status = Some("waiting".to_string());
    schedule.last_error = None;
    schedule.next_run_at = Some(retry_at);

    persist_schedules(&state, &schedules)?;
    drop(schedules);
    emit_schedule_updated(app, pier_path, Some(&op));

    Ok(())
}

#[derive(Default)]
struct ClaimedSchedules {
    due: Vec<MemorySchedule>,
    waiting: Vec<(MemorySchedule, i64)>,
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
    let claimed = claim_due_schedules(app)?;

    for (schedule, retry_at) in &claimed.waiting {
        let retry_in = (*retry_at - now_timestamp()).max(BUSY_RETRY_MIN_SECONDS);
        emit_ship_log(
            app,
            &schedule.pier_path,
            &format!(
                "[portmate] Scheduled {} is waiting for the ship to become free; retrying in about {}s",
                schedule.op, retry_in
            ),
        );
    }

    for schedule in claimed.due {
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
            if is_ship_busy_error(&error) {
                let retry_at = compute_busy_retry_at(now_timestamp(), &schedule.pier_path, &schedule.op);
                mark_schedule_waiting(app, &schedule.pier_path, &schedule.op, retry_at)?;
                emit_ship_log(
                    app,
                    &schedule.pier_path,
                    &format!(
                        "[portmate] Scheduled {} is waiting for the ship to become free; retrying soon",
                        schedule.op
                    ),
                );
                continue;
            }

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

fn claim_due_schedules(app: &AppHandle) -> Result<ClaimedSchedules, String> {
    let state = app.state::<ShipState>();
    let now = now_timestamp();
    let active_memory_ops = state.active_memory_ops.lock().unwrap().clone();
    let mut busy_ships: HashSet<String> = active_memory_ops;
    let mut schedules = state.memory_schedules.lock().unwrap();
    let mut claimed = ClaimedSchedules::default();
    let mut changed = false;

    for schedule in schedules.iter_mut() {
        if !schedule.enabled || schedule.running {
            continue;
        }

        let Some(next_run_at) = schedule.next_run_at else {
            continue;
        };

        if next_run_at > now {
            continue;
        }

        if busy_ships.contains(&schedule.pier_path) {
            let retry_at = compute_busy_retry_at(now, &schedule.pier_path, &schedule.op);
            schedule.running = false;
            schedule.last_status = Some("waiting".to_string());
            schedule.last_error = None;
            schedule.next_run_at = Some(retry_at);
            claimed.waiting.push((schedule.clone(), retry_at));
            changed = true;
            continue;
        }

        schedule.running = true;
        schedule.last_status = Some("running".to_string());
        schedule.last_error = None;
        busy_ships.insert(schedule.pier_path.clone());
        claimed.due.push(schedule.clone());
        changed = true;
    }

    if changed {
        persist_schedules(&state, &schedules)?;
    }

    drop(schedules);

    for schedule in &claimed.due {
        emit_schedule_updated(app, &schedule.pier_path, Some(&schedule.op));
    }

    for (schedule, _) in &claimed.waiting {
        emit_schedule_updated(app, &schedule.pier_path, Some(&schedule.op));
    }

    Ok(claimed)
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
    schedule.next_run_at = compute_next_run_at(
        Some(completed_at),
        schedule.interval_days,
        schedule.enabled,
        &schedule.start_time,
    );

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
    let start_time = normalize_start_time(&schedule.start_time)
        .or_else(|| existing.as_ref().and_then(|item| normalize_start_time(&item.start_time)))
        .unwrap_or_else(default_schedule_start_time);

    let next_run_at = if existing.as_ref().is_some_and(|item| item.running) {
        existing.as_ref().and_then(|item| item.next_run_at)
    } else {
        compute_next_run_at(
            existing.as_ref().and_then(|item| item.last_run_at),
            schedule.interval_days,
            schedule.enabled,
            &start_time,
        )
    };

    let new_schedule = MemorySchedule {
        pier_path: schedule.pier_path.clone(),
        op: op.clone(),
        interval_days: schedule.interval_days,
        enabled: schedule.enabled,
        start_time,
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