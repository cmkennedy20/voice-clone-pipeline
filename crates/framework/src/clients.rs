//! External inference and Discord delivery adapters (plus `test-support` doubles).

pub mod discord_publisher;
#[cfg(any(test, feature = "test-support"))]
pub mod fake_finetuner;
#[cfg(any(test, feature = "test-support"))]
pub mod fake_phrases;
#[cfg(any(test, feature = "test-support"))]
pub mod fakes;
pub mod qwen_llama;
pub mod tts_sidecar;
pub mod whisper_cuda;

#[cfg(any(test, feature = "test-support"))]
pub use fake_finetuner::{FakeVoiceFinetuner, RecordedSynthesisCall};
#[cfg(any(test, feature = "test-support"))]
pub use fakes::*;
