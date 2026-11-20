import React, { useState } from 'react';
import { useAppSelector } from '../store/hooks';

interface CheatSheetItem {
  id: string;
  title: string;
  snippet: string;
  description: string;
}

const DISCORD_CHEAT_SHEET: CheatSheetItem[] = [
  {
    id: 'voice-clone',
    title: 'Polyglot Slash Command (12-Language Picker)',
    snippet: '/voice_clone language:es text:Hey team, joining the voice call in 5 minutes!',
    description:
      'Works in any 1:1 DM, Group DM, or server via USER_INSTALL. Pick from 12 languages (es, zh, fr, de, it, pt, ja, ko, ru, ar, hi, en) and pass text or an audio clip.',
  },
  {
    id: 'voice-settings',
    title: 'Sticky 1-on-1 DM Preferences',
    snippet: '/voice_settings language:es profile:default_voice',
    description:
      'Sets your default target language and voice profile so plain 1-on-1 DM messages and voice notes synthesize automatically.',
  },
  {
    id: 'voice-profiles',
    title: 'Inspect Enrolled Voice Profiles',
    snippet: '/voice_profiles',
    description:
      'Lists your enrolled 24 kHz reference voices, preferred TTS engines (cosyvoice_3 / xtts_v2), and default languages.',
  },
  {
    id: 'direct-dm',
    title: 'Direct 1-on-1 Bot DM Chat & Voice Notes',
    snippet: 'es: Hey there! Let us grab dinner around 7:30 PM tonight.',
    description:
      'DM your bot with a lang prefix (es:, zh:, ja:, fr:...), plain text, or a recorded voice note—or use /mandarin_voice—to receive a native flags: 8192 waveform voice bubble.',
  },
];

const PLACEHOLDER_APP_ID = '123456789012345678';

export const GuideTab: React.FC = () => {
  const runtimeConfig = useAppSelector((s) => s.connection.runtimeConfig);
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const displayHost =
    typeof window !== 'undefined' &&
    window.location?.hostname &&
    window.location.hostname !== 'localhost' &&
    window.location.hostname !== '127.0.0.1'
      ? window.location.hostname
      : 'localhost';
  const httpsStudioOrigin = `https://${displayHost}:3443`;
  const httpApiOrigin = `http://${displayHost}:8080`;

  const rawAppId = String(
    runtimeConfig?.discord_application_id_str ||
      runtimeConfig?.discord_application_id ||
      '',
  ).trim();

  const hasValidAppId =
    /^\d+$/.test(rawAppId) && rawAppId !== '0' && rawAppId !== PLACEHOLDER_APP_ID;

  const userInstallUrl = hasValidAppId
    ? `https://discord.com/oauth2/authorize?client_id=${rawAppId}&integration_type=1&scope=applications.commands`
    : '';
  const botDmInstallUrl = hasValidAppId
    ? `https://discord.com/oauth2/authorize?client_id=${rawAppId}&integration_type=0&scope=bot+applications.commands&permissions=274877910016`
    : '';

  const handleCopySnippet = async (id: string, text: string) => {
    try {
      await navigator.clipboard?.writeText(text);
      setCopiedId(id);
      window.setTimeout(() => {
        setCopiedId((prev) => (prev === id ? null : prev));
      }, 1800);
    } catch {
      // Clipboard API unavailable in non-secure context; ignore gracefully.
    }
  };

  return (
    <div className="stack-col">
      <section className="panel" data-testid="discord-quick-launch-card">
        <div className="panel-header">
          <div>
            <h2>1-on-1 Discord Bot Quick-Launch &amp; Command Cheat-Sheet</h2>
            <p className="muted">
              Install your personal owner-locked Discord app once to unlock{' '}
              <code>/voice_clone</code> everywhere and open a private 1-on-1 DM chat with
              your cloned voice bot.
            </p>
          </div>
          <span className={`badge ${hasValidAppId ? 'badge-ok' : 'badge-warn'}`}>
            {hasValidAppId ? `App ID: ${rawAppId}` : 'Discord App Unconfigured'}
          </span>
        </div>

        {hasValidAppId ? (
          <div className="inline-actions" style={{ marginBottom: '1rem' }}>
            <a
              href={userInstallUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="btn btn-primary"
            >
              1. Install to Personal Account (USER_INSTALL) ↗
            </a>
            <a
              href={botDmInstallUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="btn btn-secondary"
            >
              2. Authorize Bot &amp; Open 1-on-1 Bot DM ↗
            </a>
          </div>
        ) : (
          <div className="alert alert-warn" style={{ marginBottom: '1rem' }}>
            <strong>Run just setup-discord to populate DISCORD_APPLICATION_ID</strong>{' '}
            (executes <code>./scripts/setup-discord.sh</code> to walk through creating
            your Discord app, verifying your bot token, and opening both OAuth install
            links).
          </div>
        )}

        <div className="guide-grid">
          {DISCORD_CHEAT_SHEET.map((item) => (
            <div key={item.id} className="guide-card">
              <div className="label-row">
                <h3>{item.title}</h3>
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  onClick={() => {
                    void handleCopySnippet(item.id, item.snippet);
                  }}
                >
                  {copiedId === item.id ? 'Copied!' : 'Copy'}
                </button>
              </div>
              <pre className="code-block">{item.snippet}</pre>
              <p className="muted small">{item.description}</p>
            </div>
          ))}
        </div>
      </section>

      <section className="panel">
        <h2>How Reference Voice Recording Works (Getting Studio-Grade Clones)</h2>
        <p className="muted">
          Open <code>http://localhost:3000</code> on your Windows/Linux desktop—or{' '}
          <code>{httpsStudioOrigin}</code> from any phone or laptop on your LAN (accepting the
          local self-signed SAN certificate once)—to unlock live in-browser <code>24 kHz</code>{' '}
          microphone recording. CosyVoice 3.0 and XTTS-v2 extract speaker acoustic embeddings
          directly from your enrolled clip—following these 5 rules dramatically improves warmth,
          prosody, and cross-lingual naturalness:
        </p>
        <div className="guide-grid">
          <div className="guide-card">
            <h3>1. Quiet, Dry Room</h3>
            <p>
              Turn off desk fans, AC vents, and speaker playback. Room reverb gets cloned
              alongside your voice—soft furnishings or a cardioid desk mic work wonders.
            </p>
          </div>
          <div className="guide-card">
            <h3>2. 15–20 cm Off-Axis Placement</h3>
            <p>
              Place your microphone 15–20 cm (6–8 inches) away at a slight 30° angle from
              your mouth so &quot;p&quot; and &quot;b&quot; plosives never spike the diaphragm.
            </p>
          </div>
          <div className="guide-card">
            <h3>3. Zero Clipping (Gate 0 ≤ 50 bps)</h3>
            <p>
              Keep peak levels below <code>-0.5 dBFS</code>. Gate 0 automatically rejects
              any clip with more than <code>50 bps</code> (<code>0.50%</code>) clipped
              samples so distorted harmonics never pollute your voice profile.
            </p>
          </div>
          <div className="guide-card">
            <h3>4. 12–20 Second Sweet Spot</h3>
            <p>
              Shorter than 6 seconds starves speaker conditioning; longer than 25 seconds
              slows prompt attention without improving similarity. Aim right for 15 seconds.
            </p>
          </div>
          <div className="guide-card">
            <h3>5. Exact Reference Transcript</h3>
            <p>
              Use our 1-click guided reading scripts so the English reference transcript
              matches every word spoken—preventing prompt bleed at the start of translated
              clips.
            </p>
          </div>
        </div>
      </section>

      <section className="panel">
        <h2>Compliant Personal Discord Account &amp; External Delivery Modes</h2>
        <p className="muted">
          Automating raw Discord user tokens violates Discord Community Guidelines. Instead,
          this pipeline provides 4 compliant delivery workflows across live calls, private
          1:1 DMs, iOS, and servers:
        </p>
        <div className="guide-grid">
          <div className="guide-card">
            <h3>Mode A1: Virtual Mic (`format=wav`)</h3>
            <p>
              Install <strong>BlackHole 2ch</strong> (macOS) or <strong>VB-Cable</strong>{' '}
              (Windows/Linux PipeWire) and set it as Discord’s Input Device. Stream{' '}
              <code>
                POST {httpApiOrigin}/api/v1/voice-jobs/render?format=wav&amp;lang=es
              </code>{' '}
              (`48 kHz` PCM) into the virtual cable so live voice calls come{' '}
              <strong>100% from your real personal Discord account</strong> with zero{' '}
              <code>APP</code> badge.
            </p>
          </div>
          <div className="guide-card">
            <h3>Mode A2: iOS Shortcut &amp; Clipboard (`format=ogg`)</h3>
            <p>
              Call{' '}
              <code>
                POST {httpApiOrigin}/api/v1/voice-jobs/render?format=ogg&amp;lang=zh
              </code>{' '}
              (or via <code>{httpsStudioOrigin}</code>) from an Apple Shortcut or macOS
              hotkey script to receive verified `48 kHz` VoIP Ogg/Opus audio ready to Share
              or Paste (`Cmd+V`) into any private DM or group chat.
            </p>
          </div>
          <div className="guide-card">
            <h3>Mode B: Discord `USER_INSTALL` Personal App</h3>
            <p>
              Install the bot onto your personal Discord profile (`integration_types: [1]`,
              `contexts: [0, 1, 2]`) to invoke slash voice translation inside any 1:1 DM,
              Group DM, or external server without needing server admin permissions.
            </p>
          </div>
          <div className="guide-card">
            <h3>Mode C: Native `flags: 8192` Bubble &amp; Webhooks</h3>
            <p>
              In your own Discord servers, select <code>BotVoiceBubble</code> to post native
              mobile/desktop waveform voice bubbles (`IS_VOICE_MESSAGE`, `flags: 8192`) or{' '}
              <code>ProfileWebhook</code> to match your custom display name and avatar.
            </p>
          </div>
        </div>
      </section>
    </div>
  );
};
