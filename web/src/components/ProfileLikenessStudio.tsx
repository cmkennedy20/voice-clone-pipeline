import React, { useEffect, useRef, useState } from 'react';
import {
  apiFetchProfileReferenceAudio,
  apiFetchProfileSampleAudio,
  apiGetProfileFinetuneStatus,
} from '../api/client';
import { useMicrophoneRecorder } from '../audio/useMicrophoneRecorder';
import { useAppDispatch, useAppSelector } from '../store/hooks';
import {
  addProfileSample,
  deleteProfileSample,
  fetchProfiles,
  startProfileFinetune,
  updateProfile,
} from '../store/profilesSlice';
import {
  ENROLLMENT_READING_SCRIPTS,
  type SampleStyleLabel,
  type SynthesisTuning,
  type VoiceFinetuneJob,
  type VoiceProfile,
  type VoiceProfileSample,
} from '../types/api';
import { AudioCaptureWidget } from './AudioCaptureWidget';
import { TuningControlsPanel } from './TuningControlsPanel';

export function getProfileLikenessBadge(profile: VoiceProfile): {
  label: string;
  badgeClass: string;
} {
  if (
    profile.finetune_status === 'succeeded' ||
    Boolean(profile.finetuned_checkpoint_dir)
  ) {
    return {
      label: 'Fine-Tuned on RTX 3090',
      badgeClass: 'badge-ok',
    };
  }
  if (profile.sample_count >= 3) {
    return {
      label: '3+ clips · Multi-Sample Averaged',
      badgeClass: 'badge-ok',
    };
  }
  if (profile.sample_count === 2) {
    return {
      label: '2 clips · Multi-Sample',
      badgeClass: 'badge-warn',
    };
  }
  return {
    label: '1 clip · Basic',
    badgeClass: 'badge-neutral',
  };
}

export interface ProfileLikenessStudioProps {
  profile: VoiceProfile;
}

const SAMPLE_STYLE_OPTIONS: Array<{ value: SampleStyleLabel; label: string }> = [
  { value: 'conversational', label: 'Conversational (Natural everyday)' },
  { value: 'calm', label: 'Calm (Steady resonance)' },
  { value: 'expressive', label: 'Expressive (Dynamic pitch & warmth)' },
  { value: 'crisp', label: 'Crisp (Clear consonant articulation)' },
];

export const ProfileLikenessStudio: React.FC<ProfileLikenessStudioProps> = ({
  profile,
}) => {
  const dispatch = useAppDispatch();
  const connection = useAppSelector((s) => s.connection);
  const mutationStatus = useAppSelector((s) => s.profiles.mutationStatus);

  const sampleRecorder = useMicrophoneRecorder();

  const [selectedScriptId, setSelectedScriptId] = useState<string>(
    ENROLLMENT_READING_SCRIPTS[2]?.id ?? ENROLLMENT_READING_SCRIPTS[0]?.id ?? '',
  );
  const [sampleStyle, setSampleStyle] = useState<SampleStyleLabel>(
    ENROLLMENT_READING_SCRIPTS[2]?.styleLabel ?? 'calm',
  );
  const [sampleTranscript, setSampleTranscript] = useState<string>(
    ENROLLMENT_READING_SCRIPTS[2]?.transcript ??
      ENROLLMENT_READING_SCRIPTS[0]?.transcript ??
      '',
  );
  const [makePrimary, setMakePrimary] = useState(false);
  const [sampleMessage, setSampleMessage] = useState<string | null>(null);
  const [sampleError, setSampleError] = useState<string | null>(null);

  const [sampleAudioUrls, setSampleAudioUrls] = useState<Record<string, string>>({});
  const [loadingSampleId, setLoadingSampleId] = useState<string | null>(null);

  const [tuningDraft, setTuningDraft] = useState<SynthesisTuning>({
    ...profile.tuning,
  });
  const [tuningSavedMsg, setTuningSavedMsg] = useState<string | null>(null);

  const [epochs, setEpochs] = useState<number>(6);
  const [learningRate, setLearningRate] = useState<string>('0.000005');
  const [finetuneJob, setFinetuneJob] = useState<VoiceFinetuneJob | null>(null);
  const [finetuneMsg, setFinetuneMsg] = useState<string | null>(null);
  const [finetuneLocalErr, setFinetuneLocalErr] = useState<string | null>(null);

  useEffect(() => {
    setTuningDraft({ ...profile.tuning });
  }, [profile.tuning]);

  useEffect(() => {
    return () => {
      for (const url of Object.values(sampleAudioUrls)) {
        URL.revokeObjectURL(url);
      }
    };
  }, [sampleAudioUrls]);

  const connSettings = {
    apiBaseUrl: connection.apiBaseUrl,
    bearerToken: connection.bearerToken,
    discordUserId: connection.discordUserId,
  };

  const hydratedSlugRef = useRef<string | null>(null);

  // Hydrate initial fine-tune status once on mount if the profile was already queued/running
  useEffect(() => {
    if (hydratedSlugRef.current === profile.slug) {
      return undefined;
    }
    hydratedSlugRef.current = profile.slug;
    if (profile.finetune_status !== 'queued' && profile.finetune_status !== 'running') {
      return undefined;
    }
    let cancelled = false;
    void apiGetProfileFinetuneStatus(connSettings, profile.slug)
      .then((job) => {
        if (!cancelled && job) {
          setFinetuneJob(job);
          if (job.status === 'succeeded' || job.status === 'failed') {
            void dispatch(fetchProfiles());
          }
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [
    connection.apiBaseUrl,
    connection.bearerToken,
    connection.discordUserId,
    dispatch,
    profile.finetune_status,
    profile.slug,
  ]);

  const activeFinetuneStatus = finetuneJob?.status ?? profile.finetune_status;
  const shouldPollFinetune =
    activeFinetuneStatus === 'queued' || activeFinetuneStatus === 'running';

  // Auto-poll GET /api/v1/profiles/{slug}/finetune every 2.5s while queued or running
  useEffect(() => {
    if (!shouldPollFinetune) {
      return undefined;
    }
    let cancelled = false;
    const intervalId = window.setInterval(() => {
      void apiGetProfileFinetuneStatus(
        {
          apiBaseUrl: connection.apiBaseUrl,
          bearerToken: connection.bearerToken,
          discordUserId: connection.discordUserId,
        },
        profile.slug,
      )
        .then((job) => {
          if (cancelled || !job) {
            return;
          }
          setFinetuneJob(job);
          if (job.status === 'succeeded') {
            setFinetuneMsg(
              'RTX 3090 XTTS-v2 GPT fine-tuning completed! Dedicated checkpoint is now active.',
            );
            void dispatch(fetchProfiles());
          } else if (job.status === 'failed') {
            setFinetuneLocalErr(
              job.error_message ?? 'Fine-tuning job failed on GPU worker.',
            );
            void dispatch(fetchProfiles());
          }
        })
        .catch((err: unknown) => {
          if (!cancelled) {
            setFinetuneLocalErr(
              err instanceof Error ? err.message : 'Failed polling fine-tune status.',
            );
          }
        });
    }, 2500);

    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
    };
  }, [
    connection.apiBaseUrl,
    connection.bearerToken,
    connection.discordUserId,
    dispatch,
    profile.slug,
    shouldPollFinetune,
  ]);

  const displaySamples: Array<VoiceProfileSample & { isSyntheticFallback?: boolean }> =
    profile.samples.length > 0
      ? profile.samples
      : [
          {
            id: 'primary-reference',
            profile_id: profile.id,
            wav_path: profile.reference_wav_path,
            transcript_en: profile.reference_transcript_en,
            style_label: 'conversational',
            duration_ms: profile.total_sample_duration_ms || 15_000,
            is_primary: true,
            created_at: profile.created_at,
            isSyntheticFallback: true,
          },
        ];

  const effectiveClipCount = Math.max(profile.sample_count, profile.samples.length);
  const totalSeconds = (profile.total_sample_duration_ms / 1000).toFixed(1);
  const likenessBadge = getProfileLikenessBadge(profile);

  const handleSelectScript = (scriptId: string) => {
    setSelectedScriptId(scriptId);
    const found = ENROLLMENT_READING_SCRIPTS.find((s) => s.id === scriptId);
    if (found) {
      setSampleTranscript(found.transcript);
      if (found.styleLabel) {
        setSampleStyle(found.styleLabel);
      }
    }
  };

  const handlePlaySample = async (
    sample: VoiceProfileSample & { isSyntheticFallback?: boolean },
  ) => {
    setLoadingSampleId(sample.id);
    setSampleError(null);
    try {
      const blob = sample.isSyntheticFallback
        ? await apiFetchProfileReferenceAudio(connSettings, profile.slug)
        : await apiFetchProfileSampleAudio(connSettings, profile.slug, sample.id);
      const url = URL.createObjectURL(blob);
      setSampleAudioUrls((prev) => {
        if (prev[sample.id]) {
          URL.revokeObjectURL(prev[sample.id]!);
        }
        return { ...prev, [sample.id]: url };
      });
    } catch (err) {
      setSampleError(
        err instanceof Error ? err.message : 'Failed loading sample audio.',
      );
    } finally {
      setLoadingSampleId(null);
    }
  };

  const handleAddSample = async () => {
    setSampleMessage(null);
    setSampleError(null);
    if (!sampleRecorder.clip) {
      setSampleError(
        'Please record a microphone clip or upload an audio file before adding a sample.',
      );
      return;
    }
    if (sampleRecorder.clip.metricsMeasured && !sampleRecorder.clip.gate.passed) {
      setSampleError(
        'Clip did not pass Gate 0 audio checks. Please re-record without clipping or silence.',
      );
      return;
    }
    if (!sampleTranscript.trim()) {
      setSampleError('Exact spoken English transcript is required for this sample.');
      return;
    }

    const action = await dispatch(
      addProfileSample({
        slug: profile.slug,
        payload: {
          audio_bytes: sampleRecorder.clip.base64Payload,
          transcript_en: sampleTranscript.trim(),
          style_label: sampleStyle,
          ...(makePrimary ? { make_primary: true } : {}),
        },
      }),
    );

    if (addProfileSample.fulfilled.match(action)) {
      sampleRecorder.clearClip();
      setMakePrimary(false);
      setSampleMessage(
        `Added new "${sampleStyle}" sample! Profile now averages ${action.payload.sample_count} reference clips (${(action.payload.total_sample_duration_ms / 1000).toFixed(1)}s total).`,
      );
    } else if (addProfileSample.rejected.match(action)) {
      setSampleError(action.payload ?? 'Failed adding sample to profile.');
    }
  };

  const handleDeleteSample = async (sampleId: string) => {
    setSampleMessage(null);
    setSampleError(null);
    const action = await dispatch(
      deleteProfileSample({ slug: profile.slug, sampleId }),
    );
    if (deleteProfileSample.rejected.match(action)) {
      setSampleError(action.payload ?? 'Failed deleting sample.');
    }
  };

  const handleSaveProfileTuning = async () => {
    setTuningSavedMsg(null);
    const action = await dispatch(
      updateProfile({
        slug: profile.slug,
        patch: { tuning: tuningDraft },
      }),
    );
    if (updateProfile.fulfilled.match(action)) {
      setTuningSavedMsg('Saved default synthesis tuning for this voice profile.');
    }
  };

  const handleStartFinetune = async () => {
    setFinetuneMsg(null);
    setFinetuneLocalErr(null);
    const parsedEpochs = Math.min(20, Math.max(2, Math.round(epochs || 6)));
    const parsedLr = Number(learningRate);
    const safeLr = Number.isFinite(parsedLr) && parsedLr > 0 ? parsedLr : 0.000005;

    const action = await dispatch(
      startProfileFinetune({
        slug: profile.slug,
        epochs: parsedEpochs,
        learning_rate: safeLr,
      }),
    );

    if (startProfileFinetune.fulfilled.match(action)) {
      const job = action.payload.job;
      setFinetuneJob(job);
      if (job.status === 'succeeded') {
        setFinetuneMsg(
          'RTX 3090 XTTS-v2 GPT fine-tuning completed! Dedicated checkpoint is now active.',
        );
        void dispatch(fetchProfiles());
      } else {
        setFinetuneMsg(
          `RTX 3090 fine-tune job started (${job.epochs} epochs @ LR ${job.learning_rate}). Auto-polling GPU progress every 2.5s...`,
        );
      }
    } else if (startProfileFinetune.rejected.match(action)) {
      setFinetuneLocalErr(action.payload ?? 'Failed starting RTX 3090 fine-tune.');
    }
  };

  const handleRevertZeroShot = async () => {
    setFinetuneMsg(null);
    setFinetuneLocalErr(null);
    const action = await dispatch(
      updateProfile({
        slug: profile.slug,
        patch: { use_finetuned_checkpoint: false },
      }),
    );
    if (updateProfile.fulfilled.match(action)) {
      setFinetuneJob(null);
      setFinetuneMsg(
        'Reverted profile to zero-shot multi-sample base XTTS-v2 conditioning.',
      );
    }
  };

  const hasActiveFinetuneCheckpoint =
    profile.finetune_status === 'succeeded' ||
    Boolean(profile.finetuned_checkpoint_dir) ||
    finetuneJob?.status === 'succeeded';

  const progressPct = finetuneJob
    ? Math.min(100, Math.max(0, Math.round(finetuneJob.progress_bps / 100)))
    : profile.finetune_status === 'succeeded'
      ? 100
      : 0;

  return (
    <div
      className="likeness-workspace"
      data-testid={`likeness-studio-${profile.slug}`}
    >
      {/* 1. Reference Sample Library */}
      <div className="subpanel form-stack">
        <div className="panel-header">
          <div>
            <h4>1. Reference Sample Library & Multi-Clip Averaging</h4>
            <p className="muted small">
              Stacking 3–10 diverse clips (45–120s total) lets XTTS-v2 average speaker
              latents across calm, conversational, and expressive speech for dramatically
              closer voice likeness.
            </p>
          </div>
          <div className="inline-actions">
            <span className="pill" data-testid={`profile-sample-summary-${profile.slug}`}>
              <strong>{effectiveClipCount}</strong>{' '}
              {effectiveClipCount === 1 ? 'clip' : 'clips'} ·{' '}
              <strong>{totalSeconds}s</strong> total
            </span>
            <span
              className={`badge ${likenessBadge.badgeClass}`}
              data-testid={`profile-likeness-tier-${profile.slug}`}
            >
              {likenessBadge.label}
            </span>
          </div>
        </div>

        <div className="samples-list">
          {displaySamples.map((sample, idx) => {
            const sampleUrl = sampleAudioUrls[sample.id];
            return (
              <div
                key={sample.id}
                className="sample-item-card"
                data-testid={`profile-sample-row-${sample.id}`}
              >
                <div className="sample-item-top">
                  <div className="inline-actions">
                    <strong>Clip #{idx + 1}</strong>
                    {sample.is_primary && (
                      <span className="badge badge-ok">Primary</span>
                    )}
                    <span className="badge badge-neutral">{sample.style_label}</span>
                    <span className="pill">
                      {((sample.duration_ms || 15_000) / 1000).toFixed(1)}s
                    </span>
                  </div>
                  <div className="inline-actions">
                    <button
                      type="button"
                      className="btn btn-secondary btn-sm"
                      disabled={loadingSampleId === sample.id}
                      onClick={() => void handlePlaySample(sample)}
                    >
                      {loadingSampleId === sample.id ? 'Loading...' : '▶ Play'}
                    </button>
                    {displaySamples.length > 1 && !sample.isSyntheticFallback && (
                      <button
                        type="button"
                        className="btn btn-danger btn-sm"
                        disabled={mutationStatus === 'saving'}
                        onClick={() => void handleDeleteSample(sample.id)}
                      >
                        Delete
                      </button>
                    )}
                  </div>
                </div>
                <div className="small muted">“{sample.transcript_en}”</div>
                {sampleUrl && (
                  <div className="audio-player-row">
                    <audio controls src={sampleUrl} className="audio-element" />
                  </div>
                )}
              </div>
            );
          })}
        </div>

        <div className="add-sample-box form-stack">
          <h5>Add Another Reference Clip to {profile.display_name}</h5>
          <div className="form-grid-2">
            <div className="field-group">
              <label htmlFor={`sample-script-picker-${profile.slug}`}>
                Guided Reading Script
              </label>
              <select
                id={`sample-script-picker-${profile.slug}`}
                value={selectedScriptId}
                onChange={(e) => handleSelectScript(e.target.value)}
              >
                {ENROLLMENT_READING_SCRIPTS.map((script) => (
                  <option key={script.id} value={script.id}>
                    {script.title} ({script.estimatedSeconds})
                  </option>
                ))}
              </select>
            </div>

            <div className="field-group">
              <label htmlFor={`sample-style-select-${profile.slug}`}>
                Delivery Style Tag
              </label>
              <select
                id={`sample-style-select-${profile.slug}`}
                value={sampleStyle}
                onChange={(e) => setSampleStyle(e.target.value as SampleStyleLabel)}
              >
                {SAMPLE_STYLE_OPTIONS.map((opt) => (
                  <option key={opt.value} value={opt.value}>
                    {opt.label}
                  </option>
                ))}
              </select>
            </div>
          </div>

          <div className="field-group">
            <label htmlFor={`sample-transcript-${profile.slug}`}>
              Sample Spoken English Transcript
            </label>
            <textarea
              id={`sample-transcript-${profile.slug}`}
              rows={2}
              value={sampleTranscript}
              onChange={(e) => setSampleTranscript(e.target.value)}
            />
          </div>

          <AudioCaptureWidget
            recorder={sampleRecorder}
            mode="enrollment"
            inputId={`profile-sample-upload-${profile.slug}`}
          />

          <div className="label-row">
            <label
              className="checkbox-row"
              htmlFor={`sample-primary-check-${profile.slug}`}
            >
              <input
                id={`sample-primary-check-${profile.slug}`}
                type="checkbox"
                checked={makePrimary}
                onChange={(e) => setMakePrimary(e.target.checked)}
              />
              <span>Set as Primary Reference Clip (used by single-clip CosyVoice 3.0)</span>
            </label>

            <button
              type="button"
              className="btn btn-primary"
              disabled={mutationStatus === 'saving'}
              onClick={() => void handleAddSample()}
            >
              {mutationStatus === 'saving'
                ? 'Uploading Sample...'
                : '+ Add Sample to Profile'}
            </button>
          </div>

          {sampleError && (
            <div className="alert alert-error" role="alert">
              {sampleError}
            </div>
          )}
          {sampleMessage && (
            <div className="alert alert-ok" role="status">
              {sampleMessage}
            </div>
          )}
        </div>
      </div>

      {/* 2. Profile Default Tuning Sliders */}
      <div className="subpanel form-stack">
        <div className="panel-header">
          <div>
            <h4>2. Profile Default Synthesis & Likeness Tuning</h4>
            <p className="muted small">
              Saved directly onto <code>{profile.slug}</code> and applied automatically
              whenever you synthesize without a per-clip override.
            </p>
          </div>
          <button
            type="button"
            className="btn btn-primary btn-sm"
            disabled={mutationStatus === 'saving'}
            onClick={() => void handleSaveProfileTuning()}
          >
            Save Profile Tuning
          </button>
        </div>

        <TuningControlsPanel
          value={tuningDraft}
          onChange={setTuningDraft}
          idPrefix={`profile-tuning-${profile.slug}`}
        />

        {tuningSavedMsg && (
          <div className="alert alert-ok" role="status">
            {tuningSavedMsg}
          </div>
        )}
      </div>

      {/* 3. RTX 3090 XTTS-v2 GPT Fine-Tuning Card */}
      <div
        className="subpanel form-stack"
        data-testid={`finetune-card-${profile.slug}`}
      >
        <div className="panel-header">
          <div>
            <h4>3. RTX 3090 XTTS-v2 GPT Personal Checkpoint Fine-Tuning</h4>
            <p className="muted small">
              With <strong>3–15 reference clips</strong> (45s–3m total audio), train a
              dedicated XTTS-v2 GPT checkpoint on your local 24 GB RTX 3090 (~2–4
              minutes) to lock in your exact vocal prosody, resonance, and accent across
              every language.
            </p>
          </div>
          <span
            className={`badge ${
              activeFinetuneStatus === 'succeeded'
                ? 'badge-ok'
                : activeFinetuneStatus === 'running' || activeFinetuneStatus === 'queued'
                  ? 'badge-warn'
                  : activeFinetuneStatus === 'failed'
                    ? 'badge-danger'
                    : 'badge-neutral'
            }`}
            data-testid={`finetune-status-badge-${profile.slug}`}
          >
            Status: {activeFinetuneStatus}
          </span>
        </div>

        <div className="form-grid-2">
          <div className="field-group">
            <label htmlFor={`finetune-epochs-${profile.slug}`}>
              Epochs (2–20, default 6)
            </label>
            <input
              id={`finetune-epochs-${profile.slug}`}
              type="number"
              min={2}
              max={20}
              step={1}
              value={epochs}
              onChange={(e) => setEpochs(Number(e.target.value))}
            />
          </div>

          <div className="field-group">
            <label htmlFor={`finetune-lr-${profile.slug}`}>
              Learning Rate (default 0.000005)
            </label>
            <input
              id={`finetune-lr-${profile.slug}`}
              type="number"
              step="0.000001"
              min="0.0000005"
              max="0.0005"
              value={learningRate}
              onChange={(e) => setLearningRate(e.target.value)}
            />
          </div>
        </div>

        {finetuneJob && (
          <div
            className="meter-box"
            data-testid={`finetune-progress-box-${profile.slug}`}
          >
            <div className="meter-header">
              <span>
                Epoch {finetuneJob.current_epoch} / {finetuneJob.epochs} · Progress{' '}
                {progressPct}%
                {finetuneJob.last_loss !== null
                  ? ` · Loss ${finetuneJob.last_loss.toFixed(4)}`
                  : ''}
              </span>
              <span className="mono">{finetuneJob.status}</span>
            </div>
            <div className="progress-track">
              <div
                className={`progress-fill ${
                  finetuneJob.status === 'succeeded'
                    ? 'fill-ok'
                    : finetuneJob.status === 'failed'
                      ? 'fill-danger'
                      : 'fill-accent'
                }`}
                style={{ width: `${progressPct}%` }}
              />
            </div>
            {finetuneJob.checkpoint_dir && (
              <div className="meter-subtext">
                Checkpoint: <code>{finetuneJob.checkpoint_dir}</code>
              </div>
            )}
          </div>
        )}

        {(finetuneLocalErr || profile.finetune_error) && (
          <div className="alert alert-error" role="alert">
            {finetuneLocalErr ?? profile.finetune_error}
          </div>
        )}

        {finetuneMsg && (
          <div className="alert alert-ok" role="status">
            {finetuneMsg}
          </div>
        )}

        <div className="inline-actions">
          <button
            type="button"
            className="btn btn-primary"
            disabled={shouldPollFinetune || mutationStatus === 'saving'}
            onClick={() => void handleStartFinetune()}
          >
            {shouldPollFinetune
              ? 'RTX 3090 Fine-Tune Running...'
              : '⚡ Start RTX 3090 Fine-Tune'}
          </button>

          {hasActiveFinetuneCheckpoint && (
            <button
              type="button"
              className="btn btn-secondary"
              disabled={mutationStatus === 'saving'}
              onClick={() => void handleRevertZeroShot()}
            >
              Revert to Zero-Shot Base Model
            </button>
          )}
        </div>
      </div>
    </div>
  );
};
