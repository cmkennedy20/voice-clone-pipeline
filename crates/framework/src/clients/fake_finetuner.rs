//! Deterministic `FakeVoiceFinetuner` and synthesis observation ledger for `Layers 4–5` tests.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, atomic::Ordering},
};

use domain::ports::{
    ExternalPortError, FinetuneJobProgress, FinetuneStartRequest, ReferenceSampleRef,
    SynthesisTuning, VoiceFinetuner,
};

use crate::clients::fakes::{FakeCallCounters, lock_or_recover};

/// Snapshot of the most recent [`domain::ports::VoiceSynthesisRequest`] received by `FakeVoiceSynthesizer`.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedSynthesisCall {
    pub profile_slug: String,
    pub reference_samples: Vec<ReferenceSampleRef>,
    pub tuning: SynthesisTuning,
    pub finetuned_checkpoint_dir: Option<String>,
}

#[derive(Debug, Clone)]
struct StoredFinetuneSpec {
    profile_slug: String,
    epochs: u16,
    output_dir: String,
}

/// Deterministic [`VoiceFinetuner`] double that accepts `start_finetune` in `"running"` state and
/// transitions deterministically to `"succeeded"` (`progress_bps: 10_000`, `last_loss: Some(0.38)`) on poll.
#[derive(Debug)]
pub struct FakeVoiceFinetuner {
    counters: Arc<FakeCallCounters>,
    jobs: Mutex<HashMap<String, StoredFinetuneSpec>>,
}

impl FakeVoiceFinetuner {
    #[must_use]
    pub fn new(counters: Arc<FakeCallCounters>) -> Self {
        Self {
            counters,
            jobs: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl VoiceFinetuner for FakeVoiceFinetuner {
    async fn start_finetune(
        &self,
        req: &FinetuneStartRequest,
    ) -> Result<FinetuneJobProgress, ExternalPortError> {
        self.counters.finetune_starts.fetch_add(1, Ordering::SeqCst);
        lock_or_recover(&self.jobs).insert(
            req.job_id.clone(),
            StoredFinetuneSpec {
                profile_slug: req.profile_slug.clone(),
                epochs: req.epochs,
                output_dir: req.output_dir.clone(),
            },
        );

        Ok(FinetuneJobProgress {
            job_id: req.job_id.clone(),
            profile_slug: req.profile_slug.clone(),
            status: "running".to_owned(),
            epochs: req.epochs,
            current_epoch: 1,
            progress_bps: 2_500,
            last_loss: Some(0.84),
            checkpoint_dir: None,
            error_message: None,
        })
    }

    async fn poll_finetune(&self, job_id: &str) -> Result<FinetuneJobProgress, ExternalPortError> {
        self.counters.finetune_polls.fetch_add(1, Ordering::SeqCst);
        let spec = lock_or_recover(&self.jobs)
            .get(job_id)
            .cloned()
            .ok_or_else(|| {
                ExternalPortError::Tts(format!("unknown fake finetune job '{job_id}'"))
            })?;

        Ok(FinetuneJobProgress {
            job_id: job_id.to_owned(),
            profile_slug: spec.profile_slug,
            status: "succeeded".to_owned(),
            epochs: spec.epochs,
            current_epoch: spec.epochs,
            progress_bps: 10_000,
            last_loss: Some(0.38),
            checkpoint_dir: Some(spec.output_dir),
            error_message: None,
        })
    }
}

impl FakeCallCounters {
    #[must_use]
    pub fn finetune_start_calls(&self) -> usize {
        self.finetune_starts.load(Ordering::SeqCst)
    }

    #[must_use]
    pub fn finetune_poll_calls(&self) -> usize {
        self.finetune_polls.load(Ordering::SeqCst)
    }

    #[must_use]
    pub fn last_synthesis_call(&self) -> Option<RecordedSynthesisCall> {
        lock_or_recover(&self.last_synthesis).clone()
    }
}
