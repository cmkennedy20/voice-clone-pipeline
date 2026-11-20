//! Stateful deterministic `Fake*` capability doubles gated behind `test` / `test-support`.

use std::{
    f32::consts::TAU,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicUsize, Ordering},
    },
};

use common::basis_points::BasisPoints;
use domain::{
    audio::types::{Pcm16kMono, Pcm24kMono, RenderedAudioBundle},
    ports::{
        DiscordDeliveryTarget, DiscordPublisher, ExternalPortError, MandarinTranslator,
        SpeakerSimilarityScorer, SpeechRecognizer, SpokenScript, TargetLanguage,
        TranslatedMandarin, VoiceSynthesisRequest, VoiceSynthesizer,
    },
};

pub use crate::clients::fake_finetuner::FakeVoiceFinetuner;
use crate::clients::{fake_finetuner::RecordedSynthesisCall, fake_phrases::sample_for_language};

pub(crate) fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Thread-safe call counters, seed ledger, and `GPU` concurrency tracker for `Fake*` doubles.
#[derive(Debug, Default)]
pub struct FakeCallCounters {
    asr_en: AtomicUsize,
    asr_zh: AtomicUsize,
    translator: AtomicUsize,
    tts: AtomicUsize,
    discord: AtomicUsize,
    pub(crate) finetune_starts: AtomicUsize,
    pub(crate) finetune_polls: AtomicUsize,
    active_gpu: AtomicUsize,
    max_concurrent_gpu: AtomicUsize,
    seeds: Mutex<Vec<u64>>,
    pub(crate) last_synthesis: Mutex<Option<RecordedSynthesisCall>>,
}

struct GpuFlightGuard<'a>(&'a FakeCallCounters);

impl Drop for GpuFlightGuard<'_> {
    fn drop(&mut self) {
        self.0.active_gpu.fetch_sub(1, Ordering::SeqCst);
    }
}

impl FakeCallCounters {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn asr_en_calls(&self) -> usize {
        self.asr_en.load(Ordering::SeqCst)
    }

    pub fn asr_zh_calls(&self) -> usize {
        self.asr_zh.load(Ordering::SeqCst)
    }

    pub fn translator_calls(&self) -> usize {
        self.translator.load(Ordering::SeqCst)
    }

    pub fn tts_calls(&self) -> usize {
        self.tts.load(Ordering::SeqCst)
    }

    pub fn discord_publishes(&self) -> usize {
        self.discord.load(Ordering::SeqCst)
    }

    pub fn seeds_seen(&self) -> Vec<u64> {
        lock_or_recover(&self.seeds).clone()
    }

    pub fn max_concurrent_gpu_calls(&self) -> usize {
        self.max_concurrent_gpu.load(Ordering::SeqCst)
    }

    async fn enter_gpu_call(&self) -> GpuFlightGuard<'_> {
        let active = self.active_gpu.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_concurrent_gpu.fetch_max(active, Ordering::SeqCst);
        let guard = GpuFlightGuard(self);
        tokio::task::yield_now().await;
        guard
    }
}

/// Deterministic [`SpeechRecognizer`] double supporting scripted per-attempt reverse-`ASR` outputs.
#[derive(Debug)]
pub struct FakeSpeechRecognizer {
    counters: Arc<FakeCallCounters>,
    english_transcript: String,
    scripted_zh: Vec<String>,
    fallback_zh: String,
}

impl FakeSpeechRecognizer {
    #[must_use]
    pub fn new(
        counters: Arc<FakeCallCounters>,
        english: impl Into<String>,
        mandarin: impl Into<String>,
    ) -> Self {
        Self::with_scripted_mandarin(counters, english, Vec::new(), mandarin)
    }

    #[must_use]
    pub fn with_scripted_mandarin(
        counters: Arc<FakeCallCounters>,
        english: impl Into<String>,
        scripted_zh: Vec<String>,
        fallback_zh: impl Into<String>,
    ) -> Self {
        Self {
            counters,
            english_transcript: english.into(),
            scripted_zh,
            fallback_zh: fallback_zh.into(),
        }
    }

    /// Mumbles `mumbled_zh` on 1-based attempts `< pass_on_attempt` and returns `clean_zh` thereafter.
    #[must_use]
    pub fn healing_on_attempt(
        counters: Arc<FakeCallCounters>,
        english: impl Into<String>,
        mumbled_zh: impl Into<String>,
        clean_zh: impl Into<String>,
        pass_on_attempt: usize,
    ) -> Self {
        let scripted = vec![mumbled_zh.into(); pass_on_attempt.saturating_sub(1)];
        Self::with_scripted_mandarin(counters, english, scripted, clean_zh)
    }
}

#[async_trait::async_trait]
impl SpeechRecognizer for FakeSpeechRecognizer {
    async fn transcribe_english(&self, _pcm: &Pcm16kMono) -> Result<String, ExternalPortError> {
        let _flight = self.counters.enter_gpu_call().await;
        self.counters.asr_en.fetch_add(1, Ordering::SeqCst);
        Ok(self.english_transcript.clone())
    }

    async fn transcribe_mandarin(&self, pcm: &Pcm24kMono) -> Result<String, ExternalPortError> {
        self.transcribe_in_language(pcm, TargetLanguage::Mandarin)
            .await
    }

    async fn transcribe_in_language(
        &self,
        _pcm: &Pcm24kMono,
        lang: TargetLanguage,
    ) -> Result<String, ExternalPortError> {
        let _flight = self.counters.enter_gpu_call().await;
        let idx = self.counters.asr_zh.fetch_add(1, Ordering::SeqCst);
        if lang == TargetLanguage::Mandarin {
            return Ok(self
                .scripted_zh
                .get(idx)
                .cloned()
                .unwrap_or_else(|| self.fallback_zh.clone()));
        }

        let sample = sample_for_language(lang);
        if self.scripted_zh.get(idx).is_some() || self.fallback_zh.contains("胡言乱语") {
            return Ok(sample.mumbled_back_transcribed.to_owned());
        }
        Ok(sample.clean_back_transcribed.to_owned())
    }
}

/// Deterministic [`MandarinTranslator`] double validating [`SpokenScript`] across all `12` languages.
#[derive(Debug)]
pub struct FakeMandarinTranslator {
    counters: Arc<FakeCallCounters>,
    spoken_hanzi: String,
    pinyin: String,
    english_back_gloss: String,
}

impl FakeMandarinTranslator {
    #[must_use]
    pub fn new(
        counters: Arc<FakeCallCounters>,
        spoken_hanzi: impl Into<String>,
        pinyin: impl Into<String>,
        english_back_gloss: impl Into<String>,
    ) -> Self {
        Self {
            counters,
            spoken_hanzi: spoken_hanzi.into(),
            pinyin: pinyin.into(),
            english_back_gloss: english_back_gloss.into(),
        }
    }
}

#[async_trait::async_trait]
impl MandarinTranslator for FakeMandarinTranslator {
    async fn translate_to_spoken_language(
        &self,
        _english: &str,
        target_language: TargetLanguage,
    ) -> Result<TranslatedMandarin, ExternalPortError> {
        let _flight = self.counters.enter_gpu_call().await;
        self.counters.translator.fetch_add(1, Ordering::SeqCst);

        if target_language == TargetLanguage::Mandarin {
            let spoken_hanzi =
                SpokenScript::parse_for_language(&self.spoken_hanzi, TargetLanguage::Mandarin)
                    .map_err(|e| ExternalPortError::Translation(e.to_string()))?;
            return Ok(TranslatedMandarin {
                spoken_hanzi,
                pinyin: self.pinyin.clone(),
                english_back_gloss: self.english_back_gloss.clone(),
            });
        }

        let sample = sample_for_language(target_language);
        let spoken_hanzi = SpokenScript::parse_for_language(sample.spoken_text, target_language)
            .map_err(|e| ExternalPortError::Translation(e.to_string()))?;
        Ok(TranslatedMandarin {
            spoken_hanzi,
            pinyin: sample.phonetic_guide.to_owned(),
            english_back_gloss: sample.english_back_gloss.to_owned(),
        })
    }
}

/// Deterministic [`VoiceSynthesizer`] double recording synthesis seeds and returning `24 kHz` `PCM`.
#[derive(Debug)]
pub struct FakeVoiceSynthesizer {
    counters: Arc<FakeCallCounters>,
}

impl FakeVoiceSynthesizer {
    #[must_use]
    pub fn new(counters: Arc<FakeCallCounters>) -> Self {
        Self { counters }
    }
}

#[async_trait::async_trait]
impl VoiceSynthesizer for FakeVoiceSynthesizer {
    async fn synthesize_cloned_voice(
        &self,
        request: VoiceSynthesisRequest<'_>,
    ) -> Result<Pcm24kMono, ExternalPortError> {
        let _flight = self.counters.enter_gpu_call().await;
        self.counters.tts.fetch_add(1, Ordering::SeqCst);
        lock_or_recover(&self.counters.seeds).push(request.seed);
        *lock_or_recover(&self.counters.last_synthesis) = Some(RecordedSynthesisCall {
            profile_slug: request.profile_slug.to_owned(),
            reference_samples: request.reference_samples.to_vec(),
            tuning: request.tuning,
            finetuned_checkpoint_dir: request.finetuned_checkpoint_dir.map(str::to_owned),
        });

        let seed_phase = f32::from(u16::try_from(request.seed % 1_000).unwrap_or(0)) * 0.001_f32;
        let mut samples = Vec::with_capacity(24_000);
        for step in 0..24_000_u16 {
            let t = f32::from(step) / 24_000.0_f32;
            samples.push(0.35_f32 * ((TAU * 440.0_f32 * t) + seed_phase).sin());
        }
        Pcm24kMono::new(samples).map_err(|e| ExternalPortError::Tts(e.to_string()))
    }
}

/// Deterministic [`DiscordPublisher`] double counting non-local delivery publishes.
#[derive(Debug)]
pub struct FakeDiscordPublisher {
    counters: Arc<FakeCallCounters>,
}

impl FakeDiscordPublisher {
    #[must_use]
    pub fn new(counters: Arc<FakeCallCounters>) -> Self {
        Self { counters }
    }
}

#[async_trait::async_trait]
impl DiscordPublisher for FakeDiscordPublisher {
    async fn publish(
        &self,
        target: &DiscordDeliveryTarget,
        _bundle: &RenderedAudioBundle,
        _translated: &TranslatedMandarin,
    ) -> Result<Option<u64>, ExternalPortError> {
        if matches!(target, DiscordDeliveryTarget::ClientReturnOnly) {
            return Ok(None);
        }
        let seq = self.counters.discord.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(Some(
            900_000_u64.saturating_add(u64::try_from(seq).unwrap_or(1)),
        ))
    }
}

/// Deterministic [`SpeakerSimilarityScorer`] double returning a fixed [`BasisPoints`] score.
#[derive(Debug)]
pub struct FakeSpeakerSimilarityScorer {
    counters: Arc<FakeCallCounters>,
    similarity_bps: BasisPoints,
}

impl FakeSpeakerSimilarityScorer {
    #[must_use]
    pub fn new(counters: Arc<FakeCallCounters>, similarity_bps: BasisPoints) -> Self {
        Self {
            counters,
            similarity_bps,
        }
    }
}

#[async_trait::async_trait]
impl SpeakerSimilarityScorer for FakeSpeakerSimilarityScorer {
    async fn score_similarity_bps(
        &self,
        _reference_wav_path: &str,
        _candidate_wav_bytes: &[u8],
    ) -> Result<BasisPoints, ExternalPortError> {
        let _flight = self.counters.enter_gpu_call().await;
        Ok(self.similarity_bps)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use common::basis_points::BasisPoints;
    use domain::{
        audio::types::{DiscordWaveform, RenderedAudioBundle},
        ports::{
            DiscordDeliveryTarget, DiscordPublisher, MandarinTranslator, SpeechRecognizer,
            TargetLanguage, TranslatedMandarin, VoiceSynthesisRequest, VoiceSynthesizer,
        },
        translation::{
            types::{MandarinSpokenScript, TtsEngineKind},
            verifier::evaluate_cer_gate_for_language,
        },
    };

    use super::{
        FakeCallCounters, FakeDiscordPublisher, FakeMandarinTranslator, FakeSpeechRecognizer,
        FakeVoiceSynthesizer,
    };

    #[tokio::test]
    async fn fake_asr_healing_and_tts_seed_tracking_behave_deterministically() {
        let counters = Arc::new(FakeCallCounters::new());
        let asr = FakeSpeechRecognizer::healing_on_attempt(
            Arc::clone(&counters),
            "Hello there.",
            "胡言乱语",
            "你好",
            3,
        );
        let tts = FakeVoiceSynthesizer::new(Arc::clone(&counters));
        let script = MandarinSpokenScript::parse("你好").unwrap();

        let pcm1 = tts
            .synthesize_cloned_zh(
                "default",
                "/tmp/ref.wav",
                "Hi",
                TtsEngineKind::CosyVoice3,
                &script,
                11,
            )
            .await
            .unwrap();
        let pcm2 = tts
            .synthesize_cloned_zh(
                "default",
                "/tmp/ref.wav",
                "Hi",
                TtsEngineKind::CosyVoice3,
                &script,
                22,
            )
            .await
            .unwrap();
        let pcm3 = tts
            .synthesize_cloned_zh(
                "default",
                "/tmp/ref.wav",
                "Hi",
                TtsEngineKind::CosyVoice3,
                &script,
                33,
            )
            .await
            .unwrap();

        assert_eq!(asr.transcribe_mandarin(&pcm1).await.unwrap(), "胡言乱语");
        assert_eq!(asr.transcribe_mandarin(&pcm2).await.unwrap(), "胡言乱语");
        assert_eq!(asr.transcribe_mandarin(&pcm3).await.unwrap(), "你好");
        assert_eq!(counters.tts_calls(), 3);
        assert_eq!(counters.asr_zh_calls(), 3);
        assert_eq!(counters.seeds_seen(), vec![11, 22, 33]);
    }

    #[tokio::test]
    async fn fake_polyglot_translator_and_asr_score_zero_cer_across_all_twelve_languages() {
        let counters = Arc::new(FakeCallCounters::new());
        let translator = FakeMandarinTranslator::new(
            Arc::clone(&counters),
            "今天下午三点半我们开会。",
            "jīn tiān xià wǔ sān diǎn bàn wǒ men kāi huì.",
            "We have a meeting at three thirty this afternoon.",
        );
        let asr = FakeSpeechRecognizer::new(
            Arc::clone(&counters),
            "We have a meeting at 3:30.",
            "今天下午三点半我们开会",
        );
        let tts = FakeVoiceSynthesizer::new(Arc::clone(&counters));

        for lang in TargetLanguage::ALL {
            let translated = translator
                .translate_to_spoken_language("We have a meeting at 3:30.", lang)
                .await
                .unwrap();
            let pcm = tts
                .synthesize_cloned_voice(VoiceSynthesisRequest {
                    profile_slug: "default",
                    reference_wav_path: "/tmp/ref.wav",
                    reference_transcript_en: "Hi",
                    reference_samples: &[],
                    engine: lang.recommended_engine(),
                    target_language: lang,
                    script: &translated.spoken_hanzi,
                    seed: 1,
                    tuning: domain::ports::SynthesisTuning::default(),
                    finetuned_checkpoint_dir: None,
                })
                .await
                .unwrap();
            let back = asr.transcribe_in_language(&pcm, lang).await.unwrap();
            let verdict = evaluate_cer_gate_for_language(
                translated.spoken_text(),
                &back,
                lang,
                BasisPoints::ZERO,
            );
            assert!(
                verdict.passed,
                "expected 0 bps CER for {lang:?}, got {verdict:?}"
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fake_discord_publisher_skips_client_return_only_and_tracks_concurrency() {
        let counters = Arc::new(FakeCallCounters::new());
        let tts = Arc::new(FakeVoiceSynthesizer::new(Arc::clone(&counters)));
        let script = MandarinSpokenScript::parse("你好世界").unwrap();

        let f1 = tts.synthesize_cloned_zh("p", "/ref.wav", "en", TtsEngineKind::XttsV2, &script, 1);
        let f2 = tts.synthesize_cloned_zh("p", "/ref.wav", "en", TtsEngineKind::XttsV2, &script, 2);
        let (r1, r2) = tokio::join!(f1, f2);
        r1.unwrap();
        r2.unwrap();
        assert_eq!(counters.max_concurrent_gpu_calls(), 2);

        let publisher = FakeDiscordPublisher::new(Arc::clone(&counters));
        let bundle = RenderedAudioBundle {
            ogg_opus_bytes: b"OggS".to_vec(),
            wav_48k_bytes: b"RIFF".to_vec(),
            waveform: DiscordWaveform::default(),
            duration_ms: 1_000,
        };
        let translated = TranslatedMandarin {
            spoken_hanzi: script,
            pinyin: "nǐ hǎo shì jiè".to_owned(),
            english_back_gloss: "Hello world".to_owned(),
        };

        assert_eq!(
            publisher
                .publish(
                    &DiscordDeliveryTarget::ClientReturnOnly,
                    &bundle,
                    &translated
                )
                .await
                .unwrap(),
            None
        );
        assert_eq!(counters.discord_publishes(), 0);

        let msg_id = publisher
            .publish(
                &DiscordDeliveryTarget::BotVoiceBubble {
                    channel_id: 99,
                    reply_to_message_id: None,
                },
                &bundle,
                &translated,
            )
            .await
            .unwrap();
        assert_eq!(msg_id, Some(900_001));
        assert_eq!(counters.discord_publishes(), 1);
    }
}
