//! Multi-sample reference descriptors and `RTX 3090` `XTTS-v2` fine-tuning port models.

use serde::{Deserialize, Serialize};

use crate::ports::ExternalPortError;

/// Single reference `WAV` clip and English transcript paired for multi-sample conditioning or fine-tuning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceSampleRef {
    pub wav_path: String,
    pub transcript_en: String,
}

/// Request payload dispatched to `POST /finetune` on the `PyTorch` `TTS` sidecar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FinetuneStartRequest {
    pub job_id: String,
    pub profile_slug: String,
    pub samples: Vec<ReferenceSampleRef>,
    pub output_dir: String,
    pub epochs: u16,
    pub learning_rate: f64,
}

/// Live or terminal fine-tuning job status returned by `POST /finetune` and `GET /finetune/{job_id}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FinetuneJobProgress {
    pub job_id: String,
    pub profile_slug: String,
    pub status: String,
    pub epochs: u16,
    pub current_epoch: u16,
    pub progress_bps: u32,
    pub last_loss: Option<f64>,
    pub checkpoint_dir: Option<String>,
    pub error_message: Option<String>,
}

/// Asynchronous `XTTS-v2` speaker fine-tuning orchestration port.
#[async_trait::async_trait]
pub trait VoiceFinetuner: Send + Sync {
    async fn start_finetune(
        &self,
        req: &FinetuneStartRequest,
    ) -> Result<FinetuneJobProgress, ExternalPortError>;

    async fn poll_finetune(&self, job_id: &str) -> Result<FinetuneJobProgress, ExternalPortError>;
}
