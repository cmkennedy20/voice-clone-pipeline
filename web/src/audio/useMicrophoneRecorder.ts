import { useCallback, useEffect, useRef, useState } from 'react';
import {
  blobToBytes,
  bytesToBase64,
  CLIPPING_AMPLITUDE_THRESHOLD,
  encodeMonoWav24k,
  evaluateClientAudioGate,
  type GateEvaluation,
} from './wavEncoder';

export interface CapturedAudioClip {
  fileName: string;
  mimeType: string;
  rawBytes: Uint8Array;
  base64Payload: string;
  previewUrl: string;
  durationMs: number;
  peakBps: number;
  clippedBps: number;
  metricsMeasured: boolean;
  gate: GateEvaluation;
}

export interface MicrophoneRecorderState {
  isRecording: boolean;
  elapsedMs: number;
  liveLevelBps: number;
  liveClippedBps: number;
  clip: CapturedAudioClip | null;
  error: string | null;
  startRecording: (outputFileName?: string) => Promise<void>;
  stopRecording: () => void;
  loadAudioFile: (file: File) => Promise<void>;
  clearClip: () => void;
}

function createAudioContextInstance(): AudioContext | null {
  const win = window as Window & { webkitAudioContext?: typeof AudioContext };
  const Ctor = window.AudioContext ?? win.webkitAudioContext;
  return Ctor ? new Ctor() : null;
}

export function useMicrophoneRecorder(): MicrophoneRecorderState {
  const [isRecording, setIsRecording] = useState(false);
  const [elapsedMs, setElapsedMs] = useState(0);
  const [liveLevelBps, setLiveLevelBps] = useState(0);
  const [liveClippedBps, setLiveClippedBps] = useState(0);
  const [clip, setClip] = useState<CapturedAudioClip | null>(null);
  const [error, setError] = useState<string | null>(null);

  const streamRef = useRef<MediaStream | null>(null);
  const audioCtxRef = useRef<AudioContext | null>(null);
  const processorRef = useRef<ScriptProcessorNode | null>(null);
  const sourceNodeRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const chunksRef = useRef<Float32Array[]>([]);
  const totalSamplesRef = useRef(0);
  const clippedSamplesRef = useRef(0);
  const startTimeRef = useRef(0);
  const timerRef = useRef<number | null>(null);
  const outputNameRef = useRef('studio-recording-24k.wav');
  const previewUrlRef = useRef<string | null>(null);

  const revokeCurrentPreview = useCallback(() => {
    if (previewUrlRef.current) {
      URL.revokeObjectURL(previewUrlRef.current);
      previewUrlRef.current = null;
    }
  }, []);

  const cleanupAudioGraph = useCallback(() => {
    if (timerRef.current !== null) {
      window.clearInterval(timerRef.current);
      timerRef.current = null;
    }
    if (processorRef.current) {
      processorRef.current.onaudioprocess = null;
      processorRef.current.disconnect();
      processorRef.current = null;
    }
    if (sourceNodeRef.current) {
      sourceNodeRef.current.disconnect();
      sourceNodeRef.current = null;
    }
    if (streamRef.current) {
      for (const track of streamRef.current.getTracks()) {
        track.stop();
      }
      streamRef.current = null;
    }
    if (audioCtxRef.current) {
      void audioCtxRef.current.close().catch(() => {});
      audioCtxRef.current = null;
    }
  }, []);

  useEffect(() => {
    return () => {
      cleanupAudioGraph();
      revokeCurrentPreview();
    };
  }, [cleanupAudioGraph, revokeCurrentPreview]);

  const stopRecording = useCallback(() => {
    const sampleRate = audioCtxRef.current?.sampleRate ?? 48_000;
    const chunks = chunksRef.current;
    const totalSamples = totalSamplesRef.current;

    cleanupAudioGraph();
    setIsRecording(false);
    setLiveLevelBps(0);

    if (totalSamples === 0) {
      return;
    }

    const merged = new Float32Array(totalSamples);
    let offset = 0;
    for (const chunk of chunks) {
      merged.set(chunk, offset);
      offset += chunk.length;
    }
    chunksRef.current = [];
    totalSamplesRef.current = 0;
    clippedSamplesRef.current = 0;

    const encoded = encodeMonoWav24k(merged, sampleRate);
    const gate = evaluateClientAudioGate(encoded);
    const copyBuffer = new ArrayBuffer(encoded.wavBytes.byteLength);
    new Uint8Array(copyBuffer).set(encoded.wavBytes);
    const wavBlob = new Blob([copyBuffer], { type: 'audio/wav' });
    revokeCurrentPreview();
    const previewUrl = URL.createObjectURL(wavBlob);
    previewUrlRef.current = previewUrl;

    setClip({
      fileName: outputNameRef.current,
      mimeType: 'audio/wav',
      rawBytes: encoded.wavBytes,
      base64Payload: bytesToBase64(encoded.wavBytes),
      previewUrl,
      durationMs: encoded.durationMs,
      peakBps: encoded.peakBps,
      clippedBps: encoded.clippedBps,
      metricsMeasured: true,
      gate,
    });
  }, [cleanupAudioGraph, revokeCurrentPreview]);

  const startRecording = useCallback(
    async (outputFileName = 'studio-recording-24k.wav') => {
      setError(null);
      if (!navigator.mediaDevices?.getUserMedia) {
        const isLanHttp =
          typeof window !== 'undefined' &&
          window.location?.protocol === 'http:' &&
          window.location.hostname !== '' &&
          window.location.hostname !== 'localhost' &&
          window.location.hostname !== '127.0.0.1' &&
          window.location.hostname !== '[::1]';
        const httpsTarget = isLanHttp
          ? `https://${window.location.hostname}:3443`
          : 'https://localhost:3443';
        setError(
          `Live browser microphone capture over LAN requires HTTPS (navigator.mediaDevices.getUserMedia unavailable on plain HTTP) — open ${httpsTarget} (accept the local self-signed cert once), or drag-and-drop an audio file below.`,
        );
        return;
      }

      try {
        cleanupAudioGraph();
        outputNameRef.current = outputFileName;
        chunksRef.current = [];
        totalSamplesRef.current = 0;
        clippedSamplesRef.current = 0;

        const stream = await navigator.mediaDevices.getUserMedia({
          audio: {
            channelCount: 1,
            echoCancellation: false,
            noiseSuppression: false,
            autoGainControl: false,
          },
        });
        const audioCtx = createAudioContextInstance();
        if (!audioCtx) {
          for (const track of stream.getTracks()) {
            track.stop();
          }
          setError('Web Audio API (AudioContext) is not supported in this browser.');
          return;
        }

        streamRef.current = stream;
        audioCtxRef.current = audioCtx;
        const source = audioCtx.createMediaStreamSource(stream);
        const processor = audioCtx.createScriptProcessor(4096, 1, 1);
        sourceNodeRef.current = source;
        processorRef.current = processor;

        processor.onaudioprocess = (event: AudioProcessingEvent) => {
          const input = event.inputBuffer.getChannelData(0);
          const copy = new Float32Array(input.length);
          copy.set(input);
          chunksRef.current.push(copy);
          totalSamplesRef.current += copy.length;

          let framePeak = 0;
          let frameClipped = 0;
          for (let i = 0; i < copy.length; i += 1) {
            const abs = Math.abs(copy[i] ?? 0);
            if (abs > framePeak) {
              framePeak = abs;
            }
            if (abs >= CLIPPING_AMPLITUDE_THRESHOLD) {
              frameClipped += 1;
            }
          }
          clippedSamplesRef.current += frameClipped;
          const total = Math.max(1, totalSamplesRef.current);
          setLiveLevelBps(Math.min(10_000, Math.round(framePeak * 10_000)));
          setLiveClippedBps(Math.round((clippedSamplesRef.current * 10_000) / total));
        };

        // Connect through a silent gain node so microphone audio does not feedback into speakers
        const silentGain = audioCtx.createGain();
        silentGain.gain.value = 0;
        source.connect(processor);
        processor.connect(silentGain);
        silentGain.connect(audioCtx.destination);

        startTimeRef.current = Date.now();
        setElapsedMs(0);
        setLiveClippedBps(0);
        setIsRecording(true);

        timerRef.current = window.setInterval(() => {
          setElapsedMs(Date.now() - startTimeRef.current);
        }, 100);
      } catch (err) {
        cleanupAudioGraph();
        setIsRecording(false);
        setError(
          err instanceof Error
            ? `Microphone permission or capture error: ${err.message}`
            : 'Unable to start microphone capture.',
        );
      }
    },
    [cleanupAudioGraph],
  );

  const loadAudioFile = useCallback(
    async (file: File) => {
      setError(null);
      try {
        const rawFileBytes = await blobToBytes(file);
        if (rawFileBytes.byteLength === 0) {
          setError('Selected audio file is empty.');
          return;
        }

        // Attempt browser decode so uploaded .m4a / .mp3 / .ogg / .wav can also be inspected for Gate 0 & converted to 24kHz WAV if possible
        const audioCtx = createAudioContextInstance();
        if (audioCtx && typeof audioCtx.decodeAudioData === 'function') {
          try {
            const copyBuf = new ArrayBuffer(rawFileBytes.byteLength);
            new Uint8Array(copyBuf).set(rawFileBytes);
            const decoded = await audioCtx.decodeAudioData(copyBuf);
            const mono = new Float32Array(decoded.length);
            const channels = decoded.numberOfChannels;
            for (let ch = 0; ch < channels; ch += 1) {
              const chData = decoded.getChannelData(ch);
              for (let i = 0; i < decoded.length; i += 1) {
                mono[i] = (mono[i] ?? 0) + (chData[i] ?? 0) / channels;
              }
            }
            await audioCtx.close().catch(() => {});

            const encoded = encodeMonoWav24k(mono, decoded.sampleRate);
            const gate = evaluateClientAudioGate(encoded);
            const wavBuf = new ArrayBuffer(encoded.wavBytes.byteLength);
            new Uint8Array(wavBuf).set(encoded.wavBytes);
            const wavBlob = new Blob([wavBuf], { type: 'audio/wav' });
            revokeCurrentPreview();
            const previewUrl = URL.createObjectURL(wavBlob);
            previewUrlRef.current = previewUrl;

            const baseName = file.name.replace(/\.[^.]+$/, '');
            setClip({
              fileName: `${baseName}-24k.wav`,
              mimeType: 'audio/wav',
              rawBytes: encoded.wavBytes,
              base64Payload: bytesToBase64(encoded.wavBytes),
              previewUrl,
              durationMs: encoded.durationMs,
              peakBps: encoded.peakBps,
              clippedBps: encoded.clippedBps,
              metricsMeasured: true,
              gate,
            });
            return;
          } catch {
            await audioCtx.close().catch(() => {});
          }
        }

        // Fallback: pass raw file bytes directly to the backend Symphonia decoder
        revokeCurrentPreview();
        const previewUrl = URL.createObjectURL(file);
        previewUrlRef.current = previewUrl;
        setClip({
          fileName: file.name,
          mimeType: file.type || 'application/octet-stream',
          rawBytes: rawFileBytes,
          base64Payload: bytesToBase64(rawFileBytes),
          previewUrl,
          durationMs: 0,
          peakBps: 0,
          clippedBps: 0,
          metricsMeasured: false,
          gate: { passed: true, issues: [] },
        });
      } catch (err) {
        setError(
          err instanceof Error
            ? `Failed reading audio file: ${err.message}`
            : 'Failed reading audio file.',
        );
      }
    },
    [revokeCurrentPreview],
  );

  const clearClip = useCallback(() => {
    revokeCurrentPreview();
    setClip(null);
    setElapsedMs(0);
    setLiveLevelBps(0);
    setLiveClippedBps(0);
    setError(null);
  }, [revokeCurrentPreview]);

  return {
    isRecording,
    elapsedMs,
    liveLevelBps,
    liveClippedBps,
    clip,
    error,
    startRecording,
    stopRecording,
    loadAudioFile,
    clearClip,
  };
}
