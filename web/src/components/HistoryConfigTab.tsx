import React, { useEffect, useState } from 'react';
import { apiFetchVoiceJobAudio } from '../api/client';
import { DesktopSetupPanel } from './DesktopSetupPanel';
import {
  checkHealthAndConfig,
  setApiBaseUrl,
  setBearerToken,
  setDiscordUserId,
} from '../store/connectionSlice';
import {
  fetchBenchmarks,
  fetchVoiceJobs,
  setFilterLanguage,
  setFilterProfileSlug,
} from '../store/historySlice';
import { useAppDispatch, useAppSelector } from '../store/hooks';
import { fetchLanguages } from '../store/languagesSlice';
import { fetchProfiles } from '../store/profilesSlice';
import type { AudioRenderFormat } from '../types/api';

export const HistoryConfigTab: React.FC = () => {
  const dispatch = useAppDispatch();
  const connection = useAppSelector((s) => s.connection);
  const languages = useAppSelector((s) => s.languages.items);
  const profiles = useAppSelector((s) => s.profiles.items);
  const history = useAppSelector((s) => s.history);

  const [showToken, setShowToken] = useState(false);
  const [replayUrls, setReplayUrls] = useState<Record<string, string>>({});
  const [replayLoadingKey, setReplayLoadingKey] = useState<string | null>(null);

  useEffect(() => {
    void dispatch(fetchVoiceJobs());
    void dispatch(fetchBenchmarks());
  }, [dispatch]);

  useEffect(() => {
    return () => {
      for (const url of Object.values(replayUrls)) {
        URL.revokeObjectURL(url);
      }
    };
  }, [replayUrls]);

  const handleReconnectAll = () => {
    void dispatch(checkHealthAndConfig());
    void dispatch(fetchLanguages());
    void dispatch(fetchProfiles());
    void dispatch(fetchVoiceJobs());
    void dispatch(fetchBenchmarks());
  };

  const handleReplay = async (jobId: string, format: AudioRenderFormat) => {
    const key = `${jobId}:${format}`;
    setReplayLoadingKey(key);
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
      setReplayUrls((prev) => {
        if (prev[key]) URL.revokeObjectURL(prev[key]);
        return { ...prev, [key]: url };
      });
    } finally {
      setReplayLoadingKey(null);
    }
  };

  return (
    <div className="stack-col">
      <DesktopSetupPanel
        connection={{
          apiBaseUrl: connection.apiBaseUrl,
          bearerToken: connection.bearerToken,
          discordUserId: connection.discordUserId,
        }}
        onSavedConfig={handleReconnectAll}
      />

      {/* Connection & Runtime Config */}
      <section className="panel">
        <div className="panel-header">
          <div>
            <h2>Connection & Runtime Configuration</h2>
            <p className="muted">
              Defaults to same-origin (<code>&quot;&quot;</code>) via Nginx reverse proxy
              with owner Bearer authentication.
            </p>
          </div>
          <span
            className={`badge ${
              connection.healthStatus === 'ready'
                ? 'badge-ok'
                : connection.healthStatus === 'checking'
                  ? 'badge-neutral'
                  : 'badge-danger'
            }`}
          >
            /health/ready: {connection.healthStatus.toUpperCase()}
          </span>
        </div>

        <div className="form-grid-3">
          <div className="field-group">
            <label htmlFor="conn-base-url">API Base URL</label>
            <input
              id="conn-base-url"
              type="text"
              value={connection.apiBaseUrl}
              onChange={(e) => dispatch(setApiBaseUrl(e.target.value))}
              placeholder="(Same-origin via Nginx proxy, or http://localhost:8080)"
            />
          </div>

          <div className="field-group">
            <div className="label-row">
              <label htmlFor="conn-bearer-token">Owner Bearer Token</label>
              <button
                type="button"
                className="btn-link"
                onClick={() => setShowToken((v) => !v)}
              >
                {showToken ? 'Hide Token' : 'Show Token'}
              </button>
            </div>
            <input
              id="conn-bearer-token"
              type={showToken ? 'text' : 'password'}
              value={connection.bearerToken}
              onChange={(e) => dispatch(setBearerToken(e.target.value))}
            />
          </div>

          <div className="field-group">
            <label htmlFor="conn-discord-id">
              Optional `x-discord-user-id` Header
            </label>
            <input
              id="conn-discord-id"
              type="text"
              inputMode="numeric"
              pattern="^\d{1,20}$"
              maxLength={20}
              value={connection.discordUserId}
              onChange={(e) => dispatch(setDiscordUserId(e.target.value))}
              placeholder="Defaults to server owner snowflake"
            />
          </div>
        </div>

        <div className="inline-actions">
          <button
            type="button"
            className="btn btn-primary"
            onClick={handleReconnectAll}
          >
            Test Connection & Reload Config / Profiles
          </button>
        </div>

        {connection.error && (
          <div className="alert alert-error" role="alert">
            {connection.error}
          </div>
        )}

        {connection.runtimeConfig && (
          <div className="subpanel" data-testid="runtime-config-inspector">
            <h3>Live `GET /api/v1/config` Inspector</h3>
            <div className="metrics-pills">
              <span className="pill">
                Service: <code>{connection.runtimeConfig.service_name}</code>
              </span>
              <span className="pill">
                Owner ID: <code>{connection.runtimeConfig.discord_owner_user_id}</code>
              </span>
              <span className="pill">
                App ID: <code>{connection.runtimeConfig.discord_application_id}</code>
              </span>
              <span className="pill">
                Default Lang:{' '}
                <strong>{connection.runtimeConfig.default_target_language}</strong>
              </span>
              <span className="pill">
                Max CER Gate: <strong>{connection.runtimeConfig.max_cer_bps} bps</strong>
              </span>
              <span className="pill">
                Max Seed Retries:{' '}
                <strong>{connection.runtimeConfig.max_tts_attempts}</strong>
              </span>
              <span className="pill">
                Engines:{' '}
                <code>{connection.runtimeConfig.supported_engines.join(', ')}</code>
              </span>
            </div>
          </div>
        )}
      </section>

      {/* Filterable Voice Job History */}
      <section className="panel">
        <div className="panel-header">
          <div>
            <h2>Voice Job History ({history.jobsTotal})</h2>
            <p className="muted">
              Filter past polyglot voice-clone jobs and replay <code>.ogg</code> or{' '}
              <code>48 kHz .wav</code> outputs in 1 click.
            </p>
          </div>
          <div className="inline-actions">
            <select
              aria-label="Filter History by Language"
              value={history.filterLanguage}
              onChange={(e) => {
                dispatch(setFilterLanguage(e.target.value));
              }}
            >
              <option value="">All Languages</option>
              {languages.map((l) => (
                <option key={l.code} value={l.code}>
                  {l.english_name} [{l.code}]
                </option>
              ))}
            </select>

            <select
              aria-label="Filter History by Profile"
              value={history.filterProfileSlug}
              onChange={(e) => {
                dispatch(setFilterProfileSlug(e.target.value));
              }}
            >
              <option value="">All Profiles</option>
              {profiles.map((p) => (
                <option key={p.slug} value={p.slug}>
                  {p.display_name} ({p.slug})
                </option>
              ))}
            </select>

            <button
              type="button"
              className="btn btn-secondary"
              onClick={() => void dispatch(fetchVoiceJobs())}
            >
              Apply Filter
            </button>
          </div>
        </div>

        {history.jobs.length === 0 ? (
          <div className="empty-state">No voice jobs recorded matching current filters.</div>
        ) : (
          <div className="jobs-table-wrap">
            <table className="studio-table">
              <thead>
                <tr>
                  <th>Lang / Profile</th>
                  <th>English → Spoken Output</th>
                  <th>CER & Latency</th>
                  <th>Audio Replay</th>
                </tr>
              </thead>
              <tbody>
                {history.jobs.map((job) => {
                  const oggUrl = replayUrls[`${job.job_id}:ogg`];
                  const wavUrl = replayUrls[`${job.job_id}:wav`];
                  return (
                    <tr key={job.job_id}>
                      <td>
                        <span className="badge badge-neutral">
                          {job.target_language.toUpperCase()}
                        </span>{' '}
                        <code>{job.profile_slug || job.profile_id.slice(0, 8)}</code>
                        <div className="muted small">
                          {job.engine_used} • {job.delivery_mode}
                        </div>
                      </td>
                      <td>
                        <div className="small muted">{job.english_transcript}</div>
                        <div>
                          <strong>{job.spoken_text}</strong>
                        </div>
                        <div className="small mono muted">{job.phonetic_guide}</div>
                      </td>
                      <td>
                        <span className="badge badge-ok">
                          {job.cer_bps} bps ({(job.cer_bps / 100).toFixed(2)}%)
                        </span>
                        <div className="small muted">
                          {job.total_latency_ms} ms • {job.attempts_used} try
                        </div>
                      </td>
                      <td>
                        <div className="inline-actions">
                          <button
                            type="button"
                            className="btn btn-secondary btn-sm"
                            disabled={replayLoadingKey === `${job.job_id}:ogg`}
                            onClick={() => void handleReplay(job.job_id, 'ogg')}
                          >
                            ▶ .ogg
                          </button>
                          <button
                            type="button"
                            className="btn btn-ghost btn-sm"
                            disabled={replayLoadingKey === `${job.job_id}:wav`}
                            onClick={() => void handleReplay(job.job_id, 'wav')}
                          >
                            ▶ .wav
                          </button>
                        </div>
                        {oggUrl && (
                          <audio controls src={oggUrl} className="audio-element mt-xs" />
                        )}
                        {wavUrl && (
                          <audio controls src={wavUrl} className="audio-element mt-xs" />
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {/* RTX 3090 Benchmark Runs */}
      <section className="panel">
        <div className="panel-header">
          <div>
            <h2>RTX 3090 6-Metric Benchmark Ledger ({history.benchmarksTotal})</h2>
            <p className="muted">
              Hardware release scorecards verifying Gate 0 accuracy, spoken digit hygiene,
              CER, speaker similarity, Opus/WAV compliance, and P95 latency.
            </p>
          </div>
          <button
            type="button"
            className="btn btn-secondary"
            onClick={() => void dispatch(fetchBenchmarks())}
          >
            ↻ Refresh Benchmarks
          </button>
        </div>

        {history.benchmarks.length === 0 ? (
          <div className="empty-state">
            No benchmark runs persisted yet. Run the benchmark suite via CLI or{' '}
            <code>POST /api/v1/benchmarks/run</code> to populate this ledger.
          </div>
        ) : (
          <div className="jobs-table-wrap">
            <table className="studio-table">
              <thead>
                <tr>
                  <th>Status</th>
                  <th>Engine</th>
                  <th>Clips</th>
                  <th>Mean CER</th>
                  <th>Speaker Sim</th>
                  <th>P95 Latency</th>
                  <th>Peak VRAM</th>
                </tr>
              </thead>
              <tbody>
                {history.benchmarks.map((b) => (
                  <tr key={b.id}>
                    <td>
                      <span
                        className={`badge ${b.passed_all_gates ? 'badge-ok' : 'badge-danger'}`}
                      >
                        {b.passed_all_gates ? 'PASS' : 'FAIL'}
                      </span>
                    </td>
                    <td>
                      <code>{b.engine_tested}</code>
                    </td>
                    <td>{b.clips_evaluated}</td>
                    <td>
                      {b.mean_cer_bps} bps ({(b.mean_cer_bps / 100).toFixed(2)}%)
                    </td>
                    <td>
                      {b.mean_speaker_sim_bps} bps (
                      {(b.mean_speaker_sim_bps / 100).toFixed(1)}%)
                    </td>
                    <td>{b.p95_latency_ms} ms</td>
                    <td>{b.peak_vram_mib} MiB</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  );
};
