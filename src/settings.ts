// Settings window entry point. Initializes each section, loads current
// config from Rust, wires up Save/Cancel/Reset buttons, and guards against
// losing unsaved edits.

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { $, flash, initFlash, installSectionNav } from "./settings/dom";
import {
  applyApiKeys,
  initApiKeysSection,
  onApiKeysChanged,
  readApiKeys,
} from "./settings/api-keys-section";
import {
  applyDictionary,
  initDictionarySection,
  readDictionary,
} from "./settings/dictionary-section";
import type { DictionaryEntry } from "./settings/types";
import {
  applyLlmConfig,
  initLlmSection,
  readLlmConfig,
  reflectEnabledState,
} from "./settings/llm-section";
import {
  applyRecordingConfig,
  initRecordingSection,
  readRecordingConfig,
} from "./settings/recording-section";
import {
  applyMicDeviceId,
  applyStartupEnabled,
  initAppSection,
  populateMicList,
  readMicDeviceId,
  readStartupEnabled,
  setAppVersion,
} from "./settings/app-section";
import {
  applyTriggerConfig,
  initTriggerSection,
  readTriggerConfig,
} from "./settings/trigger-section";
import type {
  ApiKeys,
  GeneralConfig,
  LlmConfig,
  RecordingConfig,
  TriggerConfig,
} from "./settings/types";

const DEFAULTS = {
  trigger: {
    input: { kind: "mouse", button: "Right" },
    modifiers: { ctrl: false, shift: false, alt: false, win: false },
    long_press_ms: 250,
  } satisfies TriggerConfig,
  apiKeys: { groq: "", openai: "", openrouter: "" } satisfies ApiKeys,
  llm: {
    enabled: true,
    model: "meta-llama/llama-3.3-70b-instruct",
    short_threshold_chars: 20,
    timeout_secs: 8,
    use_screen_context: true,
  } satisfies LlmConfig,
  recording: {
    mode: "ptt",
    auto_stop_on_silence: true,
    silence_timeout_ms: 1500,
  } satisfies RecordingConfig,
};

async function loadOrDefault<T>(cmd: string, fallback: T): Promise<T> {
  try {
    return await invoke<T>(cmd);
  } catch (e) {
    console.error(`${cmd} failed`, e);
    return fallback;
  }
}

async function tryInvoke<T>(cmd: string, fallback: T): Promise<T> {
  try {
    return await invoke<T>(cmd);
  } catch {
    return fallback;
  }
}

// --- Unsaved-change tracking ---
// Compare a serialized snapshot of every section against the last-saved one.
let savedSnapshot = "";

function snapshot(): string {
  return JSON.stringify({
    trigger: readTriggerConfig(),
    apiKeys: readApiKeys(),
    llm: readLlmConfig(),
    mic: readMicDeviceId(),
    recording: readRecordingConfig(),
    startup: readStartupEnabled(),
    dictionary: readDictionary(),
  });
}

function isDirty(): boolean {
  return savedSnapshot !== "" && snapshot() !== savedSnapshot;
}

let saving = false;

async function saveAll() {
  if (saving) return;
  saving = true;
  const saveBtn = $<HTMLButtonElement>("#save");
  saveBtn.disabled = true;
  const label = saveBtn.textContent;
  saveBtn.textContent = "保存中…";
  try {
    await invoke("set_trigger_config", { config: readTriggerConfig() });
    await invoke("set_api_keys", { keys: readApiKeys() });
    await invoke("set_llm_config", { config: readLlmConfig() });
    await invoke("set_general_config", {
      config: { mic_device_id: readMicDeviceId() } satisfies GeneralConfig,
    });
    await invoke("set_recording_config", { config: readRecordingConfig() });
    await invoke("set_startup_enabled", { enabled: readStartupEnabled() });

    // Dictionary save also generates missing readings via LLM, which can
    // take a few seconds. Show progress so the user knows we're not stuck.
    const entries = readDictionary();
    const needsReadingGen = entries.some((e) => e.readings.length === 0);
    if (needsReadingGen) flash("辞書の読みを生成中...");
    const saved = await invoke<DictionaryEntry[]>("set_dictionary_cmd", {
      entries,
    });
    applyDictionary(saved);
    savedSnapshot = snapshot();
    flash("保存しました");
  } catch (e) {
    flash(`保存失敗: ${e}`, true);
  } finally {
    saving = false;
    saveBtn.disabled = false;
    saveBtn.textContent = label;
  }
}

async function loadAll() {
  const trigger = await loadOrDefault<TriggerConfig>(
    "get_trigger_config",
    DEFAULTS.trigger,
  );
  applyTriggerConfig(trigger);

  const keys = await loadOrDefault<ApiKeys>("get_api_keys", DEFAULTS.apiKeys);
  applyApiKeys(keys);

  const llm = await loadOrDefault<LlmConfig>("get_llm_config", DEFAULTS.llm);
  applyLlmConfig(llm);

  const dict = await loadOrDefault<DictionaryEntry[]>("get_dictionary_cmd", []);
  applyDictionary(dict);

  const general = await loadOrDefault<GeneralConfig>("get_general_config", {
    mic_device_id: "",
  });
  await populateMicList(general.mic_device_id);

  const recording = await loadOrDefault<RecordingConfig>(
    "get_recording_config",
    DEFAULTS.recording,
  );
  applyRecordingConfig(recording);

  applyStartupEnabled(await tryInvoke<boolean>("get_startup_enabled", false));
  setAppVersion(await tryInvoke<string>("get_app_version", "—"));

  // The freshly-loaded state is, by definition, "saved".
  savedSnapshot = snapshot();
}

/// Reset behavioral settings to defaults. Deliberately leaves API keys and the
/// dictionary alone — those are user data, not preferences. Not persisted until
/// the user hits Save.
function resetDefaults() {
  const ok = window.confirm(
    "トリガー・録音モード・AI校正・マイク選択・自動起動を初期値に戻します。\n\nAPIキーと辞書は変更しません。保存するまで適用されません。続けますか?",
  );
  if (!ok) return;
  applyTriggerConfig(DEFAULTS.trigger);
  applyLlmConfig(DEFAULTS.llm);
  applyRecordingConfig(DEFAULTS.recording);
  applyMicDeviceId("");
  applyStartupEnabled(false);
  flash("初期値に戻しました (保存で適用)");
}

// --- Wiring ---

initFlash($<HTMLParagraphElement>("#status"));
initTriggerSection();
initApiKeysSection();
initLlmSection();
initRecordingSection();
initDictionarySection();
initAppSection();
installSectionNav();

// Keep the AI-correction key warning in sync with the OpenRouter key field.
onApiKeysChanged(reflectEnabledState);

$<HTMLButtonElement>("#save").addEventListener("click", saveAll);
$<HTMLButtonElement>("#reset-defaults").addEventListener("click", resetDefaults);

// --- Close flow with an in-page unsaved-changes confirm ---
// (window.confirm is unreliable inside the Tauri webview, so we use our own
// modal instead.)
let allowClose = false;
const modal = $<HTMLDivElement>("#confirm-modal");

function doClose() {
  allowClose = true;
  // destroy() force-closes without firing close-requested, so it can't be
  // blocked by a close-requested handler.
  getCurrentWindow()
    .destroy()
    .catch((e) => console.error("close failed", e));
}

function requestClose() {
  if (isDirty()) modal.hidden = false;
  else doClose();
}

$<HTMLButtonElement>("#cancel").addEventListener("click", requestClose);
$<HTMLButtonElement>("#modal-cancel").addEventListener("click", () => {
  modal.hidden = true;
});
$<HTMLButtonElement>("#modal-discard").addEventListener("click", doClose);
$<HTMLButtonElement>("#modal-save").addEventListener("click", async () => {
  await saveAll();
  doClose();
});

// Intercept the window's X button: block it once to show the modal, unless the
// user already chose to close (allowClose) or there's nothing unsaved.
getCurrentWindow()
  .onCloseRequested((event) => {
    if (allowClose || !isDirty()) return;
    event.preventDefault();
    modal.hidden = false;
  })
  .catch((e) => console.error("onCloseRequested failed", e));

loadAll();
