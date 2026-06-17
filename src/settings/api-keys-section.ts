// ASR / API-key inputs. Tracks the OpenAI key as a hidden value so we don't
// wipe it when the UI only exposes OpenRouter + Groq. Also wires the per-key
// reveal toggle (👁) and "接続テスト" buttons.

import { invoke } from "@tauri-apps/api/core";
import { $ } from "./dom";
import type { ApiKeys } from "./types";

let keyOpenRouter: HTMLInputElement;
let keyGroq: HTMLInputElement;
let preservedOpenAiKey = "";

const keyChangeListeners: Array<() => void> = [];

export function initApiKeysSection() {
  keyOpenRouter = $<HTMLInputElement>("#key-openrouter");
  keyGroq = $<HTMLInputElement>("#key-groq");

  // Reveal / hide toggles.
  document
    .querySelectorAll<HTMLButtonElement>("[data-reveal]")
    .forEach((btn) => {
      btn.addEventListener("click", () => {
        const input = $<HTMLInputElement>(`#${btn.dataset.reveal}`);
        const hidden = input.type === "password";
        input.type = hidden ? "text" : "password";
        btn.classList.toggle("active", hidden);
      });
    });

  // "テスト" buttons: validate each key against its provider.
  document
    .querySelectorAll<HTMLButtonElement>("[data-test-key]")
    .forEach((btn) => {
      btn.addEventListener("click", () => testKey(btn));
    });

  // Notify listeners (e.g. the LLM key-warning) when the OpenRouter key changes.
  keyOpenRouter.addEventListener("input", () => {
    for (const fn of keyChangeListeners) fn();
  });
}

/// Register a callback fired whenever the OpenRouter key field changes.
export function onApiKeysChanged(fn: () => void) {
  keyChangeListeners.push(fn);
}

export function hasOpenRouterKey(): boolean {
  return keyOpenRouter.value.trim().length > 0;
}

async function testKey(btn: HTMLButtonElement) {
  const provider = btn.dataset.testKey!;
  const input = $<HTMLInputElement>(`#key-${provider}`);
  const result = $<HTMLParagraphElement>(
    `[data-test-result="${provider}"]`,
  );
  result.className = "test-result pending";
  result.textContent = "確認中…";
  btn.disabled = true;
  try {
    const msg = await invoke<string>("test_api_key", {
      provider,
      key: input.value,
    });
    result.className = "test-result ok";
    result.textContent = `✓ ${msg}`;
  } catch (e) {
    result.className = "test-result err";
    result.textContent = `✗ ${e}`;
  } finally {
    btn.disabled = false;
  }
}

export function applyApiKeys(k: ApiKeys) {
  keyOpenRouter.value = k.openrouter ?? "";
  keyGroq.value = k.groq ?? "";
  preservedOpenAiKey = k.openai ?? "";
}

export function readApiKeys(): ApiKeys {
  return {
    openrouter: keyOpenRouter.value,
    // Preserve any previously-saved OpenAI key (UI no longer exposes it).
    openai: preservedOpenAiKey,
    groq: keyGroq.value,
  };
}
