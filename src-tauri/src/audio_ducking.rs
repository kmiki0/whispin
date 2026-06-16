// Mute all other audio sessions on the default render device for the duration
// of a recording. Tracks which PIDs we muted so we can restore them on release.

#![cfg(windows)]

use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;

use anyhow::{anyhow, Result};
use parking_lot::Mutex;
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
// Press enqueues Duck, release/stop enqueues Restore; FIFO guarantees a
// Restore always runs *after* the Duck enqueued before it. The worker owns the
// muted-PID list (no shared-state race), and every Duck first restores any
// still-muted leftovers from a previous cycle that never got its Restore — so a
// dropped Restore can no longer leak a permanent mute.
// ---------------------------------------------------------------------------

enum AudioCmd {
    Duck,
    Restore,
}

static AUDIO_TX: OnceLock<Mutex<Sender<AudioCmd>>> = OnceLock::new();

/// Start the audio-duck worker thread. Idempotent — a second call is a no-op.
pub fn init() {
    let (tx, rx) = channel::<AudioCmd>();
    if AUDIO_TX.set(Mutex::new(tx)).is_err() {
        return; // already started
    }
    let spawned = std::thread::Builder::new()
        .name("whispin-audio-duck".into())
        .spawn(move || {
            // PIDs this worker has muted and not yet restored. Owned solely by
            // this thread, so duck/restore can never race on it.
            let mut ducked: Vec<u32> = Vec::new();
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    AudioCmd::Duck => {
                        // Safety net: if a prior cycle's Restore was lost, those
                        // sessions are still muted — un-mute them before ducking
                        // again so we never accumulate a permanent mute.
                        if !ducked.is_empty() {
                            let _ = restore_sessions(&ducked);
                            ducked.clear();
                        }
                        match duck_other_sessions() {
                            Ok(pids) => {
                                eprintln!("[whispin] ducked {} session(s)", pids.len());
                                ducked = pids;
                            }
                            Err(e) => eprintln!("[whispin] duck failed: {e}"),
                        }
                    }
                    AudioCmd::Restore => {
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

/// Mute every audio session on the default render endpoint except our own
/// process. Returns the PIDs of sessions we muted so they can be restored.
/// Sessions that were already muted are ignored (so we don't un-mute them
/// later on accident).
pub fn duck_other_sessions() -> Result<Vec<u32>> {
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
        let mut muted_pids = Vec::new();

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
            let was_muted = match volume.GetMute() {
                Ok(b) => b.as_bool(),
                Err(_) => continue,
            };
            if was_muted {
                continue;
            }
            if volume.SetMute(true, std::ptr::null()).is_ok() {
                muted_pids.push(pid);
            }
        }
        Ok(muted_pids)
    }
}

/// Un-mute every session whose PID matches one in `target_pids`.
pub fn restore_sessions(target_pids: &[u32]) -> Result<()> {
    if target_pids.is_empty() {
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
            if !target_pids.contains(&pid) {
                continue;
            }
            let Ok(volume) = session_control.cast::<ISimpleAudioVolume>() else {
                continue;
            };
            let _ = volume.SetMute(false, std::ptr::null());
        }
    }
    Ok(())
}
