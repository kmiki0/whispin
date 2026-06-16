// A transparent, click-through, always-on-top overlay window that is sized to
// the OCR target window and shows a "scanning" animation (scan.html) while the
// screen is being captured + OCR'd. Driven entirely from Rust (show/hide); the
// webview itself just runs a CSS animation.

#![cfg(windows)]

use tauri::{Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

const LABEL: &str = "scan";

/// Build the overlay window once, hidden. Safe to call again (no-op if it
/// already exists).
pub fn create(app: &tauri::AppHandle) {
    if app.get_webview_window(LABEL).is_some() {
        return;
    }
    match WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("scan.html".into()))
        .title("scan")
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .focused(false)
        .visible(false)
        .resizable(false)
        .build()
    {
        Ok(win) => {
            // Let clicks pass through to the window underneath.
            let _ = win.set_ignore_cursor_events(true);
        }
        Err(e) => eprintln!("[whispin] scan overlay build failed: {e}"),
    }
}

/// Position the overlay over the given target window (HWND as isize) and show
/// it. No-op if the overlay window or the target rect is unavailable.
pub fn show_over(app: &tauri::AppHandle, target: isize) {
    let Some(win) = app.get_webview_window(LABEL) else {
        return;
    };
    let hwnd = HWND(target as *mut core::ffi::c_void);
    // Prefer the DWM "extended frame bounds" — the actual visible window edge.
    // GetWindowRect includes the invisible resize border / shadow margins
    // (~7px) on Windows 10/11, which makes the overlay frame look slightly off.
    let mut rect = RECT::default();
    let dwm_ok = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut core::ffi::c_void,
            core::mem::size_of::<RECT>() as u32,
        )
    }
    .is_ok();
    if !dwm_ok && unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return;
    }
    let w = (rect.right - rect.left).max(1);
    let h = (rect.bottom - rect.top).max(1);
    let _ = win.set_position(PhysicalPosition::new(rect.left, rect.top));
    let _ = win.set_size(PhysicalSize::new(w as u32, h as u32));
    let _ = win.set_ignore_cursor_events(true);
    let _ = win.show();
    let _ = win.set_always_on_top(true);
}

/// Toggle the "reading" scan animation (sweep + tint + badge). The focus frame
/// stays visible regardless; only this extra layer is gated.
pub fn set_reading(app: &tauri::AppHandle, on: bool) {
    if let Some(win) = app.get_webview_window(LABEL) {
        let js = if on {
            "document.body.classList.add('reading')"
        } else {
            "document.body.classList.remove('reading')"
        };
        let _ = win.eval(js);
    }
}

pub fn hide(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window(LABEL) {
        // Clear the reading state so the next show starts with just the frame.
        let _ = win.eval("document.body.classList.remove('reading')");
        let _ = win.hide();
    }
}
