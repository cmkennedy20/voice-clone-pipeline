ALTER TABLE voice_profiles
    ADD COLUMN tuning_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN finetuned_checkpoint_dir TEXT,
    ADD COLUMN finetune_status TEXT NOT NULL DEFAULT 'none'
        CHECK (finetune_status IN ('none', 'queued', 'running', 'succeeded', 'failed')),
    ADD COLUMN finetune_error TEXT,
    ADD COLUMN finetuned_at TIMESTAMPTZ;

CREATE TABLE voice_profile_samples (
    id UUID PRIMARY KEY,
    profile_id UUID NOT NULL REFERENCES voice_profiles(id) ON DELETE CASCADE,
    wav_path TEXT NOT NULL,
    transcript_en TEXT NOT NULL,
    style_label TEXT NOT NULL DEFAULT 'conversational',
    duration_ms INT NOT NULL CHECK (duration_ms > 0),
    is_primary BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_voice_profile_samples_profile_order
    ON voice_profile_samples (profile_id, is_primary DESC, created_at ASC);

INSERT INTO voice_profile_samples (
    id, profile_id, wav_path, transcript_en, style_label, duration_ms, is_primary, created_at
)
SELECT id, id, reference_wav_path, reference_transcript_en, 'primary', 15000, TRUE, created_at
  FROM voice_profiles
ON CONFLICT (id) DO NOTHING;

CREATE TABLE voice_finetune_jobs (
    id UUID PRIMARY KEY,
    profile_id UUID NOT NULL REFERENCES voice_profiles(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed')),
    epochs SMALLINT NOT NULL CHECK (epochs BETWEEN 1 AND 50),
    learning_rate DOUBLE PRECISION NOT NULL,
    sample_count SMALLINT NOT NULL CHECK (sample_count >= 1),
    current_epoch SMALLINT NOT NULL DEFAULT 0,
    progress_bps INT NOT NULL DEFAULT 0 CHECK (progress_bps BETWEEN 0 AND 10000),
    last_loss DOUBLE PRECISION,
    checkpoint_dir TEXT,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_voice_finetune_jobs_profile_created
    ON voice_finetune_jobs (profile_id, created_at DESC);
