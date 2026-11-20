import React, { useEffect, useState } from 'react';
import { apiFetchProfileReferenceAudio } from '../api/client';
import { useAppDispatch, useAppSelector } from '../store/hooks';
import {
  deleteProfile,
  fetchProfiles,
  updateProfile,
} from '../store/profilesSlice';
import { setSelectedProfileSlug } from '../store/studioSlice';
import type { TtsEngineKind, VoiceProfile } from '../types/api';
import {
  getProfileLikenessBadge,
  ProfileLikenessStudio,
} from './ProfileLikenessStudio';

export interface ProfilesTabProps {
  onSelectForPlayground: () => void;
}

export const ProfilesTab: React.FC<ProfilesTabProps> = ({ onSelectForPlayground }) => {
  const dispatch = useAppDispatch();
  const connection = useAppSelector((s) => s.connection);
  const languages = useAppSelector((s) => s.languages.items);
  const { items, status, mutationStatus, error } = useAppSelector((s) => s.profiles);
  const activeSlug = useAppSelector((s) => s.studio.selectedProfileSlug);

  const [editingSlug, setEditingSlug] = useState<string | null>(null);
  const [expandedStudioSlug, setExpandedStudioSlug] = useState<string | null>(null);
  const [editDisplayName, setEditDisplayName] = useState('');
  const [editEngine, setEditEngine] = useState<TtsEngineKind>('cosyvoice_3');
  const [editLang, setEditLang] = useState('zh');
  const [editAvatar, setEditAvatar] = useState('');
  const [editTranscript, setEditTranscript] = useState('');

  const [audioUrls, setAudioUrls] = useState<Record<string, string>>({});
  const [loadingAudioSlug, setLoadingAudioSlug] = useState<string | null>(null);
  const [audioError, setAudioError] = useState<string | null>(null);

  useEffect(() => {
    return () => {
      for (const url of Object.values(audioUrls)) {
        URL.revokeObjectURL(url);
      }
    };
  }, [audioUrls]);

  const startEditing = (profile: VoiceProfile) => {
    setEditingSlug(profile.slug);
    setEditDisplayName(profile.display_name);
    setEditEngine(profile.preferred_engine);
    setEditLang(profile.default_target_language);
    setEditAvatar(profile.avatar_url ?? '');
    setEditTranscript(profile.reference_transcript_en);
  };

  const handleSaveEdit = async (slug: string) => {
    const action = await dispatch(
      updateProfile({
        slug,
        patch: {
          display_name: editDisplayName.trim(),
          preferred_engine: editEngine,
          default_target_language: editLang,
          avatar_url: editAvatar.trim().length > 0 ? editAvatar.trim() : null,
          reference_transcript_en: editTranscript.trim(),
        },
      }),
    );
    if (updateProfile.fulfilled.match(action)) {
      setEditingSlug(null);
    }
  };

  const handleListenReference = async (slug: string) => {
    setLoadingAudioSlug(slug);
    setAudioError(null);
    try {
      const blob = await apiFetchProfileReferenceAudio(
        {
          apiBaseUrl: connection.apiBaseUrl,
          bearerToken: connection.bearerToken,
          discordUserId: connection.discordUserId,
        },
        slug,
      );
      const url = URL.createObjectURL(blob);
      setAudioUrls((prev) => {
        if (prev[slug]) {
          URL.revokeObjectURL(prev[slug]);
        }
        return { ...prev, [slug]: url };
      });
    } catch (err) {
      setAudioError(
        err instanceof Error ? err.message : 'Failed fetching reference audio.',
      );
    } finally {
      setLoadingAudioSlug(null);
    }
  };

  return (
    <section className="panel">
      <div className="panel-header">
        <div>
          <h2>Enrolled Voice Profiles ({items.length})</h2>
          <p className="muted">
            Add multi-sample reference clips, tune voice likeness parameters, train a
            dedicated RTX 3090 XTTS-v2 checkpoint, or edit profile defaults.
          </p>
        </div>
        <button
          type="button"
          className="btn btn-secondary"
          onClick={() => void dispatch(fetchProfiles())}
          disabled={status === 'loading'}
        >
          {status === 'loading' ? 'Refreshing...' : '↻ Refresh Profiles'}
        </button>
      </div>

      {error && (
        <div className="alert alert-error" role="alert">
          {error}
        </div>
      )}
      {audioError && (
        <div className="alert alert-error" role="alert">
          {audioError}
        </div>
      )}

      {items.length === 0 ? (
        <div className="empty-state">
          No voice profiles enrolled yet. Open the{' '}
          <strong>Voice Recording & Enrollment Studio</strong> tab to record your 15-second
          reference voice!
        </div>
      ) : (
        <div className="profiles-list">
          {items.map((profile) => {
            const isEditing = editingSlug === profile.slug;
            const isStudioExpanded = expandedStudioSlug === profile.slug;
            const isSelected = activeSlug === profile.slug;
            const refAudioUrl = audioUrls[profile.slug];
            const likenessBadge = getProfileLikenessBadge(profile);
            const clipCount = Math.max(profile.sample_count, profile.samples.length);
            const totalSecs = (profile.total_sample_duration_ms / 1000).toFixed(1);

            return (
              <div
                key={profile.slug}
                className={`profile-card ${isSelected ? 'profile-card-selected' : ''}`}
              >
                <div className="profile-card-top">
                  <div>
                    <div className="inline-actions">
                      <h3>{profile.display_name}</h3>
                      <code className="slug-tag">{profile.slug}</code>
                      {isSelected && (
                        <span className="badge badge-ok">Active in Playground</span>
                      )}
                      <span className={`badge ${likenessBadge.badgeClass}`}>
                        {likenessBadge.label}
                      </span>
                    </div>
                    <div className="metrics-pills">
                      <span className="pill">
                        Reference Clips:{' '}
                        <strong>
                          {clipCount} {clipCount === 1 ? 'clip' : 'clips'} ({totalSecs}s)
                        </strong>
                      </span>
                      <span className="pill">
                        Preferred Engine: <code>{profile.preferred_engine}</code>
                      </span>
                      <span className="pill">
                        Default Language:{' '}
                        <strong>{profile.default_target_language.toUpperCase()}</strong>
                      </span>
                      <span className="pill">
                        Temp: <code>{profile.tuning.temperature.toFixed(2)}</code>
                      </span>
                      <span className="pill">
                        Created:{' '}
                        {profile.created_at
                          ? new Date(profile.created_at).toLocaleString()
                          : '—'}
                      </span>
                    </div>
                  </div>

                  <div className="inline-actions">
                    <button
                      type="button"
                      className="btn btn-primary btn-sm"
                      onClick={() => {
                        dispatch(setSelectedProfileSlug(profile.slug));
                        onSelectForPlayground();
                      }}
                    >
                      Use in Playground
                    </button>
                    <button
                      type="button"
                      className={`btn btn-sm ${
                        isStudioExpanded ? 'btn-primary' : 'btn-secondary'
                      }`}
                      onClick={() =>
                        setExpandedStudioSlug(isStudioExpanded ? null : profile.slug)
                      }
                    >
                      {isStudioExpanded
                        ? 'Hide Clips, Tuning & Fine-Tune'
                        : 'Manage Clips, Tuning & RTX 3090 Fine-Tune'}
                    </button>
                    <button
                      type="button"
                      className="btn btn-secondary btn-sm"
                      disabled={loadingAudioSlug === profile.slug}
                      onClick={() => void handleListenReference(profile.slug)}
                    >
                      {loadingAudioSlug === profile.slug
                        ? 'Loading...'
                        : '▶ Listen Reference WAV'}
                    </button>
                    <button
                      type="button"
                      className="btn btn-ghost btn-sm"
                      onClick={() =>
                        isEditing ? setEditingSlug(null) : startEditing(profile)
                      }
                    >
                      {isEditing ? 'Cancel' : 'Edit'}
                    </button>
                    <button
                      type="button"
                      className="btn btn-danger btn-sm"
                      disabled={mutationStatus === 'saving'}
                      onClick={() => void dispatch(deleteProfile(profile.slug))}
                    >
                      Delete
                    </button>
                  </div>
                </div>

                <p className="profile-transcript">
                  <span className="muted">Primary Reference Transcript:</span> “
                  {profile.reference_transcript_en}”
                </p>

                {refAudioUrl && (
                  <div className="audio-player-row">
                    <audio controls src={refAudioUrl} className="audio-element" />
                    <a
                      href={refAudioUrl}
                      download={`${profile.slug}-reference.wav`}
                      className="btn btn-ghost btn-sm"
                    >
                      Download Reference WAV
                    </a>
                  </div>
                )}

                {isEditing && (
                  <div className="subpanel form-stack">
                    <div className="form-grid-3">
                      <div className="field-group">
                        <label htmlFor={`edit-name-${profile.slug}`}>Display Name</label>
                        <input
                          id={`edit-name-${profile.slug}`}
                          type="text"
                          value={editDisplayName}
                          onChange={(e) => setEditDisplayName(e.target.value)}
                        />
                      </div>
                      <div className="field-group">
                        <label htmlFor={`edit-engine-${profile.slug}`}>
                          Preferred Engine
                        </label>
                        <select
                          id={`edit-engine-${profile.slug}`}
                          value={editEngine}
                          onChange={(e) => setEditEngine(e.target.value as TtsEngineKind)}
                        >
                          <option value="cosyvoice_3">cosyvoice_3</option>
                          <option value="xtts_v2">xtts_v2</option>
                        </select>
                      </div>
                      <div className="field-group">
                        <label htmlFor={`edit-lang-${profile.slug}`}>
                          Default Target Language
                        </label>
                        <select
                          id={`edit-lang-${profile.slug}`}
                          value={editLang}
                          onChange={(e) => setEditLang(e.target.value)}
                        >
                          {languages.map((l) => (
                            <option key={l.code} value={l.code}>
                              {l.english_name} [{l.code}]
                            </option>
                          ))}
                        </select>
                      </div>
                    </div>

                    <div className="field-group">
                      <label htmlFor={`edit-avatar-${profile.slug}`}>Avatar URL</label>
                      <input
                        id={`edit-avatar-${profile.slug}`}
                        type="url"
                        value={editAvatar}
                        onChange={(e) => setEditAvatar(e.target.value)}
                      />
                    </div>

                    <div className="field-group">
                      <label htmlFor={`edit-transcript-${profile.slug}`}>
                        Reference Transcript (EN)
                      </label>
                      <textarea
                        id={`edit-transcript-${profile.slug}`}
                        rows={2}
                        value={editTranscript}
                        onChange={(e) => setEditTranscript(e.target.value)}
                      />
                    </div>

                    <div className="inline-actions">
                      <button
                        type="button"
                        className="btn btn-primary btn-sm"
                        disabled={mutationStatus === 'saving'}
                        onClick={() => void handleSaveEdit(profile.slug)}
                      >
                        Save Changes
                      </button>
                    </div>
                  </div>
                )}

                {isStudioExpanded && <ProfileLikenessStudio profile={profile} />}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
};
