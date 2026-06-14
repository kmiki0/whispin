// Windows "Run" registry entry that auto-starts Whispin at login, plus the
// related uninstall command that wipes our config + startup entry.

#![cfg(windows)]

use tauri::Manager;
use windows::core::HSTRING;
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ, REG_VALUE_TYPE,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Whispin";

unsafe fn open_run_key(write: bool) -> Result<HKEY, String> {
    let mut hkey = HKEY::default();
    let access = if write { KEY_READ | KEY_WRITE } else { KEY_READ };
    RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(RUN_KEY), 0, access, &mut hkey)
        .ok()
        .map_err(|e| format!("RegOpenKeyExW failed: {e}"))?;
    Ok(hkey)
}

#[tauri::command]
pub fn get_startup_enabled() -> bool {
    unsafe {
        let Ok(hkey) = open_run_key(false) else { return false };
        let mut data_type = REG_VALUE_TYPE(0);
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let res = RegQueryValueExW(
            hkey,
            &HSTRING::from(VALUE_NAME),
            None,
            Some(&mut data_type),
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(hkey);
        res.is_ok()
    }
}

#[tauri::command]
pub fn set_startup_enabled(enabled: bool) -> Result<(), String> {
    unsafe {
        let hkey = open_run_key(true)?;
        let result = if enabled {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let mut value: Vec<u16> = exe
                .as_os_str()
                .to_string_lossy()
                .encode_utf16()
                .collect();
            value.push(0);
            let bytes =
                std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
            RegSetValueExW(hkey, &HSTRING::from(VALUE_NAME), 0, REG_SZ, Some(bytes))
                .ok()
                .map_err(|e| format!("RegSetValueExW failed: {e}"))
        } else {
            let r = RegDeleteValueW(hkey, &HSTRING::from(VALUE_NAME));
            if r.is_ok() || r == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                Err(format!("RegDeleteValueW failed: {r:?}"))
            }
        };
        let _ = RegCloseKey(hkey);
        result
    }
}

#[tauri::command]
pub fn uninstall_app(app: tauri::AppHandle) -> Result<(), String> {
    // Best-effort: remove startup entry, delete config files, then exit.
    let _ = set_startup_enabled(false);
    if let Ok(dir) = app.path().app_config_dir() {
        let _ = std::fs::remove_file(dir.join("settings.json"));
        let _ = std::fs::remove_file(dir.join("dictionary.json"));
    }
    // Schedule exit slightly later so the IPC response can return first.
    let app_handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        app_handle.exit(0);
    });
    Ok(())
}

#[tauri::command]
pub fn open_settings_folder(app: tauri::AppHandle) -> Result<(), String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let _ = std::fs::create_dir_all(&dir);
    std::process::Command::new("explorer")
        .arg(dir.as_os_str())
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
