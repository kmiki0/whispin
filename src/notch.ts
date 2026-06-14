// Notch UI: status state machine + auto-hide animation.
// The notch has multiple visual states (idle / ready / recording / done /
// closing etc.) and a multi-stage close sequence (notch → sphere → shrink).

import { getCurrentWindow } from "@tauri-apps/api/window";

export type Status =
  | "idle"
  | "ready"
  | "recording"
  | "transcribing"
  | "correcting"
  | "done"
  | "error"
  | "to-sphere"
  | "closing";

let statusEl: HTMLElement | null = null;
let labelEl: HTMLElement | null = null;
let hideTimer: number | null = null;

const SPHERE_MORPH_MS = 320;
const CLOSE_SHRINK_MS = 380;

export function initNotch(status: HTMLElement, label: HTMLElement) {
  statusEl = status;
  labelEl = label;
  setStatus("idle");
}

export function setStatus(s: Status, detail = "") {
  if (statusEl) statusEl.dataset.status = s;
  if (labelEl) labelEl.textContent = detail || defaultLabel(s);
}

export function currentStatus(): Status | undefined {
  return statusEl?.dataset.status as Status | undefined;
}

function defaultLabel(s: Status): string {
  switch (s) {
    case "ready":
      return "Voice ready";
    case "recording":
      return "Recording...";
    case "transcribing":
      return "Transcribing...";
    case "correcting":
      return "Correcting...";
    case "done":
      return "👋";
    case "error":
      return "Error";
    default:
      return "Idle";
  }
}

export function cancelHide() {
  if (hideTimer !== null) {
    window.clearTimeout(hideTimer);
    hideTimer = null;
  }
}

export function scheduleHide(delayMs: number) {
  cancelHide();
  hideTimer = window.setTimeout(() => {
    hideTimer = null;
    const cur = currentStatus();
    // done already morphs to sphere as part of its display. Others need an
    // explicit notch → sphere morph (via to-sphere) before the closing shrink.
    if (cur === "done") {
      runClose();
    } else {
      setStatus("to-sphere");
      window.setTimeout(runClose, SPHERE_MORPH_MS);
    }
  }, delayMs);
}

function runClose() {
  setStatus("closing");
  window.setTimeout(() => {
    setStatus("idle");
    getCurrentWindow()
      .hide()
      .catch((e) => console.error("hide failed", e));
  }, CLOSE_SHRINK_MS);
}
