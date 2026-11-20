//! `HTTP` client for the local `PyTorch` `TTS` voice-cloning & speaker-similarity sidecar.

use std::{io::Cursor, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use common::basis_points::BasisPoints;
use domain::{
    audio::types::Pcm24kMono,
    ports::{ExternalPortError, SynthesisTuning, VoiceSynthesisRequest, VoiceSynthesizer},
    translation::types::TtsEngineKind,
};
use hound::{SampleFormat, WavReader};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::{context::SpeakerSimilarityScorer, dsp::decode_and_vad::resample_mono};

pub mod finetune_client;

pub(super) const MAX_ATTEMPTS: usize = 3;
pub(super) const RETRY_BACKOFF_MS: [u64; MAX_ATTEMPTS] = [50, 100, 0];

#[derive(Debug, Serialize)]
struct SynthesizePayload<'a> {
    profile_slug: &'a str,
    reference_wav_path: &'a str,
    reference_transcript_en: &'a str,
    reference_wav_paths: Vec<&'a str>,
    reference_transcripts: Vec<&'a str>,
    finetuned_checkpoint_dir: Option<&'a str>,
    tuning: SynthesisTuning,
    engine: TtsEngineKind,
    target_language: &'a str,
    spoken_text: &'a str,
    spoken_hanzi: &'a str,
    seed: u64,
}

#[derive(Debug, Serialize)]
struct SpeakerSimilarityPayload<'a> {
    reference_wav_path: &'a str,
    candidate_wav_base64: &'a str,
}

#[derive(Debug, Deserialize)]
struct SpeakerSimilarityResponse {
    similarity_bps: u32,
}

pub(super) fn is_transient_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT
    )
}

fn i32_sample_to_unit_f32(sample: i32, bits_per_sample: u16) -> f32 {
    if bits_per_sample <= 24 {
        let hi16 = i16::try_from(sample >> 8).unwrap_or(0);
        let lo8 = u8::try_from(sample & 0xFF).unwrap_or(0);
        ((f32::from(hi16) + (f32::from(lo8) / 256.0_f32)) / 32_768.0_f32).clamp(-1.0_f32, 1.0_f32)
    } else {
        let hi16 = i16::try_from(sample >> 16).unwrap_or(0);
        let lo16 = u16::try_from(sample & 0xFFFF).unwrap_or(0);
        (((f32::from(hi16) * 65_536.0_f32) + f32::from(lo16)) / 2_147_483_648.0_f32)
            .clamp(-1.0_f32, 1.0_f32)
    }
}

fn decode_wav_to_24k_mono(wav_bytes: &[u8]) -> Result<Pcm24kMono, ExternalPortError> {
    let mut reader = WavReader::new(Cursor::new(wav_bytes))
        .map_err(|err| ExternalPortError::Tts(format!("invalid sidecar WAV header: {err}")))?;
    let spec = reader.spec();
    if spec.channels == 0 {
        return Err(ExternalPortError::Tts(
            "sidecar WAV reported zero channels".to_owned(),
        ));
    }

    let interleaved: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Float, 32) => reader
            .samples::<f32>()
            .map(|res| {
                res.map(|s| s.clamp(-1.0_f32, 1.0_f32))
                    .map_err(|err| ExternalPortError::Tts(format!("corrupt float32 WAV: {err}")))
            })
            .collect::<Result<Vec<f32>, _>>()?,
        (SampleFormat::Int, 1..=8) => reader
            .samples::<i8>()
            .map(|res| {
                res.map(|s| (f32::from(s) / 128.0_f32).clamp(-1.0_f32, 1.0_f32))
                    .map_err(|err| ExternalPortError::Tts(format!("corrupt int8 WAV: {err}")))
            })
            .collect::<Result<Vec<f32>, _>>()?,
        (SampleFormat::Int, 9..=16) => reader
            .samples::<i16>()
            .map(|res| {
                res.map(|s| (f32::from(s) / 32_768.0_f32).clamp(-1.0_f32, 1.0_f32))
                    .map_err(|err| ExternalPortError::Tts(format!("corrupt int16 WAV: {err}")))
            })
            .collect::<Result<Vec<f32>, _>>()?,
        (SampleFormat::Int, bits @ 17..=32) => reader
            .samples::<i32>()
            .map(|res| {
                res.map(|s| i32_sample_to_unit_f32(s, bits))
                    .map_err(|err| ExternalPortError::Tts(format!("corrupt int32 WAV: {err}")))
            })
            .collect::<Result<Vec<f32>, _>>()?,
        (fmt, bits) => {
            return Err(ExternalPortError::Tts(format!(
                "unsupported sidecar WAV format {fmt:?} ({bits} bits)"
            )));
        }
    };

    if interleaved.is_empty() {
        return Err(ExternalPortError::Tts(
            "sidecar WAV contained zero audio samples".to_owned(),
        ));
    }

    let mono: Vec<f32> = if spec.channels == 1 {
        interleaved
    } else {
        let ch_usize = usize::from(spec.channels);
        let ch_scale = f32::from(spec.channels);
        interleaved
            .chunks_exact(ch_usize)
            .map(|frame| frame.iter().copied().sum::<f32>() / ch_scale)
            .collect()
    };

    let samples_24k = if spec.sample_rate == Pcm24kMono::SAMPLE_RATE_HZ {
        mono
    } else {
        resample_mono(&mono, spec.sample_rate, Pcm24kMono::SAMPLE_RATE_HZ)
            .map_err(|err| ExternalPortError::Tts(format!("WAV resample to 24kHz failed: {err}")))?
    };

    Pcm24kMono::new(samples_24k)
        .map_err(|err| ExternalPortError::Tts(format!("invalid synthesized 24kHz PCM: {err}")))
}

/// `HTTP` adapter to the `tts-clone-sidecar` (`CosyVoice` / `XTTS-v2` + `ECAPA-TDNN` scorer).
#[derive(Debug, Clone)]
pub struct HttpTtsSidecarClient {
    pub(super) base_url: String,
    pub(super) http: reqwest::Client,
}

impl HttpTtsSidecarClient {
    /// Creates a `TTS` sidecar client targeting `base_url` (e.g. `"http://tts-clone-sidecar:8082"`).
    #[must_use]
    pub fn new(mut base_url: String) -> Self {
        while base_url.ends_with('/') {
            base_url.pop();
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { base_url, http }
    }

    /// Probes `GET {base_url}/health` and returns the sidecar's diagnostic `JSON` status payload.
    pub async fn check_health(&self) -> Result<serde_json::Value, ExternalPortError> {
        let url = format!("{}/health", self.base_url);
        let resp =
            self.http.get(&url).send().await.map_err(|err| {
                ExternalPortError::Tts(format!("TTS health request failed: {err}"))
            })?;
        if !resp.status().is_success() {
            return Err(ExternalPortError::Tts(format!(
                "TTS sidecar unhealthy: HTTP {}",
                resp.status()
            )));
        }
        resp.json::<serde_json::Value>()
            .await
            .map_err(|err| ExternalPortError::Tts(format!("invalid TTS health JSON: {err}")))
    }
}

#[async_trait::async_trait]
impl VoiceSynthesizer for HttpTtsSidecarClient {
    async fn synthesize_cloned_voice(
        &self,
        request: VoiceSynthesisRequest<'_>,
    ) -> Result<Pcm24kMono, ExternalPortError> {
        let url = format!("{}/synthesize", self.base_url);
        let text = request.script.as_str();
        let (reference_wav_paths, reference_transcripts) = if request.reference_samples.is_empty() {
            (
                vec![request.reference_wav_path],
                vec![request.reference_transcript_en],
            )
        } else {
            (
                request
                    .reference_samples
                    .iter()
                    .map(|s| s.wav_path.as_str())
                    .collect(),
                request
                    .reference_samples
                    .iter()
                    .map(|s| s.transcript_en.as_str())
                    .collect(),
            )
        };
        let payload = SynthesizePayload {
            profile_slug: request.profile_slug,
            reference_wav_path: request.reference_wav_path,
            reference_transcript_en: request.reference_transcript_en,
            reference_wav_paths,
            reference_transcripts,
            finetuned_checkpoint_dir: request.finetuned_checkpoint_dir,
            tuning: request.tuning,
            engine: request.engine,
            target_language: request.target_language.code(),
            spoken_text: text,
            spoken_hanzi: text,
            seed: request.seed,
        };

        let mut last_err = String::from("TTS synthesis exhausted retries");

        for (attempt, &backoff_ms) in RETRY_BACKOFF_MS.iter().enumerate() {
            let send_res = self.http.post(&url).json(&payload).send().await;
            match send_res {
                Ok(resp) if resp.status().is_success() => {
                    let wav_bytes = resp.bytes().await.map_err(|err| {
                        ExternalPortError::Tts(format!("failed reading sidecar WAV body: {err}"))
                    })?;
                    return decode_wav_to_24k_mono(&wav_bytes);
                }
                Ok(resp) if is_transient_status(resp.status()) && attempt + 1 < MAX_ATTEMPTS => {
                    last_err = format!("transient HTTP {} from TTS sidecar", resp.status());
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(ExternalPortError::Tts(format!(
                        "TTS sidecar HTTP {status}: {body}"
                    )));
                }
                Err(err)
                    if (err.is_timeout() || err.is_connect()) && attempt + 1 < MAX_ATTEMPTS =>
                {
                    last_err = format!("transient TTS sidecar transport error: {err}");
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Err(err) => {
                    return Err(ExternalPortError::Tts(format!(
                        "TTS sidecar request error: {err}"
                    )));
                }
            }
        }

        Err(ExternalPortError::Tts(last_err))
    }
}

#[async_trait::async_trait]
impl SpeakerSimilarityScorer for HttpTtsSidecarClient {
    async fn score_similarity_bps(
        &self,
        reference_wav_path: &str,
        candidate_wav_bytes: &[u8],
    ) -> Result<BasisPoints, ExternalPortError> {
        if candidate_wav_bytes.is_empty() {
            return Err(ExternalPortError::Tts(
                "candidate WAV bytes must not be empty".to_owned(),
            ));
        }

        let url = format!("{}/speaker-similarity", self.base_url);
        let encoded = STANDARD.encode(candidate_wav_bytes);
        let payload = SpeakerSimilarityPayload {
            reference_wav_path,
            candidate_wav_base64: &encoded,
        };

        let mut last_err = String::from("speaker-similarity request exhausted retries");

        for (attempt, &backoff_ms) in RETRY_BACKOFF_MS.iter().enumerate() {
            let send_res = self.http.post(&url).json(&payload).send().await;
            match send_res {
                Ok(resp) if resp.status().is_success() => {
                    let body: SpeakerSimilarityResponse = resp.json().await.map_err(|err| {
                        ExternalPortError::Tts(format!(
                            "invalid speaker-similarity JSON response: {err}"
                        ))
                    })?;
                    return Ok(BasisPoints::new_clamped(body.similarity_bps));
                }
                Ok(resp) if is_transient_status(resp.status()) && attempt + 1 < MAX_ATTEMPTS => {
                    last_err = format!("transient HTTP {} from speaker-similarity", resp.status());
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return Err(ExternalPortError::Tts(format!(
                        "speaker-similarity HTTP {status}: {text}"
                    )));
                }
                Err(err)
                    if (err.is_timeout() || err.is_connect()) && attempt + 1 < MAX_ATTEMPTS =>
                {
                    last_err = format!("transient speaker-similarity transport error: {err}");
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Err(err) => {
                    return Err(ExternalPortError::Tts(format!(
                        "speaker-similarity request error: {err}"
                    )));
                }
            }
        }

        Err(ExternalPortError::Tts(last_err))
    }
}
