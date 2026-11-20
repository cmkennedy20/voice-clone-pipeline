//! Concrete `Symphonia` container demuxer, `Rubato` FFT resampler, `Silero` ONNX VAD, and `Ogg/Opus` + `WAV` encoders.

pub mod decode_and_vad;
pub mod opus_and_wav;
pub(crate) mod silero_vad;
