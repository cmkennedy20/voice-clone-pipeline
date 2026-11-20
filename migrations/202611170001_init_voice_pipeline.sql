CREATE TABLE voice_profiles (
    id UUID PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    avatar_url TEXT,
    reference_wav_path TEXT NOT NULL,
    reference_transcript_en TEXT NOT NULL,
    preferred_engine TEXT NOT NULL CHECK (preferred_engine IN ('cosyvoice_3', 'xtts_v2')),
    owner_discord_user_id BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE voice_jobs (
    id UUID PRIMARY KEY,
    profile_id UUID NOT NULL REFERENCES voice_profiles(id),
    delivery_mode TEXT NOT NULL CHECK (delivery_mode IN ('client_return_only', 'user_install_followup', 'profile_webhook', 'bot_voice_bubble')),
    source_sha256 TEXT NOT NULL,
    english_transcript TEXT NOT NULL,
    spoken_hanzi TEXT NOT NULL,
    pinyin TEXT NOT NULL,
    english_back_gloss TEXT NOT NULL,
    back_transcribed_zh TEXT NOT NULL,
    engine_used TEXT NOT NULL,
    cer_bps INT NOT NULL CHECK (cer_bps >= 0),
    attempts_used SMALLINT NOT NULL CHECK (attempts_used >= 1),
    duration_ms INT NOT NULL,
    total_latency_ms INT NOT NULL,
    output_ogg_path TEXT NOT NULL,
    discord_channel_id BIGINT,
    discord_message_id BIGINT,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE benchmark_runs (
    id UUID PRIMARY KEY,
    engine_tested TEXT NOT NULL,
    clips_evaluated SMALLINT NOT NULL,
    gate0_accuracy_bps INT NOT NULL,
    digit_hygiene_bps INT NOT NULL,
    mean_cer_bps INT NOT NULL,
    mean_speaker_sim_bps INT NOT NULL,
    format_compliance_bps INT NOT NULL,
    p95_latency_ms INT NOT NULL,
    peak_vram_mib INT NOT NULL,
    passed_all_gates BOOLEAN NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_voice_jobs_profile_created ON voice_jobs (profile_id, created_at DESC);
