// Mute all other audio sessions on the default render device for the duration
// of a recording. Tracks which PIDs we muted so we can restore them on release.

#![cfg(windows)]

use anyhow::{anyhow, Result};
use windows::core::Interface;
use windows::Win32::Media::Audio::{
    eMultimedia, eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
    ISimpleAudioVolume, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::GetCurrentProcessId;

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
