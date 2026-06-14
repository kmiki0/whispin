// Human-readable labels for VK codes + mouse buttons. Used by both the
// trigger badge and the capture flow.

import type { MouseButton, TriggerInput } from "./types";

export const VK_LABELS: Record<number, string> = {
  0x08: "Backspace",
  0x09: "Tab",
  0x0d: "Enter",
  0x13: "Pause",
  0x14: "Caps Lock",
  0x1b: "Esc",
  0x20: "Space",
  0x21: "Page Up",
  0x22: "Page Down",
  0x23: "End",
  0x24: "Home",
  0x25: "←",
  0x26: "↑",
  0x27: "→",
  0x28: "↓",
  0x2d: "Insert",
  0x2e: "Delete",
  0x5b: "Left Win",
  0x5c: "Right Win",
  0x70: "F1",
  0x71: "F2",
  0x72: "F3",
  0x73: "F4",
  0x74: "F5",
  0x75: "F6",
  0x76: "F7",
  0x77: "F8",
  0x78: "F9",
  0x79: "F10",
  0x7a: "F11",
  0x7b: "F12",
  0xa0: "Left Shift",
  0xa1: "Right Shift",
  0xa2: "Left Ctrl",
  0xa3: "Right Ctrl",
  0xa4: "Left Alt",
  0xa5: "Right Alt",
};

export const MOUSE_LABELS: Record<MouseButton, string> = {
  Left: "左クリック",
  Right: "右クリック",
  Middle: "ミドルクリック",
  X1: "サイドボタン X1 (戻る)",
  X2: "サイドボタン X2 (進む)",
};

export function labelForInput(i: TriggerInput): string {
  if (i.kind === "mouse") return `🖱  ${MOUSE_LABELS[i.button]}`;
  const name =
    VK_LABELS[i.vk] ??
    (i.vk >= 0x30 && i.vk <= 0x39
      ? String.fromCharCode(i.vk)
      : i.vk >= 0x41 && i.vk <= 0x5a
        ? String.fromCharCode(i.vk)
        : `VK 0x${i.vk.toString(16).padStart(2, "0")}`);
  return `⌨  ${name}`;
}
