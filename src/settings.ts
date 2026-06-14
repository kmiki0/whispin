// Settings window entry point. Initializes each section, loads current
// config from Rust, wires up Save/Cancel buttons.

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { $, flash, initFlash, installSectionNav } from "./settings/dom";
import {
  applyApiKeys,
  initApiKeysSection,
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
} from "./settings/llm-section";
import {
  applyRecordingConfig,
  initRecordingSection,
  readRecordingConfig,
} from "./settings/recording-section";
import {
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

async function saveAll() {
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
    flash("保存しました");
  } catch (e) {
    flash(`保存失敗: ${e}`, true);
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

$<HTMLButtonElement>("#save").addEventListener("click", saveAll);
$<HTMLButtonElement>("#cancel").addEventListener("click", () => {
  getCurrentWindow()
    .close()
    .catch((e) => console.error("close failed", e));
});

loadAll();
