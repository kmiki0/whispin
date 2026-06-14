# Whispin

日本語特化の常駐音声入力ツール。Tauri 2 + Groq 無料枠で動作。

## 動作概要 (v1)

1. **F9 長押し** で録音開始（v1暫定。v2で右Alt長押しに変更予定）
2. キーを離すと録音停止 → Whisper Large v3 Turbo で文字起こし
3. クリップボードに書き込み、元のフォーカスウィンドウに復帰して `Ctrl+V` を送出

## 必要環境

- Windows 11
- Node.js LTS
- Rust (rustup, stable-x86_64-pc-windows-msvc)
- Visual Studio 2022 Build Tools (C++ ワークロード + Windows 11 SDK)
- WebView2 ランタイム (Win11 はプリインストール済み)

## セットアップ

### 1. Groq API キー

クレカ不要で取得 → `https://console.groq.com/keys`

環境変数にセット (例 PowerShell ユーザー設定):
```powershell
[System.Environment]::SetEnvironmentVariable('GROQ_API_KEY', 'gsk_xxx...', 'User')
```

新しいシェルを開いて反映を確認:
```powershell
$env:GROQ_API_KEY
```

### 2. 依存インストール

```powershell
npm install
```

## 開発実行

このマシンでは VS BuildTools の `vcvars64.bat` が `INCLUDE`/`LIB` を設定しない (Windows SDK のレジストリ登録が欠けている) ため、ラッパースクリプトで env を補ってから dev サーバを起動する:

```powershell
pwsh scripts/dev.ps1
```

初回ビルドは 5〜10 分かかる。以降は差分のみ。

## 設計

`docs/design.md` 参照予定 (要件・アーキテクチャ・段階実装計画)。

## v1 のスコープと制限

- ホットキー: F9 長押し (push-to-talk)。本来は右Alt長押し希望だが、`tauri-plugin-global-shortcut` が bare AltRight を Windows VK にマップしないため暫定で F9
- 文脈補正 (OCR / Qwen3 後処理) は v2 で追加
- オーバーレイの音声反応アニメは v3 で追加
- 録音終了は「キーを離す」のみ (無音検知は未実装)
