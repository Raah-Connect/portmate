use std::path::PathBuf;
use std::sync::Mutex;

mod commands;

use commands::boot::{
    boot_comet, delete_ship, download_urbit, get_platform_info, get_running_ships, is_ship_running,
    restart_ship, send_dojo, stop_ship,
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
}

pub struct ShipState {
    pub processes: Mutex<Vec<(String, std::process::Child)>>,
    pub stdin_txs: Mutex<Vec<(String, std::sync::mpsc::Sender<String>)>>,
    pub ships: Mutex<Vec<ShipInfo>>,
    pub data_dir: PathBuf,
}

impl ShipState {
    pub fn new(data_dir: PathBuf) -> Self {
        let ships = Self::load_from_disk(&data_dir)
            .unwrap_or_default()
            .into_iter()
            .map(|mut s| {
                // Kill any orphaned urbit process holding the pier lock
                let lock_file = std::path::Path::new(&s.pier_path)
                    .join(".urb")
                    .join("lock");
                
                if lock_file.exists() {
                    match std::fs::read_to_string(&lock_file) {
                        Ok(pid_str) => {
                            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                                eprintln!("[portmate] Found orphaned process {} for pier {}, terminating...", pid, s.name);
                                
                                // 🔻🔻🔻 UPDATED KILL BLOCK - KILLS PROCESS GROUP 🔻🔻🔻
                                #[cfg(unix)]
                                unsafe {
                                    // Kill the worker PID from the lock file
                                    libc::kill(pid as i32, libc::SIGKILL);
                                    
                                    // Also kill the parent process group to catch the launcher
                                    // Negative PID kills the entire process group
                                    libc::kill(-(pid as i32), libc::SIGKILL);
                                }
                                
                                #[cfg(windows)]
                                {
                                    use std::process::Command;
                                    // Kill the process tree on Windows
                                    let _ = Command::new("taskkill")
                                        .args(&["/PID", &pid.to_string(), "/F", "/T"])
                                        .output();
                                }
                                // 🔻🔻🔻 END OF UPDATED KILL BLOCK 🔻🔻🔻
                                
                                // Give processes time to die
                                std::thread::sleep(std::time::Duration::from_millis(800));
                                
                                // Try to remove the lock file after killing
                                let _ = std::fs::remove_file(&lock_file);
                            } else {
                                eprintln!("[portmate] Invalid PID in lock file for {}", s.name);
                            }
                        }
                        Err(e) => {
                            eprintln!("[portmate] Could not read lock file for {}: {}", s.name, e);
                        }
                    }
                } else {
                    eprintln!("[portmate] No lock file found for {}, assuming clean shutdown", s.name);
                }
                
                // Mark ship as stopped regardless
                s.status = "stopped".to_string();
                s.pid = None;
                s
            })
            .collect();

        Self {
            processes: Mutex::new(Vec::new()),
            stdin_txs: Mutex::new(Vec::new()),
            ships: Mutex::new(ships),
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
    
    // Helper to verify if a ship's process is actually running
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

// Helper function to check if a process is running
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
    output.map_or(false, |o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("portmate");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(ShipState::new(data_dir))
        .invoke_handler(tauri::generate_handler![
            get_platform_info,
            download_urbit,
            boot_comet,
            send_dojo,
            restart_ship,
            stop_ship,
            delete_ship,
            is_ship_running,
            get_running_ships,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}