// Proper-noun dictionary section. Each chip shows the canonical word and
// (if generated) its phonetic readings underneath. Words and readings are
// editable inline (click to edit); duplicates are matched case-insensitively.
// The whole list can be exported / imported as JSON via the clipboard.

import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { $, flash } from "./dom";
import type { DictionaryEntry } from "./types";

let dictInput: HTMLInputElement;
let dictAdd: HTMLButtonElement;
let dictList: HTMLDivElement;
let dictCount: HTMLSpanElement;
let dictFilter: HTMLInputElement;
let entries: DictionaryEntry[] = [];
let filter = "";

export function initDictionarySection() {
  dictInput = $<HTMLInputElement>("#dict-input");
  dictAdd = $<HTMLButtonElement>("#dict-add");
  dictList = $<HTMLDivElement>("#dict-list");
  dictCount = $<HTMLSpanElement>("#dict-count");
  dictFilter = $<HTMLInputElement>("#dict-filter");

  dictAdd.addEventListener("click", addEntry);
  dictInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      addEntry();
    }
  });
  dictFilter.addEventListener("input", () => {
    filter = dictFilter.value.trim().toLowerCase();
    paintDict();
  });
  $<HTMLButtonElement>("#dict-export").addEventListener("click", exportDict);
  $<HTMLButtonElement>("#dict-import").addEventListener("click", importDict);
}

export function applyDictionary(list: DictionaryEntry[]) {
  entries = list.map(normalize);
  paintDict();
}

export function readDictionary(): DictionaryEntry[] {
  return entries.map((e) => ({ word: e.word, readings: e.readings.slice() }));
}

function normalize(raw: unknown): DictionaryEntry {
  if (typeof raw === "string") {
    return { word: raw, readings: [] };
  }
  const e = raw as Partial<DictionaryEntry>;
  return {
    word: e.word ?? "",
    readings: Array.isArray(e.readings) ? e.readings.filter(Boolean) : [],
  };
}

/// Index of an existing entry whose word matches `word` case-insensitively,
/// optionally ignoring the entry at `exceptIdx` (used while editing).
function findDup(word: string, exceptIdx = -1): number {
  const lc = word.toLowerCase();
  return entries.findIndex(
    (e, i) => i !== exceptIdx && e.word.toLowerCase() === lc,
  );
}

function paintDict() {
  dictList.innerHTML = "";
  let shown = 0;
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    if (filter && !matchesFilter(entry, filter)) continue;
    shown++;
    dictList.appendChild(makeChip(entry, i));
  }
  dictCount.textContent = filter
    ? `${shown}/${entries.length}`
    : String(entries.length);
}

function matchesFilter(e: DictionaryEntry, f: string): boolean {
  if (e.word.toLowerCase().includes(f)) return true;
  return e.readings.some((r) => r.toLowerCase().includes(f));
}

function makeChip(entry: DictionaryEntry, i: number): HTMLDivElement {
  const chip = document.createElement("div");
  chip.className = "dict-entry";

  const body = document.createElement("div");
  body.className = "dict-entry-body";

  const word = document.createElement("span");
  word.className = "dict-entry-word";
  word.textContent = entry.word;
  word.title = "クリックで編集";
  word.addEventListener("click", () =>
    editField(word, entry.word, (v) => commitWord(i, v)),
  );
  body.appendChild(word);

  const readings = document.createElement("span");
  readings.className = "dict-entry-readings";
  readings.textContent = entry.readings.join(" / ") || "読みを追加…";
  readings.title = "クリックで読みを編集 (/ 区切り)";
  readings.addEventListener("click", () =>
    editField(readings, entry.readings.join(" / "), (v) =>
      commitReadings(i, v),
    ),
  );
  body.appendChild(readings);

  chip.appendChild(body);

  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "dict-remove";
  btn.textContent = "×";
  btn.title = "削除";
  btn.addEventListener("click", () => {
    entries.splice(i, 1);
    paintDict();
  });
  chip.appendChild(btn);
  return chip;
}

/// Swap a static span for an input; commit on Enter/blur, cancel on Esc.
function editField(
  span: HTMLElement,
  initial: string,
  commit: (value: string) => void,
) {
  const input = document.createElement("input");
  input.type = "text";
  input.className = "dict-edit-input";
  input.value = initial;
  span.replaceWith(input);
  input.focus();
  input.select();

  let done = false;
  const finish = (save: boolean) => {
    if (done) return;
    done = true;
    if (save) commit(input.value.trim());
    paintDict();
  };
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      finish(true);
    } else if (e.key === "Escape") {
      e.preventDefault();
      finish(false);
    }
  });
  input.addEventListener("blur", () => finish(true));
}

function commitWord(i: number, value: string) {
  if (!value || value === entries[i].word) return;
  if (findDup(value, i) >= 0) {
    flash("既に登録済み", true);
    return;
  }
  entries[i].word = value;
}

function commitReadings(i: number, value: string) {
  entries[i].readings = value
    .split("/")
    .map((s) => s.trim())
    .filter(Boolean);
}

function addEntry() {
  const v = dictInput.value.trim();
  if (!v) return;
  if (findDup(v) >= 0) {
    flash("既に登録済み", true);
    return;
  }
  entries.push({ word: v, readings: [] });
  dictInput.value = "";
  paintDict();
}

async function exportDict() {
  try {
    await writeText(JSON.stringify(readDictionary(), null, 2));
    flash(`${entries.length} 件をクリップボードにコピーしました`);
  } catch (e) {
    flash(`エクスポート失敗: ${e}`, true);
  }
}

async function importDict() {
  let text: string;
  try {
    text = await readText();
  } catch (e) {
    flash(`クリップボード読取失敗: ${e}`, true);
    return;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    flash("クリップボードが有効な JSON ではありません", true);
    return;
  }
  if (!Array.isArray(parsed)) {
    flash("配列形式の辞書 JSON を貼り付けてください", true);
    return;
  }
  let added = 0;
  for (const raw of parsed) {
    const e = normalize(raw);
    if (!e.word || findDup(e.word) >= 0) continue;
    entries.push(e);
    added++;
  }
  paintDict();
  flash(`${added} 件を追加しました (重複は除外)`);
}
