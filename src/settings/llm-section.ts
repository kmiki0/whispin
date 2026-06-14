// LLM correction settings: enabled toggle, model dropdown (with custom input),
// short-text skip threshold, request timeout.

import { $ } from "./dom";
import type { LlmConfig } from "./types";

const DEFAULT_MODEL = "meta-llama/llama-3.3-70b-instruct";

let llmEnabled: HTMLInputElement;
let llmScreenContext: HTMLInputElement;
let llmModelSelect: HTMLSelectElement;
let llmModelCustom: HTMLInputElement;
let llmShort: HTMLInputElement;
let llmShortVal: HTMLSpanElement;
let llmTimeout: HTMLInputElement;
let llmTimeoutVal: HTMLSpanElement;
let presets: Set<string>;

export function initLlmSection() {
  llmEnabled = $<HTMLInputElement>("#llm-enabled");
  llmScreenContext = $<HTMLInputElement>("#llm-screen-context");
  llmModelSelect = $<HTMLSelectElement>("#llm-model-select");
  llmModelCustom = $<HTMLInputElement>("#llm-model-custom");
  llmShort = $<HTMLInputElement>("#llm-short");
  llmShortVal = $<HTMLSpanElement>("#llm-short-val");
  llmTimeout = $<HTMLInputElement>("#llm-timeout");
  llmTimeoutVal = $<HTMLSpanElement>("#llm-timeout-val");

  presets = new Set(
    Array.from(llmModelSelect.options)
      .map((o) => o.value)
      .filter((v) => v !== "__custom__"),
  );

  llmModelSelect.addEventListener("change", () => {
    const isCustom = llmModelSelect.value === "__custom__";
    llmModelCustom.hidden = !isCustom;
    if (isCustom) llmModelCustom.focus();
  });
  llmShort.addEventListener("input", () => {
    llmShortVal.textContent = `${llmShort.value} 文字`;
  });
  llmTimeout.addEventListener("input", () => {
    llmTimeoutVal.textContent = `${llmTimeout.value} 秒`;
  });
}

export function applyLlmConfig(c: LlmConfig) {
  llmEnabled.checked = c.enabled;
  llmScreenContext.checked = c.use_screen_context ?? true;
  setLlmModel(c.model);
  llmShort.value = String(c.short_threshold_chars);
  llmShortVal.textContent = `${c.short_threshold_chars} 文字`;
  llmTimeout.value = String(c.timeout_secs);
  llmTimeoutVal.textContent = `${c.timeout_secs} 秒`;
}

export function readLlmConfig(): LlmConfig {
  return {
    enabled: llmEnabled.checked,
    model: getLlmModel(),
    short_threshold_chars: Number(llmShort.value),
    timeout_secs: Number(llmTimeout.value),
    use_screen_context: llmScreenContext.checked,
  };
}

function setLlmModel(model: string) {
  if (!model || presets.has(model)) {
    llmModelSelect.value = model || DEFAULT_MODEL;
    llmModelCustom.hidden = true;
    llmModelCustom.value = "";
  } else {
    llmModelSelect.value = "__custom__";
    llmModelCustom.hidden = false;
    llmModelCustom.value = model;
  }
}

function getLlmModel(): string {
  if (llmModelSelect.value === "__custom__") {
    return llmModelCustom.value.trim();
  }
  return llmModelSelect.value;
}
