// Proper-noun dictionary section. Each chip shows the canonical word and
// (if generated) its phonetic readings underneath in a smaller font.

import { $, flash } from "./dom";
import type { DictionaryEntry } from "./types";

let dictInput: HTMLInputElement;
let dictAdd: HTMLButtonElement;
let dictList: HTMLDivElement;
let dictCount: HTMLSpanElement;
let entries: DictionaryEntry[] = [];

export function initDictionarySection() {
  dictInput = $<HTMLInputElement>("#dict-input");
  dictAdd = $<HTMLButtonElement>("#dict-add");
  dictList = $<HTMLDivElement>("#dict-list");
  dictCount = $<HTMLSpanElement>("#dict-count");

  dictAdd.addEventListener("click", addEntry);
  dictInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      addEntry();
    }
  });
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

function paintDict() {
  dictList.innerHTML = "";
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    const chip = document.createElement("div");
    chip.className = "dict-entry";

    const body = document.createElement("div");
    body.className = "dict-entry-body";

    const word = document.createElement("span");
    word.className = "dict-entry-word";
    word.textContent = entry.word;
    body.appendChild(word);

    if (entry.readings.length > 0) {
      const readings = document.createElement("span");
      readings.className = "dict-entry-readings";
      readings.textContent = entry.readings.join(" / ");
      body.appendChild(readings);
    }
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
    dictList.appendChild(chip);
  }
  dictCount.textContent = String(entries.length);
}

function addEntry() {
  const v = dictInput.value.trim();
  if (!v) return;
  if (entries.some((e) => e.word === v)) {
    flash("既に登録済み", true);
    return;
  }
  entries.push({ word: v, readings: [] });
  dictInput.value = "";
  paintDict();
}
