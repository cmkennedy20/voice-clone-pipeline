//! `ffprobe` JSON stream inspection for `M5` dual-format (`48 kHz` mono `Opus` + `WAV`) verification.

use std::{io::ErrorKind, process::Stdio};

use serde::Deserialize;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Default, Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
}

#[derive(Debug, Default, Deserialize)]
struct FfprobeStream {
    codec_name: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u32>,
}

fn validate_ffprobe_json(json_bytes: &[u8], expected_codec: Option<&str>) -> bool {
    let Ok(parsed) = serde_json::from_slice::<FfprobeOutput>(json_bytes) else {
        return false;
    };
    let Some(stream) = parsed.streams.first() else {
        return false;
    };
    let codec_ok = match expected_codec {
        Some(expected) => stream.codec_name.as_deref() == Some(expected),
        None => true,
    };
    codec_ok && stream.sample_rate.as_deref() == Some("48000") && stream.channels == Some(1)
}

async fn probe_audio_bytes_via_ffprobe(bytes: &[u8], expected_codec: Option<&str>) -> bool {
    let mut child = match tokio::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,sample_rate,channels",
            "-of",
            "json",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(proc) => proc,
        Err(err) if err.kind() == ErrorKind::NotFound => return true,
        Err(_) => return false,
    };

    if let Some(mut stdin) = child.stdin.take() {
        if let Err(err) = stdin.write_all(bytes).await
            && err.kind() != ErrorKind::BrokenPipe
        {
            return false;
        }
        drop(stdin);
    }

    let Ok(output) = child.wait_with_output().await else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    validate_ffprobe_json(&output.stdout, expected_codec)
}

/// Verifies `.ogg` (`codec_name == "opus"`, `48000 Hz`, `1` channel) and `.wav` (`48000 Hz`, `1`
/// channel) via `ffprobe` when the `ffprobe` binary is present on `PATH`.
pub(crate) async fn verify_ffprobe_dual_bundle(
    ogg_opus_bytes: &[u8],
    wav_48k_bytes: &[u8],
) -> bool {
    probe_audio_bytes_via_ffprobe(ogg_opus_bytes, Some("opus")).await
        && probe_audio_bytes_via_ffprobe(wav_48k_bytes, None).await
}

#[cfg(test)]
mod tests {
    use super::validate_ffprobe_json;

    #[test]
    fn validates_48khz_mono_opus_and_wav_ffprobe_json_streams() {
        let valid_opus = br#"{
            "streams": [{"codec_name": "opus", "sample_rate": "48000", "channels": 1}]
        }"#;
        assert!(validate_ffprobe_json(valid_opus, Some("opus")));

        let valid_wav = br#"{
            "streams": [{"codec_name": "pcm_s16le", "sample_rate": "48000", "channels": 1}]
        }"#;
        assert!(validate_ffprobe_json(valid_wav, None));

        let wrong_codec = br#"{
            "streams": [{"codec_name": "vorbis", "sample_rate": "48000", "channels": 1}]
        }"#;
        assert!(!validate_ffprobe_json(wrong_codec, Some("opus")));

        let wrong_rate = br#"{
            "streams": [{"codec_name": "opus", "sample_rate": "24000", "channels": 1}]
        }"#;
        assert!(!validate_ffprobe_json(wrong_rate, Some("opus")));

        let stereo_stream = br#"{
            "streams": [{"codec_name": "opus", "sample_rate": "48000", "channels": 2}]
        }"#;
        assert!(!validate_ffprobe_json(stereo_stream, Some("opus")));

        assert!(!validate_ffprobe_json(br#"{"streams": []}"#, None));
    }
}
