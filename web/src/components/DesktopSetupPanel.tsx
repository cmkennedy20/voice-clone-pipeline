import React, { useEffect, useState } from 'react';
import {
  apiGetDesktopSystemStatus,
  apiSaveDiscordConfig,
  type ApiConnectionSettings,
} from '../api/client';
import type { DesktopSystemStatus, SaveDiscordConfigReceipt } from '../types/api';

interface DesktopSetupPanelProps {
  connection: ApiConnectionSettings;
  onSavedConfig: () => void;
}

export const DesktopSetupPanel: React.FC<DesktopSetupPanelProps> = ({
  connection,
  onSavedConfig,
}) => {
  const [status, setStatus] = useState<DesktopSystemStatus | null>(null);
  const [appId, setAppId] = useState('');
  const [botToken, setBotToken] = useState('');
  const [ownerId, setOwnerId] = useState('');
  const [verifyOnline, setVerifyOnline] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [receipt, setReceipt] = useState<SaveDiscordConfigReceipt | null>(null);

  useEffect(() => {
    let cancelled = false;
    void apiGetDesktopSystemStatus(connection).then((loaded) => {
      if (cancelled || !loaded) return;
      setStatus(loaded);
      if (loaded.discord_application_id_str && loaded.discord_application_id_str !== '0') {
        setAppId((prev) => prev || loaded.discord_application_id_str);
      }
      if (
        loaded.discord_owner_user_id_str &&
        loaded.discord_owner_user_id_str !== '0' &&
        loaded.discord_owner_user_id_str !== '1'
      ) {
        setOwnerId((prev) => prev || loaded.discord_owner_user_id_str);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [connection]);

  if (!status) {
    return null;
  }

  const handleSaveDiscord = async (e: React.FormEvent) => {
    e.preventDefault();
    setSaving(true);
    setSaveError(null);
    try {
      const saved = await apiSaveDiscordConfig(connection, {
        discord_application_id: appId.trim(),
        discord_bot_token: botToken.trim(),
        discord_owner_user_id: ownerId.trim(),
        verify_online: verifyOnline,
      });
      setReceipt(saved);
      setBotToken('');
      const refreshed = await apiGetDesktopSystemStatus(connection);
      if (refreshed) {
        setStatus(refreshed);
      }
      onSavedConfig();
    } catch (err) {
      setSaveError(err instanceof Error ? err.message : 'Failed saving Discord configuration.');
    } finally {
      setSaving(false);
    }
  };

  const userInstallUrl = receipt?.user_install_url ?? status.user_install_url;
  const botDmUrl = receipt?.bot_dm_url ?? status.bot_dm_url;

  return (
    <section className="panel" data-testid="desktop-setup-panel">
      <div className="panel-header">
        <div>
          <h2>Desktop Hardware Tier, Discord Onboarding & Agent CLI</h2>
          <p className="muted">
            Self-contained Windows desktop hardware detection, zero-restart Discord bot pairing,
            and copyable <code>--json</code> CLI commands for AI agents.
          </p>
        </div>
        <span
          className={`badge ${status.discord_configured ? 'badge-ok' : 'badge-neutral'}`}
        >
          {status.discord_configured ? '● Discord Bot Ready' : '◌ Local Studio Mode'}
        </span>
      </div>

      <div className="subpanel">
        <h3>1. Detected Desktop Hardware & Adaptive Tier</h3>
        <div className="metrics-pills">
          <span className="pill">
            Tier: <code>{status.hardware_plan.tier}</code>
          </span>
          <span className="pill">
            GPU:{' '}
            <strong>
              {status.hardware_plan.selected_gpu_name ?? 'CPU / Integrated Graphics'}
            </strong>
            {status.hardware_plan.selected_gpu_vram_mib > 0
              ? ` (${status.hardware_plan.selected_gpu_vram_mib} MiB)`
              : ''}
          </span>
          <span className="pill">
            Whisper: <code>{status.hardware_plan.whisper_model_filename}</code>
          </span>
          <span className="pill">
            Qwen3 LLM: <code>{status.hardware_plan.llm_model_filename}</code>
          </span>
          <span className="pill">
            CPU Cores: <strong>{status.hardware_specs.cpu_logical_cores}</strong>
          </span>
        </div>
        {status.hardware_plan.warnings.map((w) => (
          <div key={w} className="alert alert-error mt-xs">
            {w}
          </div>
        ))}
      </div>

      {status.studio_env_writable && (
        <form className="subpanel mt-xs" onSubmit={(e) => void handleSaveDiscord(e)}>
          <h3>2. Optional 1-on-1 Discord Bot Configuration (Auto-Hot-Reloads Worker)</h3>
          <div className="form-grid-3">
            <div className="field-group">
              <label htmlFor="desktop-discord-app-id">Discord Application ID</label>
              <input
                id="desktop-discord-app-id"
                type="text"
                value={appId}
                onChange={(e) => setAppId(e.target.value)}
                placeholder="17-20 digit Application ID"
                required
              />
            </div>

            <div className="field-group">
              <label htmlFor="desktop-discord-bot-token">Discord Bot Token</label>
              <input
                id="desktop-discord-bot-token"
                type="password"
                value={botToken}
                onChange={(e) => setBotToken(e.target.value)}
                placeholder="Paste Bot Token from Discord Dev Portal"
                required
              />
            </div>

            <div className="field-group">
              <label htmlFor="desktop-discord-owner-id">Your Discord User ID</label>
              <input
                id="desktop-discord-owner-id"
                type="text"
                value={ownerId}
                onChange={(e) => setOwnerId(e.target.value)}
                placeholder="17-20 digit Owner Snowflake"
                required
              />
            </div>
          </div>

          <div className="inline-actions mt-xs">
            <label className="small">
              <input
                type="checkbox"
                checked={verifyOnline}
                onChange={(e) => setVerifyOnline(e.target.checked)}
              />{' '}
              Verify Bot Token live against Discord API v10
            </label>
            <button type="submit" className="btn btn-primary" disabled={saving}>
              {saving ? 'Saving...' : 'Save Discord Config & Start Bot'}
            </button>
          </div>

          {saveError && (
            <div className="alert alert-error mt-xs" role="alert">
              {saveError}
            </div>
          )}

          {receipt && (
            <div className="alert mt-xs" data-testid="discord-save-receipt">
              Saved Discord credentials to <code>studio.env</code>
              {receipt.verified_bot_username
                ? ` (verified @${receipt.verified_bot_username})`
                : ''}
              ! Authorize your personal install links below:
            </div>
          )}

          {userInstallUrl && botDmUrl && (
            <div className="inline-actions mt-xs">
              <a
                className="btn btn-secondary btn-sm"
                href={userInstallUrl}
                target="_blank"
                rel="noreferrer"
              >
                ↗ 1) Authorize Personal Account Install (USER_INSTALL)
              </a>
              <a
                className="btn btn-secondary btn-sm"
                href={botDmUrl}
                target="_blank"
                rel="noreferrer"
              >
                ↗ 2) Open 1-on-1 Bot Server / DM Link
              </a>
            </div>
          )}
        </form>
      )}

      <div className="subpanel mt-xs">
        <h3>3. Agent-First WSL2 / Docker Setup & Verification Commands</h3>
        <pre className="code-block">
          {[
            '# Run inside WSL2 (Ubuntu/Debian) with Docker Desktop WSL2 integration:',
            'make setup ARGS="--non-interactive --json"',
            'make pull-models',
            'make up',
            'make doctor ARGS="--json"',
          ].join('\n')}
        </pre>
      </div>
    </section>
  );
};
