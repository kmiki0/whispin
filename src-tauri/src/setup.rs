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

    // Start the serialized audio-duck worker before any trigger can fire.
    audio_ducking::init(state_for_shortcut.clone());

    install_tray(app)?;
    place_main_window(app);
    crate::scan_overlay::create(app.handle());
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
    let settings = settings::load(app);
    let mode = settings.recording.mode;
    let ocr_enabled = settings.llm.use_screen_context;
    let was_recording = state.is_recording.swap(true, Ordering::SeqCst);
    match mode {
        RecordingMode::Ptt => {
            eprintln!("[whispin] PTT pressed (mode=PTT)");
            begin_recording_side_effects(state, app, ocr_enabled);
            emit(app, "start-recording");
        }
        RecordingMode::Toggle => {
            if was_recording {
                eprintln!("[whispin] Toggle: 2nd tap → stop");
                emit(app, "stop-recording");
                // Audio restore + flag reset happen via notify_recording_stopped from JS.
            } else {
                eprintln!("[whispin] Toggle: 1st tap → start");
                begin_recording_side_effects(state, app, ocr_enabled);
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
    // Hand restore to the serialized audio worker. If a quick tap inverts the
    // duck/restore order, the worker's post-duck is_recording check restores
    // anyway (see audio_ducking backstops).
    audio_ducking::request_restore();
}

fn emit(app: &tauri::AppHandle, event: &str) {
    if let Err(e) = app.emit(event, ()) {
        eprintln!("[whispin] emit {event} failed: {e}");
    }
}

/// Side effects that fire once when a recording *starts*: capture HWND, kick
/// off OCR + audio ducking on background threads, show the notch window.
fn begin_recording_side_effects(state: &Arc<AppState>, app: &tauri::AppHandle, ocr_enabled: bool) {
    let captured = capture_foreground_hwnd(state);
    audio_ducking::request_duck();
    if let Some(hwnd_val) = captured {
        // Always frame the active window while recording — this is just a
        // visual marker (no screen capture), so it shows regardless of the
        // screen-context setting.
        crate::scan_overlay::show_over(app, hwnd_val);
        // Only actually read the screen (capture + OCR + the reading sweep)
        // when the user allows screen context.
        if ocr_enabled {
            spawn_ocr(state, app, hwnd_val);
        }
    }
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        // The scan overlay (corner focus markers) is also always-on-top and was
        // just raised above everything in show_over(). Re-assert topmost on the
        // notch so it sits *above* the overlay's markers instead of behind them.
        // Toggle off→on to force the z-order raise even when the flag is already
        // set (a plain set_always_on_top(true) can be a no-op). No focus is
        // stolen (SWP_NOACTIVATE), and nothing repaints until we yield the
        // message loop, so the brief non-topmost state isn't visible.
        let _ = win.set_always_on_top(false);
        let _ = win.set_always_on_top(true);
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

fn spawn_ocr(state: &Arc<AppState>, app: &tauri::AppHandle, hwnd_val: isize) {
    let state_clone = state.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        // The focus frame is already shown by the caller. Run the "reading"
        // sweep only while OCR is actually happening; the frame itself stays up
        // until recording stops (hidden in notify_recording_stopped).
        crate::scan_overlay::set_reading(&app, true);
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
        // Keep the reading sweep visible long enough to register, even if OCR
        // was near-instant.
        const MIN_READING: std::time::Duration = std::time::Duration::from_millis(900);
        let elapsed = started.elapsed();
        if elapsed < MIN_READING {
            std::thread::sleep(MIN_READING - elapsed);
        }
        crate::scan_overlay::set_reading(&app, false);
    });
}
