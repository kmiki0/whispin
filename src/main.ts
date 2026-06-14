// Entry point for the notch (main) window. Wires Tauri events to the
// recorder + notch state machine; nothing else.

import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { initNotch, setStatus, scheduleHide } from "./notch";
import { startRecording, stopRecording } from "./recorder";
import { initCanvas } from "./visualizer";

window.addEventListener("DOMContentLoaded", () => {
  const statusEl = document.querySelector<HTMLElement>("#status");
  const labelEl = document.querySelector<HTMLElement>("#label");
  const canvasEl = document.querySelector<HTMLCanvasElement>("#wave");
  if (statusEl && labelEl) initNotch(statusEl, labelEl);
  if (canvasEl) initCanvas(canvasEl);

  listen("start-recording", () => startRecording());
  listen("stop-recording", () => stopRecording());
  listen("correction-started", () => setStatus("correcting"));
  // We accept correction-chunk events but don't render them; the static
  // "Correcting..." label is enough.
  listen<string>("correction-chunk", () => {});

  // Briefly show the notch on startup so the user can see the app is ready.
  window.setTimeout(() => {
    getCurrentWindow()
      .show()
      .catch((e) => console.error("startup show failed", e));
    setStatus("ready");
    scheduleHide(2500);
  }, 250);
});
