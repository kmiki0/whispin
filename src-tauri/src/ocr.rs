// Active-window screen capture + Windows.Media.Ocr (Japanese) pipeline.
// Designed to run on a blocking task in parallel with audio recording so the
// OCR latency is hidden behind the user's speech duration.

#![cfg(windows)]

use anyhow::{anyhow, Result};
use image::ImageEncoder;
use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::BitmapDecoder;
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

/// Capture the window with the given HWND (as isize) and run Japanese OCR
/// on the result. Returns the extracted text (may contain newlines).
///
/// This function blocks. Run it on a thread that can block — e.g. via
/// `tokio::task::spawn_blocking` or `std::thread::spawn`.
pub fn capture_and_ocr(hwnd: isize) -> Result<String> {
    let png = capture_window_png(hwnd)?;
    ocr_japanese_png(&png)
}

fn capture_window_png(hwnd: isize) -> Result<Vec<u8>> {
    let windows = xcap::Window::all()
        .map_err(|e| anyhow!("xcap::Window::all failed: {e}"))?;
    let target = windows
        .into_iter()
        .find(|w| matches!(w.id(), Ok(id) if id as isize == hwnd))
        .ok_or_else(|| anyhow!("no xcap window matched hwnd {hwnd}"))?;
    let img = target
        .capture_image()
        .map_err(|e| anyhow!("capture_image failed: {e}"))?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err(anyhow!("captured image was empty ({w}x{h})"));
    }
    let mut png = Vec::with_capacity((w * h) as usize);
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .map_err(|e| anyhow!("png encode failed: {e}"))?;
    Ok(png)
}

fn ocr_japanese_png(png_bytes: &[u8]) -> Result<String> {
    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(png_bytes)?;
    writer.StoreAsync()?.get()?;
    writer.FlushAsync()?.get()?;
    writer.DetachStream()?;
    stream.Seek(0)?;

    let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
    let bitmap = decoder.GetSoftwareBitmapAsync()?.get()?;

    let lang = Language::CreateLanguage(&HSTRING::from("ja"))?;
    let engine = OcrEngine::TryCreateFromLanguage(&lang)
        .map_err(|e| anyhow!("OcrEngine::TryCreateFromLanguage failed: {e}"))?;
    let result = engine.RecognizeAsync(&bitmap)?.get()?;

    // Use Lines() rather than Text() so we keep one OCR line per output line.
    // Windows OCR's Text accessor returns everything joined with spaces only,
    // losing all paragraph / row structure.
    let lines = result.Lines()?;
    let mut out = String::new();
    for line in lines {
        let line_text = line.Text()?.to_string();
        out.push_str(&line_text);
        out.push('\n');
    }
    Ok(out)
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x303F |  // CJK Symbols and Punctuation (、 。 「 」 etc.)
        0x3040..=0x309F |  // Hiragana
        0x30A0..=0x30FF |  // Katakana (includes ー U+30FC)
        0x4E00..=0x9FFF |  // CJK Unified Ideographs
        0x3400..=0x4DBF |  // CJK Extension A
        0xFF00..=0xFFEF    // Halfwidth and Fullwidth Forms (includes ｦ-ﾝ ascii fullwidth)
    )
}

/// Characters we always keep as their own token even if they would otherwise
/// be classed as "pure symbol". Arrows are common in code/UI and may carry
/// meaning; dashes and similar may actually be a long-vowel mark (ー) that
/// the OCR engine misrecognized.
const PRESERVE_CHARS: &[char] = &[
    '→', '←', '↑', '↓', '⇒', '⇐', '⇑', '⇓',
    // dash family — may be a chōonpu ー misread
    '-', '−', '—', '–', '―',
];

/// Tokens we consistently see from OCR misrecognizing window-chrome glyphs
/// (icons, min/max/close buttons, separators). Single-character only.
/// NOTE: We deliberately do NOT include 'ロ' (katakana ro) or '第', because
/// those appear in legitimate Japanese (ログ, 第一 etc.). The (ロ, X) pair
/// pattern below handles the maximize+close button case.
const NOISE_SINGLE_CHARS: &[char] = &[
    '・', '|', '·', '○', '●', '◇', '◆', '□', '■', '▶', '▷', '◀', '◁', '▲', '▼',
    '✕', '☓',
];

/// Tokens that show up next to icons but aren't useful on their own.
const NOISE_PAIRS: &[(&str, &str)] = &[
    ("ロ", "X"),
    ("口", "X"),
    ("ロ", "x"),
    ("口", "x"),
];

fn is_noise_token(t: &str) -> bool {
    let chars: Vec<char> = t.chars().collect();
    if chars.is_empty() {
        return true;
    }
    // Whitelist: keep arrows / dashes (likely chōonpu) untouched.
    if chars.iter().all(|c| PRESERVE_CHARS.contains(c)) {
        return false;
    }
    if chars.len() == 1 && NOISE_SINGLE_CHARS.contains(&chars[0]) {
        return true;
    }
    // Pure symbol token (no letters/digits/CJK).
    let has_meaning = chars
        .iter()
        .any(|c| c.is_alphanumeric() || is_cjk(*c));
    if !has_meaning {
        return true;
    }
    false
}

/// Clean common OCR artifacts: drop window-chrome glyph noise, collapse
/// spaces inserted between adjacent CJK characters, but preserve line
/// structure so the LLM can read paragraphs naturally.
pub fn clean_text(raw: &str) -> String {
    let cleaned_lines: Vec<String> = raw
        .lines()
        .map(clean_line)
        .filter(|s| !s.trim().is_empty())
        .collect();
    cleaned_lines.join("\n")
}

fn clean_line(line: &str) -> String {
    // Step 1: tokenize by whitespace within the line, drop noise tokens.
    let raw_tokens: Vec<&str> = line.split_whitespace().collect();
    let mut tokens: Vec<&str> = Vec::with_capacity(raw_tokens.len());
    let mut i = 0;
    while i < raw_tokens.len() {
        let t = raw_tokens[i];
        if is_noise_token(t) {
            i += 1;
            continue;
        }
        if i + 1 < raw_tokens.len() {
            let next = raw_tokens[i + 1];
            if NOISE_PAIRS.iter().any(|(a, b)| *a == t && *b == next) {
                i += 2;
                continue;
            }
        }
        tokens.push(t);
        i += 1;
    }

    // Step 2: rejoin and collapse single spaces between adjacent CJK chars.
    let joined: String = tokens.join(" ");
    let chars: Vec<char> = joined.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        out.push(c);
        if is_cjk(c)
            && i + 2 < chars.len()
            && chars[i + 1] == ' '
            && is_cjk(chars[i + 2])
        {
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

