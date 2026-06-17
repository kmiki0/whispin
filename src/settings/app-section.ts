// "App general" section: startup-at-login, mic device picker, version display,
// open-config-folder, uninstall.

import { invoke } from "@tauri-apps/api/core";
import { $, flash } from "./dom";

let appStartup: HTMLInputElement;
let appMic: HTMLSelectElement;
let appVersionEl: HTMLSpanElement;

// Live mic-test state, so the test can be toggled off and cleaned up.
let micTestBtn: HTMLButtonElement;
let micMeterFill: HTMLDivElement;
let micStream: MediaStream | null = null;
let micCtx: AudioContext | null = null;
let micRaf = 0;
let micAutoStop = 0;
// The user's intended mic, remembered across device-list refreshes so an
// unplug→replug restores the selection instead of dropping to "既定".
let desiredMicId = "";

export function initAppSection() {
  appStartup = $<HTMLInputElement>("#app-startup");
  appMic = $<HTMLSelectElement>("#app-mic");
  appVersionEl = $<HTMLSpanElement>("#app-version");
  micTestBtn = $<HTMLButtonElement>("#mic-test");
  micMeterFill = $<HTMLDivElement>("#mic-meter-fill");
  const appOpenFolder = $<HTMLButtonElement>("#app-open-folder");
  const appRestoreAudio = $<HTMLButtonElement>("#app-restore-audio");
  const appUninstall = $<HTMLButtonElement>("#app-uninstall");

  micTestBtn.addEventListener("click", () =>
    micStream ? stopMicTest() : startMicTest(),
  );
  // Re-running the test with a different device requires a restart of the stream.
  appMic.addEventListener("change", () => {
    desiredMicId = appMic.value;
    if (micStream) stopMicTest();
  });
  // Keep the list live: refresh when a mic is plugged in / removed while the
  // settings window is open (labels are already granted, so no re-prompt).
  navigator.mediaDevices.addEventListener("devicechange", () => {
    fillMicOptions(desiredMicId);
  });

  appOpenFolder.addEventListener("click", async () => {
    try {
      await invoke("open_settings_folder");
    } catch (e) {
      flash(`フォルダを開けません: ${e}`, true);
    }
  });

  appRestoreAudio.addEventListener("click", async () => {
    try {
      const msg = await invoke<string>("force_restore_audio");
      flash(typeof msg === "string" && msg ? msg : "音声を復元しました");
    } catch (e) {
      flash(`音声の復元に失敗: ${e}`, true);
    }
  });

  appUninstall.addEventListener("click", async () => {
    const ok = window.confirm(
      "本当にアンインストールしますか?\n\n設定 (settings.json) と辞書 (dictionary.json) を削除し、スタートアップ登録を解除してアプリを終了します。元に戻せません。",
    );
    if (!ok) return;
    try {
      await invoke("uninstall_app");
    } catch (e) {
      flash(`アンインストール失敗: ${e}`, true);
    }
  });
}

async function startMicTest() {
  const deviceId = appMic.value;
  try {
    micStream = await navigator.mediaDevices.getUserMedia({
      audio: deviceId ? { deviceId: { exact: deviceId } } : true,
    });
  } catch (e) {
    flash(`マイクを開けません: ${e}`, true);
    return;
  }
  micCtx = new AudioContext();
  const src = micCtx.createMediaStreamSource(micStream);
  const analyser = micCtx.createAnalyser();
  analyser.fftSize = 512;
  src.connect(analyser);
  const buf = new Uint8Array(analyser.fftSize);

  micTestBtn.textContent = "停止";
  const tick = () => {
    analyser.getByteTimeDomainData(buf);
    // RMS around the 128 midpoint → 0..1 level.
    let sum = 0;
    for (const v of buf) {
      const d = (v - 128) / 128;
      sum += d * d;
    }
    const rms = Math.sqrt(sum / buf.length);
    const pct = Math.min(100, Math.round(rms * 250));
    micMeterFill.style.width = `${pct}%`;
    micRaf = requestAnimationFrame(tick);
  };
  tick();
  // Don't hold the mic open indefinitely.
  micAutoStop = window.setTimeout(stopMicTest, 15000);
}

function stopMicTest() {
  if (micRaf) cancelAnimationFrame(micRaf);
  if (micAutoStop) window.clearTimeout(micAutoStop);
  micRaf = 0;
  micAutoStop = 0;
  micStream?.getTracks().forEach((t) => t.stop());
  micStream = null;
  micCtx?.close().catch(() => {});
  micCtx = null;
  micMeterFill.style.width = "0%";
  micTestBtn.textContent = "入力テスト";
}

export function applyStartupEnabled(enabled: boolean) {
  appStartup.checked = enabled;
}

export function readStartupEnabled(): boolean {
  return appStartup.checked;
}

export function readMicDeviceId(): string {
  return appMic.value;
}

/// Set the mic selection to a known device id (falls back to "既定" if absent).
export function applyMicDeviceId(id: string) {
  desiredMicId = id;
  appMic.value = Array.from(appMic.options).some((o) => o.value === id)
    ? id
    : "";
}

export function setAppVersion(v: string) {
  appVersionEl.textContent = v;
}

export async function populateMicList(selectedId: string) {
  desiredMicId = selectedId;
  // Touch getUserMedia briefly to grant labels; ignore failures.
  try {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    stream.getTracks().forEach((t) => t.stop());
  } catch {
    // permission denied; proceed with limited info
  }
  await fillMicOptions(selectedId);
}

/// (Re)build the mic <select> from the current device list, preserving the
/// given selection if it still exists. Does NOT request permission, so it's
/// safe to call repeatedly (e.g. on devicechange).
async function fillMicOptions(selectedId: string) {
  let devices: MediaDeviceInfo[] = [];
  try {
    devices = await navigator.mediaDevices.enumerateDevices();
  } catch (e) {
    console.error("enumerateDevices failed", e);
  }
  appMic.innerHTML = "";
  const defaultOpt = document.createElement("option");
  defaultOpt.value = "";
  defaultOpt.textContent = "既定 (OS 設定に従う)";
  appMic.appendChild(defaultOpt);
  let i = 1;
  for (const d of devices) {
    if (d.kind !== "audioinput") continue;
    const opt = document.createElement("option");
    opt.value = d.deviceId;
    opt.textContent = d.label || `マイク ${i}`;
    appMic.appendChild(opt);
    i++;
  }
  appMic.value =
    selectedId && Array.from(appMic.options).some((o) => o.value === selectedId)
      ? selectedId
      : "";
}
