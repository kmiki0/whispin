// Backend type mirrors. Kept in one place so each section module can import
// what it needs without redeclaring.

export type MouseButton = "Left" | "Right" | "Middle" | "X1" | "X2";

export type TriggerInput =
  | { kind: "key"; vk: number }
  | { kind: "mouse"; button: MouseButton };

export type Modifiers = {
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  win: boolean;
};

export type TriggerConfig = {
  input: TriggerInput;
  modifiers: Modifiers;
  long_press_ms: number;
};

export type ApiKeys = {
  groq: string;
  openai: string;
  openrouter: string;
};

export type LlmConfig = {
  enabled: boolean;
  model: string;
  short_threshold_chars: number;
  timeout_secs: number;
  use_screen_context: boolean;
};

export type GeneralConfig = {
  mic_device_id: string;
};

export type DictionaryEntry = {
  word: string;
  readings: string[];
};

export type RecordingMode = "ptt" | "toggle";

export type RecordingConfig = {
  mode: RecordingMode;
  auto_stop_on_silence: boolean;
  silence_timeout_ms: number;
};
