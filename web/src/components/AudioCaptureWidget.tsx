import React, { useRef, useState } from 'react';
import type { MicrophoneRecorderState } from '../audio/useMicrophoneRecorder';
import { GATE0_MAX_CLIPPED_BPS } from '../audio/wavEncoder';

export interface AudioCaptureWidgetProps {
  recorder: MicrophoneRecorderState;
  mode: 'enrollment' | 'clip';
  inputId: string;
}

export const AudioCaptureWidget: React.FC<AudioCaptureWidgetProps> = ({
  recorder,
  mode,
  inputId,
}) => {
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const [isDragging, setIsDragging] = useState(false);

  const activeDurationSec = recorder.isRecording
    ? recorder.elapsedMs / 1_000
    : (recorder.clip?.durationMs ?? 0) / 1_000;

  const levelPercent = Math.min(100, Math.round(recorder.liveLevelBps / 100));
  const activeClippedBps = recorder.isRecording
    ? recorder.liveClippedBps
    : (recorder.clip?.clippedBps ?? 0);
  const isClipping = activeClippedBps > GATE0_MAX_CLIPPED_BPS;

  // Target 12–20s window progress bar (capped visually at 22s = 100%)
  const enrollmentProgressPct = Math.min(100, Math.round((activeDurationSec / 20) * 100));
  const inSweetSpot = activeDurationSec >= 12 && activeDurationSec <= 20;

  const lanHttpsUrl =
    typeof window !== 'undefined' &&
    window.location?.protocol === 'http:' &&
    window.location.hostname !== '' &&
    window.location.hostname !== 'localhost' &&
    window.location.hostname !== '127.0.0.1' &&
    window.location.hostname !== '[::1]'
      ? `https://${window.location.hostname}:3443`
      : null;

  const handleDrop = (event: React.DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    setIsDragging(false);
    const file = event.dataTransfer.files?.[0];
    if (file) {
      void recorder.loadAudioFile(file);
    }
  };

  const handleFileChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (file) {
      void recorder.loadAudioFile(file);
    }
  };

  return (
    <div className="capture-widget">
      {lanHttpsUrl && (
        <div className="alert alert-warn" data-testid="lan-https-mic-callout">
          Live browser microphone capture over LAN requires HTTPS —{' '}
          <a
            href={lanHttpsUrl}
            style={{ color: 'inherit', fontWeight: 700, textDecoration: 'underline' }}
          >
            click here to open <code>{lanHttpsUrl}</code>
          </a>{' '}
          (accept the local self-signed cert once), or drag-and-drop an audio file below.
        </div>
      )}

      <div className="capture-controls-row">
        {!recorder.isRecording ? (
          <button
            type="button"
            className="btn btn-record"
            onClick={() =>
              void recorder.startRecording(
                mode === 'enrollment' ? 'reference-24k-mono.wav' : 'voice-prompt-24k.wav',
              )
            }
          >
            ● Record Microphone (24 kHz WAV)
          </button>
        ) : (
          <button
            type="button"
            className="btn btn-danger"
            onClick={() => recorder.stopRecording()}
          >
            ■ Stop Recording ({activeDurationSec.toFixed(1)}s)
          </button>
        )}

        <button
          type="button"
          className="btn btn-secondary"
          onClick={() => fileInputRef.current?.click()}
        >
          Upload Audio (.wav / .m4a / .mp3 / .ogg)
        </button>

        {recorder.clip && (
          <button
            type="button"
            className="btn btn-ghost"
            onClick={() => recorder.clearClip()}
          >
            Clear Clip
          </button>
        )}

        <input
          id={inputId}
          ref={fileInputRef}
          type="file"
          accept=".wav,.m4a,.mp3,.ogg,audio/*"
          onChange={handleFileChange}
          className="sr-only-file"
          aria-label={
            mode === 'enrollment'
              ? 'Upload Reference Audio File'
              : 'Upload Source Voice Clip'
          }
        />
      </div>

      {/* Real-time Mic Level & Gate 0 Clipping Meter */}
      <div className="meters-grid">
        <div className="meter-box">
          <div className="meter-header">
            <span>Live Mic Level</span>
            <span className="mono">{levelPercent}%</span>
          </div>
          <div className="progress-track" aria-label="Microphone input level">
            <div
              className={`progress-fill ${
                levelPercent > 95
                  ? 'fill-danger'
                  : levelPercent > 75
                    ? 'fill-warn'
                    : 'fill-ok'
              }`}
              style={{ width: `${levelPercent}%` }}
            />
          </div>
        </div>

        <div className="meter-box">
          <div className="meter-header">
            <span>Gate 0 Clipping Guard (≤ 50 bps / 0.50%)</span>
            <span className={`badge ${isClipping ? 'badge-danger' : 'badge-ok'}`}>
              {activeClippedBps} bps ({(activeClippedBps / 100).toFixed(2)}%)
            </span>
          </div>
          <div className="meter-subtext">
            {isClipping
              ? 'Clipping detected! Lower mic gain or step 15–20 cm back.'
              : 'Clean headroom — within Gate 0 acoustic ceiling.'}
          </div>
        </div>
      </div>

      {mode === 'enrollment' && (
        <div className="meter-box sweet-spot-box">
          <div className="meter-header">
            <span>Enrollment Target Duration (12–20s Sweet Spot)</span>
            <span className={`badge ${inSweetSpot ? 'badge-ok' : 'badge-neutral'}`}>
              {activeDurationSec.toFixed(1)}s / 15.0s target
            </span>
          </div>
          <div className="progress-track sweet-spot-track">
            <div
              className={`progress-fill ${inSweetSpot ? 'fill-ok' : 'fill-accent'}`}
              style={{ width: `${enrollmentProgressPct}%` }}
            />
          </div>
          <div className="meter-subtext">
            {activeDurationSec === 0
              ? 'Aim for 12–20 seconds of natural, steady English speech.'
              : activeDurationSec < 12
                ? 'Keep reading naturally until the bar reaches 12–20 seconds...'
                : activeDurationSec <= 20
                  ? 'Sweet spot reached! Finish your current sentence and press Stop.'
                  : 'Plenty of reference audio captured — you can stop whenever ready.'}
          </div>
        </div>
      )}

      {/* Drag & drop area */}
      <div
        className={`dropzone ${isDragging ? 'dropzone-active' : ''}`}
        onDragOver={(e) => {
          e.preventDefault();
          setIsDragging(true);
        }}
        onDragLeave={() => setIsDragging(false)}
        onDrop={handleDrop}
      >
        <span>
          Drag & drop a <code>.wav</code>, <code>.m4a</code>, <code>.mp3</code>, or{' '}
          <code>.ogg</code> clip here — browser recordings auto-encode to{' '}
          <strong>24,000 Hz 16-bit Mono PCM WAV</strong>.
        </span>
      </div>

      {recorder.error && (
        <div className="alert alert-error" role="alert">
          {recorder.error}
        </div>
      )}

      {recorder.clip && (
        <div className="captured-preview-card" data-testid="captured-audio-card">
          <div className="captured-header">
            <div>
              <strong>{recorder.clip.fileName}</strong>{' '}
              <span className="muted">
                ({(recorder.clip.rawBytes.byteLength / 1024).toFixed(1)} KB)
              </span>
            </div>
            {recorder.clip.metricsMeasured ? (
              <span
                className={`badge ${recorder.clip.gate.passed ? 'badge-ok' : 'badge-danger'}`}
              >
                {recorder.clip.gate.passed ? 'Gate 0 Pre-Check Passed' : 'Gate 0 Warning'}
              </span>
            ) : (
              <span className="badge badge-neutral">Raw Audio Ready for Server Decode</span>
            )}
          </div>

          {recorder.clip.metricsMeasured && (
            <div className="metrics-pills">
              <span className="pill">
                Duration: <strong>{(recorder.clip.durationMs / 1000).toFixed(2)}s</strong>
              </span>
              <span className="pill">
                Peak: <strong>{recorder.clip.peakBps} bps</strong> (
                {(recorder.clip.peakBps / 100).toFixed(1)}%)
              </span>
              <span className="pill">
                Clipped: <strong>{recorder.clip.clippedBps} bps</strong> (
                {(recorder.clip.clippedBps / 100).toFixed(2)}%)
              </span>
            </div>
          )}

          {!recorder.clip.gate.passed && (
            <ul className="gate-issues-list" role="alert">
              {recorder.clip.gate.issues.map((issue) => (
                <li key={issue}>{issue}</li>
              ))}
            </ul>
          )}

          <div className="audio-player-row">
            <audio controls src={recorder.clip.previewUrl} className="audio-element" />
            <a
              href={recorder.clip.previewUrl}
              download={recorder.clip.fileName}
              className="btn btn-ghost btn-sm"
            >
              Download Clip
            </a>
          </div>
        </div>
      )}
    </div>
  );
};
