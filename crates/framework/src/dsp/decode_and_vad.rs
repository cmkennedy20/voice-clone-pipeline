//! Container demuxing (`Symphonia` + `Opus`), `Rubato` resampling, and `Gate 0` VAD metrics.

use std::{
    io::{Cursor, ErrorKind},
    sync::Mutex,
};

use common::basis_points::BasisPoints;
use domain::{
    audio::types::{Pcm16kMono, RawAudioMetrics},
    ports::ExternalPortError,
};
use rubato::{FftFixedIn, Resampler};
use symphonia::core::{
    audio::{Channels, SampleBuffer},
    codecs::{CODEC_TYPE_OPUS, CodecParameters, DecoderOptions},
    errors::Error as SymphoniaError,
    formats::{FormatOptions, FormatReader, Packet},
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe::Hint,
};

use crate::dsp::silero_vad::trim_and_count_voiced_16k;

const CLIPPING_ABS_THRESHOLD: f32 = 0.999;
const MAX_OPUS_PACKET_SAMPLES_PER_CH: usize = 5_760;

fn codec_err(context: &str, err: impl std::fmt::Display) -> ExternalPortError {
    ExternalPortError::AudioCodec(format!("{context}: {err}"))
}

fn sanitize_f32(sample: f32) -> f32 {
    if sample.is_finite() { sample } else { 0.0 }
}

fn samples_to_ms(sample_count: usize, sample_rate_hz: u32) -> u32 {
    let count = u128::try_from(sample_count).unwrap_or(u128::MAX);
    u32::try_from((count * 1_000) / u128::from(sample_rate_hz.max(1))).unwrap_or(u32::MAX)
}

fn unit_peak_to_bps(peak: f32) -> BasisPoints {
    if !peak.is_finite() || peak <= 0.0 {
        return BasisPoints::ZERO;
    }
    let scaled = (f64::from(peak) * 10_000.0).min(10_000.0);
    let (mut low, mut high) = (0_u32, 10_000_u32);
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        if f64::from(mid) - 0.5 <= scaled {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    BasisPoints::new_clamped(low)
}

fn next_track_packet(
    format: &mut dyn FormatReader,
    track_id: u32,
) -> Result<Option<Packet>, ExternalPortError> {
    loop {
        match format.next_packet() {
            Ok(pkt) if pkt.track_id() == track_id => return Ok(Some(pkt)),
            Ok(_) => {}
            Err(SymphoniaError::IoError(io_err)) if io_err.kind() == ErrorKind::UnexpectedEof => {
                return Ok(None);
            }
            Err(err) => return Err(codec_err("packet read failed", err)),
        }
    }
}

fn mix_interleaved_to_mono(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    let ch_count = channels.max(1);
    if ch_count == 1 {
        out.extend(interleaved.iter().copied().map(sanitize_f32));
        return;
    }
    let inv_ch = 1.0_f32 / f32::from(u16::try_from(ch_count).unwrap_or(1));
    for frame in interleaved.chunks_exact(ch_count) {
        let sum: f32 = frame.iter().copied().map(sanitize_f32).sum();
        out.push((sum * inv_ch).clamp(-1.0, 1.0));
    }
}

fn decode_opus_track(
    format: &mut dyn FormatReader,
    track_id: u32,
    params: &CodecParameters,
) -> Result<(Vec<f32>, u32), ExternalPortError> {
    let rate_hz = params.sample_rate.unwrap_or(48_000);
    let ch_count = params.channels.map_or(1, Channels::count).clamp(1, 2);
    let ch_mode = if ch_count == 2 {
        opus::Channels::Stereo
    } else {
        opus::Channels::Mono
    };
    let mut decoder = opus::Decoder::new(rate_hz, ch_mode)
        .map_err(|err| codec_err("opus decoder init failed", err))?;
    let mut decode_buf = vec![0.0_f32; MAX_OPUS_PACKET_SAMPLES_PER_CH * ch_count];
    let mut mono = Vec::new();

    while let Some(pkt) = next_track_packet(format, track_id)? {
        let frames = decoder
            .decode_float(pkt.buf(), &mut decode_buf, false)
            .map_err(|err| codec_err("opus packet decode failed", err))?;
        mix_interleaved_to_mono(
            &decode_buf[..frames.saturating_mul(ch_count)],
            ch_count,
            &mut mono,
        );
    }

    let pre_skip = usize::try_from(params.delay.unwrap_or(0)).unwrap_or(0);
    if pre_skip > 0 && mono.len() > pre_skip {
        mono.drain(..pre_skip);
    }
    if let Some(n) = params.n_frames.and_then(|v| usize::try_from(v).ok())
        && n > 0
        && n < mono.len()
    {
        mono.truncate(n);
    }
    Ok((mono, rate_hz))
}

fn decode_symphonia_track(
    format: &mut dyn FormatReader,
    track_id: u32,
    params: &CodecParameters,
) -> Result<(Vec<f32>, u32), ExternalPortError> {
    let rate_hz = params.sample_rate.ok_or_else(|| {
        ExternalPortError::AudioCodec("audio track missing sample rate".to_owned())
    })?;
    let mut decoder = symphonia::default::get_codecs()
        .make(params, &DecoderOptions::default())
        .map_err(|err| codec_err("unsupported audio codec", err))?;
    let mut mono = Vec::new();

    while let Some(pkt) = next_track_packet(format, track_id)? {
        let audio_buf = match decoder.decode(&pkt) {
            Ok(buf) => buf,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(err) => return Err(codec_err("audio packet decode failed", err)),
        };
        let spec = *audio_buf.spec();
        let mut sample_buf =
            SampleBuffer::<f32>::new(u64::try_from(audio_buf.capacity()).unwrap_or(0), spec);
        sample_buf.copy_interleaved_ref(audio_buf);
        mix_interleaved_to_mono(sample_buf.samples(), spec.channels.count(), &mut mono);
    }
    Ok((mono, rate_hz))
}

fn measure_raw_signal(
    mono_samples: &[f32],
    sample_rate_hz: u32,
) -> (u32, BasisPoints, BasisPoints) {
    let total_duration_ms = samples_to_ms(mono_samples.len(), sample_rate_hz);
    let mut max_abs = 0.0_f32;
    let mut clipped_count: u64 = 0;
    for &sample in mono_samples {
        let mag = sanitize_f32(sample).abs();
        max_abs = max_abs.max(mag);
        if mag >= CLIPPING_ABS_THRESHOLD {
            clipped_count = clipped_count.saturating_add(1);
        }
    }
    let denom = u64::try_from(mono_samples.len()).unwrap_or(u64::MAX);
    let clipped_bps = BasisPoints::from_ratio_round_half_up(clipped_count, denom);
    (total_duration_ms, clipped_bps, unit_peak_to_bps(max_abs))
}

fn push_resampled_chunk(
    resampler: &mut FftFixedIn<f32>,
    chunk: &[f32],
    chunk_size: usize,
    collected: &mut Vec<f32>,
) -> Result<(), ExternalPortError> {
    let mut padded;
    let input: &[f32] = if chunk.len() == chunk_size {
        chunk
    } else {
        padded = vec![0.0_f32; chunk_size];
        padded[..chunk.len()].copy_from_slice(chunk);
        &padded
    };
    let out = resampler
        .process(&[input], None)
        .map_err(|err| codec_err("resample chunk failed", err))?;
    if let Some(mono) = out.first() {
        collected.extend_from_slice(mono);
    }
    Ok(())
}

/// Resamples mono `f32` PCM between arbitrary positive sample rates via `Rubato` `FftFixedIn`.
pub fn resample_mono(
    samples: &[f32],
    from_rate_hz: u32,
    to_rate_hz: u32,
) -> Result<Vec<f32>, ExternalPortError> {
    if from_rate_hz == 0 || to_rate_hz == 0 {
        return Err(ExternalPortError::AudioCodec(
            "resample rate must be greater than zero".to_owned(),
        ));
    }
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    let sanitized: Vec<f32> = samples.iter().copied().map(sanitize_f32).collect();
    if from_rate_hz == to_rate_hz {
        return Ok(sanitized);
    }

    let (from_u, to_u) = (
        usize::try_from(from_rate_hz).unwrap_or(16_000),
        usize::try_from(to_rate_hz).unwrap_or(16_000),
    );
    let num = u128::try_from(sanitized.len()).unwrap_or(0) * u128::from(to_rate_hz)
        + (u128::from(from_rate_hz) / 2);
    let target_len = usize::try_from(num / u128::from(from_rate_hz))
        .unwrap_or(sanitized.len())
        .max(1);

    let mut resampler = FftFixedIn::<f32>::new(from_u, to_u, 1_024, 2, 1)
        .map_err(|err| codec_err("resampler init failed", err))?;
    let (delay, chunk_size) = (resampler.output_delay(), resampler.input_frames_next());
    let mut collected = Vec::with_capacity(delay.saturating_add(target_len).saturating_add(1_024));

    for chunk in sanitized.chunks(chunk_size) {
        push_resampled_chunk(&mut resampler, chunk, chunk_size, &mut collected)?;
    }
    let zero_chunk = vec![0.0_f32; chunk_size];
    for _ in 0..64 {
        if collected.len() >= delay.saturating_add(target_len) {
            break;
        }
        push_resampled_chunk(&mut resampler, &zero_chunk, chunk_size, &mut collected)?;
    }

    let start = delay.min(collected.len());
    let mut output: Vec<f32> = collected[start..]
        .iter()
        .copied()
        .map(sanitize_f32)
        .collect();
    output.resize(target_len, 0.0_f32);
    Ok(output)
}

/// Demuxes container bytes, computes `Gate 0` metrics, resamples to `16 kHz`, and trims dead air.
pub fn decode_and_measure_audio(
    raw_bytes: &[u8],
    vad_session: Option<&Mutex<ort::session::Session>>,
) -> Result<(Pcm16kMono, RawAudioMetrics), ExternalPortError> {
    if raw_bytes.is_empty() {
        return Err(ExternalPortError::AudioCodec(
            "empty audio payload".to_owned(),
        ));
    }
    let cursor = Box::new(Cursor::new(raw_bytes.to_vec()));
    let mss = MediaSourceStream::new(cursor, MediaSourceStreamOptions::default());
    let (fmt_opts, meta_opts) = (FormatOptions::default(), MetadataOptions::default());
    let mut format = symphonia::default::get_probe()
        .format(&Hint::new(), mss, &fmt_opts, &meta_opts)
        .map_err(|err| codec_err("container probe failed", err))?
        .format;

    let track = format
        .default_track()
        .or_else(|| format.tracks().first())
        .ok_or_else(|| ExternalPortError::AudioCodec("no audio track in container".to_owned()))?;
    let (track_id, params) = (track.id, track.codec_params.clone());

    let (mono_samples, rate_hz) = if params.codec == CODEC_TYPE_OPUS {
        decode_opus_track(format.as_mut(), track_id, &params)?
    } else {
        decode_symphonia_track(format.as_mut(), track_id, &params)?
    };
    if mono_samples.is_empty() {
        return Err(ExternalPortError::AudioCodec(
            "zero decoded samples".to_owned(),
        ));
    }

    let (total_duration_ms, clipped_samples_bps, peak_amplitude_bps) =
        measure_raw_signal(&mono_samples, rate_hz);
    let resampled_16k = resample_mono(&mono_samples, rate_hz, Pcm16kMono::SAMPLE_RATE_HZ)?;
    let (trimmed_16k, voiced_duration_ms) = trim_and_count_voiced_16k(&resampled_16k, vad_session);

    let pcm_16k =
        Pcm16kMono::new(trimmed_16k).map_err(|err| codec_err("invalid 16kHz PCM output", err))?;
    let metrics = RawAudioMetrics {
        total_duration_ms,
        voiced_duration_ms,
        clipped_samples_bps,
        peak_amplitude_bps,
    };
    Ok((pcm_16k, metrics))
}
