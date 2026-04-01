use std::sync::Mutex;

mod commands;

use commands::boot::{
    get_platform_info,
    download_urbit,
    boot_comet,
    send_dojo,
    stop_ship,
    is_ship_running,
};

pub struct ShipState {
    pub process:  Mutex<Option<std::process::Child>>,
    pub stdin_tx: Mutex<Option<std::sync::mpsc::Sender<String>>>,
}

impl Default for ShipState {
    fn default() -> Self {
        Self {
            process:  Mutex::new(None),
            stdin_tx: Mutex::new(None),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(ShipState::default())
        .invoke_handler(tauri::generate_handler![
            get_platform_info,
            download_urbit,
            boot_comet,
            send_dojo,
            stop_ship,
            is_ship_running,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}