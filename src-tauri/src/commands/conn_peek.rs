use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri::State;

use crate::ShipState;

// ── Newt helpers ─────────────────────────────────────────────────────────────

fn jam_expression(urbit_bin: &str, expr: &str) -> Result<Vec<u8>, String> {
    let mut child = Command::new(urbit_bin)
        .args(["eval", "-jn"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn urbit eval -jn: {e}"))?;

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(expr.as_bytes())
        .map_err(|e| e.to_string())?;

    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    Ok(output.stdout)
}

fn cue_response(urbit_bin: &str, frame: &[u8]) -> Result<String, String> {
    let mut child = Command::new(urbit_bin)
        .args(["eval", "-cn"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn urbit eval -cn: {e}"))?;

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(frame)
        .map_err(|e| e.to_string())?;

    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn send_peek(sock_path: &str, jammed: &[u8]) -> Result<Vec<u8>, String> {
    let mut stream = UnixStream::connect(sock_path)
        .map_err(|e| format!("socket connect failed: {e}"))?;

    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;

    stream
        .write_all(jammed)
        .map_err(|e| format!("socket write failed: {e}"))?;

    // read tag byte
    let mut tag = [0u8; 1];
    stream
        .read_exact(&mut tag)
        .map_err(|e| format!("failed to read tag byte: {e}"))?;

    // read 4-byte little-endian length
    let mut len_bytes = [0u8; 4];
    stream
        .read_exact(&mut len_bytes)
        .map_err(|e| format!("failed to read length: {e}"))?;

    let length = u32::from_le_bytes(len_bytes) as usize;

    // read payload
    let mut payload = vec![0u8; length];
    stream
        .read_exact(&mut payload)
        .map_err(|e| format!("failed to read payload: {e}"))?;

    // reassemble full newt frame for urbit eval -cn
    let mut frame = Vec::with_capacity(5 + length);
    frame.extend_from_slice(&tag);
    frame.extend_from_slice(&len_bytes);
    frame.extend_from_slice(&payload);

    Ok(frame)
}

// ── @p atom formatter (no external deps) ─────────────────────────────────────

fn extract_trailing_atom(raw: &str) -> Option<u128> {
    let raw = raw.trim();
    // match 0x... or decimal at end of noun string
    let re_hex = raw.rfind("0x");
    let re_dec = raw.rfind(|c: char| c.is_ascii_digit());

    if let Some(pos) = re_hex {
        let hex_str: String = raw[pos + 2..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        u128::from_str_radix(&hex_str, 16).ok()
    } else if let Some(pos) = re_dec {
        // walk back to start of number
        let start = raw[..=pos]
            .rfind(|c: char| !c.is_ascii_digit())
            .map(|i| i + 1)
            .unwrap_or(0);
        raw[start..=pos].parse::<u128>().ok()
    } else {
        None
    }
}

// ── Public commands ───────────────────────────────────────────────────────────

/// Low-level peek: send any hoon expression, get raw decoded noun back
#[tauri::command]
pub fn conn_peek(
    urbit_bin: String,
    sock_path: String,
    expr: String,
) -> Result<String, String> {
    let jammed = jam_expression(&urbit_bin, &expr)?;
    let frame = send_peek(&sock_path, &jammed)?;
    let raw = cue_response(&urbit_bin, &frame)?;
    Ok(raw.trim().to_string())
}

/// Convenience command: get access code for a ship by name from state
#[tauri::command]
pub fn get_access_code(
    ship_name: String,
    state: State<'_, ShipState>,
) -> Result<String, String> {
    let (urbit_bin, pier_path, ship_patp) = {
        let ships = state.ships.lock().unwrap();
        let ship = ships
            .iter()
            .find(|s| s.name == ship_name)
            .ok_or_else(|| format!("ship '{ship_name}' not found"))?;
        (
            ship.binary_path.clone(),
            ship.pier_path.clone(),
            ship.name.clone(),
        )
    };

    let sock_path = Path::new(&pier_path)
        .join(".urb")
        .join("conn.sock")
        .to_string_lossy()
        .to_string();

    if !Path::new(&sock_path).exists() {
        return Err(format!("conn.sock not found — is '{ship_name}' running?"));
    }

    let expr = format!(
        "[0 %peek [%| [%once %j %code /~{}]]]",
        ship_patp.trim_start_matches('~')
    );

    let jammed = jam_expression(&urbit_bin, &expr)?;
    let frame = send_peek(&sock_path, &jammed)?;
    let raw = cue_response(&urbit_bin, &frame)?;

    let atom = extract_trailing_atom(&raw)
        .ok_or_else(|| format!("could not parse atom from response: {raw}"))?;

    // convert @p atom to syllable string via phonemic encoding
    // atom 0 = ~zod, so we just return the decimal for now and let
    // the frontend call (scow %p atom) if needed — or use the raw code
    // The atom IS the access code integer; format as Urbit decimal
    let s = atom.to_string();
    let mut parts: Vec<&str> = Vec::new();
    let mut i = 0;
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let rem = len % 3;
    if rem > 0 {
        parts.push(&s[..rem]);
    }
    let mut pos = rem;
    while pos < len {
        parts.push(&s[pos..pos + 3]);
        pos += 3;
    }
    let urbit_decimal = parts.join(".");

    Ok(urbit_decimal)
}
