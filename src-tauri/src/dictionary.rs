// Proper-noun dictionary. Each entry is a `word` (the canonical written form)
// plus zero-or-more `readings` (katakana / hiragana phonetic candidates the
// ASR is likely to emit). At transcription time we substring-replace any
// reading we find in the Whisper output with its canonical word — this runs
// unconditionally so even short utterances (which skip the LLM correction
// pass) still get proper-noun fixes.
//
// Readings are filled in by the LLM (OpenRouter) on save. The user only ever
// types the word.

#![cfg(windows)]

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::Manager;

const OPENROUTER_CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const READING_GEN_MODEL: &str = "meta-llama/llama-3.3-70b-instruct";

const DEFAULT_ENTRIES: &[&str] = &[
    "Claude Code",
    "Claude",
    "Tauri",
    "Groq",
    "OpenRouter",
    "Whisper",
    "AquaVoice",
    "Hanaseru",
    "TypeScript",
    "JavaScript",
    "Rust",
    "Python",
    "Next.js",
    "Vite",
    "Obsidian",
    "VS Code",
    "claude.ai",
    "GitHub",
    "Whispin",
    "Qwen",
    "MeCab",
    "WebView2",
    "Tesseract",
];

#[derive(Serialize, Clone, Debug, Default)]
pub struct Entry {
    pub word: String,
    #[serde(default)]
    pub readings: Vec<String>,
}

/// Accept the new {word, readings} form AND the legacy bare-string form
/// ("Claude Code") so dictionary.json files from before the schema change
/// keep loading.
impl<'de> Deserialize<'de> for Entry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Simple(String),
            Detailed {
                word: String,
                #[serde(default)]
                readings: Vec<String>,
            },
        }
        Ok(match Raw::deserialize(deserializer)? {
            Raw::Simple(s) => Entry {
                word: s,
                readings: vec![],
            },
            Raw::Detailed { word, readings } => Entry { word, readings },
        })
    }
}

fn default_entries() -> Vec<Entry> {
    DEFAULT_ENTRIES
        .iter()
        .map(|s| Entry {
            word: s.to_string(),
            readings: vec![],
        })
        .collect()
}

pub fn path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("dictionary.json"))
}

pub fn load(app: &tauri::AppHandle) -> Vec<Entry> {
    let Some(path) = path(app) else {
        eprintln!("[whispin] app_config_dir unavailable; using built-in dictionary");
        return default_entries();
    };
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let defaults = default_entries();
        write(&path, &defaults);
        return defaults;
    }
    match std::fs::read_to_string(&path) {
        Ok(s) => match serde_json::from_str::<Vec<Entry>>(&s) {
            Ok(v) if !v.is_empty() => v,
            Ok(_) => {
                eprintln!("[whispin] dictionary file empty; using built-in");
                default_entries()
            }
            Err(e) => {
                eprintln!("[whispin] dictionary parse failed: {e}; using built-in");
                default_entries()
            }
        },
        Err(e) => {
            eprintln!("[whispin] dictionary read failed: {e}; using built-in");
            default_entries()
        }
    }
}

fn write(path: &std::path::Path, entries: &[Entry]) {
    match serde_json::to_string_pretty(entries) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                eprintln!("[whispin] dictionary write failed: {e}");
            }
        }
        Err(e) => eprintln!("[whispin] dictionary serialize failed: {e}"),
    }
}

/// Just the words, used to feed the LLM correction prompt's noun list.
pub fn words(entries: &[Entry]) -> Vec<String> {
    entries.iter().map(|e| e.word.clone()).collect()
}

/// Substitute every reading occurrence in `text` with its canonical word.
/// Handles hiragana/katakana ambiguity by matching the katakana-normalized
/// reading against the katakana-normalized text, then mapping back to the
/// original positions via a simple replace loop.
pub fn apply(entries: &[Entry], text: &str) -> String {
    if text.is_empty() {
        return text.to_string();
    }
    let mut out = text.to_string();
    for entry in entries {
        for reading in &entry.readings {
            if reading.is_empty() {
                continue;
            }
            // 1. Try the reading as-is.
            if out.contains(reading.as_str()) {
                out = out.replace(reading.as_str(), &entry.word);
            }
            // 2. Try the hiragana version of the reading (in case Whisper
            //    chose hiragana for that word).
            let as_hira = kata_to_hira(reading);
            if as_hira != *reading && out.contains(&as_hira) {
                out = out.replace(&as_hira, &entry.word);
            }
        }
    }
    out
}

fn kata_to_hira(s: &str) -> String {
    s.chars()
        .map(|c| {
            let code = c as u32;
            if (0x30A1..=0x30F6).contains(&code) {
                char::from_u32(code - 0x60).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

#[derive(Deserialize)]
struct ReadingsResponse {
    #[serde(flatten)]
    map: HashMap<String, Vec<String>>,
}

/// Ask the LLM for likely katakana readings for each word that doesn't have
/// any yet. Mutates `entries` in place. Returns the number of entries that
/// were filled in (0 if nothing missing or if the call failed).
pub async fn fill_missing_readings(api_key: &str, entries: &mut [Entry]) -> usize {
    let missing: Vec<String> = entries
        .iter()
        .filter(|e| e.readings.is_empty())
        .map(|e| e.word.clone())
        .collect();
    if missing.is_empty() {
        return 0;
    }

    let word_list = missing
        .iter()
        .map(|w| format!("- {w}"))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "以下の単語/フレーズについて、日本語話者が音声入力した際に Whisper などの音声認識が出力しそうなカタカナ表記を1〜5個ずつ挙げてください。同音異字、長音の有無、誤読を含める。\n\n単語リスト:\n{word_list}\n\n出力は JSON オブジェクトのみ。キーは入力した単語そのもの、値はカタカナ文字列の配列。説明や前置きは禁止。\n\n例:\n{{\n  \"Claude Code\": [\"クロードコード\", \"クラウドコード\"],\n  \"Tauri\": [\"タウリ\", \"タウリィ\"]\n}}"
    );

    let body = serde_json::json!({
        "model": READING_GEN_MODEL,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.3,
        "response_format": {"type": "json_object"},
        "provider": {
            "order": ["Groq"],
            "allow_fallbacks": true
        }
    });

    let client = match reqwest::Client::builder().timeout(Duration::from_secs(30)).build() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[whispin] reading-gen client build failed: {e}");
            return 0;
        }
    };
    let resp = match client
        .post(OPENROUTER_CHAT_URL)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[whispin] reading-gen request failed: {e}");
            return 0;
        }
    };
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        eprintln!("[whispin] reading-gen API error {status}: {body}");
        return 0;
    }
    let parsed: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[whispin] reading-gen JSON parse failed: {e}");
            return 0;
        }
    };
    let Some(content) = parsed
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
    else {
        eprintln!("[whispin] reading-gen response missing message content");
        return 0;
    };
    let map: ReadingsResponse = match serde_json::from_str(content) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[whispin] reading-gen content not JSON: {e}; content={content}");
            return 0;
        }
    };

    let mut updated = 0usize;
    for entry in entries.iter_mut() {
        if !entry.readings.is_empty() {
            continue;
        }
        if let Some(readings) = map.map.get(&entry.word) {
            entry.readings = readings.iter().filter(|r| !r.is_empty()).cloned().collect();
            if !entry.readings.is_empty() {
                updated += 1;
            }
        }
    }
    updated
}

#[tauri::command]
pub fn get_dictionary_cmd(app: tauri::AppHandle) -> Vec<Entry> {
    load(&app)
}

/// Save the entries, then (if an OpenRouter key is available) fill in any
/// missing readings via the LLM and save again. The whole thing is awaited
/// so the UI's "保存しました" only fires once all readings are persisted.
#[tauri::command]
pub async fn set_dictionary_cmd(
    app: tauri::AppHandle,
    entries: Vec<Entry>,
) -> Result<Vec<Entry>, String> {
    let path = path(&app).ok_or_else(|| "app_config_dir unavailable".to_string())?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut entries = entries;
    write(&path, &entries);

    let api_key = crate::settings::load(&app).api_keys.openrouter;
    let trimmed = api_key.trim();
    let key = if trimmed.is_empty() {
        std::env::var("OPENROUTER_API_KEY").ok().filter(|v| !v.is_empty())
    } else {
        Some(trimmed.to_string())
    };
    if let Some(key) = key {
        let updated = fill_missing_readings(&key, &mut entries).await;
        if updated > 0 {
            eprintln!("[whispin] generated readings for {updated} entr{}", if updated == 1 { "y" } else { "ies" });
            write(&path, &entries);
        }
    } else {
        eprintln!("[whispin] no OpenRouter key; skipping reading generation");
    }
    Ok(entries)
}
