use std::fs;
use std::path::Path;

use tauri::{AppHandle, Emitter, State};

use crate::ShipState;

// ── Size calculation ─────────────────────────────────────────────────────────

pub fn compute_pier_size_bytes(pier_path: &str) -> Result<u64, String> {
    let path = Path::new(pier_path);

    if !path.exists() {
        return Err(format!("Pier path does not exist: {pier_path}"));
    }

    let mut total: u64 = 0;
    accumulate_dir_size(path, &mut total)?;
    Ok(total)
}

fn accumulate_dir_size(path: &Path, total: &mut u64) -> Result<(), String> {
    if path.is_file() {
        let meta = fs::metadata(path).map_err(|e| e.to_string())?;
        *total = total.saturating_add(meta.len());
        return Ok(());
    }

    if !path.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let child_path = entry.path();

        // Skip symlinks / unusual entries if metadata fails
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        if file_type.is_symlink() {
            continue;
        }

        if child_path.is_dir() {
            accumulate_dir_size(&child_path, total)?;
        } else if child_path.is_file() {
            let meta = fs::metadata(&child_path).map_err(|e| e.to_string())?;
            *total = total.saturating_add(meta.len());
        }
    }

    Ok(())
}

// ── State update helper ───────────────────────────────────────────────────────

pub fn refresh_ship_size(
    pier_path: &str,
    app: &AppHandle,
    state: &State<'_, ShipState>,
) -> Result<u64, String> {
    let size = compute_pier_size_bytes(pier_path)?;

    {
        let mut ships = state.ships.lock().unwrap();
        if let Some(ship) = ships.iter_mut().find(|s| s.pier_path == pier_path) {
            // NOTE: you'll need to add this field to ShipInfo in lib.rs
            ship.pier_size_bytes = Some(size);
        } else {
            return Err(format!("No ship found at {pier_path}"));
        }
    }

    let _ = state.save();

    let _ = app.emit(
        "ship-size-updated",
        serde_json::json!({
            "pierPath": pier_path,
            "pierSizeBytes": size
        }),
    );

    Ok(size)
}

// ── Commands ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn refresh_ship_size_command(
    pier_path: String,
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<u64, String> {
    refresh_ship_size(&pier_path, &app, &state)
}

#[tauri::command]
pub fn refresh_all_ship_sizes(
    app: AppHandle,
    state: State<'_, ShipState>,
) -> Result<(), String> {
    let pier_paths: Vec<String> = {
        let ships = state.ships.lock().unwrap();
        ships.iter().map(|s| s.pier_path.clone()).collect()
    };

    for pier_path in pier_paths {
        let _ = refresh_ship_size(&pier_path, &app, &state);
    }

    Ok(())
}

// ── Startup helper ───────────────────────────────────────────────────────────
// Call this after ships are loaded or whenever you want to refresh all sizes.

pub fn refresh_all_sizes_now(
    app: &AppHandle,
    state: &State<'_, ShipState>,
) -> Result<(), String> {
    let pier_paths: Vec<String> = {
        let ships = state.ships.lock().unwrap();
        ships.iter().map(|s| s.pier_path.clone()).collect()
    };

    for pier_path in pier_paths {
        let _ = refresh_ship_size(&pier_path, app, state);
    }

    Ok(())
}