// PTT (push-to-talk) trigger abstraction backed by Win32 low-level keyboard
// and mouse hooks. Lets a future settings UI pick any key OR any mouse button,
// any modifier combination, and any long-press threshold.
//
// For mouse-button triggers, short clicks (released before the long-press
// threshold) are forwarded to the OS as a normal click via SendInput, so
// the app under the cursor still gets its usual behavior (e.g. context menu).

#![cfg(windows)]

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYEVENTF_KEYUP, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_XDOWN,
    MOUSEEVENTF_XUP, MOUSEINPUT, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

const XBUTTON1: u16 = 0x0001;
const XBUTTON2: u16 = 0x0002;
/// Bit set on KBDLLHOOKSTRUCT.flags when an event was injected by SendInput.
const LLKHF_INJECTED: u32 = 0x10;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, WH_KEYBOARD_LL,
    WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

const SYNTHETIC_MAGIC: usize = 0x5748_4953_5049_4E31; // "WHISPIN1"
const LLMHF_INJECTED: u32 = 0x00000001;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TriggerInput {
    Key { vk: u32 },
    Mouse { button: MouseButton },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        ctrl: false,
        shift: false,
        alt: false,
        win: false,
    };

    pub fn is_none(&self) -> bool {
        *self == Modifiers::NONE
    }
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct TriggerConfig {
    pub input: TriggerInput,
    pub modifiers: Modifiers,
    /// 0 = immediate, >0 = require N ms held before firing the pressed callback.
    pub long_press_ms: u32,
}

type Callback = Box<dyn Fn() + Send + Sync + 'static>;

struct State {
    config: parking_lot::Mutex<TriggerConfig>,
    on_pressed: Callback,
    on_released: Callback,
    pressed_at: AtomicI64, // ms-since-epoch; 0 = not pressed
    is_active_ptt: AtomicBool,
}

static STATE: OnceLock<State> = OnceLock::new();

/// Replace the active trigger configuration. Takes effect on the next input
/// event. Returns Err if the listener hasn't been started yet.
pub fn set_config(config: TriggerConfig) -> Result<(), String> {
    let state = STATE.get().ok_or("trigger listener not started")?;
    *state.config.lock() = config;
    state.pressed_at.store(0, Ordering::SeqCst);
    state.is_active_ptt.store(false, Ordering::SeqCst);
    Ok(())
}

pub fn current_config() -> Option<TriggerConfig> {
    STATE.get().map(|s| *s.config.lock())
}

pub fn start_listener<P, R>(
    config: TriggerConfig,
    on_pressed: P,
    on_released: R,
) -> Result<(), String>
where
    P: Fn() + Send + Sync + 'static,
    R: Fn() + Send + Sync + 'static,
{
    let state = State {
        config: parking_lot::Mutex::new(config),
        on_pressed: Box::new(on_pressed),
        on_released: Box::new(on_released),
        pressed_at: AtomicI64::new(0),
        is_active_ptt: AtomicBool::new(false),
    };
    STATE
        .set(state)
        .map_err(|_| "trigger listener already started".to_string())?;

    std::thread::Builder::new()
        .name("whispin-trigger-hook".into())
        .spawn(|| unsafe { run_hook_thread() })
        .map_err(|e| format!("spawn hook thread failed: {e}"))?;
    Ok(())
}

unsafe fn run_hook_thread() {
    let module = match GetModuleHandleW(None) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[trigger] GetModuleHandleW failed: {e}");
            return;
        }
    };
    let kb_hook: HHOOK =
        match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), module, 0) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[trigger] SetWindowsHookExW (keyboard) failed: {e}");
                return;
            }
        };
    let mouse_hook: HHOOK =
        match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), module, 0) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[trigger] SetWindowsHookExW (mouse) failed: {e}");
                let _ = UnhookWindowsHookEx(kb_hook);
                return;
            }
        };

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    let _ = UnhookWindowsHookEx(mouse_hook);
    let _ = UnhookWindowsHookEx(kb_hook);
}

unsafe extern "system" fn keyboard_hook_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code < 0 {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }
    let Some(state) = STATE.get() else {
        return CallNextHookEx(None, n_code, w_param, l_param);
    };

    let cfg = *state.config.lock();
    let target_vk = match cfg.input {
        TriggerInput::Key { vk } => vk,
        _ => return CallNextHookEx(None, n_code, w_param, l_param),
    };

    let kb = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
    // Always pass through injected events (our own synthesize_keypress or
    // anyone else's SendInput). Otherwise short-press synthesis would feed
    // itself into the hook in an infinite loop.
    if (kb.flags.0 & LLKHF_INJECTED) != 0 || kb.dwExtraInfo == SYNTHETIC_MAGIC {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    let vk = kb.vkCode;
    let msg = w_param.0 as u32;
    let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

    if vk != target_vk || (!is_down && !is_up) {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }
    if !cfg.modifiers.is_none() {
        let current = current_modifier_state();
        if !modifiers_match(cfg.modifiers, current) {
            return CallNextHookEx(None, n_code, w_param, l_param);
        }
    }

    if is_down {
        handle_press();
        return LRESULT(1);
    }
    // release
    let was_pressed = state.pressed_at.swap(0, Ordering::SeqCst) != 0;
    if !was_pressed {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }
    let was_active = state.is_active_ptt.swap(false, Ordering::SeqCst);
    // Defer the heavy work so the hook proc returns immediately. A short
    // press (was_pressed but not active) re-injects the keypress so the
    // focused window still receives it — same idea as the mouse path.
    std::thread::spawn(move || {
        if was_active {
            if let Some(s) = STATE.get() {
                (s.on_released)();
            }
        } else {
            unsafe { synthesize_keypress(target_vk) };
        }
    });
    LRESULT(1)
}

unsafe extern "system" fn mouse_hook_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code < 0 {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }
    let Some(state) = STATE.get() else {
        return CallNextHookEx(None, n_code, w_param, l_param);
    };
    let cfg = *state.config.lock();
    let TriggerInput::Mouse { button: target } = cfg.input else {
        return CallNextHookEx(None, n_code, w_param, l_param);
    };

    let ms = &*(l_param.0 as *const MSLLHOOKSTRUCT);
    // Always pass through any injected input (our SendInput synthesis or
    // anything else). Belt+suspenders: check both LLMHF_INJECTED and our magic
    // in dwExtraInfo. Failing to detect injected events leads to a feedback
    // loop where the hook re-triggers itself.
    if (ms.flags & LLMHF_INJECTED) != 0 || ms.dwExtraInfo == SYNTHETIC_MAGIC {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    let msg = w_param.0 as u32;
    let (event_button, is_down) = match msg {
        WM_LBUTTONDOWN => (Some(MouseButton::Left), true),
        WM_LBUTTONUP => (Some(MouseButton::Left), false),
        WM_RBUTTONDOWN => (Some(MouseButton::Right), true),
        WM_RBUTTONUP => (Some(MouseButton::Right), false),
        WM_MBUTTONDOWN => (Some(MouseButton::Middle), true),
        WM_MBUTTONUP => (Some(MouseButton::Middle), false),
        WM_XBUTTONDOWN => (Some(x_button_from_mouse_data(ms.mouseData)), true),
        WM_XBUTTONUP => (Some(x_button_from_mouse_data(ms.mouseData)), false),
        _ => (None, false),
    };
    let Some(b) = event_button else {
        return CallNextHookEx(None, n_code, w_param, l_param);
    };
    if b != target {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }
    if !cfg.modifiers.is_none() {
        let current = current_modifier_state();
        if !modifiers_match(cfg.modifiers, current) {
            return CallNextHookEx(None, n_code, w_param, l_param);
        }
    }

    if is_down {
        handle_press();
        return LRESULT(1); // suppress; we'll synthesize on short release if needed
    }
    // release
    let was_pressed = state.pressed_at.swap(0, Ordering::SeqCst) != 0;
    if !was_pressed {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }
    let was_active = state.is_active_ptt.swap(false, Ordering::SeqCst);
    // Defer the heavy work so the hook proc returns immediately.
    std::thread::spawn(move || {
        if was_active {
            if let Some(s) = STATE.get() {
                (s.on_released)();
            }
        } else {
            unsafe { synthesize_click(target) };
        }
    });
    LRESULT(1)
}

fn handle_press() {
    let Some(state) = STATE.get() else { return };
    let already = state.pressed_at.load(Ordering::SeqCst) != 0;
    if already {
        return;
    }
    let now = current_millis();
    state.pressed_at.store(now, Ordering::SeqCst);

    let long_press_ms = state.config.lock().long_press_ms;
    if long_press_ms == 0 {
        state.is_active_ptt.store(true, Ordering::SeqCst);
        // Dispatch on a worker thread so the low-level hook proc stays
        // under Windows' ~300ms hook timeout (after which the OS silently
        // removes the hook).
        std::thread::spawn(|| {
            if let Some(s) = STATE.get() {
                (s.on_pressed)();
            }
        });
    } else {
        let threshold = long_press_ms as u64;
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(threshold));
            let Some(s) = STATE.get() else { return };
            if s.pressed_at.load(Ordering::SeqCst) == now
                && !s.is_active_ptt.load(Ordering::SeqCst)
            {
                s.is_active_ptt.store(true, Ordering::SeqCst);
                (s.on_pressed)();
            }
        });
    }
}

fn x_button_from_mouse_data(mouse_data: u32) -> MouseButton {
    // High word of mouseData carries the XBUTTONx code.
    let hi = (mouse_data >> 16) as u16;
    if hi == XBUTTON2 {
        MouseButton::X2
    } else {
        MouseButton::X1
    }
}

unsafe fn synthesize_keypress(vk: u32) {
    let inputs = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk as u16),
                    wScan: 0,
                    dwFlags: Default::default(), // keydown
                    time: 0,
                    dwExtraInfo: SYNTHETIC_MAGIC,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk as u16),
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: SYNTHETIC_MAGIC,
                },
            },
        },
    ];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
}

unsafe fn synthesize_click(button: MouseButton) {
    let (down_flag, up_flag, x_code) = match button {
        MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, 0u32),
        MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, 0u32),
        MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, 0u32),
        MouseButton::X1 => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, XBUTTON1 as u32),
        MouseButton::X2 => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, XBUTTON2 as u32),
    };

    let inputs = [
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: x_code,
                    dwFlags: down_flag,
                    time: 0,
                    dwExtraInfo: SYNTHETIC_MAGIC,
                },
            },
        },
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: x_code,
                    dwFlags: up_flag,
                    time: 0,
                    dwExtraInfo: SYNTHETIC_MAGIC,
                },
            },
        },
    ];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
}

fn current_modifier_state() -> Modifiers {
    unsafe {
        Modifiers {
            ctrl: key_down(VK_CONTROL.0 as i32),
            shift: key_down(VK_SHIFT.0 as i32),
            alt: key_down(VK_MENU.0 as i32),
            win: key_down(VK_LWIN.0 as i32) || key_down(VK_RWIN.0 as i32),
        }
    }
}

unsafe fn key_down(vk: i32) -> bool {
    (GetAsyncKeyState(vk) as u16 & 0x8000) != 0
}

fn modifiers_match(required: Modifiers, current: Modifiers) -> bool {
    required.ctrl == current.ctrl
        && required.shift == current.shift
        && required.alt == current.alt
        && required.win == current.win
}

fn current_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
