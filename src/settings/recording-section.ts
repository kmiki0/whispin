// Recording-mode section: PTT vs Toggle radio.
// Silence auto-stop is no longer a separate user toggle — it's implied by
// the mode (PTT never auto-stops; Toggle always does). The silence-duration
// slider only matters for Toggle mode and is hidden when PTT is selected.

import { $ } from "./dom";
import type { RecordingConfig, RecordingMode } from "./types";

let recModeRadios: HTMLInputElement[];
let recSilence: HTMLInputElement;
let recSilenceVal: HTMLSpanElement;
let silenceGroup: HTMLElement;

export function initRecordingSection() {
  recModeRadios = Array.from(
    document.querySelectorAll<HTMLInputElement>('input[name="rec-mode"]'),
  );
  recSilence = $<HTMLInputElement>("#rec-silence");
  recSilenceVal = $<HTMLSpanElement>("#rec-silence-val");
  silenceGroup = $<HTMLElement>("#silence-group");

  recSilence.addEventListener("input", () => {
    paintSilence(Number(recSilence.value));
  });
  for (const r of recModeRadios) {
    r.addEventListener("change", reflectModeVisibility);
  }
}

export function applyRecordingConfig(c: RecordingConfig) {
  for (const r of recModeRadios) r.checked = r.value === c.mode;
  recSilence.value = String(c.silence_timeout_ms);
  paintSilence(c.silence_timeout_ms);
  reflectModeVisibility();
}

export function readRecordingConfig(): RecordingConfig {
  const mode =
    (recModeRadios.find((r) => r.checked)?.value as RecordingMode) ?? "ptt";
  return {
    mode,
    // PTT mode → no auto-stop, ever. Toggle mode → auto-stop always on.
    auto_stop_on_silence: mode === "toggle",
    silence_timeout_ms: Number(recSilence.value),
  };
}

function paintSilence(ms: number) {
  recSilenceVal.textContent = `${(ms / 1000).toFixed(1)} 秒`;
}

function reflectModeVisibility() {
  const mode =
    (recModeRadios.find((r) => r.checked)?.value as RecordingMode) ?? "ptt";
  silenceGroup.hidden = mode !== "toggle";
}
