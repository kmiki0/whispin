// Trigger settings section: shows the current trigger, lets the user "capture"
// a new key / mouse button (with modifier combos), and provides the long-press
// threshold slider.

import { $ } from "./dom";
import { labelForInput } from "./key-labels";
import type { Modifiers, MouseButton, TriggerConfig, TriggerInput } from "./types";

const THRESHOLD_STEPS = [150, 250, 400, 600];
const THRESHOLD_LABELS = ["速い", "標準", "遅め", "ゆっくり"];

const DEFAULT_HINT =
  "ボックスをクリックして、設定したいキーまたはマウスボタンを押してください。";

let currentEl: HTMLButtonElement;
let thresholdRange: HTMLInputElement;
let thresholdValueEl: HTMLSpanElement;
let captureHint: HTMLParagraphElement;

let triggerInput: TriggerInput = { kind: "mouse", button: "Right" };
let currentMods: Modifiers = { ctrl: false, shift: false, alt: false, win: false };
let capturing = false;
let pendingModifier: KeyboardEvent | null = null;

export function initTriggerSection() {
  currentEl = $<HTMLButtonElement>("#current-trigger");
  thresholdRange = $<HTMLInputElement>("#threshold");
  thresholdValueEl = $<HTMLSpanElement>("#threshold-value");
  captureHint = $<HTMLParagraphElement>("#capture-hint");

  paintTrigger();
  captureHint.textContent = DEFAULT_HINT;

  thresholdRange.addEventListener("input", () => {
    paintThreshold(Number(thresholdRange.value));
  });
  // The label box itself is the capture control: click to start, click again
  // (or Esc) to cancel.
  currentEl.addEventListener("click", (e) => {
    e.preventDefault();
    if (capturing) stopCapture();
    else startCapture();
  });
}

export function applyTriggerConfig(cfg: TriggerConfig) {
  triggerInput = cfg.input;
  currentMods = { ...cfg.modifiers };
  setThreshold(cfg.long_press_ms);
  paintTrigger();
}

export function readTriggerConfig(): TriggerConfig {
  return {
    input: triggerInput,
    modifiers: { ...currentMods },
    long_press_ms: readThreshold(),
  };
}

function paintTrigger() {
  currentEl.innerHTML = "";
  const order: Array<[keyof Modifiers, string]> = [
    ["ctrl", "Ctrl"],
    ["shift", "Shift"],
    ["alt", "Alt"],
    ["win", "Win"],
  ];
  const parts: HTMLElement[] = [];
  for (const [k, label] of order) {
    if (currentMods[k]) {
      const chip = document.createElement("span");
      chip.className = "mod-chip";
      chip.textContent = label;
      parts.push(chip);
    }
  }
  const main = document.createElement("span");
  main.textContent = labelForInput(triggerInput);
  parts.push(main);

  for (let i = 0; i < parts.length; i++) {
    if (i > 0) {
      const plus = document.createElement("span");
      plus.className = "combo-plus";
      plus.textContent = "+";
      currentEl.appendChild(plus);
    }
    currentEl.appendChild(parts[i]);
  }
}

function indexForMs(ms: number): number {
  let bestIdx = 0;
  let bestDiff = Infinity;
  for (let i = 0; i < THRESHOLD_STEPS.length; i++) {
    const d = Math.abs(THRESHOLD_STEPS[i] - ms);
    if (d < bestDiff) {
      bestDiff = d;
      bestIdx = i;
    }
  }
  return bestIdx;
}

function paintThreshold(idx: number) {
  thresholdValueEl.textContent = `${THRESHOLD_LABELS[idx]} · ${THRESHOLD_STEPS[idx]} ms`;
}

function setThreshold(ms: number) {
  const idx = indexForMs(ms);
  thresholdRange.value = String(idx);
  paintThreshold(idx);
}

function readThreshold(): number {
  const idx = Math.max(0, Math.min(3, Number(thresholdRange.value)));
  return THRESHOLD_STEPS[idx];
}

function startCapture() {
  capturing = true;
  pendingModifier = null;
  currentEl.classList.add("capturing");
  currentEl.textContent = "次の入力を待っています...";
  captureHint.textContent =
    "キー / Shift+M のような組合せ / マウスボタンを押下 (Esc でキャンセル)";
  window.addEventListener("keydown", onCaptureKeyDown, true);
  window.addEventListener("keyup", onCaptureKeyUp, true);
  window.addEventListener("mousedown", onCaptureMouse, true);
  window.addEventListener("contextmenu", suppressContext, true);
  // If the window loses focus mid-capture, abort so we don't stay stuck
  // listening for input the user can no longer see.
  window.addEventListener("blur", onCaptureBlur, true);
}

function onCaptureBlur() {
  if (capturing) stopCapture();
}

function stopCapture() {
  capturing = false;
  pendingModifier = null;
  currentEl.classList.remove("capturing");
  paintTrigger();
  captureHint.textContent = DEFAULT_HINT;
  window.removeEventListener("keydown", onCaptureKeyDown, true);
  window.removeEventListener("keyup", onCaptureKeyUp, true);
  window.removeEventListener("mousedown", onCaptureMouse, true);
  window.removeEventListener("contextmenu", suppressContext, true);
  window.removeEventListener("blur", onCaptureBlur, true);
}

function isModifierKey(k: string): boolean {
  return k === "Shift" || k === "Control" || k === "Alt" || k === "Meta";
}

function commitKey(vk: number, mods: Modifiers) {
  triggerInput = { kind: "key", vk };
  currentMods = { ...mods };
  stopCapture();
}

function onCaptureKeyDown(e: KeyboardEvent) {
  if (!capturing) return;
  e.preventDefault();
  e.stopPropagation();
  if (e.key === "Escape") {
    stopCapture();
    return;
  }
  if (isModifierKey(e.key)) {
    // Defer until either keyup (modifier alone) or a non-modifier keydown (combo).
    pendingModifier = e;
    return;
  }
  const vk = (e as KeyboardEvent & { keyCode?: number }).keyCode || 0;
  if (vk) {
    pendingModifier = null;
    commitKey(vk, {
      ctrl: e.ctrlKey,
      shift: e.shiftKey,
      alt: e.altKey,
      win: e.metaKey,
    });
  }
}

function onCaptureKeyUp(e: KeyboardEvent) {
  if (!capturing) return;
  if (pendingModifier && e.code === pendingModifier.code) {
    e.preventDefault();
    e.stopPropagation();
    const vk =
      (pendingModifier as KeyboardEvent & { keyCode?: number }).keyCode || 0;
    pendingModifier = null;
    if (vk) {
      commitKey(vk, { ctrl: false, shift: false, alt: false, win: false });
    }
  }
}

function onCaptureMouse(e: MouseEvent) {
  if (!capturing) return;
  const target = e.target as HTMLElement | null;
  // Footer buttons stay clickable; everything else — including the trigger box
  // itself — is fair game as a mouse-button trigger.
  if (target?.closest("#cancel, #save")) return;
  e.preventDefault();
  e.stopPropagation();
  const button = buttonFromMouseEvent(e.button);
  if (!button) return;
  pendingModifier = null;
  triggerInput = { kind: "mouse", button };
  currentMods = {
    ctrl: e.ctrlKey,
    shift: e.shiftKey,
    alt: e.altKey,
    win: e.metaKey,
  };
  // This mousedown will be followed by a click (or contextmenu for right-click)
  // landing on the box. Swallow it so it doesn't immediately restart capture
  // via the box's own click handler.
  swallowNextMouseEvent();
  stopCapture();
}

/// Suppress the next click/contextmenu (whichever the just-captured button
/// produces) so it doesn't reach the trigger box. Self-cleans on a timeout in
/// case the button (e.g. X1/X2) produces neither.
function swallowNextMouseEvent() {
  let timer = 0;
  const handler = (ev: Event) => {
    ev.preventDefault();
    ev.stopPropagation();
    done();
  };
  const done = () => {
    window.removeEventListener("click", handler, true);
    window.removeEventListener("contextmenu", handler, true);
    if (timer) window.clearTimeout(timer);
  };
  window.addEventListener("click", handler, true);
  window.addEventListener("contextmenu", handler, true);
  timer = window.setTimeout(done, 400);
}

function buttonFromMouseEvent(button: number): MouseButton | null {
  switch (button) {
    case 0:
      return "Left";
    case 1:
      return "Middle";
    case 2:
      return "Right";
    case 3:
      return "X1";
    case 4:
      return "X2";
    default:
      return null;
  }
}

function suppressContext(e: MouseEvent) {
  if (capturing) {
    e.preventDefault();
    e.stopPropagation();
  }
}
