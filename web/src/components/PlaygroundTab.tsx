import React, { useEffect, useState } from 'react';
import { apiFetchVoiceJobAudio } from '../api/client';
import {
  buildDirectRenderMultipartCurl,
  buildVoiceJobJsonCurl,
} from '../api/curlBuilder';
import { useMicrophoneRecorder } from '../audio/useMicrophoneRecorder';
import { useAppDispatch, useAppSelector } from '../store/hooks';
import {
  buildDeliveryTargetFromStudio,
  setBotChannelId,
  setBotReplyMessageId,
  setCustomTuning,
  setDeliveryMode,
  setEngineSelection,
  setInputMode,
  setMaxCerBpsOverride,
  setOutputFormat,
  setSelectedProfileSlug,
  setSourceText,
  setTargetLanguage,
  setUseCustomTuning,
  setWebhookAvatarUrl,
  setWebhookUrl,
  setWebhookUsername,
  submitVoiceJob,
  type StudioDeliveryMode,
} from '../store/studioSlice';
import type { AudioRenderFormat, EngineSelection } from '../types/api';
import { AudioCaptureWidget } from './AudioCaptureWidget';
import { getProfileLikenessBadge } from './ProfileLikenessStudio';
import { TuningControlsPanel } from './TuningControlsPanel';

export interface PlaygroundTabProps {
  onNavigateToEnroll: () => void;
}

export const PlaygroundTab: React.FC<PlaygroundTabProps> = ({ onNavigateToEnroll }) => {
  const dispatch = useAppDispatch();
  const connection = useAppSelector((s) => s.connection);
  const languages = useAppSelector((s) => s.languages.items);
  const profiles = useAppSelector((s) => s.profiles.items);
  const studio = useAppSelector((s) => s.studio);

  const recorder = useMicrophoneRecorder();
  const [curlTab, setCurlTab] = useState<'json' | 'render'>('json');
  const [copiedCurl, setCopiedCurl] = useState(false);

  const [oggBlobUrl, setOggBlobUrl] = useState<string | null>(null);
  const [wavBlobUrl, setWavBlobUrl] = useState<string | null>(null);
  const [audioLoadingFormat, setAudioLoadingFormat] = useState<AudioRenderFormat | null>(
    null,
  );
  const [audioFetchError, setAudioFetchError] = useState<string | null>(null);

  const currentLang =
    languages.find((l) => l.code === studio.targetLanguage) ?? languages[0];

  const recommendedEngine = currentLang?.recommended_engine ?? 'cosyvoice_3';
  const effectiveEngine =
    studio.engineSelection === 'auto' ? recommendedEngine : studio.engineSelection;
  const isEngineUnsupportedForLang =
    currentLang !== undefined &&
    studio.engineSelection !== 'auto' &&
    !currentLang.supported_engines.includes(studio.engineSelection);

  const defaultMaxCerBps = connection.runtimeConfig?.max_cer_bps ?? 1200;

  // Clean up previous audio object URLs when a new receipt arrives
  useEffect(() => {
    return () => {
      if (oggBlobUrl) URL.revokeObjectURL(oggBlobUrl);
      if (wavBlobUrl) URL.revokeObjectURL(wavBlobUrl);
    };
  }, [oggBlobUrl, wavBlobUrl]);

  const handleLoadReceiptAudio = async (jobId: string, format: AudioRenderFormat) => {
    setAudioLoadingFormat(format);
    setAudioFetchError(null);
    try {
      const blob = await apiFetchVoiceJobAudio(
        {
          apiBaseUrl: connection.apiBaseUrl,
          bearerToken: connection.bearerToken,
          discordUserId: connection.discordUserId,
        },
        jobId,
        format,
      );
      const url = URL.createObjectURL(blob);
      if (format === 'ogg') {
        if (oggBlobUrl) URL.revokeObjectURL(oggBlobUrl);
        setOggBlobUrl(url);
      } else {
        if (wavBlobUrl) URL.revokeObjectURL(wavBlobUrl);
        setWavBlobUrl(url);
      }
    } catch (err) {
      setAudioFetchError(
        err instanceof Error ? err.message : `Failed loading ${format.toUpperCase()} audio.`,
      );
    } finally {
      setAudioLoadingFormat(null);
    }
  };

  const handleRunSynthesis = async (e: React.FormEvent) => {
    e.preventDefault();
    if (oggBlobUrl) {
      URL.revokeObjectURL(oggBlobUrl);
      setOggBlobUrl(null);
    }
    if (wavBlobUrl) {
      URL.revokeObjectURL(wavBlobUrl);
      setWavBlobUrl(null);
    }
    const action = await dispatch(
      submitVoiceJob({
        audioBase64: recorder.clip?.base64Payload ?? null,
      }),
    );
    if (submitVoiceJob.fulfilled.match(action)) {
      void handleLoadReceiptAudio(action.payload.job_id, studio.outputFormat);
    }
  };

  const activeProfile =
    profiles.find((p) => p.slug === studio.selectedProfileSlug) ?? null;

  const curlInput = {
    apiBaseUrl: connection.apiBaseUrl,
    discordUserId: connection.discordUserId,
    profileSlug: studio.selectedProfileSlug || 'default_voice',
    targetLanguage: studio.targetLanguage,
    engineSelection: studio.engineSelection,
    maxCerBpsOverride: studio.maxCerBpsOverride,
    inputMode: studio.inputMode,
    sourceText: studio.sourceText,
    outputFormat: studio.outputFormat,
    delivery: buildDeliveryTargetFromStudio(studio),
    tuningOverride: studio.useCustomTuning ? studio.customTuning : null,
  };

  const activeCurl =
    curlTab === 'json'
      ? buildVoiceJobJsonCurl(curlInput)
      : buildDirectRenderMultipartCurl(curlInput);

  const handleCopyCurl = async () => {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(activeCurl).catch(() => {});
    }
    setCopiedCurl(true);
    window.setTimeout(() => setCopiedCurl(false), 1800);
  };

  const receipt = studio.lastReceipt;
  const receiptLang = languages.find((l) => l.code === receipt?.target_language);

  return (
    <div className="studio-two-col">
      <section className="panel">
        <div className="panel-header">
          <div>
            <h2>Multilingual Voice Clone & Translation Playground</h2>
            <p className="muted">
              Speak or type in English, clone your enrolled voice into 12+ languages, and
              verify pronunciation with Whisper reverse-ASR CER gating.
            </p>
          </div>
        </div>

        <form noValidate onSubmit={(e) => void handleRunSynthesis(e)} className="form-stack">
          {/* Input Mode Toggle */}
          <div className="segmented-toggle" role="group" aria-label="Source Input Mode">
            <button
              type="button"
              className={`seg-btn ${studio.inputMode === 'text' ? 'seg-active' : ''}`}
              onClick={() => dispatch(setInputMode('text'))}
            >
              ✎ Type English Text Directly
            </button>
            <button
              type="button"
              className={`seg-btn ${studio.inputMode === 'audio' ? 'seg-active' : ''}`}
              onClick={() => dispatch(setInputMode('audio'))}
            >
              🎙 Speak Into Mic / Upload Audio
            </button>
          </div>

          {studio.inputMode === 'text' ? (
            <div className="field-group">
              <div className="label-row">
                <label htmlFor="studio-source-text">English Source Text (`source_text`)</label>
                {currentLang?.sample_prompt && (
                  <button
                    type="button"
                    className="btn-link"
                    onClick={() => dispatch(setSourceText(currentLang.sample_prompt))}
                  >
                    Load {currentLang.english_name} Sample Prompt
                  </button>
                )}
              </div>
              <textarea
                id="studio-source-text"
                rows={3}
                value={studio.sourceText}
                onChange={(e) => dispatch(setSourceText(e.target.value))}
                placeholder="Type what you want to say in English (numbers & times are automatically expanded into natural spoken words)..."
              />
            </div>
          ) : (
            <div className="field-group">
              <label htmlFor="playground-audio-upload">
                English Voice Input (Whisper Large-v3 ASR → Polyglot Clone)
              </label>
              <AudioCaptureWidget
                recorder={recorder}
                mode="clip"
                inputId="playground-audio-upload"
              />
            </div>
          )}

          {/* Profile + Target Language */}
          <div className="form-grid-2">
            <div className="field-group">
              <div className="label-row">
                <label htmlFor="studio-profile-select">Voice Profile</label>
                <button
                  type="button"
                  className="btn-link"
                  onClick={onNavigateToEnroll}
                >
                  + Enroll New Voice
                </button>
              </div>
              {profiles.length > 0 ? (
                <select
                  id="studio-profile-select"
                  value={studio.selectedProfileSlug}
                  onChange={(e) => dispatch(setSelectedProfileSlug(e.target.value))}
                >
                  {profiles.map((p) => (
                    <option key={p.slug} value={p.slug}>
                      {p.display_name} ({p.slug})
                    </option>
                  ))}
                </select>
              ) : (
                <input
                  id="studio-profile-select"
                  type="text"
                  value={studio.selectedProfileSlug}
                  onChange={(e) => dispatch(setSelectedProfileSlug(e.target.value))}
                  placeholder="default_voice (enroll a profile in Tab 2)"
                />
              )}
              {activeProfile && (
                <div
                  className="inline-actions mt-xs"
                  data-testid="playground-profile-likeness-summary"
                >
                  <span className="pill">
                    Clips:{' '}
                    <strong>
                      {Math.max(activeProfile.sample_count, activeProfile.samples.length)}
                    </strong>{' '}
                    ({(activeProfile.total_sample_duration_ms / 1000).toFixed(1)}s)
                  </span>
                  <span
                    className={`badge ${getProfileLikenessBadge(activeProfile).badgeClass}`}
                  >
                    {getProfileLikenessBadge(activeProfile).label}
                  </span>
                </div>
              )}
            </div>

            <div className="field-group">
              <label htmlFor="studio-language-select">Target Language</label>
              <select
                id="studio-language-select"
                value={studio.targetLanguage}
                onChange={(e) => dispatch(setTargetLanguage(e.target.value))}
              >
                {languages.map((lang) => (
                  <option key={lang.code} value={lang.code}>
                    {lang.english_name} — {lang.native_name} [{lang.code}]
                  </option>
                ))}
              </select>
            </div>
          </div>

          {/* Engine + CER Gate + Output Format */}
          <div className="form-grid-3">
            <div className="field-group">
              <label htmlFor="studio-engine-select">TTS Cloning Engine</label>
              <select
                id="studio-engine-select"
                value={studio.engineSelection}
                onChange={(e) =>
                  dispatch(setEngineSelection(e.target.value as EngineSelection))
                }
              >
                <option value="auto">
                  Auto (Recommended: {recommendedEngine})
                </option>
                <option value="cosyvoice_3">CosyVoice 3.0 (cosyvoice_3)</option>
                <option value="xtts_v2">Coqui XTTS-v2 (xtts_v2)</option>
              </select>
              <div className="field-hint" data-testid="recommended-engine-badge">
                Active for <strong>{currentLang?.english_name ?? studio.targetLanguage}</strong>:{' '}
                <code>{effectiveEngine}</code> (supported:{' '}
                {(currentLang?.supported_engines ?? ['cosyvoice_3', 'xtts_v2']).join(', ')})
              </div>
            </div>

            <div className="field-group">
              <label htmlFor="studio-cer-override">Max CER Gate (`bps`)</label>
              <input
                id="studio-cer-override"
                type="number"
                min={100}
                max={10000}
                step={50}
                value={studio.maxCerBpsOverride}
                onChange={(e) => dispatch(setMaxCerBpsOverride(e.target.value))}
                placeholder={`Default (${defaultMaxCerBps} bps = ${(defaultMaxCerBps / 100).toFixed(1)}%)`}
              />
              <div className="field-hint">
                100 bps = 1.00% character/word error ceiling.
              </div>
            </div>

            <div className="field-group">
              <label htmlFor="studio-format-select">Preferred Audio Format</label>
              <select
                id="studio-format-select"
                value={studio.outputFormat}
                onChange={(e) =>
                  dispatch(setOutputFormat(e.target.value as AudioRenderFormat))
                }
              >
                <option value="ogg">Ogg/Opus 48 kHz (Discord / iOS)</option>
                <option value="wav">WAV 48 kHz PCM (BlackHole / VB-Cable)</option>
              </select>
              <div className="field-hint">
                Both formats are generated for every job.
              </div>
            </div>
          </div>

          {isEngineUnsupportedForLang && (
            <div className="alert alert-warn">
              Note: <code>{studio.engineSelection}</code> is not listed in{' '}
              <strong>{currentLang?.english_name}</strong>’s primary engines (
              <code>{currentLang?.supported_engines.join(', ')}</code>). Switching to{' '}
              <strong>Auto ({recommendedEngine})</strong> is recommended.
            </div>
          )}

          {/* Optional Per-Clip Voice Likeness & Synthesis Tuning Override */}
          <div className="subpanel form-stack">
            <label
              className="checkbox-row"
              htmlFor="studio-override-tuning-toggle"
            >
              <input
                id="studio-override-tuning-toggle"
                type="checkbox"
                checked={studio.useCustomTuning}
                onChange={(e) => {
                  const enabled = e.target.checked;
                  if (enabled && activeProfile && !studio.useCustomTuning) {
                    dispatch(setCustomTuning({ ...activeProfile.tuning }));
                  }
                  dispatch(setUseCustomTuning(enabled));
                }}
              />
              <span>
                <strong>Override Voice Likeness & Synthesis Tuning for This Clip</strong>{' '}
                <span className="muted small">
                  (Default off — uses profile’s saved tuning)
                </span>
              </span>
            </label>

            {studio.useCustomTuning && (
              <TuningControlsPanel
                value={studio.customTuning}
                onChange={(next) => dispatch(setCustomTuning(next))}
                idPrefix="playground-tuning"
                compact
              />
            )}
          </div>

          {/* Delivery Mode */}
          <div className="field-group">
            <label htmlFor="studio-delivery-select">Delivery Mode</label>
            <select
              id="studio-delivery-select"
              value={studio.deliveryMode}
              onChange={(e) =>
                dispatch(setDeliveryMode(e.target.value as StudioDeliveryMode))
              }
            >
              <option value="client_return_only">
                ClientReturnOnly — Local Browser Playback, Download & Virtual Mic (Zero APP Tag)
              </option>
              <option value="profile_webhook">
                ProfileWebhook — Discord Channel Webhook with Custom Name & Avatar
              </option>
              <option value="bot_voice_bubble">
                BotVoiceBubble — Native Discord Voice Message Bubble (flags: 8192)
              </option>
            </select>
          </div>

          {studio.deliveryMode === 'profile_webhook' && (
            <div className="subpanel form-grid-3">
              <div className="field-group">
                <label htmlFor="webhook-url">Discord Webhook URL</label>
                <input
                  id="webhook-url"
                  type="url"
                  required
                  value={studio.webhookUrl}
                  onChange={(e) => dispatch(setWebhookUrl(e.target.value))}
                  placeholder="https://discord.com/api/webhooks/..."
                />
              </div>
              <div className="field-group">
                <label htmlFor="webhook-username">Override Username</label>
                <input
                  id="webhook-username"
                  type="text"
                  required
                  value={studio.webhookUsername}
                  onChange={(e) => dispatch(setWebhookUsername(e.target.value))}
                />
              </div>
              <div className="field-group">
                <label htmlFor="webhook-avatar">Avatar URL (Optional)</label>
                <input
                  id="webhook-avatar"
                  type="url"
                  value={studio.webhookAvatarUrl}
                  onChange={(e) => dispatch(setWebhookAvatarUrl(e.target.value))}
                  placeholder="https://..."
                />
              </div>
            </div>
          )}

          {studio.deliveryMode === 'bot_voice_bubble' && (
            <div className="subpanel form-grid-2">
              <div className="field-group">
                <label htmlFor="bot-channel-id">Target Discord Channel ID (Snowflake)</label>
                <input
                  id="bot-channel-id"
                  type="text"
                  inputMode="numeric"
                  pattern="^\d{1,20}$"
                  maxLength={20}
                  required
                  value={studio.botChannelId}
                  onChange={(e) => dispatch(setBotChannelId(e.target.value))}
                  placeholder="e.g. 1234567890123456789"
                />
              </div>
              <div className="field-group">
                <label htmlFor="bot-reply-id">Reply To Message ID (Optional)</label>
                <input
                  id="bot-reply-id"
                  type="text"
                  inputMode="numeric"
                  pattern="^\d{1,20}$"
                  maxLength={20}
                  value={studio.botReplyMessageId}
                  onChange={(e) => dispatch(setBotReplyMessageId(e.target.value))}
                  placeholder="Optional 64-bit message snowflake"
                />
              </div>
            </div>
          )}

          {studio.error && (
            <div className="alert alert-error" role="alert" data-testid="studio-error-alert">
              <strong>Pipeline Error:</strong> {studio.error}
            </div>
          )}

          <button
            type="submit"
            className="btn btn-primary btn-lg"
            disabled={studio.status === 'running'}
          >
            {studio.status === 'running'
              ? 'Synthesizing & Running Reverse-ASR Verification...'
              : `Clone Voice & Translate to ${currentLang?.english_name ?? studio.targetLanguage}`}
          </button>
        </form>
      </section>

      {/* Right Column: Pipeline Result Inspector + Live cURL Snippet */}
      <div className="stack-col">
        <section className="panel">
          <div className="panel-header">
            <h2>Pipeline Verification & Audio Output</h2>
            {receipt && (
              <span
                className={`badge ${receipt.cer_bps <= defaultMaxCerBps ? 'badge-ok' : 'badge-warn'}`}
                data-testid="receipt-cer-badge"
              >
                CER: {receipt.cer_bps} bps ({(receipt.cer_bps / 100).toFixed(2)}%)
              </span>
            )}
          </div>

          {!receipt ? (
            <div className="empty-state">
              Run a translation & voice-clone job on the left to inspect the spoken script,
              phonetic pronunciation guide, Whisper reverse-ASR verification, latency, and
              playable <code>.ogg</code> / <code>48 kHz .wav</code> outputs.
            </div>
          ) : (
            <div className="receipt-card" data-testid="voice-job-receipt">
              <div className="metrics-pills">
                <span className="pill">
                  Language: <strong>{receipt.target_language.toUpperCase()}</strong>
                </span>
                <span className="pill">
                  Engine: <code>{receipt.engine_used}</code>
                </span>
                <span className="pill">
                  Seed Attempts: <strong>{receipt.attempts_used}</strong>
                </span>
                <span className="pill">
                  Clip Duration: <strong>{(receipt.duration_ms / 1000).toFixed(2)}s</strong>
                </span>
                <span className="pill">
                  GPU Latency: <strong>{receipt.total_latency_ms} ms</strong>
                </span>
                {receipt.discord_message_id && (
                  <span className="pill">
                    Discord Msg: <code>{receipt.discord_message_id}</code>
                  </span>
                )}
              </div>

              <div className="script-block">
                <div className="script-label">English Source Transcript</div>
                <div className="script-text">{receipt.english_transcript}</div>
              </div>

              <div className="script-block highlight-block">
                <div className="script-label">
                  Target Spoken Script ({receiptLang?.english_name ?? receipt.target_language})
                </div>
                <div className="script-text spoken-large">{receipt.spoken_text}</div>
              </div>

              <div className="script-block">
                <div className="script-label">
                  {receiptLang?.phonetic_label ?? 'Phonetic / Pronunciation Guide'}
                </div>
                <div className="script-text mono">{receipt.phonetic_guide}</div>
              </div>

              <div className="form-grid-2">
                <div className="script-block">
                  <div className="script-label">English Back-Gloss</div>
                  <div className="script-text">{receipt.english_back_gloss}</div>
                </div>
                <div className="script-block">
                  <div className="script-label">Whisper Reverse-ASR Check</div>
                  <div className="script-text">{receipt.back_transcribed_text}</div>
                </div>
              </div>

              <div className="audio-outputs-box">
                <div className="audio-format-card">
                  <div className="label-row">
                    <strong>48 kHz Ogg/Opus (Discord / iOS)</strong>
                    <button
                      type="button"
                      className="btn btn-secondary btn-sm"
                      disabled={audioLoadingFormat === 'ogg'}
                      onClick={() => void handleLoadReceiptAudio(receipt.job_id, 'ogg')}
                    >
                      {oggBlobUrl ? 'Reload .ogg' : 'Load .ogg Player'}
                    </button>
                  </div>
                  {oggBlobUrl && (
                    <div className="audio-player-row">
                      <audio controls src={oggBlobUrl} className="audio-element" />
                      <a
                        href={oggBlobUrl}
                        download={`voice-${receipt.target_language}-${receipt.job_id}.ogg`}
                        className="btn btn-ghost btn-sm"
                      >
                        Download .ogg
                      </a>
                    </div>
                  )}
                </div>

                <div className="audio-format-card">
                  <div className="label-row">
                    <strong>48 kHz PCM WAV (BlackHole / VB-Cable)</strong>
                    <button
                      type="button"
                      className="btn btn-secondary btn-sm"
                      disabled={audioLoadingFormat === 'wav'}
                      onClick={() => void handleLoadReceiptAudio(receipt.job_id, 'wav')}
                    >
                      {wavBlobUrl ? 'Reload .wav' : 'Load .wav Player'}
                    </button>
                  </div>
                  {wavBlobUrl && (
                    <div className="audio-player-row">
                      <audio controls src={wavBlobUrl} className="audio-element" />
                      <a
                        href={wavBlobUrl}
                        download={`voice-${receipt.target_language}-${receipt.job_id}-48k.wav`}
                        className="btn btn-ghost btn-sm"
                      >
                        Download .wav
                      </a>
                    </div>
                  )}
                </div>

                {audioFetchError && (
                  <div className="alert alert-error">{audioFetchError}</div>
                )}
              </div>
            </div>
          )}
        </section>

        {/* Live Copyable cURL / REST API Explorer */}
        <section className="panel">
          <div className="panel-header">
            <div>
              <h3>Live REST API / cURL Explorer</h3>
              <p className="muted">
                Updates automatically as you switch languages, profiles, engines, or delivery
                modes.
              </p>
            </div>
            <div className="inline-actions">
              <button
                type="button"
                className={`btn btn-sm ${curlTab === 'json' ? 'btn-secondary' : 'btn-ghost'}`}
                onClick={() => setCurlTab('json')}
              >
                JSON Receipt API
              </button>
              <button
                type="button"
                className={`btn btn-sm ${curlTab === 'render' ? 'btn-secondary' : 'btn-ghost'}`}
                onClick={() => setCurlTab('render')}
              >
                Binary Stream (/render)
              </button>
              <button
                type="button"
                className="btn btn-primary btn-sm"
                onClick={() => void handleCopyCurl()}
              >
                {copiedCurl ? '✓ Copied' : 'Copy cURL'}
              </button>
            </div>
          </div>
          <pre className="code-block" data-testid="live-curl-snippet">
            <code>{activeCurl}</code>
          </pre>
        </section>
      </div>
    </div>
  );
};
