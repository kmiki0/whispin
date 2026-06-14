// Audio → text pipeline:
//   1. Pick an ASR provider (OpenRouter → OpenAI → Groq) based on which API
//      key is available (settings.json first, env vars as fallback).
//   2. Send the recorded blob to Whisper-equivalent endpoint, get raw text.
//   3. (Optional) Stream the text + OCR context + dictionary through an LLM
//      to clean up filler words, fix proper-noun transliteration, and add
//      Japanese punctuation. Bypass for very short utterances and when
//      disabled in settings.
//   4. Copy result to clipboard and synthesize Ctrl+V into the original
//      foreground window.

#![cfg(windows)]

use std::sync::Arc;

use base64::{engine::general_purpose, Engine as _};
use tauri::Emitter;
use tauri_plugin_clipboard_manager::ClipboardExt;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

use crate::dictionary;
use crate::settings::{self, ApiKeys, DEFAULT_CORRECTION_MODEL};
use crate::state::AppState;

const GROQ_TRANSCRIPTION_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";
const GROQ_MODEL: &str = "whisper-large-v3-turbo";
const OPENAI_TRANSCRIPTION_URL: &str = "https://api.openai.com/v1/audio/transcriptions";
const OPENAI_MODEL: &str = "whisper-1";
const OPENROUTER_TRANSCRIPTION_URL: &str =
    "https://openrouter.ai/api/v1/audio/transcriptions";
const OPENROUTER_MODEL: &str = "openai/whisper-large-v3-turbo";

const OPENROUTER_CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

enum AsrApi {
    OpenAiCompatibleMultipart {
        url: &'static str,
        model: &'static str,
    },
    OpenRouterJson {
        url: &'static str,
        model: &'static str,
    },
}

struct AsrProvider {
    api: AsrApi,
    api_key: String,
}

fn resolve_key(from_settings: &str, env_var: &str) -> Option<String> {
    let trimmed = from_settings.trim();
    if !trimmed.is_empty() {
        return Some(trimmed.to_string());
    }
    std::env::var(env_var).ok().filter(|v| !v.is_empty())
}

fn select_asr_provider(keys: &ApiKeys) -> Result<AsrProvider, String> {
    if let Some(key) = resolve_key(&keys.openrouter, "OPENROUTER_API_KEY") {
        return Ok(AsrProvider {
            api: AsrApi::OpenRouterJson {
                url: OPENROUTER_TRANSCRIPTION_URL,
                model: OPENROUTER_MODEL,
            },
            api_key: key,
        });
    }
    if let Some(key) = resolve_key(&keys.openai, "OPENAI_API_KEY") {
        return Ok(AsrProvider {
            api: AsrApi::OpenAiCompatibleMultipart {
                url: OPENAI_TRANSCRIPTION_URL,
                model: OPENAI_MODEL,
            },
            api_key: key,
        });
    }
    if let Some(key) = resolve_key(&keys.groq, "GROQ_API_KEY") {
        return Ok(AsrProvider {
            api: AsrApi::OpenAiCompatibleMultipart {
                url: GROQ_TRANSCRIPTION_URL,
                model: GROQ_MODEL,
            },
            api_key: key,
        });
    }
    Err("APIキーが未設定です (OpenRouter / OpenAI / Groq のいずれかを設定してください)"
        .to_string())
}

fn audio_format(mime_type: &str) -> &'static str {
    if mime_type.contains("webm") {
        "webm"
    } else if mime_type.contains("ogg") {
        "ogg"
    } else if mime_type.contains("mp4") || mime_type.contains("m4a") {
        "m4a"
    } else if mime_type.contains("mpeg") || mime_type.contains("mp3") {
        "mp3"
    } else if mime_type.contains("flac") {
        "flac"
    } else {
        "wav"
    }
}

fn trim_tail_chars(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        chars[chars.len() - max_chars..].iter().collect()
    }
}

async fn correct_with_llm_streaming(
    app: &tauri::AppHandle,
    api_key: &str,
    model: &str,
    timeout_secs: u64,
    whisper_text: &str,
    ocr_text: Option<&str>,
    dictionary: &[String],
) -> Result<String, String> {
    use futures_util::StreamExt;

    let model = if model.trim().is_empty() {
        DEFAULT_CORRECTION_MODEL.to_string()
    } else {
        model.to_string()
    };
    let model = std::env::var("WHISPIN_CORRECTION_MODEL").unwrap_or(model);

    let dict_block = if dictionary.is_empty() {
        String::new()
    } else {
        let lines: Vec<String> = dictionary.iter().map(|w| format!("- {w}")).collect();
        format!(
            "【固有名詞リスト】(音が近い箇所は必ずこの表記に置換):\n{}\n",
            lines.join("\n")
        )
    };

    let context_section = match ocr_text {
        Some(t) if !t.trim().is_empty() => {
            let trimmed = trim_tail_chars(t.trim(), 300);
            format!("【画面文脈】:\n{trimmed}\n\n")
        }
        _ => String::new(),
    };

    let user_msg = format!(
        "{dict_block}\n{context_section}【生テキスト】:\n{whisper_text}\n\n校正後の本文のみを出力してください。"
    );

    let system_msg = "あなたは日本語音声入力の校正器です。以下のルールで校正してください:\n\
1. 固有名詞リストにある語と音が近い (カタカナ化/誤認識) 箇所は、必ずリストの表記に置換する。\n   例: 「クロードコード」「クラウドコード」→ Claude Code\n   例: 「タウリ」「タウリン」→ Tauri\n2. 同音異義語は画面文脈や前後関係から正しい漢字を選ぶ。\n3. 句読点 (、。) を自然な位置に挿入する。\n4. フィラー (えーと / あのー / なんか / まあ / その) を除去する。\n5. 意味の改変や要約はしない。解釈が大きく割れる場合は原文を優先。\n6. 校正後の本文のみを出力。前置き、説明、囲み (引用符/コードブロック) は一切なし。";

    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system_msg},
            {"role": "user", "content": user_msg}
        ],
        "temperature": 0.2,
        "stream": true,
        "provider": {
            "order": ["Groq"],
            "allow_fallbacks": true
        }
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs.clamp(2, 60)))
        .build()
        .map_err(|e| format!("client build failed: {e}"))?;
    let resp = client
        .post(OPENROUTER_CHAT_URL)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("LLM request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("LLM API error {status}: {body}"));
    }

    let _ = app.emit("correction-started", ());

    let mut stream = resp.bytes_stream();
    let mut sse_buf = String::new();
    let mut full = String::new();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| format!("LLM stream read failed: {e}"))?;
        sse_buf.push_str(&String::from_utf8_lossy(&bytes));

        loop {
            let Some(idx) = sse_buf.find('\n') else { break };
            let line = sse_buf[..idx].trim_end_matches('\r').to_string();
            sse_buf.drain(..=idx);

            // OpenRouter sends ": OPENROUTER PROCESSING" lines as keep-alives.
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let Some(data) = line.strip_prefix("data: ") else { continue };
            if data.trim() == "[DONE]" {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
            if let Some(delta) = v
                .pointer("/choices/0/delta/content")
                .and_then(|x| x.as_str())
            {
                if !delta.is_empty() {
                    full.push_str(delta);
                    let _ = app.emit("correction-chunk", delta.to_string());
                }
            }
        }
    }

    Ok(full.trim().to_string())
}

async fn run_whisper(
    provider: &AsrProvider,
    audio_bytes: Vec<u8>,
    mime_type: &str,
    fmt: &str,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    match &provider.api {
        AsrApi::OpenAiCompatibleMultipart { url, model } => {
            let part = reqwest::multipart::Part::bytes(audio_bytes)
                .file_name(format!("audio.{fmt}"))
                .mime_str(mime_type)
                .map_err(|e| e.to_string())?;
            let form = reqwest::multipart::Form::new()
                .part("file", part)
                .text("model", *model)
                .text("language", "ja")
                .text("response_format", "text");
            let resp = client
                .post(*url)
                .bearer_auth(&provider.api_key)
                .multipart(form)
                .send()
                .await
                .map_err(|e| format!("ASR request failed: {e}"))?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("ASR API error {status}: {body}"));
            }
            Ok(resp.text().await.map_err(|e| e.to_string())?.trim().to_string())
        }
        AsrApi::OpenRouterJson { url, model } => {
            let body = serde_json::json!({
                "model": model,
                "input_audio": {
                    "data": general_purpose::STANDARD.encode(&audio_bytes),
                    "format": fmt,
                },
                "language": "ja",
            });
            let resp = client
                .post(*url)
                .bearer_auth(&provider.api_key)
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("ASR request failed: {e}"))?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("ASR API error {status}: {body}"));
            }
            let parsed: serde_json::Value =
                resp.json().await.map_err(|e| e.to_string())?;
            Ok(parsed
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim()
                .to_string())
        }
    }
}

fn paste_via_keyboard() -> Result<(), Box<dyn std::error::Error>> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let mut enigo = Enigo::new(&Settings::default())?;
    enigo.key(Key::Control, Direction::Press)?;
    enigo.key(Key::Unicode('v'), Direction::Click)?;
    enigo.key(Key::Control, Direction::Release)?;
    Ok(())
}

#[tauri::command]
pub async fn transcribe(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    audio_b64: String,
    mime_type: String,
) -> Result<String, String> {
    let settings = settings::load(&app);
    let provider = select_asr_provider(&settings.api_keys)?;

    let audio_bytes = general_purpose::STANDARD
        .decode(&audio_b64)
        .map_err(|e| format!("base64 decode failed: {e}"))?;
    let fmt = audio_format(&mime_type);

    {
        let ocr = state.ocr_text.lock();
        match ocr.as_ref() {
            Some(t) => eprintln!(
                "[whispin] OCR context available: {} chars, preview: {:?}",
                t.chars().count(),
                t.chars().take(80).collect::<String>()
            ),
            None => eprintln!("[whispin] no OCR context yet"),
        }
    }

    let whisper_started = std::time::Instant::now();
    let text = run_whisper(&provider, audio_bytes, &mime_type, fmt).await?;
    eprintln!(
        "[whispin] Whisper ok ({} chars, {} ms)",
        text.chars().count(),
        whisper_started.elapsed().as_millis()
    );

    if text.is_empty() {
        return Ok(String::new());
    }

    // Local proper-noun substitution. Always runs, so short utterances that
    // skip the LLM still benefit from the dictionary.
    let dict_entries = dictionary::load(&app);
    let text = dictionary::apply(&dict_entries, &text);

    let final_text = maybe_correct_text(&app, &state, &settings, &dict_entries, &text).await;

    app.clipboard()
        .write_text(final_text.clone())
        .map_err(|e| format!("clipboard write failed: {e}"))?;

    if let Some(hwnd_val) = state.target_hwnd.lock().take() {
        unsafe {
            let _ = SetForegroundWindow(HWND(hwnd_val as *mut std::ffi::c_void));
        }
        std::thread::sleep(std::time::Duration::from_millis(80));
    }

    paste_via_keyboard().map_err(|e| format!("paste failed: {e}"))?;
    Ok(final_text)
}

async fn maybe_correct_text(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    settings: &settings::AppSettings,
    dict_entries: &[dictionary::Entry],
    text: &str,
) -> String {
    let text_chars = text.chars().count();

    if !settings.llm.enabled {
        eprintln!("[whispin] LLM correction disabled in settings");
        return text.to_string();
    }
    if std::env::var("WHISPIN_DISABLE_CORRECTION").is_ok() {
        eprintln!("[whispin] correction disabled via WHISPIN_DISABLE_CORRECTION");
        return text.to_string();
    }
    if text_chars < settings.llm.short_threshold_chars {
        eprintln!(
            "[whispin] skipping LLM correction ({} chars < {})",
            text_chars, settings.llm.short_threshold_chars
        );
        return text.to_string();
    }
    let Some(or_key) = resolve_key(&settings.api_keys.openrouter, "OPENROUTER_API_KEY")
    else {
        eprintln!("[whispin] no OpenRouter key for LLM -- skipping correction");
        return text.to_string();
    };

    let ocr_opt = state.ocr_text.lock().clone();
    let dict_words = dictionary::words(dict_entries);
    let llm_started = std::time::Instant::now();
    match correct_with_llm_streaming(
        app,
        &or_key,
        &settings.llm.model,
        settings.llm.timeout_secs,
        text,
        ocr_opt.as_deref(),
        &dict_words,
    )
    .await
    {
        Ok(c) => {
            eprintln!(
                "[whispin] LLM correction ok ({} -> {} chars, {} ms)",
                text_chars,
                c.chars().count(),
                llm_started.elapsed().as_millis()
            );
            c
        }
        Err(e) => {
            eprintln!("[whispin] LLM correction failed: {e} -- using raw transcript");
            text.to_string()
        }
    }
}
