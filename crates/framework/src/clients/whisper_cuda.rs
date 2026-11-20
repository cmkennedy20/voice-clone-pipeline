//! Single-load `whisper-rs` `CUDA`/`CPU` adapter supporting eager or lazy model hydration.

use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use domain::{
    audio::types::{Pcm16kMono, Pcm24kMono},
    ports::{ExternalPortError, SpeechRecognizer, TargetLanguage},
};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::dsp::decode_and_vad::resample_mono;

const MANDARIN_SPOKEN_PROMPT: &str = "以下是普通话口语。";

/// Shared `whisper-rs` (`ggml-*`) recognizer executing passes on Tokio's blocking pool while
/// keeping weights resident once (and allowing standby startup before weights finish downloading).
#[derive(Clone)]
pub struct WhisperCudaRecognizer {
    model_path: PathBuf,
    prefer_gpu: bool,
    ctx_slot: Arc<Mutex<Option<Arc<WhisperContext>>>>,
}

impl fmt::Debug for WhisperCudaRecognizer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WhisperCudaRecognizer")
            .field("model_path", &self.model_path)
            .field("prefer_gpu", &self.prefer_gpu)
            .finish_non_exhaustive()
    }
}

fn open_whisper_context(
    path: &Path,
    prefer_gpu: bool,
) -> Result<Arc<WhisperContext>, ExternalPortError> {
    whisper_rs::install_logging_hooks();

    let mut ctx_params = WhisperContextParameters::default();
    ctx_params.use_gpu(prefer_gpu);

    match WhisperContext::new_with_params(path, ctx_params) {
        Ok(ctx) => Ok(Arc::new(ctx)),
        Err(gpu_err) if prefer_gpu && path.is_file() => {
            tracing::warn!(
                model_path = %path.display(),
                error = %gpu_err,
                "Whisper GPU initialization failed; falling back to CPU execution"
            );
            let mut cpu_params = WhisperContextParameters::default();
            cpu_params.use_gpu(false);
            let ctx = WhisperContext::new_with_params(path, cpu_params).map_err(|err| {
                ExternalPortError::Asr(format!(
                    "failed loading Whisper model '{}' on CPU fallback: {err}",
                    path.display()
                ))
            })?;
            Ok(Arc::new(ctx))
        }
        Err(err) => Err(ExternalPortError::Asr(format!(
            "failed loading Whisper model '{}': {err}",
            path.display()
        ))),
    }
}

impl WhisperCudaRecognizer {
    /// Strictly loads a `GGML` `Whisper` model from `model_path` immediately.
    pub fn load<P: AsRef<Path>>(model_path: P) -> Result<Self, ExternalPortError> {
        let path = model_path.as_ref();
        if path.as_os_str().is_empty() {
            return Err(ExternalPortError::Asr(
                "Whisper model path must not be empty".to_owned(),
            ));
        }
        let ctx = open_whisper_context(path, true)?;
        Ok(Self {
            model_path: path.to_path_buf(),
            prefer_gpu: true,
            ctx_slot: Arc::new(Mutex::new(Some(ctx))),
        })
    }

    /// Loads `model_path` immediately if present on disk, otherwise stands by and loads lazily on first call.
    pub fn load_or_standby<P: AsRef<Path>>(
        model_path: P,
        prefer_gpu: bool,
    ) -> Result<Self, ExternalPortError> {
        let path = model_path.as_ref();
        if path.as_os_str().is_empty() {
            return Err(ExternalPortError::Asr(
                "Whisper model path must not be empty".to_owned(),
            ));
        }
        let initial = if path.is_file() {
            Some(open_whisper_context(path, prefer_gpu)?)
        } else {
            tracing::info!(
                model_path = %path.display(),
                "Whisper weights not yet on disk; entering lazy standby until downloaded"
            );
            None
        };
        Ok(Self {
            model_path: path.to_path_buf(),
            prefer_gpu,
            ctx_slot: Arc::new(Mutex::new(initial)),
        })
    }

    fn resolve_or_load_context(&self) -> Result<Arc<WhisperContext>, ExternalPortError> {
        let mut guard = match self.ctx_slot.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(existing) = guard.as_ref() {
            return Ok(Arc::clone(existing));
        }
        let loaded = open_whisper_context(&self.model_path, self.prefer_gpu)?;
        *guard = Some(Arc::clone(&loaded));
        Ok(loaded)
    }

    async fn run_whisper_pass(
        &self,
        samples_16k: Vec<f32>,
        language: &'static str,
        initial_prompt: Option<&'static str>,
    ) -> Result<String, ExternalPortError> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let ctx = this.resolve_or_load_context()?;
            let mut state = ctx.create_state().map_err(|err| {
                ExternalPortError::Asr(format!("failed creating Whisper state: {err}"))
            })?;

            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            params.set_language(Some(language));
            params.set_translate(false);
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            if let Some(prompt) = initial_prompt {
                params.set_initial_prompt(prompt);
            }

            state.full(params, &samples_16k).map_err(|err| {
                ExternalPortError::Asr(format!("Whisper inference failed ({language}): {err}"))
            })?;

            let mut segments = Vec::new();
            for segment in state.as_iter() {
                let text = segment.to_str_lossy().map_err(|err| {
                    ExternalPortError::Asr(format!("failed decoding Whisper segment UTF-8: {err}"))
                })?;
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    segments.push(trimmed.to_owned());
                }
            }

            let joiner = if matches!(language, "zh" | "ja") {
                ""
            } else {
                " "
            };
            Ok(segments.join(joiner).trim().to_owned())
        })
        .await
        .map_err(|join_err| {
            ExternalPortError::Asr(format!("Whisper blocking task aborted: {join_err}"))
        })?
    }
}

#[async_trait::async_trait]
impl SpeechRecognizer for WhisperCudaRecognizer {
    async fn transcribe_english(&self, pcm: &Pcm16kMono) -> Result<String, ExternalPortError> {
        self.run_whisper_pass(pcm.samples().to_vec(), "en", None)
            .await
    }

    async fn transcribe_mandarin(&self, pcm: &Pcm24kMono) -> Result<String, ExternalPortError> {
        self.transcribe_in_language(pcm, TargetLanguage::Mandarin)
            .await
    }

    async fn transcribe_in_language(
        &self,
        pcm: &Pcm24kMono,
        lang: TargetLanguage,
    ) -> Result<String, ExternalPortError> {
        let resampled_16k = resample_mono(pcm.samples(), 24_000, 16_000)?;
        let prompt = (lang == TargetLanguage::Mandarin).then_some(MANDARIN_SPOKEN_PROMPT);
        self.run_whisper_pass(resampled_16k, lang.whisper_code(), prompt)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::WhisperCudaRecognizer;

    #[test]
    fn load_rejects_empty_or_missing_model_paths_and_standby_accepts_pending_download() {
        let empty_err = WhisperCudaRecognizer::load("").unwrap_err();
        assert!(empty_err.to_string().contains("empty"));

        let missing_err =
            WhisperCudaRecognizer::load("/nonexistent/whisper/ggml-missing.bin").unwrap_err();
        assert!(
            missing_err
                .to_string()
                .contains("failed loading Whisper model")
        );

        let standby = WhisperCudaRecognizer::load_or_standby(
            "/nonexistent/whisper/ggml-pending-download.bin",
            false,
        );
        assert!(standby.is_ok());
    }
}
