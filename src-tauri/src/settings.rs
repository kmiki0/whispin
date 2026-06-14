// Persistent app settings: the user's chosen trigger, API keys, LLM behavior,
// recording mode, mic preference, etc. All sections live in one settings.json
// under the OS app-config dir.

#![cfg(windows)]

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::trigger;

pub const DEFAULT_CORRECTION_MODEL: &str = "meta-llama/llama-3.3-70b-instruct";
pub const CORRECTION_TIMEOUT_SECS: u64 = 8;
pub const SHORT_TEXT_THRESHOLD_CHARS: usize = 20;

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct AppSettings {
    #[serde(default)]
    pub trigger: Option<trigger::TriggerConfig>,
    #[serde(default)]
    pub api_keys: ApiKeys,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub recording: RecordingConfig,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct ApiKeys {
    #[serde(default)]
    pub groq: String,
    #[serde(default)]
    pub openai: String,
    #[serde(default)]
    pub openrouter: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LlmConfig {
    pub enabled: bool,
    pub model: String,
    pub short_threshold_chars: usize,
    pub timeout_secs: u64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model: DEFAULT_CORRECTION_MODEL.to_string(),
            short_threshold_chars: SHORT_TEXT_THRESHOLD_CHARS,
            timeout_secs: CORRECTION_TIMEOUT_SECS,
        }
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct GeneralConfig {
    #[serde(default)]
    pub mic_device_id: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RecordingMode {
    Ptt,
    Toggle,
}

impl Default for RecordingMode {
    fn default() -> Self {
        RecordingMode::Ptt
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RecordingConfig {
    pub mode: RecordingMode,
    pub auto_stop_on_silence: bool,
    pub silence_timeout_ms: u32,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            mode: RecordingMode::Ptt,
            auto_stop_on_silence: true,
            silence_timeout_ms: 1500,
        }
    }
}

pub fn default_trigger() -> trigger::TriggerConfig {
    trigger::TriggerConfig {
        input: trigger::TriggerInput::Mouse {
            button: trigger::MouseButton::Right,
        },
        modifiers: trigger::Modifiers::NONE,
        long_press_ms: 250,
    }
}

pub fn settings_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("settings.json"))
}

/// API keys are stored on disk encrypted with Windows DPAPI but held in memory
/// as plaintext. These helpers convert at the load/save boundary so the rest of
/// the app never sees ciphertext.
fn decrypt_keys(keys: &mut ApiKeys) {
    keys.groq = crate::crypto::unprotect(&keys.groq);
    keys.openai = crate::crypto::unprotect(&keys.openai);
    keys.openrouter = crate::crypto::unprotect(&keys.openrouter);
}

fn encrypt_keys(keys: &mut ApiKeys) {
    keys.groq = crate::crypto::protect(&keys.groq);
    keys.openai = crate::crypto::protect(&keys.openai);
    keys.openrouter = crate::crypto::protect(&keys.openrouter);
}

pub fn load(app: &tauri::AppHandle) -> AppSettings {
    let Some(path) = settings_path(app) else {
        return AppSettings::default();
    };
    if !path.exists() {
        return AppSettings::default();
    }
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let mut settings: AppSettings = serde_json::from_str(&s).unwrap_or_else(|e| {
                eprintln!("[whispin] settings parse failed: {e}");
                AppSettings::default()
            });
            decrypt_keys(&mut settings.api_keys);
            settings
        }
        Err(e) => {
            eprintln!("[whispin] settings read failed: {e}");
            AppSettings::default()
        }
    }
}

pub fn save(app: &tauri::AppHandle, settings: &AppSettings) -> Result<(), String> {
    let path = settings_path(app).ok_or_else(|| "app_config_dir unavailable".to_string())?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Encrypt keys on a clone so the caller's in-memory copy stays plaintext.
    let mut to_store = settings.clone();
    encrypt_keys(&mut to_store.api_keys);
    let json = serde_json::to_string_pretty(&to_store).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

// ---- Tauri commands ----

#[tauri::command]
pub fn get_trigger_config() -> Result<trigger::TriggerConfig, String> {
    trigger::current_config().ok_or_else(|| "trigger not started".to_string())
}

#[tauri::command]
pub fn set_trigger_config(
    app: tauri::AppHandle,
    config: trigger::TriggerConfig,
) -> Result<(), String> {
    trigger::set_config(config)?;
    let mut settings = load(&app);
    settings.trigger = Some(config);
    save(&app, &settings)?;
    eprintln!("[whispin] trigger config updated: {:?}", config);
    Ok(())
}

#[tauri::command]
pub fn get_api_keys(app: tauri::AppHandle) -> ApiKeys {
    load(&app).api_keys
}

#[tauri::command]
pub fn set_api_keys(app: tauri::AppHandle, keys: ApiKeys) -> Result<(), String> {
    let mut settings = load(&app);
    settings.api_keys = keys;
    save(&app, &settings)
}

#[tauri::command]
pub fn get_llm_config(app: tauri::AppHandle) -> LlmConfig {
    load(&app).llm
}

#[tauri::command]
pub fn set_llm_config(app: tauri::AppHandle, config: LlmConfig) -> Result<(), String> {
    let mut settings = load(&app);
    settings.llm = config;
    save(&app, &settings)
}

#[tauri::command]
pub fn get_general_config(app: tauri::AppHandle) -> GeneralConfig {
    load(&app).general
}

#[tauri::command]
pub fn set_general_config(
    app: tauri::AppHandle,
    config: GeneralConfig,
) -> Result<(), String> {
    let mut settings = load(&app);
    settings.general = config;
    save(&app, &settings)
}

#[tauri::command]
pub fn get_recording_config(app: tauri::AppHandle) -> RecordingConfig {
    load(&app).recording
}

#[tauri::command]
pub fn set_recording_config(
    app: tauri::AppHandle,
    config: RecordingConfig,
) -> Result<(), String> {
    let mut settings = load(&app);
    settings.recording = config;
    save(&app, &settings)
}
