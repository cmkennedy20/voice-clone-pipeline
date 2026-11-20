export const TARGET_WAV_SAMPLE_RATE = 24_000;
export const GATE0_MIN_DURATION_MS = 400;
export const GATE0_MAX_DURATION_MS = 120_000;
export const GATE0_MIN_PEAK_BPS = 100;
export const GATE0_MAX_CLIPPED_BPS = 50;
export const CLIPPING_AMPLITUDE_THRESHOLD = 0.999;

export interface EncodedWavResult {
  wavBytes: Uint8Array;
  durationMs: number;
  peakBps: number;
  clippedBps: number;
}

export interface GateEvaluation {
  passed: boolean;
  issues: string[];
}

function writeAscii(view: DataView, offset: number, text: string): void {
  for (let i = 0; i < text.length; i += 1) {
    view.setUint8(offset + i, text.charCodeAt(i));
  }
}

export function resampleTo24kMono(
  samples: Float32Array,
  inputSampleRate: number,
): Float32Array {
  if (samples.length === 0 || inputSampleRate <= 0) {
    return new Float32Array(0);
  }
  if (inputSampleRate === TARGET_WAV_SAMPLE_RATE) {
    return samples;
  }

  const ratio = inputSampleRate / TARGET_WAV_SAMPLE_RATE;
  const outputLength = Math.max(1, Math.round(samples.length / ratio));
  const out = new Float32Array(outputLength);
  const lastIndex = samples.length - 1;

  for (let i = 0; i < outputLength; i += 1) {
    const srcPos = i * ratio;
    const idx0 = Math.min(Math.floor(srcPos), lastIndex);
    const idx1 = Math.min(idx0 + 1, lastIndex);
    const frac = srcPos - idx0;
    const s0 = samples[idx0] ?? 0;
    const s1 = samples[idx1] ?? 0;
    out[i] = s0 + (s1 - s0) * frac;
  }

  return out;
}

export function measureSamplesBps(samples: Float32Array): {
  peakBps: number;
  clippedBps: number;
} {
  if (samples.length === 0) {
    return { peakBps: 0, clippedBps: 0 };
  }

  let peak = 0;
  let clippedCount = 0;

  for (let i = 0; i < samples.length; i += 1) {
    const abs = Math.abs(samples[i] ?? 0);
    if (abs > peak) {
      peak = abs;
    }
    if (abs >= CLIPPING_AMPLITUDE_THRESHOLD) {
      clippedCount += 1;
    }
  }

  const clampedPeak = Math.min(1, peak);
  const peakBps = Math.round(clampedPeak * 10_000);
  const clippedBps = Math.round((clippedCount * 10_000) / samples.length);
  return { peakBps, clippedBps };
}

export function encodeMonoWav24k(
  samples: Float32Array,
  inputSampleRate: number,
): EncodedWavResult {
  const resampled = resampleTo24kMono(samples, inputSampleRate);
  const inputMetrics = measureSamplesBps(samples);
  const resampledMetrics = measureSamplesBps(resampled);

  const peakBps = Math.max(inputMetrics.peakBps, resampledMetrics.peakBps);
  const clippedBps = Math.max(inputMetrics.clippedBps, resampledMetrics.clippedBps);
  const durationMs = Math.round((resampled.length * 1_000) / TARGET_WAV_SAMPLE_RATE);

  const dataByteLength = resampled.length * 2;
  const buffer = new ArrayBuffer(44 + dataByteLength);
  const view = new DataView(buffer);

  writeAscii(view, 0, 'RIFF');
  view.setUint32(4, 36 + dataByteLength, true);
  writeAscii(view, 8, 'WAVE');
  writeAscii(view, 12, 'fmt ');
  view.setUint32(16, 16, true); // PCM chunk size
  view.setUint16(20, 1, true); // AudioFormat = 1 (Linear PCM)
  view.setUint16(22, 1, true); // NumChannels = 1 (Mono)
  view.setUint32(24, TARGET_WAV_SAMPLE_RATE, true);
  view.setUint32(28, TARGET_WAV_SAMPLE_RATE * 2, true); // ByteRate = SampleRate * 1 * 2
  view.setUint16(32, 2, true); // BlockAlign = 2 bytes
  view.setUint16(34, 16, true); // BitsPerSample = 16
  writeAscii(view, 36, 'data');
  view.setUint32(40, dataByteLength, true);

  let byteOffset = 44;
  for (let i = 0; i < resampled.length; i += 1) {
    const clamped = Math.max(-1, Math.min(1, resampled[i] ?? 0));
    const pcm16 = clamped < 0 ? Math.round(clamped * 32_768) : Math.round(clamped * 32_767);
    view.setInt16(byteOffset, pcm16, true);
    byteOffset += 2;
  }

  return {
    wavBytes: new Uint8Array(buffer),
    durationMs,
    peakBps,
    clippedBps,
  };
}

export function evaluateClientAudioGate(metrics: {
  durationMs: number;
  peakBps: number;
  clippedBps: number;
}): GateEvaluation {
  const issues: string[] = [];

  if (metrics.durationMs < GATE0_MIN_DURATION_MS) {
    issues.push(
      `Clip duration (${metrics.durationMs} ms) is shorter than Gate 0 minimum (${GATE0_MIN_DURATION_MS} ms).`,
    );
  }
  if (metrics.durationMs > GATE0_MAX_DURATION_MS) {
    issues.push(
      `Clip duration (${metrics.durationMs} ms) exceeds Gate 0 maximum (${GATE0_MAX_DURATION_MS} ms).`,
    );
  }
  if (metrics.peakBps < GATE0_MIN_PEAK_BPS) {
    issues.push(
      `Peak level (${metrics.peakBps} bps / ${(metrics.peakBps / 100).toFixed(2)}%) is below Gate 0 silence floor (${GATE0_MIN_PEAK_BPS} bps).`,
    );
  }
  if (metrics.clippedBps > GATE0_MAX_CLIPPED_BPS) {
    issues.push(
      `Clipped samples (${metrics.clippedBps} bps / ${(metrics.clippedBps / 100).toFixed(2)}%) exceed Gate 0 ceiling (${GATE0_MAX_CLIPPED_BPS} bps / 0.50%). Step slightly back from the mic or reduce input gain.`,
    );
  }

  return {
    passed: issues.length === 0,
    issues,
  };
}

export function bytesToBase64(bytes: Uint8Array): string {
  const chunkSize = 0x8000;
  const chunks: string[] = [];
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    const slice = bytes.subarray(offset, offset + chunkSize);
    let binary = '';
    for (let i = 0; i < slice.length; i += 1) {
      binary += String.fromCharCode(slice[i] ?? 0);
    }
    chunks.push(binary);
  }
  return btoa(chunks.join(''));
}

export async function blobToBytes(blob: Blob): Promise<Uint8Array> {
  if (typeof blob.arrayBuffer === 'function') {
    const arrayBuffer = await blob.arrayBuffer();
    return new Uint8Array(arrayBuffer);
  }
  return new Promise<Uint8Array>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      if (reader.result instanceof ArrayBuffer) {
        resolve(new Uint8Array(reader.result));
      } else {
        reject(new Error('Unexpected FileReader result'));
      }
    };
    reader.onerror = () => reject(reader.error ?? new Error('FileReader error'));
    reader.readAsArrayBuffer(blob);
  });
}
