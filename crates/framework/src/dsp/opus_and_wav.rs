//! `RFC 7845` `48 kHz` `Ogg/Opus` and `16-bit` `48 kHz` `RIFF WAV` dual encoder (`RealAudioCodecEngine`).

use std::{
    io::Cursor,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use domain::{
    audio::{
        types::{Pcm16kMono, Pcm24kMono, RawAudioMetrics, RenderedAudioBundle},
        waveform::quantize_256_waveform_bins,
    },
    ports::{AudioCodecEngine, ExternalPortError},
};
use ogg::writing::{PacketWriteEndInfo, PacketWriter};

use crate::dsp::{
    decode_and_vad::{decode_and_measure_audio, resample_mono},
    silero_vad::load_silero_vad_session,
};

/// Output sample rate required by Discord `IS_VOICE_MESSAGE` (`48_000 Hz`).
pub const OPUS_SAMPLE_RATE_HZ: u32 = 48_000;

/// Standard `20 ms` `Opus` frame length at `48 kHz` (`960` samples).
const OPUS_FRAME_SAMPLES_48K: usize = 960;

/// Standard `RFC 7845` `VoIP` encoder lookahead pre-skip at `48 kHz` (`312` samples).
const OPUS_PRE_SKIP_SAMPLES: u16 = 312;

/// Deterministic logical `Ogg` bitstream serial identifier (`"VCP1"`).
const OGG_STREAM_SERIAL: u32 = 0x5643_5031;

/// Maximum encoded `Opus` packet size in bytes.
const MAX_OPUS_PACKET_BYTES: usize = 4_000;

/// Concrete `Symphonia` + `Rubato` + `Silero ONNX` + `libopus` + `hound` [`AudioCodecEngine`].
#[derive(Debug, Clone, Default)]
pub struct RealAudioCodecEngine {
    vad_model_path: Option<PathBuf>,
    vad_session: Option<Arc<Mutex<ort::session::Session>>>,
}

impl RealAudioCodecEngine {
    /// Creates a new [`RealAudioCodecEngine`] with built-in acoustic frame VAD.
    #[must_use]
    pub fn new() -> Self {
        Self {
            vad_model_path: None,
            vad_session: None,
        }
    }

    /// Creates a [`RealAudioCodecEngine`] loading a cached `Silero VAD` `ONNX` session once when
    /// `vad_model_path` exists on disk.
    #[must_use]
    pub fn with_vad_model_path(vad_model_path: Option<PathBuf>) -> Self {
        let vad_session = vad_model_path.as_deref().and_then(load_silero_vad_session);
        Self {
            vad_model_path,
            vad_session,
        }
    }

    /// Returns the configured `Silero VAD` `ONNX` model path, if any.
    #[must_use]
    pub fn vad_model_path(&self) -> Option<&std::path::Path> {
        self.vad_model_path.as_deref()
    }
}

fn build_opus_head_packet() -> Vec<u8> {
    let mut head = Vec::with_capacity(19);
    head.extend_from_slice(b"OpusHead");
    head.push(1_u8); // RFC 7845 encapsulation version 1
    head.push(1_u8); // 1 channel (mono)
    head.extend_from_slice(&OPUS_PRE_SKIP_SAMPLES.to_le_bytes());
    head.extend_from_slice(&OPUS_SAMPLE_RATE_HZ.to_le_bytes());
    head.extend_from_slice(&0_i16.to_le_bytes()); // 0 dB output gain
    head.push(0_u8); // Channel mapping family 0 (RTP mono/stereo)
    head
}

fn build_opus_tags_packet() -> Vec<u8> {
    const VENDOR: &[u8] = b"voice-clone-pipeline";
    let vendor_len = u32::try_from(VENDOR.len()).unwrap_or(0);
    let mut tags = Vec::with_capacity(16 + VENDOR.len());
    tags.extend_from_slice(b"OpusTags");
    tags.extend_from_slice(&vendor_len.to_le_bytes());
    tags.extend_from_slice(VENDOR);
    tags.extend_from_slice(&0_u32.to_le_bytes()); // 0 user comment strings
    tags
}

fn f32_to_pcm_i16(sample: f32) -> i16 {
    if !sample.is_finite() || sample == 0.0 {
        return 0;
    }
    let clamped = f64::from(sample).clamp(-1.0, 1.0);
    let scaled = clamped * f64::from(i16::MAX);
    let mut low = i32::from(-i16::MAX);
    let mut high = i32::from(i16::MAX);
    while low < high {
        let mid = low + (high - low + 1) / 2;
        if f64::from(mid) - 0.5 <= scaled {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    i16::try_from(low).unwrap_or(0)
}

fn encode_ogg_opus_48k_mono(pcm_48k: &[f32]) -> Result<Vec<u8>, ExternalPortError> {
    let mut encoder = opus::Encoder::new(
        OPUS_SAMPLE_RATE_HZ,
        opus::Channels::Mono,
        opus::Application::Voip,
    )
    .map_err(|err| ExternalPortError::AudioCodec(format!("opus encoder init failed: {err}")))?;

    let mut packet_writer = PacketWriter::new(Vec::new());
    packet_writer
        .write_packet(
            build_opus_head_packet(),
            OGG_STREAM_SERIAL,
            PacketWriteEndInfo::EndPage,
            0,
        )
        .map_err(|err| ExternalPortError::AudioCodec(format!("OpusHead write failed: {err}")))?;

    packet_writer
        .write_packet(
            build_opus_tags_packet(),
            OGG_STREAM_SERIAL,
            PacketWriteEndInfo::EndPage,
            0,
        )
        .map_err(|err| ExternalPortError::AudioCodec(format!("OpusTags write failed: {err}")))?;

    let pre_skip_usize = usize::from(OPUS_PRE_SKIP_SAMPLES);
    let mut padded_pcm = Vec::with_capacity(pcm_48k.len().saturating_add(pre_skip_usize));
    padded_pcm.extend_from_slice(pcm_48k);
    padded_pcm.resize(pcm_48k.len().saturating_add(pre_skip_usize), 0.0_f32);

    let total_frames = padded_pcm.len().div_ceil(OPUS_FRAME_SAMPLES_48K).max(1);
    let end_granule = u64::from(OPUS_PRE_SKIP_SAMPLES) + u64::try_from(pcm_48k.len()).unwrap_or(0);
    let mut frame_buf = [0.0_f32; OPUS_FRAME_SAMPLES_48K];
    let mut packet_buf = vec![0_u8; MAX_OPUS_PACKET_BYTES];

    for (frame_idx, chunk) in padded_pcm.chunks(OPUS_FRAME_SAMPLES_48K).enumerate() {
        frame_buf.fill(0.0);
        frame_buf[..chunk.len()].copy_from_slice(chunk);

        let encoded_len = encoder
            .encode_float(&frame_buf, &mut packet_buf)
            .map_err(|err| ExternalPortError::AudioCodec(format!("opus encode failed: {err}")))?;

        let is_last = frame_idx + 1 == total_frames;
        let end_info = if is_last {
            PacketWriteEndInfo::EndStream
        } else if frame_idx + 2 == total_frames {
            PacketWriteEndInfo::EndPage
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        let granule_pos = if is_last {
            end_granule
        } else {
            u64::try_from((frame_idx + 1) * OPUS_FRAME_SAMPLES_48K).unwrap_or(end_granule)
        };

        packet_writer
            .write_packet(
                packet_buf[..encoded_len].to_vec(),
                OGG_STREAM_SERIAL,
                end_info,
                granule_pos,
            )
            .map_err(|err| {
                ExternalPortError::AudioCodec(format!("ogg audio packet write failed: {err}"))
            })?;
    }

    Ok(packet_writer.into_inner())
}

fn encode_wav_48k_mono(pcm_48k: &[f32]) -> Result<Vec<u8>, ExternalPortError> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: OPUS_SAMPLE_RATE_HZ,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec).map_err(|err| {
            ExternalPortError::AudioCodec(format!("wav header write failed: {err}"))
        })?;
        for &sample in pcm_48k {
            writer.write_sample(f32_to_pcm_i16(sample)).map_err(|err| {
                ExternalPortError::AudioCodec(format!("wav sample write failed: {err}"))
            })?;
        }
        writer
            .finalize()
            .map_err(|err| ExternalPortError::AudioCodec(format!("wav finalize failed: {err}")))?;
    }
    Ok(cursor.into_inner())
}

impl AudioCodecEngine for RealAudioCodecEngine {
    fn decode_and_measure(
        &self,
        raw_bytes: &[u8],
    ) -> Result<(Pcm16kMono, RawAudioMetrics), ExternalPortError> {
        decode_and_measure_audio(raw_bytes, self.vad_session.as_deref())
    }

    fn encode_bundle(
        &self,
        pcm_24k: &Pcm24kMono,
    ) -> Result<RenderedAudioBundle, ExternalPortError> {
        let pcm_48k = resample_mono(
            pcm_24k.samples(),
            Pcm24kMono::SAMPLE_RATE_HZ,
            OPUS_SAMPLE_RATE_HZ,
        )?;
        let ogg_opus_bytes = encode_ogg_opus_48k_mono(&pcm_48k)?;
        let wav_48k_bytes = encode_wav_48k_mono(&pcm_48k)?;
        let waveform = quantize_256_waveform_bins(pcm_24k.samples());
        let duration_ms = pcm_24k.duration_ms();

        Ok(RenderedAudioBundle {
            ogg_opus_bytes,
            wav_48k_bytes,
            waveform,
            duration_ms,
        })
    }
}
