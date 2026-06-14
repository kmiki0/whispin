// Shared per-app runtime state, held by Tauri's `manage()` and accessed from
// commands and the trigger callback. Cheap to clone (we wrap in Arc).

use parking_lot::Mutex;
use std::sync::atomic::AtomicBool;

#[derive(Default)]
pub struct AppState {
    /// HWND of the window that was focused when the trigger fired. Restored
    /// before paste so we land in the right field.
    pub target_hwnd: Mutex<Option<isize>>,
    /// OCR result for the same window, captured on press and consumed when
    /// the LLM correction prompt is built.
    pub ocr_text: Mutex<Option<String>>,
    /// PIDs whose audio sessions we muted while recording, so we can un-mute
    /// them on release / auto-stop.
    pub ducked_pids: Mutex<Vec<u32>>,
    /// True while a recording session is active. Used by the trigger handler
    /// to distinguish 1st-tap vs 2nd-tap in Toggle mode.
    pub is_recording: AtomicBool,
}
