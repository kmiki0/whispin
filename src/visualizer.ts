// Mirrored-histogram audio level visualizer. Owns just the canvas + the
// rolling sample buffer; the actual level value is fed in from the audio
// graph in recorder.ts.

const SAMPLES = 96;
const samples = new Float32Array(SAMPLES);

let canvasEl: HTMLCanvasElement | null = null;
let ctx: CanvasRenderingContext2D | null = null;

export function initCanvas(canvas: HTMLCanvasElement) {
  canvasEl = canvas;
  ctx = canvas.getContext("2d");
}

export function clearSamples() {
  for (let i = 0; i < SAMPLES; i++) samples[i] = 0;
}

export function pushSample(level: number) {
  samples.copyWithin(0, 1);
  samples[SAMPLES - 1] = level;
  drawWave();
}

export function drawWave() {
  if (!ctx || !canvasEl) return;
  const w = canvasEl.width;
  const h = canvasEl.height;
  ctx.clearRect(0, 0, w, h);
  ctx.fillStyle = "rgba(230, 110, 110, 0.92)";

  const BAR_W = 2.5;
  const BAR_GAP = 1.5;
  const stride = BAR_W + BAR_GAP;
  const numBars = Math.floor(w / stride);
  const minH = 2;
  const maxH = h - 2;
  const step = SAMPLES / numBars;

  for (let i = 0; i < numBars; i++) {
    const start = Math.floor(i * step);
    const end = Math.max(start + 1, Math.floor((i + 1) * step));
    let sum = 0;
    let count = 0;
    for (let j = start; j < end && j < SAMPLES; j++) {
      sum += samples[j];
      count++;
    }
    const level = count > 0 ? sum / count : 0;
    const barH = Math.max(minH, level * maxH);
    const x = i * stride;
    const y = h - barH;
    ctx.beginPath();
    ctx.roundRect(x, y, BAR_W, barH, BAR_W / 2);
    ctx.fill();
  }
}
