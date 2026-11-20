//! Deterministic `FixedClock` and pre-wired `AppContext` builders for `Layers 4–5` tests.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeDelta, Utc};
use common::{basis_points::BasisPoints, clock::Clock, secret::Secret};

use crate::{
    clients::{
        FakeCallCounters, FakeDiscordPublisher, FakeMandarinTranslator,
        FakeSpeakerSimilarityScorer, FakeSpeechRecognizer, FakeVoiceFinetuner,
        FakeVoiceSynthesizer,
    },
    config::AppConfig,
    context::AppContext,
    dsp::opus_and_wav::RealAudioCodecEngine,
};

const DEFAULT_ENGLISH_TRANSCRIPT: &str = "We are having a meeting at three thirty this afternoon.";
const DEFAULT_SPOKEN_HANZI: &str = "今天下午三点半我们开会。";
const DEFAULT_PINYIN: &str = "jīn tiān xià wǔ sān diǎn bàn wǒ men kāi huì.";
const DEFAULT_BACK_GLOSS: &str = "We have a meeting at three thirty this afternoon.";
const CLEAN_BACK_TRANSCRIBED_ZH: &str = "今天下午三点半我们开会";
const MUMBLING_HALLUCINATED_ZH: &str = "完全不同的胡言乱语测试文本";
const PINNED_EPOCH_SECS: i64 = 1_794_974_400; // 2026-11-18T04:00:00Z

/// Thread-safe deterministic [`Clock`] pinned to an explicit `UTC` timestamp.
#[derive(Debug)]
pub struct FixedClock {
    now: Mutex<DateTime<Utc>>,
}

impl FixedClock {
    /// Creates a [`FixedClock`] frozen at `timestamp`.
    #[must_use]
    pub const fn new(timestamp: DateTime<Utc>) -> Self {
        Self {
            now: Mutex::new(timestamp),
        }
    }

    /// Replaces the current pinned `UTC` timestamp.
    pub fn set(&self, timestamp: DateTime<Utc>) {
        match self.now.lock() {
            Ok(mut guard) => *guard = timestamp,
            Err(poisoned) => *poisoned.into_inner() = timestamp,
        }
    }

    /// Advances the pinned `UTC` timestamp forward by `delta`.
    pub fn advance(&self, delta: TimeDelta) {
        match self.now.lock() {
            Ok(mut guard) => *guard += delta,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                *guard += delta;
            }
        }
    }
}

impl Default for FixedClock {
    fn default() -> Self {
        let ts =
            DateTime::from_timestamp(PINNED_EPOCH_SECS, 0).unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
        Self::new(ts)
    }
}

impl Clock for FixedClock {
    fn now_utc(&self) -> DateTime<Utc> {
        match self.now.lock() {
            Ok(guard) => *guard,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }
}

fn assemble_context_with_asr(
    pool: sqlx::PgPool,
    counters: &Arc<FakeCallCounters>,
    asr: FakeSpeechRecognizer,
) -> AppContext {
    let mut config = AppConfig::test_default();
    config.discord_owner_user_id = 777_888_999;
    config.owner_api_bearer_token = Secret::new("test-owner-bearer-token".to_owned());
    config.max_cer_bps = BasisPoints::from_raw(1_200);
    config.max_tts_attempts = 3;

    let translator = FakeMandarinTranslator::new(
        Arc::clone(counters),
        DEFAULT_SPOKEN_HANZI,
        DEFAULT_PINYIN,
        DEFAULT_BACK_GLOSS,
    );
    let tts = FakeVoiceSynthesizer::new(Arc::clone(counters));
    let finetuner = FakeVoiceFinetuner::new(Arc::clone(counters));
    let discord = FakeDiscordPublisher::new(Arc::clone(counters));
    let speaker_sim =
        FakeSpeakerSimilarityScorer::new(Arc::clone(counters), BasisPoints::from_raw(8_650));

    AppContext {
        config: Arc::new(config),
        db: pool,
        gpu_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        clock: Arc::new(FixedClock::default()),
        codec: Arc::new(RealAudioCodecEngine::new()),
        asr: Arc::new(asr),
        translator: Arc::new(translator),
        tts: Arc::new(tts),
        finetuner: Arc::new(finetuner),
        discord: Arc::new(discord),
        speaker_sim: Arc::new(speaker_sim),
    }
}

/// Builds a nominal [`AppContext`] with real `Symphonia`/`Opus`/`WAV` codecs and clean `0 bps`
/// `CER` Mandarin translation/verification doubles.
#[must_use]
pub fn build_test_context(pool: sqlx::PgPool) -> (AppContext, Arc<FakeCallCounters>) {
    let counters = Arc::new(FakeCallCounters::new());
    let asr = FakeSpeechRecognizer::new(
        Arc::clone(&counters),
        DEFAULT_ENGLISH_TRANSCRIPT,
        CLEAN_BACK_TRANSCRIBED_ZH,
    );
    let ctx = assemble_context_with_asr(pool, &counters, asr);
    (ctx, counters)
}

/// Builds an [`AppContext`] whose reverse-`ASR` always returns hallucinated Mandarin
/// (`"完全不同的胡言乱语测试文本"`), forcing all `3` seed-shifted `TTS` attempts to fail `CER`.
#[must_use]
pub fn build_test_context_with_always_mumbling_tts(
    pool: sqlx::PgPool,
) -> (AppContext, Arc<FakeCallCounters>) {
    let counters = Arc::new(FakeCallCounters::new());
    let asr = FakeSpeechRecognizer::new(
        Arc::clone(&counters),
        DEFAULT_ENGLISH_TRANSCRIPT,
        MUMBLING_HALLUCINATED_ZH,
    );
    let ctx = assemble_context_with_asr(pool, &counters, asr);
    (ctx, counters)
}

/// Builds an [`AppContext`] that mumbles on attempts `< pass_on_attempt` and self-heals to clean
/// matching Hanzi (`0 bps` `CER`) on `>= pass_on_attempt`.
#[must_use]
pub fn build_test_context_healing_on_attempt(
    pool: sqlx::PgPool,
    pass_on_attempt: usize,
) -> (AppContext, Arc<FakeCallCounters>) {
    let counters = Arc::new(FakeCallCounters::new());
    let asr = FakeSpeechRecognizer::healing_on_attempt(
        Arc::clone(&counters),
        DEFAULT_ENGLISH_TRANSCRIPT,
        MUMBLING_HALLUCINATED_ZH,
        CLEAN_BACK_TRANSCRIBED_ZH,
        pass_on_attempt,
    );
    let ctx = assemble_context_with_asr(pool, &counters, asr);
    (ctx, counters)
}

/// Builds a DB-free lazy [`AppContext`] for testing stateless `HTTP` routes (`SPA`, `manifest`, `status`).
pub fn build_lazy_test_context() -> Result<(AppContext, Arc<FakeCallCounters>), sqlx::Error> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://test_user:test_password@127.0.0.1:5432/unused")?;
    Ok(build_test_context(pool))
}

/// Creates an isolated migrated `PostgreSQL` schema pool on `DATABASE_URL` for `oneshot` API tests.
pub async fn create_isolated_test_pool() -> Result<sqlx::PgPool, sqlx::Error> {
    let db_url = AppConfig::database_url_from_env()
        .map_err(|err| sqlx::Error::Configuration(Box::new(err)))?;
    let schema = format!("api_test_{}", uuid::Uuid::now_v7().simple());
    let admin_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(db_url.expose_secret())
        .await?;
    sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
        .execute(&admin_pool)
        .await?;
    admin_pool.close().await;

    let search_sql = format!("SET search_path TO \"{schema}\"");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |conn, _meta| {
            let sql = search_sql.clone();
            Box::pin(async move {
                sqlx::Executor::execute(conn, sql.as_str()).await?;
                Ok(())
            })
        })
        .connect(db_url.expose_secret())
        .await?;
    sqlx::migrate!("../../migrations").run(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use chrono::{TimeDelta, TimeZone, Utc};
    use common::clock::Clock;

    use super::FixedClock;

    #[test]
    fn fixed_clock_preserves_and_advances_utc_time() {
        let initial = Utc.with_ymd_and_hms(2026, 11, 18, 4, 30, 0).unwrap();
        let clock = FixedClock::new(initial);
        assert_eq!(clock.now_utc(), initial);

        clock.advance(TimeDelta::seconds(45));
        assert_eq!(
            clock.now_utc(),
            Utc.with_ymd_and_hms(2026, 11, 18, 4, 30, 45).unwrap()
        );
    }
}
