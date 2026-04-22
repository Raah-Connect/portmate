use std::{collections::HashSet, path::PathBuf, sync::Mutex};
use tauri::Manager;

mod commands;

use commands::boot::{
    boot_comet, delete_ship, download_urbit, get_platform_info, get_running_ships, is_ship_running,
    request_access_code, restart_ship, send_dojo, stop_ship,
};
use commands::boot_existing::boot_existing;
use commands::boot_key::boot_key;
use commands::memory::{chop_ship, meld_ship, pack_ship, roll_ship};
use commands::memory_sched::{
    clear_all_memory_schedules_for_ship, clear_memory_schedule, get_memory_schedule,
    get_memory_schedules_for_ship, list_memory_schedules, load_schedules_into_state,
    set_memory_schedule, start_memory_scheduler_loop, MemorySchedule,
};
use commands::ship_stats::{
    refresh_all_ship_sizes, refresh_all_sizes_now, refresh_ship_size_command,
};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ShipInfo {
    pub name: String,
    pub pier_path: String,
    pub url: String,
    pub access_code: String,
    pub status: String,
    pub binary_path: String,
    pub pid: Option<u32>,
    #[serde(default)]
    pub loopback_port: Option<u16>,
    #[serde(default)]
    pub pier_size_bytes: Option<u64>,
}

pub struct ShipState {
    pub processes: Mutex<Vec<(String, std::process::Child)>>,
    pub stdin_txs: Mutex<Vec<(String, std::sync::mpsc::Sender<String>)>>,
    pub ships: Mutex<Vec<ShipInfo>>,
    pub memory_schedules: Mutex<Vec<MemorySchedule>>,
    pub active_memory_ops: Mutex<HashSet<String>>,
    pub data_dir: PathBuf,
}

impl ShipState {
    pub fn new(data_dir: PathBuf) -> Self {
        let ships = Self::load_from_disk(&data_dir)
            .unwrap_or_default()
            .into_iter()
            .map(|mut s| {
                let lock_file = std::path::Path::new(&s.pier_path)
                    .join(".urb")
                    .join("lock");

                if lock_file.exists() {
                    match std::fs::read_to_string(&lock_file) {
                        Ok(pid_str) => {
                            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                                eprintln!(
                                    "[portmate] Found orphaned process {} for pier {}, terminating…",
                                    pid, s.name
                                );

                                #[cfg(unix)]
                                unsafe {
                                    libc::kill(pid as i32, libc::SIGKILL);
                                    libc::kill(-(pid as i32), libc::SIGKILL);
                                }

                                #[cfg(windows)]
                                {
                                    use std::process::Command;
                                    let _ = Command::new("taskkill")
                                        .args(&["/PID", &pid.to_string(), "/F", "/T"])
                                        .output();
                                }

                                std::thread::sleep(std::time::Duration::from_millis(800));
                                let _ = std::fs::remove_file(&lock_file);
                            } else {
                                eprintln!(
                                    "[portmate] Invalid PID in lock file for {}",
                                    s.name
                                );
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "[portmate] Could not read lock file for {}: {}",
                                s.name, e
                            );
                        }
                    }
                } else {
                    eprintln!(
                        "[portmate] No lock file found for {}, assuming clean shutdown",
                        s.name
                    );
                }

                s.status = "stopped".to_string();
                s.pid = None;
                s
            })
            .collect();

        Self {
            processes: Mutex::new(Vec::new()),
            stdin_txs: Mutex::new(Vec::new()),
            ships: Mutex::new(ships),
            memory_schedules: Mutex::new(Vec::new()),
            active_memory_ops: Mutex::new(HashSet::new()),
            data_dir,
        }
    }

    fn state_file(data_dir: &PathBuf) -> PathBuf {
        data_dir.join("portmate_ships.json")
    }

    pub fn save(&self) -> Result<(), String> {
        let ships = self.ships.lock().unwrap().clone();
        let json = serde_json::to_string_pretty(&ships).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&self.data_dir).map_err(|e| e.to_string())?;
        std::fs::write(Self::state_file(&self.data_dir), json).map_err(|e| e.to_string())
    }

    fn load_from_disk(data_dir: &PathBuf) -> Option<Vec<ShipInfo>> {
        let path = Self::state_file(data_dir);
        if !path.exists() {
            return Some(Vec::new());
        }
        let json = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&json).ok()
    }

    pub fn verify_ship_status(&self, pier_path: &str) -> bool {
        let lock_file = std::path::Path::new(pier_path).join(".urb").join("lock");
        if lock_file.exists() {
            if let Ok(pid_str) = std::fs::read_to_string(&lock_file) {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    return is_process_running(pid);
                }
            }
        }
        false
    }
}

#[cfg(unix)]
fn is_process_running(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(windows)]
fn is_process_running(pid: u32) -> bool {
    use std::process::Command;
    let output = Command::new("tasklist")
        .args(&["/FI", &format!("PID eq {}", pid), "/NH"])
        .output()
        .ok();
    output.map_or(false, |o| {
        String::from_utf8_lossy(&o.stdout).contains(&pid.to_string())
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("portmate");

    let state = ShipState::new(data_dir);
    if let Err(e) = load_schedules_into_state(&state) {
        eprintln!("[portmate] Failed to load memory schedules: {e}");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(|app| {
            let app_handle = app.handle().clone();
            start_memory_scheduler_loop(app_handle.clone());

            let state: tauri::State<'_, ShipState> = app.state();
            if let Err(e) = refresh_all_sizes_now(&app_handle, &state) {
                eprintln!("[portmate] Failed to refresh ship sizes at startup: {e}");
            }

            let app_handle_for_thread = app_handle.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(6 * 60 * 60));

                let state: tauri::State<'_, ShipState> = app_handle_for_thread.state();
                if let Err(e) = refresh_all_sizes_now(&app_handle_for_thread, &state) {
                    eprintln!("[portmate] Failed periodic ship size refresh: {e}");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // platform / download
            get_platform_info,
            download_urbit,
            // ship lifecycle
            boot_comet,
            boot_existing,
            boot_key,
            send_dojo,
            restart_ship,
            stop_ship,
            delete_ship,
            is_ship_running,
            get_running_ships,
            request_access_code,
            // memory management
            pack_ship,
            meld_ship,
            roll_ship,
            chop_ship,
            // memory schedules
            get_memory_schedule,
            get_memory_schedules_for_ship,
            set_memory_schedule,
            clear_memory_schedule,
            clear_all_memory_schedules_for_ship,
            list_memory_schedules,
            // ship stats
            refresh_ship_size_command,
            refresh_all_ship_sizes,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}