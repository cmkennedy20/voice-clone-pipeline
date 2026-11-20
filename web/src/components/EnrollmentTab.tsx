import React, { useEffect, useRef, useState } from 'react';
import { useMicrophoneRecorder } from '../audio/useMicrophoneRecorder';
import { useAppDispatch, useAppSelector } from '../store/hooks';
import { enrollProfile } from '../store/profilesSlice';
import {
  DEFAULT_SYNTHESIS_TUNING,
  ENROLLMENT_READING_SCRIPTS,
  isValidProfileSlug,
  type GuidedReadingScript,
  type SampleStyleLabel,
  type SynthesisTuning,
  type TtsEngineKind,
} from '../types/api';
import { AudioCaptureWidget } from './AudioCaptureWidget';
import { TuningControlsPanel } from './TuningControlsPanel';

export interface EnrollmentTabProps {
  onEnrolledSuccess?: () => void;
}

interface QueuedEnrollmentClip {
  id: string;
  fileName: string;
  audioBase64: string;
  transcriptEn: string;
  styleLabel: SampleStyleLabel;
  durationMs: number;
  previewUrl: string;
}

export const EnrollmentTab: React.FC<EnrollmentTabProps> = ({ onEnrolledSuccess }) => {
  const dispatch = useAppDispatch();
  const languages = useAppSelector((s) => s.languages.items);
  const { mutationStatus, error } = useAppSelector((s) => s.profiles);

  const recorder = useMicrophoneRecorder();

  const [slug, setSlug] = useState('studio_voice');
  const [displayName, setDisplayName] = useState('My Studio Voice');
  const [preferredEngine, setPreferredEngine] = useState<TtsEngineKind>('cosyvoice_3');
  const [defaultLanguage, setDefaultLanguage] = useState('es');
  const [avatarUrl, setAvatarUrl] = useState('');
  const [clipStyle, setClipStyle] = useState<SampleStyleLabel>('conversational');
  const [transcriptEn, setTranscriptEn] = useState(
    ENROLLMENT_READING_SCRIPTS[0]?.transcript ?? '',
  );
  const [queuedClips, setQueuedClips] = useState<QueuedEnrollmentClip[]>([]);
  const [initialTuning, setInitialTuning] = useState<SynthesisTuning>({
    ...DEFAULT_SYNTHESIS_TUNING,
  });
  const [localSuccess, setLocalSuccess] = useState<string | null>(null);
  const [localValidation, setLocalValidation] = useState<string | null>(null);

  const queuedUrlsRef = useRef<string[]>([]);

  useEffect(() => {
    return () => {
      for (const url of queuedUrlsRef.current) {
        URL.revokeObjectURL(url);
      }
    };
  }, []);

  const handleSelectGuidedScript = (script: GuidedReadingScript) => {
    setTranscriptEn(script.transcript);
    if (script.styleLabel) {
      setClipStyle(script.styleLabel);
    }
  };

  const handleQueueCurrentClip = () => {
    setLocalValidation(null);
    setLocalSuccess(null);

    if (!recorder.clip) {
      setLocalValidation(
        'Record a microphone clip or upload an audio file before adding to the multi-sample queue.',
      );
      return;
    }
    if (recorder.clip.metricsMeasured && !recorder.clip.gate.passed) {
      setLocalValidation(
        'Reference clip did not pass Gate 0 audio checks. Please re-record without clipping or silence.',
      );
      return;
    }
    if (!transcriptEn.trim()) {
      setLocalValidation(
        'Please enter or populate the exact spoken English transcript before queuing this clip.',
      );
      return;
    }

    const copyBuf = new ArrayBuffer(recorder.clip.rawBytes.byteLength);
    new Uint8Array(copyBuf).set(recorder.clip.rawBytes);
    const blob = new Blob([copyBuf], { type: recorder.clip.mimeType || 'audio/wav' });
    const ownedPreviewUrl = URL.createObjectURL(blob);
    queuedUrlsRef.current.push(ownedPreviewUrl);

    const nextItem: QueuedEnrollmentClip = {
      id: `queued-${Date.now()}-${queuedClips.length + 1}`,
      fileName: recorder.clip.fileName,
      audioBase64: recorder.clip.base64Payload,
      transcriptEn: transcriptEn.trim(),
      styleLabel: clipStyle,
      durationMs: recorder.clip.durationMs || 15_000,
      previewUrl: ownedPreviewUrl,
    };

    setQueuedClips((prev) => [...prev, nextItem]);
    recorder.clearClip();
  };

  const handleRemoveQueuedClip = (id: string) => {
    setQueuedClips((prev) => {
      const target = prev.find((c) => c.id === id);
      if (target) {
        URL.revokeObjectURL(target.previewUrl);
        queuedUrlsRef.current = queuedUrlsRef.current.filter(
          (u) => u !== target.previewUrl,
        );
      }
      return prev.filter((c) => c.id !== id);
    });
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setLocalSuccess(null);
    setLocalValidation(null);

    if (recorder.clip && recorder.clip.metricsMeasured && !recorder.clip.gate.passed) {
      setLocalValidation(
        'Reference clip did not pass Gate 0 audio checks. Please re-record without clipping or silence.',
      );
      return;
    }

    const combinedClips: Array<{
      audioBase64: string;
      transcriptEn: string;
      styleLabel: SampleStyleLabel;
    }> = queuedClips.map((q) => ({
      audioBase64: q.audioBase64,
      transcriptEn: q.transcriptEn,
      styleLabel: q.styleLabel,
    }));

    if (recorder.clip) {
      if (!transcriptEn.trim()) {
        setLocalValidation(
          'Profile slug, display name, and exact English reference transcript are required.',
        );
        return;
      }
      combinedClips.push({
        audioBase64: recorder.clip.base64Payload,
        transcriptEn: transcriptEn.trim(),
        styleLabel: clipStyle,
      });
    }

    if (combinedClips.length === 0) {
      setLocalValidation(
        'Please record a 12–20 second microphone clip or upload a reference audio file first.',
      );
      return;
    }

    const trimmedSlug = slug.trim();
    const primaryClip = combinedClips[0]!;
    if (!trimmedSlug || !displayName.trim() || !primaryClip.transcriptEn) {
      setLocalValidation(
        'Profile slug, display name, and exact English reference transcript are required.',
      );
      return;
    }
    if (!isValidProfileSlug(trimmedSlug)) {
      setLocalValidation(
        'Profile slug must be 2–64 characters, start with a lowercase letter (a–z), and contain only lowercase letters, digits, `-`, or `_`.',
      );
      return;
    }

    const additionalSamples =
      combinedClips.length > 1
        ? combinedClips.slice(1).map((c) => ({
            audio_bytes: c.audioBase64,
            transcript_en: c.transcriptEn,
            style_label: c.styleLabel,
          }))
        : undefined;

    const action = await dispatch(
      enrollProfile({
        slug: trimmedSlug,
        display_name: displayName.trim(),
        avatar_url: avatarUrl.trim().length > 0 ? avatarUrl.trim() : null,
        reference_audio_bytes: primaryClip.audioBase64,
        reference_transcript_en: primaryClip.transcriptEn,
        preferred_engine: preferredEngine,
        default_target_language: defaultLanguage,
        ...(additionalSamples ? { additional_samples: additionalSamples } : {}),
        tuning: initialTuning,
      }),
    );

    if (enrollProfile.fulfilled.match(action)) {
      for (const q of queuedClips) {
        URL.revokeObjectURL(q.previewUrl);
      }
      queuedUrlsRef.current = [];
      setQueuedClips([]);
      recorder.clearClip();
      setLocalSuccess(
        `Enrolled voice profile "${action.payload.display_name}" (${action.payload.slug}) with ${action.payload.sample_count} reference clip(s)! It is now selected in the Playground.`,
      );
      onEnrolledSuccess?.();
    }
  };

  const totalQueuedMs = queuedClips.reduce((acc, c) => acc + c.durationMs, 0);

  return (
    <div className="studio-two-col">
      <section className="panel">
        <div className="panel-header">
          <div>
            <h2>Step 1: Record 1–6 Diverse Reference Voice Clips</h2>
            <p className="muted">
              Enroll immediately with <strong>1 clip (12–20s)</strong>, or stack{' '}
              <strong>3–6 diverse clips (60–90s total)</strong> across calm,
              conversational, and expressive styles for multi-sample latent averaging and
              RTX 3090 fine-tuning.
            </p>
          </div>
        </div>

        <div className="tips-strip">
          <div className="tip-chip">Quiet room (no fan/echo)</div>
          <div className="tip-chip">15–20 cm off-axis from mic</div>
          <div className="tip-chip">3–6 clips = Best Likeness</div>
          <div className="tip-chip">Zero red clipping (≤ 50 bps)</div>
        </div>

        <div className="guided-scripts-section">
          <h3>6 Diverse Phonetically-Balanced Guided Reading Scripts (1-Click Populate)</h3>
          <div className="scripts-grid">
            {ENROLLMENT_READING_SCRIPTS.map((script) => (
              <div key={script.id} className="script-card">
                <div className="label-row">
                  <strong>{script.title}</strong>
                  <span className="badge badge-neutral">{script.estimatedSeconds}</span>
                </div>
                <p className="reading-passage">“{script.transcript}”</p>
                <div className="label-row">
                  {script.styleLabel && (
                    <span className="pill">Style: {script.styleLabel}</span>
                  )}
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={() => handleSelectGuidedScript(script)}
                  >
                    Use This Script & Populate Transcript
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>

        <AudioCaptureWidget
          recorder={recorder}
          mode="enrollment"
          inputId="enrollment-audio-upload"
        />

        <div className="subpanel mt-xs form-stack">
          <div className="form-grid-2">
            <div className="field-group">
              <label htmlFor="enroll-clip-style">Clip Delivery Style Tag</label>
              <select
                id="enroll-clip-style"
                value={clipStyle}
                onChange={(e) => setClipStyle(e.target.value as SampleStyleLabel)}
              >
                <option value="conversational">conversational</option>
                <option value="calm">calm</option>
                <option value="expressive">expressive</option>
                <option value="crisp">crisp</option>
              </select>
            </div>
            <div className="field-group">
              <label>Multi-Sample Enrollment Queue</label>
              <button
                type="button"
                className="btn btn-secondary"
                disabled={!recorder.clip}
                onClick={handleQueueCurrentClip}
              >
                + Add Clip to Profile Queue
              </button>
            </div>
          </div>

          {queuedClips.length > 0 && (
            <div className="samples-list" data-testid="enrollment-clip-queue">
              <div className="label-row">
                <strong>
                  Queued Reference Clips ({queuedClips.length}{' '}
                  {queuedClips.length === 1 ? 'clip' : 'clips'} ·{' '}
                  {(totalQueuedMs / 1000).toFixed(1)}s total)
                </strong>
                <span
                  className={`badge ${
                    queuedClips.length >= 3 ? 'badge-ok' : 'badge-neutral'
                  }`}
                >
                  {queuedClips.length >= 3
                    ? '3+ Clips · Multi-Sample Ready'
                    : 'Add 2+ more clips for Multi-Sample Averaging'}
                </span>
              </div>

              {queuedClips.map((item, idx) => (
                <div key={item.id} className="sample-item-card">
                  <div className="sample-item-top">
                    <div className="inline-actions">
                      <strong>Clip #{idx + 1}</strong>
                      {idx === 0 && <span className="badge badge-ok">Primary</span>}
                      <span className="badge badge-neutral">{item.styleLabel}</span>
                      <span className="pill">{(item.durationMs / 1000).toFixed(1)}s</span>
                    </div>
                    <button
                      type="button"
                      className="btn btn-ghost btn-sm"
                      onClick={() => handleRemoveQueuedClip(item.id)}
                    >
                      Remove
                    </button>
                  </div>
                  <div className="small muted">“{item.transcriptEn}”</div>
                  <audio controls src={item.previewUrl} className="audio-element" />
                </div>
              ))}
            </div>
          )}
        </div>
      </section>

      <section className="panel">
        <div className="panel-header">
          <div>
            <h2>Step 2: Save Voice Profile Metadata & Likeness Tuning</h2>
            <p className="muted">
              Exact English transcripts align acoustic tokens in CosyVoice 3.0 & XTTS-v2,
              while initial synthesis tuning presets dial in speaker similarity.
            </p>
          </div>
        </div>

        <form noValidate onSubmit={(e) => void handleSubmit(e)} className="form-stack">
          <div className="form-grid-2">
            <div className="field-group">
              <label htmlFor="enroll-slug">Profile Slug (`slug`)</label>
              <input
                id="enroll-slug"
                type="text"
                required
                minLength={2}
                maxLength={64}
                pattern="^[a-z][a-z0-9_-]{1,63}$"
                value={slug}
                onChange={(e) => setSlug(e.target.value)}
                placeholder="studio_voice"
              />
              <div className="field-hint">
                2–64 chars; starts with `a-z`, then lowercase letters, digits, `-`, or `_`.
              </div>
            </div>

            <div className="field-group">
              <label htmlFor="enroll-display-name">Display Name</label>
              <input
                id="enroll-display-name"
                type="text"
                required
                value={displayName}
                onChange={(e) => setDisplayName(e.target.value)}
                placeholder="My Studio Voice"
              />
            </div>
          </div>

          <div className="form-grid-2">
            <div className="field-group">
              <label htmlFor="enroll-preferred-engine">Default TTS Engine</label>
              <select
                id="enroll-preferred-engine"
                value={preferredEngine}
                onChange={(e) => setPreferredEngine(e.target.value as TtsEngineKind)}
              >
                <option value="cosyvoice_3">CosyVoice 3.0 (cosyvoice_3)</option>
                <option value="xtts_v2">Coqui XTTS-v2 (xtts_v2)</option>
              </select>
            </div>

            <div className="field-group">
              <label htmlFor="enroll-default-lang">Default Target Language</label>
              <select
                id="enroll-default-lang"
                value={defaultLanguage}
                onChange={(e) => setDefaultLanguage(e.target.value)}
              >
                {languages.map((lang) => (
                  <option key={lang.code} value={lang.code}>
                    {lang.english_name} ({lang.native_name}) [{lang.code}]
                  </option>
                ))}
              </select>
            </div>
          </div>

          <div className="field-group">
            <label htmlFor="enroll-avatar-url">Discord Webhook Avatar URL (Optional)</label>
            <input
              id="enroll-avatar-url"
              type="url"
              value={avatarUrl}
              onChange={(e) => setAvatarUrl(e.target.value)}
              placeholder="https://cdn.discordapp.com/avatars/..."
            />
          </div>

          <div className="field-group">
            <label htmlFor="enroll-transcript">
              Exact Spoken English Transcript (`reference_transcript_en`)
            </label>
            <textarea
              id="enroll-transcript"
              rows={3}
              required={queuedClips.length === 0}
              value={transcriptEn}
              onChange={(e) => setTranscriptEn(e.target.value)}
              placeholder="Type the exact words spoken in your current reference recording..."
            />
          </div>

          <div className="field-group">
            <label>Initial Voice Likeness & Synthesis Tuning</label>
            <TuningControlsPanel
              value={initialTuning}
              onChange={setInitialTuning}
              idPrefix="enroll-tuning"
              compact
            />
          </div>

          {localValidation && (
            <div className="alert alert-error" role="alert">
              {localValidation}
            </div>
          )}

          {error && (
            <div className="alert alert-error" role="alert">
              {error}
            </div>
          )}

          {localSuccess && (
            <div className="alert alert-ok" role="status">
              {localSuccess}
            </div>
          )}

          <button
            type="submit"
            className="btn btn-primary btn-lg"
            disabled={mutationStatus === 'saving'}
          >
            {mutationStatus === 'saving'
              ? 'Uploading 24 kHz WAV & Enrolling Profile...'
              : queuedClips.length > 0
                ? `Enroll Voice Profile (${queuedClips.length + (recorder.clip ? 1 : 0)} Reference Clips)`
                : 'Enroll Voice Profile'}
          </button>
        </form>
      </section>
    </div>
  );
};
