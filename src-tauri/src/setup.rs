// Setup callback for the Tauri builder: tray icon, main-window positioning,
// trigger callbacks (PTT / Toggle).
//
// Kept out of lib.rs so the entry point reads as a top-level wiring file
// instead of mixing in 150 lines of closure logic.

#![cfg(windows)]

use std::sync::Arc;

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::settings::{self, RecordingMode};
use crate::state::AppState;
use crate::{audio_ducking, dictionary, ocr, trigger};

pub fn run_setup(
    app: &mut tauri::App,
    state_for_shortcut: Arc<AppState>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Touch the dictionary on startup so it's created with defaults if missing.
    let _ = dictionary::load(app.handle());

    install_tray(app)?;
    place_main_window(app);
    install_trigger(app, state_for_shortcut)?;
    Ok(())
}

fn install_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let settings_item =
        MenuItem::with_id(app, "settings", "Settings...", true, None::<&str>)?;
    let open_dict =
        MenuItem::with_id(app, "open_dict", "Open dictionary", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&settings_item, &open_dict, &quit])?;
    let _tray = TrayIconBuilder::with_id("main-tray")
        .tooltip("Whispin")
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "settings" => crate::open_settings_from_tray(app),
            "open_dict" => {
                if let Some(p) = dictionary::path(app) {
                    let _ = std::process::Command::new("cmd")
                        .args(["/c", "start", "", &p.display().to_string()])
                        .spawn();
                }
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn place_main_window(app: &mut tauri::App) {
    let Some(win) = app.get_webview_window("main") else { return };
    let Ok(Some(monitor)) = win.current_monitor() else { return };
    let mon_size = monitor.size();
    let win_size = win.outer_size().unwrap_or(*mon_size);
    let x = (mon_size.width as i32 - win_size.width as i32) / 2;
    let y = 8i32;
    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
}

fn install_trigger(
    app: &mut tauri::App,
    state_for_shortcut: Arc<AppState>,
) -> Result<(), Box<dyn std::error::Error>> {
    let app_for_press = app.handle().clone();
    let app_for_release = app.handle().clone();
    let state_press = state_for_shortcut.clone();
    let state_release = state_for_shortcut;
    let initial_trigger = settings::load(app.handle())
        .trigger
        .unwrap_or_else(settings::default_trigger);

    trigger::start_listener(
        initial_trigger,
        move || on_trigger_pressed(&app_for_press, &state_press),
        move || on_trigger_released(&app_for_release, &state_release),
    )
    .map_err(|e| anyhow::anyhow!("trigger start failed: {e}"))?;
    Ok(())
}

fn on_trigger_pressed(app: &tauri::AppHandle, state: &Arc<AppState>) {
    use std::sync::atomic::Ordering;
    let mode = settings::load(app).recording.mode;
    let was_recording = state.is_recording.swap(true, Ordering::SeqCst);
    match mode {
        RecordingMode::Ptt => {
            eprintln!("[whispin] PTT pressed (mode=PTT)");
            begin_recording_side_effects(state, app);
            emit(app, "start-recording");
        }
        RecordingMode::Toggle => {
            if was_recording {
                eprintln!("[whispin] Toggle: 2nd tap → stop");
                emit(app, "stop-recording");
                // Audio restore + flag reset happen via notify_recording_stopped from JS.
            } else {
                eprintln!("[whispin] Toggle: 1st tap → start");
                begin_recording_side_effects(state, app);
                emit(app, "start-recording");
            }
        }
    }
}

fn on_trigger_released(app: &tauri::AppHandle, state: &Arc<AppState>) {
    use std::sync::atomic::Ordering;
    let mode = settings::load(app).recording.mode;
    let RecordingMode::Ptt = mode else { return };

    eprintln!("[whispin] PTT released");
    state.is_recording.store(false, Ordering::SeqCst);
    emit(app, "stop-recording");

    let state_clone = state.clone();
    std::thread::spawn(move || {
        let pids = std::mem::take(&mut *state_clone.ducked_pids.lock());
        if !pids.is_empty() {
            let started = std::time::Instant::now();
            if let Err(e) = audio_ducking::restore_sessions(&pids) {
                eprintln!("[whispin] restore failed: {e}");
            } else {
                eprintln!(
                    "[whispin] restored {} session(s) ({} ms)",
                    pids.len(),
                    started.elapsed().as_millis()
                );
            }
        }
    });
}

fn emit(app: &tauri::AppHandle, event: &str) {
    if let Err(e) = app.emit(event, ()) {
        eprintln!("[whispin] emit {event} failed: {e}");
    }
}

/// Side effects that fire once when a recording *starts*: capture HWND, kick
/// off OCR + audio ducking on background threads, show the notch window.
fn begin_recording_side_effects(state: &Arc<AppState>, app: &tauri::AppHandle) {
    let captured = capture_foreground_hwnd(state);
    spawn_duck(state);
    if let Some(hwnd_val) = captured {
        spawn_ocr(state, hwnd_val);
    }
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
    }
}

fn capture_foreground_hwnd(state: &Arc<AppState>) -> Option<isize> {
    unsafe {
        let hwnd = GetForegroundWindow();
        let val = hwnd.0 as isize;
        if val == 0 {
            return None;
        }
        *state.target_hwnd.lock() = Some(val);
        *state.ocr_text.lock() = None;
        Some(val)
    }
}

fn spawn_duck(state: &Arc<AppState>) {
    let state_clone = state.clone();
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        match audio_ducking::duck_other_sessions() {
            Ok(pids) => {
                eprintln!(
                    "[whispin] ducked {} session(s) ({} ms)",
                    pids.len(),
                    started.elapsed().as_millis()
                );
                *state_clone.ducked_pids.lock() = pids;
            }
            Err(e) => eprintln!("[whispin] duck failed: {e}"),
        }
    });
}

fn spawn_ocr(state: &Arc<AppState>, hwnd_val: isize) {
    let state_clone = state.clone();
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        match ocr::capture_and_ocr(hwnd_val) {
            Ok(raw) => {
                let cleaned = ocr::clean_text(&raw);
                eprintln!(
                    "[whispin] OCR ok ({} → {} chars, {} ms)",
                    raw.chars().count(),
                    cleaned.chars().count(),
                    started.elapsed().as_millis()
                );
                *state_clone.ocr_text.lock() = Some(cleaned);
            }
            Err(e) => eprintln!("[whispin] OCR failed: {e}"),
        }
    });
}
