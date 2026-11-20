import React, { useEffect, useState } from 'react';
import { EnrollmentTab } from './components/EnrollmentTab';
import { GuideTab } from './components/GuideTab';
import { HistoryConfigTab } from './components/HistoryConfigTab';
import { PlaygroundTab } from './components/PlaygroundTab';
import { ProfilesTab } from './components/ProfilesTab';
import { checkHealthAndConfig } from './store/connectionSlice';
import { useAppDispatch, useAppSelector } from './store/hooks';
import { fetchLanguages } from './store/languagesSlice';
import { fetchProfiles } from './store/profilesSlice';

export type StudioTabId = 'playground' | 'enroll' | 'profiles' | 'history' | 'guide';

export const App: React.FC = () => {
  const dispatch = useAppDispatch();
  const healthStatus = useAppSelector((s) => s.connection.healthStatus);
  const runtimeConfig = useAppSelector((s) => s.connection.runtimeConfig);
  const profilesCount = useAppSelector((s) => s.profiles.items.length);
  const languagesCount = useAppSelector((s) => s.languages.items.length);
  const activeProfileSlug = useAppSelector((s) => s.studio.selectedProfileSlug);

  const [activeTab, setActiveTab] = useState<StudioTabId>('playground');

  useEffect(() => {
    void dispatch(checkHealthAndConfig());
    void dispatch(fetchLanguages());
    void dispatch(fetchProfiles());
  }, [dispatch]);

  return (
    <div className="studio-shell">
      <header className="studio-topbar">
        <div className="brand-block">
          <div className="brand-mark" aria-hidden="true">
            ◈
          </div>
          <div>
            <h1>Polyglot Voice Clone Studio</h1>
            <p className="topbar-sub">
              Local RTX 3090 Zero-Shot Voice Cloning • Qwen3 Spoken Script • Whisper
              Reverse-ASR CER Gate
            </p>
          </div>
        </div>

        <div className="status-cluster">
          <span
            className={`badge ${
              healthStatus === 'ready'
                ? 'badge-ok'
                : healthStatus === 'checking'
                  ? 'badge-neutral'
                  : 'badge-danger'
            }`}
            data-testid="topbar-health-badge"
          >
            {healthStatus === 'ready'
              ? '● GPU API Ready'
              : healthStatus === 'checking'
                ? '◌ Checking API...'
                : '○ API Offline / Unreachable'}
          </span>
          <span className="pill">
            Languages: <strong>{languagesCount}</strong>
          </span>
          <span className="pill">
            Profiles: <strong>{profilesCount}</strong>
          </span>
          {activeProfileSlug && (
            <span className="pill">
              Active Voice: <code>{activeProfileSlug}</code>
            </span>
          )}
          {runtimeConfig && (
            <span className="pill">
              CER Gate: <strong>{runtimeConfig.max_cer_bps} bps</strong>
            </span>
          )}
          {runtimeConfig?.standalone_desktop_mode && !runtimeConfig.discord_configured && (
            <button
              type="button"
              className="btn btn-secondary btn-sm"
              onClick={() => setActiveTab('history')}
            >
              ⚙ Pair Discord Bot
            </button>
          )}
        </div>
      </header>

      <nav className="studio-tabs" aria-label="Studio Navigation">
        <button
          type="button"
          className={`tab-btn ${activeTab === 'playground' ? 'tab-active' : ''}`}
          onClick={() => setActiveTab('playground')}
        >
          1. Multilingual Playground & API
        </button>
        <button
          type="button"
          className={`tab-btn ${activeTab === 'enroll' ? 'tab-active' : ''}`}
          onClick={() => setActiveTab('enroll')}
        >
          2. Voice Recording & Enrollment Studio
        </button>
        <button
          type="button"
          className={`tab-btn ${activeTab === 'profiles' ? 'tab-active' : ''}`}
          onClick={() => setActiveTab('profiles')}
        >
          3. Profile Management ({profilesCount})
        </button>
        <button
          type="button"
          className={`tab-btn ${activeTab === 'history' ? 'tab-active' : ''}`}
          onClick={() => setActiveTab('history')}
        >
          4. History, Config & Benchmarks
        </button>
        <button
          type="button"
          className={`tab-btn ${activeTab === 'guide' ? 'tab-active' : ''}`}
          onClick={() => setActiveTab('guide')}
        >
          5. How Recording & Delivery Works
        </button>
      </nav>

      <main className="studio-main">
        {activeTab === 'playground' && (
          <PlaygroundTab onNavigateToEnroll={() => setActiveTab('enroll')} />
        )}
        {activeTab === 'enroll' && <EnrollmentTab />}
        {activeTab === 'profiles' && (
          <ProfilesTab onSelectForPlayground={() => setActiveTab('playground')} />
        )}
        {activeTab === 'history' && <HistoryConfigTab />}
        {activeTab === 'guide' && <GuideTab />}
      </main>
    </div>
  );
};
