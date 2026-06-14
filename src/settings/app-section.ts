// "App general" section: startup-at-login, mic device picker, version display,
// open-config-folder, uninstall.

import { invoke } from "@tauri-apps/api/core";
import { $, flash } from "./dom";

let appStartup: HTMLInputElement;
let appMic: HTMLSelectElement;
let appVersionEl: HTMLSpanElement;

export function initAppSection() {
  appStartup = $<HTMLInputElement>("#app-startup");
  appMic = $<HTMLSelectElement>("#app-mic");
  appVersionEl = $<HTMLSpanElement>("#app-version");
  const appOpenFolder = $<HTMLButtonElement>("#app-open-folder");
  const appUninstall = $<HTMLButtonElement>("#app-uninstall");

  appOpenFolder.addEventListener("click", async () => {
    try {
      await invoke("open_settings_folder");
    } catch (e) {
      flash(`フォルダを開けません: ${e}`, true);
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

export function applyStartupEnabled(enabled: boolean) {
  appStartup.checked = enabled;
}

export function readStartupEnabled(): boolean {
  return appStartup.checked;
}

export function readMicDeviceId(): string {
  return appMic.value;
}

export function setAppVersion(v: string) {
  appVersionEl.textContent = v;
}

export async function populateMicList(selectedId: string) {
  // Touch getUserMedia briefly to grant labels; ignore failures.
  try {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    stream.getTracks().forEach((t) => t.stop());
  } catch {
    // permission denied; proceed with limited info
  }
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
  if (
    selectedId &&
    Array.from(appMic.options).some((o) => o.value === selectedId)
  ) {
    appMic.value = selectedId;
  } else {
    appMic.value = "";
  }
}
