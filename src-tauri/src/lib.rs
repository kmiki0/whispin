// Whispin entry point. Real logic lives in the submodules:
//   - settings    : config schema + persistence + get/set commands
//   - dictionary  : proper-noun dictionary + commands
//   - startup     : Windows Run-key registration + uninstall + folder open
//   - transcribe  : ASR + LLM correction + paste pipeline
//   - state       : shared AppState held by Tauri's manage()
//   - setup       : tray icon, main-window positioning, trigger callbacks
//   - trigger     : Win32 low-level keyboard/mouse hook abstraction
//   - ocr         : Windows.Media.Ocr screen-capture pipeline
//   - audio_ducking : mute/restore other audio sessions while recording

#[cfg(windows)]
mod audio_ducking;
#[cfg(windows)]
mod crypto;
#[cfg(windows)]
mod dictionary;
#[cfg(windows)]
mod ocr;
#[cfg(windows)]
mod scan_overlay;
#[cfg(windows)]
mod settings;
#[cfg(windows)]
mod setup;
#[cfg(windows)]
mod startup;
#[cfg(windows)]
mod state;
#[cfg(windows)]
mod transcribe;
#[cfg(windows)]
mod trigger;

use std::sync::Arc;
use tauri::Manager;

#[cfg(windows)]
use crate::state::AppState;

/// JS calls this when a recording session ends (release in PTT mode, or
/// auto-stop / 2nd-tap in Toggle mode). Restores ducked audio and resets
/// the is_recording flag, since the Rust trigger handler can't know when
/// MediaRecorder actually finishes.
#[tauri::command]
#[cfg(windows)]
fn notify_recording_stopped(app: tauri::AppHandle, state: tauri::State<'_, Arc<AppState>>) {
    use std::sync::atomic::Ordering;
    // Recording ended → take down the screen-context focus overlay.
    scan_overlay::hide(&app);
    state.is_recording.store(false, Ordering::SeqCst);
    // Restore via the serialized audio worker (Toggle mode / auto-stop path).
    audio_ducking::request_restore();
}

#[tauri::command]
fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Settings-screen safety button: restore any audio this app ducked and un-mute
/// every other session, recovering from a stuck duck/mute.
#[tauri::command]
#[cfg(windows)]
fn force_restore_audio() -> Result<String, String> {
    match audio_ducking::force_restore_all() {
        Ok(n) => Ok(format!("音声を復元しました（{n} 件のミュートを解除）")),
        Err(e) => Err(e.to_string()),
    }
}

fn show_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(
        &app,
        "settings",
        tauri::WebviewUrl::App("settings.html".into()),
    )
    .title("Whispin 設定")
    .inner_size(640.0, 520.0)
    .resizable(false)
    .skip_taskbar(false)
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn open_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    show_settings_window(app)
}

/// Crate-public wrapper so setup.rs's tray handler can open the settings
/// window without going through the Tauri IPC layer.
pub fn open_settings_from_tray(app: &tauri::AppHandle) {
    if let Err(e) = show_settings_window(app.clone()) {
        eprintln!("[whispin] open settings failed: {e}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    let state = Arc::new(AppState::default());
    #[cfg(windows)]
    let state_for_shortcut = state.clone();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init());

    #[cfg(windows)]
    let builder = builder.manage(state).invoke_handler(tauri::generate_handler![
        transcribe::transcribe,
        transcribe::test_api_key,
        settings::get_trigger_config,
        settings::set_trigger_config,
        settings::get_api_keys,
        settings::set_api_keys,
        settings::get_llm_config,
        settings::set_llm_config,
        settings::get_general_config,
        settings::set_general_config,
        settings::get_recording_config,
        settings::set_recording_config,
        dictionary::get_dictionary_cmd,
        dictionary::set_dictionary_cmd,
        startup::open_settings_folder,
        startup::get_startup_enabled,
        startup::set_startup_enabled,
        startup::uninstall_app,
        notify_recording_stopped,
        get_app_version,
        force_restore_audio,
        open_settings_window,
    ]);

    #[cfg(not(windows))]
    let builder = builder.invoke_handler(tauri::generate_handler![get_app_version, open_settings_window]);

    let app = builder
        .setup(move |app| {
            #[cfg(windows)]
            {
                setup::run_setup(app, state_for_shortcut.clone())?;
            }
            #[cfg(not(windows))]
            let _ = app;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // Drive the event loop ourselves so we can un-mute any ducked audio sessions
    // when the app exits — otherwise a quit while muted (or with a Restore still
    // queued) leaves other apps, e.g. a browser, silent until the user un-mutes
    // them by hand.
    app.run(move |_app_handle, _event| {
        #[cfg(windows)]
        if let tauri::RunEvent::Exit = _event {
            audio_ducking::restore_now_blocking();
        }
    });
}
