// Duck (lower the volume of) all other audio sessions on the default render
// device for the duration of a recording, then restore each session's original
// volume on release. We attenuate volume rather than hard-muting: a stuck mute
// leaves an app silent until the user un-mutes it by hand, whereas a missed
// volume restore is both rarer to leave audible damage and never touches the
// per-app mute flag the user controls.

#![cfg(windows)]

/// Fraction of its own volume each ducked session is lowered to while recording
/// (0.2 = 20%). Multiplicative, so a session is only ever lowered, never raised.
const DUCK_LEVEL: f32 = 0.2;

use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, OnceLock};

use anyhow::{anyhow, Result};
use parking_lot::Mutex;

use crate::state::AppState;
use windows::core::Interface;
use windows::Win32::Media::Audio::{
    eMultimedia, eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
    ISimpleAudioVolume, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::GetCurrentProcessId;

// ---------------------------------------------------------------------------
// Serialized audio-duck worker.
//
// Ducking (mute others) on press and restoring on release used to run on two
// independent spawned threads sharing `AppState.ducked_pids`. A quick tap could
// run the restore's `take` *before* the duck thread stored its PIDs, so restore
// saw an empty list, then the duck thread muted everything and stored the PIDs
// with nobody left to un-mute them — other apps stayed muted forever.
//
// Both operations now go through a single worker thread fed by a FIFO channel.
// Press enqueues Duck, release/stop enqueues Restore. The worker owns the
// muted-PID list (no shared-state race on it).
//
// FIFO alone is NOT enough: Duck and Restore are enqueued from two different
// callback threads (on_trigger_pressed / on_trigger_released), so a very quick
// tap can invert their order and a Restore can be dequeued before its Duck.
// Two backstops close that gap:
//   1. After ducking, the worker re-checks `is_recording`. If the recording has
//      already ended (the Restore was processed first, or finished mid-duck),
//      it un-mutes immediately — so an inverted pair self-heals.
//   2. Every Duck first restores any still-muted leftovers from a previous
//      cycle, so even a fully-dropped Restore can't accumulate a permanent mute
//      (it's cleaned up on the next recording).
// A fully order-independent fix would assign the duck/restore pair an id on the
// (physically-ordered) input-hook thread; that's a larger change left as a
// follow-up. In practice (1)+(2) make a stuck mute unreachable for any normal
// tap timing.
//
// The worker's muted-PID list lives behind `MUTED` (Arc<Mutex<..>>) rather than
// a thread-local, so an app-exit handler can synchronously un-mute on the way
// out (see `restore_now_blocking`). The worker is still the only thing that
// mutes, and restore is idempotent, so the exit path racing the worker is safe.
// ---------------------------------------------------------------------------

enum AudioCmd {
    Duck,
    Restore,
}

static AUDIO_TX: OnceLock<Mutex<Sender<AudioCmd>>> = OnceLock::new();
/// Sessions we have ducked and not yet restored, as (pid, original_volume).
/// Shared so the exit handler can flush it synchronously; the duck worker is
/// otherwise the sole writer.
static MUTED: OnceLock<Arc<Mutex<Vec<(u32, f32)>>>> = OnceLock::new();

/// Start the audio-duck worker thread. Idempotent — a second call is a no-op.
/// `state` is read to tell whether a recording is still active when a duck
/// completes (see backstop 1 above).
pub fn init(state: Arc<AppState>) {
    let (tx, rx) = channel::<AudioCmd>();
    if AUDIO_TX.set(Mutex::new(tx)).is_err() {
        return; // already started
    }
    // Shared muted-PID list (see MUTED docs). Clone for the worker; the original
    // stays in the static for the exit handler.
    let muted: Arc<Mutex<Vec<(u32, f32)>>> = Arc::new(Mutex::new(Vec::new()));
    let _ = MUTED.set(muted.clone());
    let spawned = std::thread::Builder::new()
        .name("whispin-audio-duck".into())
        .spawn(move || {
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    AudioCmd::Duck => {
                        let mut ducked = muted.lock();
                        // Backstop 2: un-mute any leftovers from a prior cycle
                        // whose Restore was lost, before ducking again.
                        if !ducked.is_empty() {
                            let _ = restore_sessions(&ducked);
                            ducked.clear();
                        }
                        match duck_other_sessions() {
                            Ok(pids) => {
                                eprintln!("[whispin] ducked {} session(s)", pids.len());
                                *ducked = pids;
                            }
                            Err(e) => eprintln!("[whispin] duck failed: {e}"),
                        }
                        // Backstop 1: if the recording already ended (a quick
                        // tap whose Restore raced ahead of this Duck), restore
                        // right away instead of leaving other apps muted.
                        if !ducked.is_empty() && !state.is_recording.load(Ordering::SeqCst) {
                            let n = ducked.len();
                            match restore_sessions(&ducked) {
                                Ok(()) => eprintln!(
                                    "[whispin] restored {n} session(s) (recording already ended)"
                                ),
                                Err(e) => eprintln!("[whispin] restore failed: {e}"),
                            }
                            ducked.clear();
                        }
                    }
                    AudioCmd::Restore => {
                        let mut ducked = muted.lock();
                        if ducked.is_empty() {
                            continue;
                        }
                        let n = ducked.len();
                        match restore_sessions(&ducked) {
                            Ok(()) => eprintln!("[whispin] restored {n} session(s)"),
                            Err(e) => eprintln!("[whispin] restore failed: {e}"),
                        }
                        ducked.clear();
                    }
                }
            }
        });
    if let Err(e) = spawned {
        eprintln!("[whispin] audio-duck worker spawn failed: {e}");
    }
}

/// Synchronously restore every session we ducked, right now, on the calling
/// thread. Intended for the app-exit path: the channel-fed worker may never be
/// scheduled again during shutdown, so a queued Restore can be lost and leave
/// other apps (e.g. a browser) quiet for good. Restore is idempotent, so this
/// racing the worker is harmless.
pub fn restore_now_blocking() {
    let Some(muted) = MUTED.get() else { return };
    let mut ducked = muted.lock();
    if ducked.is_empty() {
        return;
    }
    let n = ducked.len();
    match restore_sessions(&ducked) {
        Ok(()) => eprintln!("[whispin] restored {n} session(s) on exit"),
        Err(e) => eprintln!("[whispin] exit restore failed: {e}"),
    }
    ducked.clear();
}

/// Emergency "give me my audio back", used by the settings safety button.
/// Restores any sessions this app ducked (to their captured volume) AND un-mutes
/// every other session on the default render endpoint — a blanket net that also
/// recovers stuck mutes left by older hard-mute builds or lost-restore edge
/// cases. Only the mute flag is cleared on untracked sessions; their volume is
/// left as-is (we never captured an original to restore). Returns how many
/// sessions it un-muted.
pub fn force_restore_all() -> Result<usize> {
    // Precisely restore volumes for sessions we ducked this run.
    restore_now_blocking();

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| anyhow!("CoCreateInstance(MMDeviceEnumerator): {e}"))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|e| anyhow!("GetDefaultAudioEndpoint: {e}"))?;
        let session_manager: IAudioSessionManager2 = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| anyhow!("device.Activate(IAudioSessionManager2): {e}"))?;
        let sessions = session_manager
            .GetSessionEnumerator()
            .map_err(|e| anyhow!("GetSessionEnumerator: {e}"))?;

        let our_pid = GetCurrentProcessId();
        let count = sessions.GetCount().map_err(|e| anyhow!("GetCount: {e}"))?;
        let mut unmuted = 0usize;
        for i in 0..count {
            let Ok(session_control) = sessions.GetSession(i) else {
                continue;
            };
            let Ok(session2) = session_control.cast::<IAudioSessionControl2>() else {
                continue;
            };
            let pid = match session2.GetProcessId() {
                Ok(p) => p,
                Err(_) => continue,
            };
            if pid == our_pid {
                continue;
            }
            let Ok(volume) = session_control.cast::<ISimpleAudioVolume>() else {
                continue;
            };
            let is_muted = matches!(volume.GetMute(), Ok(b) if b.as_bool());
            if is_muted && volume.SetMute(false, std::ptr::null()).is_ok() {
                unmuted += 1;
            }
        }
        eprintln!("[whispin] force-restore: un-muted {unmuted} session(s)");
        Ok(unmuted)
    }
}

/// Mute other audio sessions for the current recording (enqueued; runs on the
/// worker). No-op if the worker hasn't been started.
pub fn request_duck() {
    if let Some(tx) = AUDIO_TX.get() {
        let _ = tx.lock().send(AudioCmd::Duck);
    }
}

/// Restore the sessions muted for the current recording (enqueued; always runs
/// after this recording's duck). No-op if the worker hasn't been started.
pub fn request_restore() {
    if let Some(tx) = AUDIO_TX.get() {
        let _ = tx.lock().send(AudioCmd::Restore);
    }
}

/// Lower the volume of every audio session on the default render endpoint
/// except our own process to `DUCK_LEVEL` of its current level. Returns
/// (pid, original_volume) for each session we lowered so it can be restored.
/// Sessions already at (near) zero are left alone, so restore never raises a
/// session above where the user had it.
pub fn duck_other_sessions() -> Result<Vec<(u32, f32)>> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| anyhow!("CoCreateInstance(MMDeviceEnumerator): {e}"))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|e| anyhow!("GetDefaultAudioEndpoint: {e}"))?;
        let session_manager: IAudioSessionManager2 = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| anyhow!("device.Activate(IAudioSessionManager2): {e}"))?;
        let sessions = session_manager
            .GetSessionEnumerator()
            .map_err(|e| anyhow!("GetSessionEnumerator: {e}"))?;

        let our_pid = GetCurrentProcessId();
        let count = sessions.GetCount().map_err(|e| anyhow!("GetCount: {e}"))?;
        let mut ducked = Vec::new();

        for i in 0..count {
            let Ok(session_control) = sessions.GetSession(i) else {
                continue;
            };
            let Ok(session2) = session_control.cast::<IAudioSessionControl2>() else {
                continue;
            };
            let pid = match session2.GetProcessId() {
                Ok(p) => p,
                Err(_) => continue,
            };
            if pid == 0 || pid == our_pid {
                continue;
            }
            let Ok(volume) = session_control.cast::<ISimpleAudioVolume>() else {
                continue;
            };
            let original = match volume.GetMasterVolume() {
                Ok(v) => v,
                Err(_) => continue,
            };
            // Already silent → nothing to duck, and recording it would let a
            // later restore bump it up.
            if original <= 0.0001 {
                continue;
            }
            if volume
                .SetMasterVolume(original * DUCK_LEVEL, std::ptr::null())
                .is_ok()
            {
                ducked.push((pid, original));
            }
        }
        Ok(ducked)
    }
}

/// Restore each session whose PID matches one in `targets` to the original
/// volume captured at duck time. We only set volume (never the mute flag), so
/// a user's manual mute is preserved.
pub fn restore_sessions(targets: &[(u32, f32)]) -> Result<()> {
    if targets.is_empty() {
        return Ok(());
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| anyhow!("CoCreateInstance(MMDeviceEnumerator): {e}"))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|e| anyhow!("GetDefaultAudioEndpoint: {e}"))?;
        let session_manager: IAudioSessionManager2 = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| anyhow!("device.Activate(IAudioSessionManager2): {e}"))?;
        let sessions = session_manager
            .GetSessionEnumerator()
            .map_err(|e| anyhow!("GetSessionEnumerator: {e}"))?;

        let count = sessions.GetCount().map_err(|e| anyhow!("GetCount: {e}"))?;
        for i in 0..count {
            let Ok(session_control) = sessions.GetSession(i) else {
                continue;
            };
            let Ok(session2) = session_control.cast::<IAudioSessionControl2>() else {
                continue;
            };
            let pid = match session2.GetProcessId() {
                Ok(p) => p,
                Err(_) => continue,
            };
            let Some(&(_, original)) = targets.iter().find(|(p, _)| *p == pid) else {
                continue;
            };
            let Ok(volume) = session_control.cast::<ISimpleAudioVolume>() else {
                continue;
            };
            let _ = volume.SetMasterVolume(original, std::ptr::null());
        }
    }
    Ok(())
}
