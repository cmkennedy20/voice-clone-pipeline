//! Pure desktop hardware classification, adaptive `VRAM` tiering, and model manifest planning.

pub mod manifest;

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

pub use self::manifest::{ModelArtifactSpec, required_model_artifacts};

/// Graphics device vendor classification for compute capability routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

/// Discovered graphics adapter metadata on a desktop host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedGpu {
    pub index: u32,
    pub name: String,
    pub vendor: GpuVendor,
    pub vram_total_mib: u32,
    pub vram_free_mib: u32,
    pub driver_version: Option<String>,
    pub uuid: Option<String>,
}

/// Host CPU, memory, free storage, and graphics inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemSpecs {
    pub os: String,
    pub arch: String,
    pub cpu_logical_cores: u32,
    pub system_ram_mib: u64,
    pub free_disk_mib: u64,
    pub gpus: Vec<DetectedGpu>,
}

/// Adaptive execution profile tailored to the user's desktop hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareTier {
    /// `>= 16 GiB` `NVIDIA` `VRAM` (e.g. `RTX 3090`, `4080`, `4090`, `5080`, `5090`).
    EnthusiastCuda,
    /// `8..=15 GiB` `NVIDIA` `VRAM` (e.g. `RTX 3060 12GB`, `3070`, `4060`, `4070`).
    MainstreamCuda,
    /// `4..=7 GiB` `NVIDIA` `VRAM` (e.g. `RTX 3050`, `6 GB` laptop `GPUs`).
    EntryCuda,
    /// `< 4 GiB` `NVIDIA` `VRAM`, `AMD`/`Intel` `Vulkan`, or `CPU`-only desktop.
    CpuOrIntegrated,
}

impl HardwareTier {
    /// Canonical lowercase `snake_case` slug for CLI flags and JSON output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EnthusiastCuda => "enthusiast_cuda",
            Self::MainstreamCuda => "mainstream_cuda",
            Self::EntryCuda => "entry_cuda",
            Self::CpuOrIntegrated => "cpu_or_integrated",
        }
    }
}

impl fmt::Display for HardwareTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Parse error when converting a CLI or config string into [`HardwareTier`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "unsupported hardware tier `{0}` (expected `enthusiast_cuda`, `mainstream_cuda`, `entry_cuda`, or `cpu_or_integrated`)"
)]
pub struct ParseHardwareTierError(pub String);

impl FromStr for HardwareTier {
    type Err = ParseHardwareTierError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "enthusiast" | "enthusiast_cuda" | "high" | "full" => Ok(Self::EnthusiastCuda),
            "mainstream" | "mainstream_cuda" | "mid" | "compact" => Ok(Self::MainstreamCuda),
            "entry" | "entry_cuda" | "low" => Ok(Self::EntryCuda),
            "cpu" | "cpu_or_integrated" | "vulkan" | "igpu" => Ok(Self::CpuOrIntegrated),
            other => Err(ParseHardwareTierError(other.to_owned())),
        }
    }
}

/// Deterministic runtime parameters and model filenames derived for a host's hardware.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwarePlan {
    pub tier: HardwareTier,
    pub selected_gpu_index: Option<u32>,
    pub selected_gpu_name: Option<String>,
    pub selected_gpu_vram_mib: u32,
    pub cuda_visible_devices: Option<String>,
    pub whisper_use_gpu: bool,
    pub whisper_model_filename: String,
    pub llm_model_filename: String,
    pub llm_hf_repo: String,
    pub llm_gpu_layers: u32,
    pub llm_context_size: u32,
    pub llm_cpu_threads: u32,
    pub prewarm_engines: Vec<String>,
    pub estimated_download_mib: u64,
    pub warnings: Vec<String>,
}

struct TierSettings {
    whisper_use_gpu: bool,
    whisper_file: &'static str,
    llm_repo: &'static str,
    llm_file: &'static str,
    llm_ngl: u32,
    prewarm: Vec<String>,
    include_cosy_est: bool,
    cuda_dev: Option<String>,
    sel_idx: Option<u32>,
    sel_name: Option<String>,
    sel_vram: u32,
}

fn select_nvidia_gpu(specs: &SystemSpecs, override_gpu: Option<u32>) -> Option<&DetectedGpu> {
    if let Some(requested_idx) = override_gpu {
        return specs
            .gpus
            .iter()
            .find(|g| g.index == requested_idx && g.vendor == GpuVendor::Nvidia);
    }
    specs
        .gpus
        .iter()
        .filter(|g| g.vendor == GpuVendor::Nvidia)
        .max_by_key(|g| (g.vram_total_mib, g.vram_free_mib))
}

fn classify_tier(selected_nvidia: Option<&DetectedGpu>) -> HardwareTier {
    let Some(gpu) = selected_nvidia else {
        return HardwareTier::CpuOrIntegrated;
    };
    if gpu.vram_total_mib >= 15_500 {
        HardwareTier::EnthusiastCuda
    } else if gpu.vram_total_mib >= 7_800 {
        HardwareTier::MainstreamCuda
    } else if gpu.vram_total_mib >= 4_000 {
        HardwareTier::EntryCuda
    } else {
        HardwareTier::CpuOrIntegrated
    }
}

fn build_tier_settings(
    tier: HardwareTier,
    specs: &SystemSpecs,
    nvidia_gpu: Option<&DetectedGpu>,
    override_gpu: Option<u32>,
) -> TierSettings {
    let idx = nvidia_gpu.map(|g| g.index).or(override_gpu);
    let is_windows = specs.os.eq_ignore_ascii_case("windows");
    match tier {
        HardwareTier::EnthusiastCuda => TierSettings {
            whisper_use_gpu: idx.is_some(),
            whisper_file: "ggml-large-v3.bin",
            llm_repo: "Qwen/Qwen3-8B-GGUF",
            llm_file: "Qwen3-8B-Q8_0.gguf",
            llm_ngl: 99,
            prewarm: if is_windows {
                vec!["xtts_v2".to_owned()]
            } else {
                vec!["cosyvoice_3".to_owned(), "xtts_v2".to_owned()]
            },
            include_cosy_est: !is_windows,
            cuda_dev: idx.map(|i| i.to_string()),
            sel_idx: idx,
            sel_name: nvidia_gpu.map(|g| g.name.clone()),
            sel_vram: nvidia_gpu.map_or(0, |g| g.vram_total_mib),
        },
        HardwareTier::MainstreamCuda => {
            let vram = nvidia_gpu.map_or(8_192, |g| g.vram_total_mib);
            TierSettings {
                whisper_use_gpu: idx.is_some(),
                whisper_file: "ggml-small.bin",
                llm_repo: "Qwen/Qwen3-8B-GGUF",
                llm_file: "Qwen3-8B-Q4_K_M.gguf",
                llm_ngl: if vram >= 11_500 { 99 } else { 28 },
                prewarm: vec!["xtts_v2".to_owned()],
                include_cosy_est: false,
                cuda_dev: idx.map(|i| i.to_string()),
                sel_idx: idx,
                sel_name: nvidia_gpu.map(|g| g.name.clone()),
                sel_vram: vram,
            }
        }
        HardwareTier::EntryCuda => TierSettings {
            whisper_use_gpu: idx.is_some(),
            whisper_file: "ggml-base.bin",
            llm_repo: "Qwen/Qwen3-4B-GGUF",
            llm_file: "Qwen3-4B-Q4_K_M.gguf",
            llm_ngl: 24,
            prewarm: Vec::new(),
            include_cosy_est: false,
            cuda_dev: idx.map(|i| i.to_string()),
            sel_idx: idx,
            sel_name: nvidia_gpu.map(|g| g.name.clone()),
            sel_vram: nvidia_gpu.map_or(6_144, |g| g.vram_total_mib),
        },
        HardwareTier::CpuOrIntegrated => {
            let best_non_nv = specs.gpus.iter().max_by_key(|g| g.vram_total_mib);
            let vulkan_ngl = match best_non_nv {
                Some(gpu) if gpu.vram_total_mib >= 6_000 => 99,
                _ => 0,
            };
            TierSettings {
                whisper_use_gpu: false,
                whisper_file: "ggml-base.bin",
                llm_repo: "Qwen/Qwen3-4B-GGUF",
                llm_file: "Qwen3-4B-Q4_K_M.gguf",
                llm_ngl: vulkan_ngl,
                prewarm: Vec::new(),
                include_cosy_est: false,
                cuda_dev: None,
                sel_idx: best_non_nv.map(|g| g.index),
                sel_name: best_non_nv.map(|g| g.name.clone()),
                sel_vram: best_non_nv.map_or(0, |g| g.vram_total_mib),
            }
        }
    }
}

/// Derives an adaptive, zero-OOM [`HardwarePlan`] from probed [`SystemSpecs`] and optional overrides.
pub fn recommend_hardware_plan(
    specs: &SystemSpecs,
    override_tier: Option<HardwareTier>,
    override_gpu: Option<u32>,
) -> HardwarePlan {
    let nvidia_gpu = select_nvidia_gpu(specs, override_gpu);
    let tier = override_tier.unwrap_or_else(|| classify_tier(nvidia_gpu));
    let cpu_threads = (specs.cpu_logical_cores / 2).clamp(2, 16);
    let ts = build_tier_settings(tier, specs, nvidia_gpu, override_gpu);

    let mut plan = HardwarePlan {
        tier,
        selected_gpu_index: ts.sel_idx,
        selected_gpu_name: ts.sel_name,
        selected_gpu_vram_mib: ts.sel_vram,
        cuda_visible_devices: ts.cuda_dev,
        whisper_use_gpu: ts.whisper_use_gpu,
        whisper_model_filename: ts.whisper_file.to_owned(),
        llm_model_filename: ts.llm_file.to_owned(),
        llm_hf_repo: ts.llm_repo.to_owned(),
        llm_gpu_layers: ts.llm_ngl,
        llm_context_size: 4_096,
        llm_cpu_threads: cpu_threads,
        prewarm_engines: ts.prewarm,
        estimated_download_mib: 0,
        warnings: Vec::new(),
    };

    let artifacts = required_model_artifacts(&plan, ts.include_cosy_est);
    let est_mib: u64 = artifacts.iter().map(|a| a.approx_size_mib).sum();
    plan.estimated_download_mib = est_mib;

    if specs.free_disk_mib > 0 && specs.free_disk_mib < est_mib.saturating_add(2_048) {
        plan.warnings.push(format!(
            "low free disk space: {} MiB available, ~{} MiB recommended for models and database",
            specs.free_disk_mib,
            est_mib.saturating_add(2_048)
        ));
    }
    if specs.system_ram_mib > 0 && specs.system_ram_mib < 12_000 {
        plan.warnings.push(format!(
            "low system RAM ({} MiB detected; >= 16384 MiB recommended for smooth synthesis)",
            specs.system_ram_mib
        ));
    }

    plan
}
