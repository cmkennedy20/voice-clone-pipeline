//! Cached `Silero VAD v5` `ONNX` (`ort`) session loader, `512`-sample (`16 kHz`) inference,
//! and deterministic acoustic `RMS` frame fallback.

use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

use domain::audio::types::Pcm16kMono;
use ort::{session::Session, value::Tensor};

/// Standard `Silero VAD v5` window size at `16 kHz` (`512` samples = `32 ms`).
const SILERO_FRAME_SAMPLES_16K: usize = 512;

/// Duration in milliseconds of a full `512`-sample `Silero VAD v5` frame at `16 kHz`.
const SILERO_FRAME_MS: u32 = 32;

/// Recurrent state element count (`[2, 1, 128]` = `256` `f32` values).
const SILERO_STATE_LEN: usize = 2 * 128;

/// Speech probability threshold separating voiced frames from silence/noise.
const SILERO_SPEECH_PROB_THRESHOLD: f32 = 0.5;

/// Acoustic fallback window (`320` samples = `20 ms` at `16 kHz`) and `RMS` gate (`0.015`).
const ACOUSTIC_VAD_FRAME_SAMPLES_16K: usize = 320;
const ACOUSTIC_VAD_FRAME_MS: u32 = 20;
const ACOUSTIC_VAD_RMS_THRESHOLD: f64 = 0.015;

fn lock_session(mutex: &Mutex<Session>) -> MutexGuard<'_, Session> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn sanitize_sample(sample: f32) -> f32 {
    if sample.is_finite() { sample } else { 0.0 }
}

fn samples_to_ms(sample_count: usize, sample_rate_hz: u32) -> u32 {
    let count = u128::try_from(sample_count).unwrap_or(u128::MAX);
    u32::try_from((count * 1_000) / u128::from(sample_rate_hz.max(1))).unwrap_or(u32::MAX)
}

fn frame_rms(frame: &[f32]) -> f64 {
    let sum_sq: f64 = frame
        .iter()
        .map(|&s| f64::from(sanitize_sample(s)).powi(2))
        .sum();
    (sum_sq / u32::try_from(frame.len()).map_or(1.0_f64, f64::from)).sqrt()
}

/// Loads a `Silero VAD` `ONNX` [`Session`] once from disk when `path` exists and is a regular file.
pub(crate) fn load_silero_vad_session(path: &Path) -> Option<Arc<Mutex<Session>>> {
    if !path.is_file() {
        return None;
    }
    let session = ort::init()
        .build()
        .and_then(|env| Session::builder(&env)?.commit_from_file(path));
    match session {
        Ok(loaded) => {
            tracing::info!(path = %path.display(), "loaded cached Silero VAD ONNX session");
            Some(Arc::new(Mutex::new(loaded)))
        }
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "failed loading Silero VAD ONNX session; falling back to acoustic RMS VAD"
            );
            None
        }
    }
}

fn try_silero_vad_16k(
    samples_16k: &[f32],
    session_mutex: &Mutex<Session>,
) -> Option<(Vec<f32>, u32)> {
    if samples_16k.is_empty() {
        return Some((Vec::new(), 0));
    }

    let mut session = lock_session(session_mutex);
    let sr_is_scalar = session
        .inputs()
        .iter()
        .find(|inp| inp.name() == "sr")
        .and_then(|inp| inp.dtype().tensor_shape())
        .is_some_and(|shape| shape.is_empty());

    let mut state = vec![0.0_f32; SILERO_STATE_LEN];
    let mut voiced_ms: u32 = 0;
    let mut bounds: Option<(usize, usize)> = None;

    for (idx, chunk) in samples_16k.chunks(SILERO_FRAME_SAMPLES_16K).enumerate() {
        let mut frame = vec![0.0_f32; SILERO_FRAME_SAMPLES_16K];
        for (dst, &src) in frame.iter_mut().zip(chunk.iter()) {
            *dst = sanitize_sample(src);
        }

        let input_tensor =
            Tensor::<f32>::from_array(([1_usize, SILERO_FRAME_SAMPLES_16K], frame)).ok()?;
        let state_tensor = Tensor::<f32>::from_array(([2_usize, 1, 128], state.clone())).ok()?;
        let sr_tensor = if sr_is_scalar {
            Tensor::<i64>::from_array(([0_usize; 0], vec![16_000_i64])).ok()?
        } else {
            Tensor::<i64>::from_array(([1_usize], vec![16_000_i64])).ok()?
        };

        let outputs = session
            .run(ort::inputs![
                "input" => input_tensor,
                "state" => state_tensor,
                "sr" => sr_tensor,
            ])
            .ok()?;

        let out_val = outputs
            .get("output")
            .or_else(|| outputs.keys().next().and_then(|k| outputs.get(k)))?;
        let (_, probs) = out_val.try_extract_tensor::<f32>().ok()?;
        let speech_prob = probs.first().copied()?;

        if let Some(state_val) = outputs
            .get("stateN")
            .or_else(|| outputs.keys().nth(1).and_then(|k| outputs.get(k)))
            && let Ok((_, next_state)) = state_val.try_extract_tensor::<f32>()
            && next_state.len() == state.len()
        {
            state.copy_from_slice(next_state);
        }

        if speech_prob.is_finite() && speech_prob >= SILERO_SPEECH_PROB_THRESHOLD {
            let ms = if chunk.len() == SILERO_FRAME_SAMPLES_16K {
                SILERO_FRAME_MS
            } else {
                samples_to_ms(chunk.len(), Pcm16kMono::SAMPLE_RATE_HZ)
            };
            voiced_ms = voiced_ms.saturating_add(ms);
            bounds = Some((bounds.map_or(idx, |(first, _)| first), idx));
        }
    }

    let Some((first, last)) = bounds else {
        return Some((samples_16k[..samples_16k.len().min(1)].to_vec(), 0));
    };
    let start = first.saturating_mul(SILERO_FRAME_SAMPLES_16K);
    let end = (last + 1)
        .saturating_mul(SILERO_FRAME_SAMPLES_16K)
        .min(samples_16k.len());
    Some((samples_16k[start..end].to_vec(), voiced_ms))
}

/// Evaluates `16 kHz` speech activity via cached `Silero VAD v5` `ONNX` when available, falling
/// back gracefully to deterministic `0.015` `RMS` frame gating.
pub(crate) fn trim_and_count_voiced_16k(
    samples_16k: &[f32],
    vad_session: Option<&Mutex<Session>>,
) -> (Vec<f32>, u32) {
    if let Some(session_mutex) = vad_session {
        if let Some(silero_out) = try_silero_vad_16k(samples_16k, session_mutex) {
            return silero_out;
        }
        tracing::debug!("Silero VAD ONNX schema differed; falling back to acoustic RMS VAD");
    }

    let mut voiced_ms: u32 = 0;
    let mut bounds: Option<(usize, usize)> = None;

    for (idx, frame) in samples_16k
        .chunks(ACOUSTIC_VAD_FRAME_SAMPLES_16K)
        .enumerate()
    {
        if frame_rms(frame) >= ACOUSTIC_VAD_RMS_THRESHOLD {
            let ms = if frame.len() == ACOUSTIC_VAD_FRAME_SAMPLES_16K {
                ACOUSTIC_VAD_FRAME_MS
            } else {
                samples_to_ms(frame.len(), Pcm16kMono::SAMPLE_RATE_HZ)
            };
            voiced_ms = voiced_ms.saturating_add(ms);
            bounds = Some((bounds.map_or(idx, |(first, _)| first), idx));
        }
    }

    let Some((first, last)) = bounds else {
        return (samples_16k[..samples_16k.len().min(1)].to_vec(), 0);
    };
    let start = first.saturating_mul(ACOUSTIC_VAD_FRAME_SAMPLES_16K);
    let end = (last + 1)
        .saturating_mul(ACOUSTIC_VAD_FRAME_SAMPLES_16K)
        .min(samples_16k.len());
    (samples_16k[start..end].to_vec(), voiced_ms)
}
