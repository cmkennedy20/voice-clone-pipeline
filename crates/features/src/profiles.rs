//! Speaker voice profile enrollment, multi-sample management, tuning, fine-tuning, and CRUD workflows.

pub mod delete_profile;
pub mod enroll_profile;
pub mod finetune_profile;
pub mod list_profiles;
pub mod manage_samples;
pub mod read_reference_audio;
pub mod update_profile;

pub use crate::shared_queries::{
    profile_loader::{VoiceProfileId, VoiceProfileRecord},
    sample_queries::{VoiceFinetuneJobId, VoiceProfileSampleRecord, VoiceSampleId},
};
pub use delete_profile::delete_profile;
pub use enroll_profile::{AdditionalSampleInput, EnrollProfileInput, enroll_profile};
pub use finetune_profile::{
    StartProfileFinetuneInput, VoiceFinetuneJobRecord, get_profile_finetune_status,
    start_profile_finetune,
};
pub use list_profiles::list_profiles;
pub use manage_samples::{
    AddProfileSampleInput, add_profile_sample, delete_profile_sample, list_profile_samples,
    read_profile_sample_audio,
};
pub use read_reference_audio::read_profile_reference_audio;
pub use update_profile::{AvatarPatch, UpdateProfileInput, update_profile};
