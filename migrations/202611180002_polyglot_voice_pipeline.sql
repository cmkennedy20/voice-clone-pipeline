ALTER TABLE voice_profiles
    ADD COLUMN default_target_language TEXT NOT NULL DEFAULT 'zh';

ALTER TABLE voice_jobs
    ADD COLUMN target_language TEXT NOT NULL DEFAULT 'zh',
    ADD COLUMN output_wav_path TEXT;

CREATE INDEX idx_voice_jobs_lang_created ON voice_jobs (target_language, created_at DESC);
