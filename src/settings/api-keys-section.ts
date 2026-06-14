// ASR / API-key inputs. Tracks the OpenAI key as a hidden value so we don't
// wipe it when the UI only exposes OpenRouter + Groq.

import { $ } from "./dom";
import type { ApiKeys } from "./types";

let keyOpenRouter: HTMLInputElement;
let keyGroq: HTMLInputElement;
let preservedOpenAiKey = "";

export function initApiKeysSection() {
  keyOpenRouter = $<HTMLInputElement>("#key-openrouter");
  keyGroq = $<HTMLInputElement>("#key-groq");
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
