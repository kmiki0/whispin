// Recording pipeline: getUserMedia → MediaRecorder + audio graph (visualizer
// + compressor/gain → MediaRecorder). On stop, base64-encodes the blob and
// hands it to the Rust transcribe command.
//
// Also drives silence-detection auto-stop using the same AnalyserNode the
// visualizer reads.

import { invoke } from "@tauri-apps/api/core";
import { setStatus, scheduleHide, cancelHide } from "./notch";
import { clearSamples, drawWave, pushSample } from "./visualizer";

type RecordingConfig = {
  mode: "ptt" | "toggle";
  auto_stop_on_silence: boolean;
  silence_timeout_ms: number;
};

const DEFAULT_RECORDING_CONFIG: RecordingConfig = {
  mode: "ptt",
  auto_stop_on_silence: true,
  silence_timeout_ms: 1500,
};

const SILENCE_LEVEL_THRESHOLD = 0.04;
const SILENCE_GRACE_MS = 600;
// If the loudest moment of the whole take stays below this, treat it as "no
// speech" and skip transcription entirely. Whisper otherwise hallucinates text
// (e.g. "ご視聴ありがとうございました") on silent / near-silent audio.
const SPEECH_PEAK_THRESHOLD = 0.12;

let mediaRecorder: MediaRecorder | null = null;
let activeStream: MediaStream | null = null;
let chunks: BlobPart[] = [];
let audioCtx: AudioContext | null = null;
let analyser: AnalyserNode | null = null;
let analyserBuffer: Uint8Array | null = null;
let rafId: number | null = null;
let currentLevel = 0;
let peakLevel = 0;

let activeRecordingConfig: RecordingConfig = DEFAULT_RECORDING_CONFIG;
let silenceStartMs = 0;
let recordingStartedAt = 0;

// Pre-warmed mic stream. getUserMedia() can stall 100-500ms (device wake +
// permission + AGC negotiation), which clips the first words when acquired on
// the hot path. We acquire once at startup and keep it live so a trigger press
// starts capture in parallel with the notch appearing, not after it.
let warmStream: MediaStream | null = null;
let warmMicId: string | null = null;

function setupAudioGraph(stream: MediaStream): MediaStream | null {
  try {
    audioCtx = new AudioContext();
    const source = audioCtx.createMediaStreamSource(stream);

    // Branch 1: raw → analyser (visualizer + silence detect).
    analyser = audioCtx.createAnalyser();
    analyser.fftSize = 1024;
    analyser.smoothingTimeConstant = 0.4;
    source.connect(analyser);
    analyserBuffer = new Uint8Array(analyser.fftSize);
    clearSamples();
    currentLevel = 0;
    drawWave();

    // Branch 2: raw → compressor → gain → destination (recording).
    // Software AGC: smooths quiet/loud talkers so Whisper sees uniform volume.
    const compressor = audioCtx.createDynamicsCompressor();
    compressor.threshold.value = -32;
    compressor.knee.value = 24;
    compressor.ratio.value = 6;
    compressor.attack.value = 0.003;
    compressor.release.value = 0.18;

    const gain = audioCtx.createGain();
    gain.gain.value = 1.6;

    const dest = audioCtx.createMediaStreamDestination();
    source.connect(compressor);
    compressor.connect(gain);
    gain.connect(dest);

    rafId = requestAnimationFrame(tick);
    return dest.stream;
  } catch (e) {
    console.error("audio graph setup failed", e);
    return null;
  }
}

function tick() {
  if (!analyser || !analyserBuffer) return;
  analyser.getByteTimeDomainData(analyserBuffer);
  let sumSq = 0;
  for (let i = 0; i < analyserBuffer.length; i++) {
    const v = (analyserBuffer[i] - 128) / 128.0;
    sumSq += v * v;
  }
  const rms = Math.sqrt(sumSq / analyserBuffer.length);
  // Higher gain + sqrt curve so normal conversational volume reaches ~70-90%.
  const lin = Math.max(0, (rms - 0.012) * 7.0);
  const raw = Math.min(1, Math.sqrt(lin));
  // Fast attack / slow release for natural breathing.
  const alpha = raw > currentLevel ? 0.5 : 0.15;
  currentLevel = currentLevel + (raw - currentLevel) * alpha;
  if (currentLevel > peakLevel) peakLevel = currentLevel;
  pushSample(currentLevel);
  maybeAutoStopOnSilence();
  rafId = requestAnimationFrame(tick);
}

function maybeAutoStopOnSilence() {
  // PTT mode never auto-stops; release is the only stop signal.
  if (activeRecordingConfig.mode !== "toggle") return;
  if (!mediaRecorder || mediaRecorder.state !== "recording") return;
  const now = performance.now();
  if (now - recordingStartedAt < SILENCE_GRACE_MS) {
    silenceStartMs = now;
    return;
  }
  if (currentLevel > SILENCE_LEVEL_THRESHOLD) {
    silenceStartMs = now;
    return;
  }
  if (now - silenceStartMs >= activeRecordingConfig.silence_timeout_ms) {
    console.log("[whispin] auto-stop on silence");
    stopRecording();
  }
}

function stopVisualizer() {
  if (rafId !== null) {
    cancelAnimationFrame(rafId);
    rafId = null;
  }
  if (audioCtx) {
    audioCtx.close().catch(() => undefined);
    audioCtx = null;
  }
  analyser = null;
  analyserBuffer = null;
  clearSamples();
}

function pickMimeType(): string {
  const candidates = [
    "audio/webm;codecs=opus",
    "audio/webm",
    "audio/ogg;codecs=opus",
    "audio/mp4",
  ];
  for (const m of candidates) {
    if (MediaRecorder.isTypeSupported(m)) return m;
  }
  return "";
}

function micConstraints(micId: string): MediaTrackConstraints {
  const constraints: MediaTrackConstraints = {
    echoCancellation: true,
    noiseSuppression: true,
    // OS-level AGC off so the visualizer sees raw amplitude. The recording
    // branch has its own software AGC (compressor+gain).
    autoGainControl: false,
  };
  if (micId) {
    constraints.deviceId = { exact: micId };
  }
  return constraints;
}

async function acquireMicStream(micId: string): Promise<MediaStream> {
  try {
    return await navigator.mediaDevices.getUserMedia({
      audio: micConstraints(micId),
    });
  } catch (e) {
    // The pinned mic may have been unplugged. Rather than fail the whole
    // recording, fall back to the OS default device.
    if (micId) {
      console.warn(
        "[whispin] selected mic unavailable; falling back to default",
        e,
      );
      return await navigator.mediaDevices.getUserMedia({
        audio: micConstraints(""),
      });
    }
    throw e;
  }
}

function streamLive(stream: MediaStream): boolean {
  return stream.getAudioTracks().some((t) => t.readyState === "live");
}

async function loadMicId(): Promise<string> {
  try {
    const general = await invoke<{ mic_device_id: string }>(
      "get_general_config",
    );
    return general?.mic_device_id || "";
  } catch {
    // not available; fall back to default mic
    return "";
  }
}

async function loadConfigs(): Promise<{ micId: string }> {
  const micId = await loadMicId();
  try {
    activeRecordingConfig = await invoke<RecordingConfig>(
      "get_recording_config",
    );
  } catch {
    // keep previous / default
  }
  return { micId };
}

// Acquire the mic ahead of time (at startup and after each take) so the next
// trigger press can start recording without waiting on getUserMedia.
export async function prewarmMic(): Promise<void> {
  if (warmStream && streamLive(warmStream)) return;
  try {
    const micId = await loadMicId();
    warmStream = await acquireMicStream(micId);
    warmMicId = micId;
  } catch (e) {
    console.warn(
      "[whispin] mic prewarm failed (will acquire on demand)",
      e,
    );
    warmStream = null;
    warmMicId = null;
  }
}

export async function startRecording() {
  cancelHide();
  if (mediaRecorder && mediaRecorder.state !== "inactive") return;
  try {
    const { micId } = await loadConfigs();
    // Reuse the pre-warmed stream when it still matches the configured mic and
    // is live — this skips getUserMedia on the hot path so capture starts in
    // parallel with the notch appearing. Otherwise acquire one now.
    if (warmStream && warmMicId === micId && streamLive(warmStream)) {
      activeStream = warmStream;
    } else {
      if (warmStream) {
        warmStream.getTracks().forEach((t) => t.stop());
      }
      activeStream = await acquireMicStream(micId);
    }
    warmStream = null;
    warmMicId = null;
    const recordingStream = setupAudioGraph(activeStream);
    if (!recordingStream) {
      throw new Error("audio graph setup failed");
    }
    chunks = [];
    const mimeType = pickMimeType();
    mediaRecorder = new MediaRecorder(
      recordingStream,
      mimeType ? { mimeType } : undefined,
    );
    mediaRecorder.ondataavailable = (e) => {
      if (e.data.size > 0) chunks.push(e.data);
    };
    mediaRecorder.onstop = handleStop;
    mediaRecorder.start();
    recordingStartedAt = performance.now();
    silenceStartMs = recordingStartedAt;
    peakLevel = 0;
    setStatus("recording");
  } catch (e) {
    console.error("startRecording failed", e);
    setStatus("error", `mic: ${(e as Error).message}`);
    scheduleHide(2000);
    // Recording never started — make sure the screen-context overlay is taken
    // down and state is reset.
    invoke("notify_recording_stopped").catch(() => undefined);
  }
}

export function stopRecording() {
  if (mediaRecorder && mediaRecorder.state !== "inactive") {
    mediaRecorder.stop();
  }
}

async function handleStop() {
  stopVisualizer();
  const recorder = mediaRecorder;
  mediaRecorder = null;
  if (!recorder) return;
  const mime = recorder.mimeType || "audio/webm";
  const blob = new Blob(chunks, { type: mime });
  chunks = [];
  activeStream?.getTracks().forEach((t) => t.stop());
  activeStream = null;
  // Re-warm for the next take (keeps the mic ready so subsequent presses also
  // start instantly).
  void prewarmMic();

  invoke("notify_recording_stopped").catch((e) =>
    console.error("notify_recording_stopped failed", e),
  );

  if (blob.size < 1000) {
    setStatus("error", "too short");
    scheduleHide(1200);
    return;
  }

  // No clear speech in the whole take → skip ASR so Whisper can't hallucinate
  // text onto silence.
  if (peakLevel < SPEECH_PEAK_THRESHOLD) {
    console.log(
      `[whispin] no speech detected (peak ${peakLevel.toFixed(3)}) — skipping transcription`,
    );
    setStatus("done", "🙊");
    scheduleHide(1000);
    return;
  }

  setStatus("transcribing");
  try {
    const buffer = await blob.arrayBuffer();
    const b64 = arrayBufferToBase64(buffer);
    await invoke<string>("transcribe", { audioB64: b64, mimeType: mime });
    setStatus("done", "👋");
    // morph (320ms) + buffer (100ms) + reveal (200ms) + visible waving (~900ms).
    scheduleHide(1500);
  } catch (e) {
    console.error("transcribe failed", e);
    setStatus("error", String(e).slice(0, 80));
    scheduleHide(3000);
  }
}

function arrayBufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let binary = "";
  const chunkSize = 0x8000;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    binary += String.fromCharCode.apply(
      null,
      Array.from(bytes.subarray(i, i + chunkSize)),
    );
  }
  return btoa(binary);
}
