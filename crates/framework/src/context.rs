//! Shared `AppContext` dependency container and `SpeakerSimilarityScorer` port re-export.

use std::{fmt, sync::Arc};

use common::clock::{Clock, SystemClock};
pub use domain::ports::SpeakerSimilarityScorer;
use domain::ports::{
    AudioCodecEngine, DiscordPublisher, ExternalPortError, MandarinTranslator, SpeechRecognizer,
    VoiceFinetuner, VoiceSynthesizer,
};

use crate::{
    clients::{
        discord_publisher::HttpDiscordPublisher, qwen_llama::QwenLlamaTranslator,
        tts_sidecar::HttpTtsSidecarClient, whisper_cuda::WhisperCudaRecognizer,
    },
    config::AppConfig,
    dsp::opus_and_wav::RealAudioCodecEngine,
};

/// Clonable application state carrying configuration, `PostgreSQL` pool, single-`GPU` semaphore,
/// deterministic clock, and narrow domain capability ports.
#[derive(Clone)]
pub struct AppContext {
    pub config: Arc<AppConfig>,
    pub db: sqlx::PgPool,
    pub gpu_semaphore: Arc<tokio::sync::Semaphore>,
    pub clock: Arc<dyn Clock>,
    pub codec: Arc<dyn AudioCodecEngine>,
    pub asr: Arc<dyn SpeechRecognizer>,
    pub translator: Arc<dyn MandarinTranslator>,
    pub tts: Arc<dyn VoiceSynthesizer>,
    pub finetuner: Arc<dyn VoiceFinetuner>,
    pub discord: Arc<dyn DiscordPublisher>,
    pub speaker_sim: Arc<dyn SpeakerSimilarityScorer>,
}

impl fmt::Debug for AppContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppContext")
            .field("config", &self.config)
            .field(
                "gpu_available_permits",
                &self.gpu_semaphore.available_permits(),
            )
            .finish_non_exhaustive()
    }
}

impl AppContext {
    /// Wires production `CUDA`, `DSP`, `HTTP` sidecar, and `Discord` `REST` adapters into an [`AppContext`].
    pub fn from_production_adapters(
        config: AppConfig,
        db: sqlx::PgPool,
    ) -> Result<Self, ExternalPortError> {
        let codec: Arc<dyn AudioCodecEngine> = Arc::new(RealAudioCodecEngine::with_vad_model_path(
            config.silero_vad_model_path.clone(),
        ));
        let asr: Arc<dyn SpeechRecognizer> = Arc::new(WhisperCudaRecognizer::load_or_standby(
            &config.whisper_model_path,
            config.whisper_use_gpu,
        )?);
        let translator: Arc<dyn MandarinTranslator> =
            Arc::new(QwenLlamaTranslator::new(config.translator_base_url.clone()));
        let tts_client = Arc::new(HttpTtsSidecarClient::new(config.tts_sidecar_url.clone()));
        let tts: Arc<dyn VoiceSynthesizer> = Arc::clone(&tts_client) as Arc<dyn VoiceSynthesizer>;
        let finetuner: Arc<dyn VoiceFinetuner> = Arc::clone(&tts_client) as Arc<dyn VoiceFinetuner>;
        let speaker_sim: Arc<dyn SpeakerSimilarityScorer> = tts_client;
        let discord: Arc<dyn DiscordPublisher> = Arc::new(
            HttpDiscordPublisher::new(config.discord_bot_token.clone())
                .with_studio_env_path(config.desktop.studio_env_path.clone()),
        );

        Ok(Self {
            config: Arc::new(config),
            db,
            gpu_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
            clock: Arc::new(SystemClock),
            codec,
            asr,
            translator,
            tts,
            finetuner,
            discord,
            speaker_sim,
        })
    }
}
