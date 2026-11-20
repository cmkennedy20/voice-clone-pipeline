//! Declarative `HuggingFace` model artifact manifest for pure-Rust resumable downloads.

use serde::{Deserialize, Serialize};

use super::{HardwarePlan, HardwareTier};

/// Single downloadable model weight or configuration file required at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelArtifactSpec {
    pub component: String,
    pub relative_path: String,
    pub download_url: String,
    pub approx_size_mib: u64,
    pub required: bool,
}

fn hf_resolve_url(repo: &str, path_in_repo: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/main/{path_in_repo}?download=true")
}

fn whisper_approx_mib(filename: &str) -> u64 {
    match filename {
        "ggml-large-v3.bin" => 2_952,
        "ggml-medium.bin" => 1_464,
        "ggml-small.bin" => 466,
        _ => 142,
    }
}

fn llm_approx_mib(filename: &str) -> u64 {
    match filename {
        "Qwen3-8B-Q8_0.gguf" => 8_310,
        "Qwen3-8B-Q4_K_M.gguf" => 4_800,
        _ => 2_500,
    }
}

/// Computes the deterministic list of model files required for `plan`.
pub fn required_model_artifacts(
    plan: &HardwarePlan,
    include_cosyvoice: bool,
) -> Vec<ModelArtifactSpec> {
    let mut items = vec![
        ModelArtifactSpec {
            component: "whisper".to_owned(),
            relative_path: format!("whisper/{}", plan.whisper_model_filename),
            download_url: hf_resolve_url("ggerganov/whisper.cpp", &plan.whisper_model_filename),
            approx_size_mib: whisper_approx_mib(&plan.whisper_model_filename),
            required: true,
        },
        ModelArtifactSpec {
            component: "silero_vad".to_owned(),
            relative_path: "vad/silero_vad.onnx".to_owned(),
            download_url: hf_resolve_url("onnx-community/silero-vad", "onnx/model.onnx"),
            approx_size_mib: 5,
            required: true,
        },
        ModelArtifactSpec {
            component: "qwen_llm".to_owned(),
            relative_path: format!("llm/{}", plan.llm_model_filename),
            download_url: hf_resolve_url(&plan.llm_hf_repo, &plan.llm_model_filename),
            approx_size_mib: llm_approx_mib(&plan.llm_model_filename),
            required: true,
        },
    ];

    for (file, size_mib) in [
        ("config.json", 1_u64),
        ("vocab.json", 1),
        ("speakers_xtts.pth", 8),
        ("mel_stats.pth", 1),
        ("dvae.pth", 202),
        ("model.pth", 1_785),
    ] {
        items.push(ModelArtifactSpec {
            component: "xtts_v2".to_owned(),
            relative_path: format!("tts/xtts-v2/{file}"),
            download_url: hf_resolve_url("coqui/XTTS-v2", file),
            approx_size_mib: size_mib,
            required: true,
        });
    }

    if include_cosyvoice || plan.tier == HardwareTier::EnthusiastCuda {
        for (file, size_mib) in [
            ("cosyvoice2.yaml", 1_u64),
            ("campplus.onnx", 28),
            ("speech_tokenizer_v2.onnx", 475),
            ("flow.pt", 432),
            ("hift.pt", 82),
            ("llm.pt", 1_960),
        ] {
            items.push(ModelArtifactSpec {
                component: "cosyvoice".to_owned(),
                relative_path: format!("tts/cosyvoice/{file}"),
                download_url: hf_resolve_url("FunAudioLLM/CosyVoice2-0.5B", file),
                approx_size_mib: size_mib,
                required: false,
            });
        }
    }

    items
}
