import { describe, expect, it } from 'vitest';
import {
  bytesToBase64,
  encodeMonoWav24k,
  evaluateClientAudioGate,
} from '../audio/wavEncoder';

function readAscii(bytes: Uint8Array, offset: number, length: number): string {
  return String.fromCharCode(...bytes.subarray(offset, offset + length));
}

describe('wavEncoder (24 kHz Mono 16-Bit PCM RIFF/WAVE + Gate 0 Metrics)', () => {
  it('resamples 48 kHz mono float32 audio into a valid 24,000 Hz 16-bit PCM RIFF/WAVE buffer', () => {
    const inputSampleRate = 48_000;
    const inputSamples = new Float32Array(48_000); // 1.000 second
    for (let i = 0; i < inputSamples.length; i += 1) {
      inputSamples[i] = 0.5 * Math.sin((2 * Math.PI * 400 * i) / inputSampleRate);
    }

    const result = encodeMonoWav24k(inputSamples, inputSampleRate);

    // 24,000 samples * 2 bytes + 44-byte RIFF header = 48,044 bytes
    expect(result.wavBytes.byteLength).toBe(48_044);
    expect(result.durationMs).toBe(1_000);
    expect(result.clippedBps).toBe(0);
    expect(result.peakBps).toBe(5_000);

    const view = new DataView(
      result.wavBytes.buffer,
      result.wavBytes.byteOffset,
      result.wavBytes.byteLength,
    );

    expect(readAscii(result.wavBytes, 0, 4)).toBe('RIFF');
    expect(view.getUint32(4, true)).toBe(48_036); // 36 + 48,000
    expect(readAscii(result.wavBytes, 8, 4)).toBe('WAVE');
    expect(readAscii(result.wavBytes, 12, 4)).toBe('fmt ');
    expect(view.getUint32(16, true)).toBe(16); // PCM fmt chunk length
    expect(view.getUint16(20, true)).toBe(1); // PCM format = 1
    expect(view.getUint16(22, true)).toBe(1); // Mono = 1 channel
    expect(view.getUint32(24, true)).toBe(24_000); // Sample rate = 24,000 Hz
    expect(view.getUint32(28, true)).toBe(48_000); // Byte rate = 24,000 * 1 * 2
    expect(view.getUint16(32, true)).toBe(2); // Block align = 2 bytes
    expect(view.getUint16(34, true)).toBe(16); // 16 bits per sample
    expect(readAscii(result.wavBytes, 36, 4)).toBe('data');
    expect(view.getUint32(40, true)).toBe(48_000); // 24,000 * 2 bytes
  });

  it('computes exact clippedBps and peakBps and flags Gate 0 violations at exact boundaries', () => {
    // 24,000 samples at 24 kHz: 240 clipped samples (1.00% = 100 bps), rest at 0.25 amplitude
    const samples = new Float32Array(24_000);
    samples.fill(0.25);
    for (let i = 0; i < 240; i += 1) {
      samples[i] = i % 2 === 0 ? 1.0 : -1.0;
    }

    const encoded = encodeMonoWav24k(samples, 24_000);
    expect(encoded.peakBps).toBe(10_000);
    expect(encoded.clippedBps).toBe(100);

    const clippedGate = evaluateClientAudioGate(encoded);
    expect(clippedGate.passed).toBe(false);
    expect(clippedGate.issues.join(' ')).toMatch(/50 bps/i);

    // Exact inclusive Gate 0 boundaries: 400 ms, 100 bps peak, 50 bps clipping
    expect(
      evaluateClientAudioGate({
        durationMs: 400,
        peakBps: 100,
        clippedBps: 50,
      }),
    ).toEqual({ passed: true, issues: [] });

    // Just outside boundaries: 399 ms, 99 bps peak, 51 bps clipping
    const failing = evaluateClientAudioGate({
      durationMs: 399,
      peakBps: 99,
      clippedBps: 51,
    });
    expect(failing.passed).toBe(false);
    expect(failing.issues).toHaveLength(3);
  });

  it('encodes RFC 4648 base64 accurately including large multi-kilobyte WAV payloads without stack overflow', () => {
    expect(bytesToBase64(new Uint8Array([0x4d, 0x61, 0x6e]))).toBe('TWFu');
    expect(bytesToBase64(new Uint8Array([0x52, 0x49, 0x46, 0x46]))).toBe('UklGRg==');

    const largePayload = new Uint8Array(120_000);
    for (let i = 0; i < largePayload.length; i += 1) {
      largePayload[i] = i % 256;
    }
    const b64 = bytesToBase64(largePayload);
    expect(b64.length).toBe(160_000); // 120,000 / 3 * 4
  });
});
