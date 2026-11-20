#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::str::FromStr;

use domain::hardware::{
    DetectedGpu, GpuVendor, HardwareTier, SystemSpecs, recommend_hardware_plan,
    required_model_artifacts,
};

#[test]
fn selects_highest_vram_nvidia_gpu_on_multi_gpu_windows_desktop() {
    let specs = SystemSpecs {
        os: "windows".to_owned(),
        arch: "x86_64".to_owned(),
        cpu_logical_cores: 24,
        system_ram_mib: 65_536,
        free_disk_mib: 120_000,
        gpus: vec![
            DetectedGpu {
                index: 0,
                name: "NVIDIA GeForce RTX 3070".to_owned(),
                vendor: GpuVendor::Nvidia,
                vram_total_mib: 8_192,
                vram_free_mib: 7_400,
                driver_version: Some("560.94".to_owned()),
                uuid: Some("GPU-3070".to_owned()),
            },
            DetectedGpu {
                index: 1,
                name: "NVIDIA GeForce RTX 3090".to_owned(),
                vendor: GpuVendor::Nvidia,
                vram_total_mib: 24_576,
                vram_free_mib: 23_500,
                driver_version: Some("560.94".to_owned()),
                uuid: Some("GPU-3090".to_owned()),
            },
        ],
    };

    let plan = recommend_hardware_plan(&specs, None, None);
    assert_eq!(plan.tier, HardwareTier::EnthusiastCuda);
    assert_eq!(plan.selected_gpu_index, Some(1));
    assert_eq!(
        plan.selected_gpu_name.as_deref(),
        Some("NVIDIA GeForce RTX 3090")
    );
    assert_eq!(plan.cuda_visible_devices.as_deref(), Some("1"));
    assert!(plan.whisper_use_gpu);
    assert_eq!(plan.whisper_model_filename, "ggml-large-v3.bin");
    assert_eq!(plan.llm_model_filename, "Qwen3-8B-Q8_0.gguf");
    assert_eq!(plan.llm_gpu_layers, 99);
    assert_eq!(plan.prewarm_engines, vec!["xtts_v2"]);
    assert_eq!(plan.warnings, Vec::<String>::new());

    let mut linux_specs = specs;
    linux_specs.os = "linux".to_owned();
    let linux_plan = recommend_hardware_plan(&linux_specs, None, None);
    assert_eq!(linux_plan.prewarm_engines, vec!["cosyvoice_3", "xtts_v2"]);
}

#[test]
fn tunes_8gb_and_12gb_mainstream_nvidia_desktops_without_vram_oom() {
    let specs_8gb = SystemSpecs {
        os: "windows".to_owned(),
        arch: "x86_64".to_owned(),
        cpu_logical_cores: 16,
        system_ram_mib: 32_768,
        free_disk_mib: 80_000,
        gpus: vec![DetectedGpu {
            index: 0,
            name: "NVIDIA GeForce RTX 4060".to_owned(),
            vendor: GpuVendor::Nvidia,
            vram_total_mib: 8_188,
            vram_free_mib: 7_500,
            driver_version: Some("566.14".to_owned()),
            uuid: None,
        }],
    };
    let plan_8gb = recommend_hardware_plan(&specs_8gb, None, None);
    assert_eq!(plan_8gb.tier, HardwareTier::MainstreamCuda);
    assert_eq!(plan_8gb.whisper_model_filename, "ggml-small.bin");
    assert_eq!(plan_8gb.llm_model_filename, "Qwen3-8B-Q4_K_M.gguf");
    assert_eq!(plan_8gb.llm_gpu_layers, 28);
    assert_eq!(plan_8gb.prewarm_engines, vec!["xtts_v2"]);

    let mut specs_12gb = specs_8gb;
    specs_12gb.gpus[0].name = "NVIDIA GeForce RTX 4070".to_owned();
    specs_12gb.gpus[0].vram_total_mib = 12_282;
    let plan_12gb = recommend_hardware_plan(&specs_12gb, None, None);
    assert_eq!(plan_12gb.tier, HardwareTier::MainstreamCuda);
    assert_eq!(plan_12gb.llm_gpu_layers, 99);
}

#[test]
fn supports_6gb_entry_cuda_and_cpu_or_non_nvidia_desktops_with_disk_warnings() {
    let specs_6gb = SystemSpecs {
        os: "windows".to_owned(),
        arch: "x86_64".to_owned(),
        cpu_logical_cores: 12,
        system_ram_mib: 16_384,
        free_disk_mib: 50_000,
        gpus: vec![DetectedGpu {
            index: 0,
            name: "NVIDIA GeForce RTX 3050 Laptop GPU".to_owned(),
            vendor: GpuVendor::Nvidia,
            vram_total_mib: 6_144,
            vram_free_mib: 5_500,
            driver_version: Some("555.85".to_owned()),
            uuid: None,
        }],
    };
    let entry_plan = recommend_hardware_plan(&specs_6gb, None, None);
    assert_eq!(entry_plan.tier, HardwareTier::EntryCuda);
    assert_eq!(entry_plan.whisper_model_filename, "ggml-base.bin");
    assert_eq!(entry_plan.llm_model_filename, "Qwen3-4B-Q4_K_M.gguf");
    assert_eq!(entry_plan.prewarm_engines, Vec::<String>::new());

    let specs_amd_low_disk = SystemSpecs {
        os: "windows".to_owned(),
        arch: "x86_64".to_owned(),
        cpu_logical_cores: 16,
        system_ram_mib: 16_384,
        free_disk_mib: 2_000,
        gpus: vec![DetectedGpu {
            index: 0,
            name: "AMD Radeon RX 7800 XT".to_owned(),
            vendor: GpuVendor::Amd,
            vram_total_mib: 16_384,
            vram_free_mib: 15_000,
            driver_version: None,
            uuid: None,
        }],
    };
    let cpu_plan = recommend_hardware_plan(&specs_amd_low_disk, None, None);
    assert_eq!(cpu_plan.tier, HardwareTier::CpuOrIntegrated);
    assert!(!cpu_plan.whisper_use_gpu);
    assert_eq!(cpu_plan.cuda_visible_devices, None);
    assert_eq!(cpu_plan.llm_cpu_threads, 8);
    // Vulkan llama-server still offloads layers when an AMD/Intel discrete GPU >= 6 GiB is present!
    assert_eq!(cpu_plan.llm_gpu_layers, 99);
    assert!(
        cpu_plan.warnings.iter().any(|w| w.contains("disk space")),
        "expected low disk space warning, got: {:?}",
        cpu_plan.warnings
    );
}

#[test]
fn honors_agent_tier_and_gpu_overrides_and_produces_complete_model_manifest() {
    assert_eq!(
        HardwareTier::from_str("mainstream_cuda").unwrap(),
        HardwareTier::MainstreamCuda
    );
    assert_eq!(
        HardwareTier::from_str("cpu").unwrap(),
        HardwareTier::CpuOrIntegrated
    );

    let specs = SystemSpecs {
        os: "windows".to_owned(),
        arch: "x86_64".to_owned(),
        cpu_logical_cores: 8,
        system_ram_mib: 32_768,
        free_disk_mib: 100_000,
        gpus: vec![
            DetectedGpu {
                index: 0,
                name: "NVIDIA GeForce RTX 3070".to_owned(),
                vendor: GpuVendor::Nvidia,
                vram_total_mib: 8_192,
                vram_free_mib: 7_500,
                driver_version: None,
                uuid: None,
            },
            DetectedGpu {
                index: 1,
                name: "NVIDIA GeForce RTX 3090".to_owned(),
                vendor: GpuVendor::Nvidia,
                vram_total_mib: 24_576,
                vram_free_mib: 23_000,
                driver_version: None,
                uuid: None,
            },
        ],
    };

    let overridden = recommend_hardware_plan(&specs, Some(HardwareTier::MainstreamCuda), Some(0));
    assert_eq!(overridden.tier, HardwareTier::MainstreamCuda);
    assert_eq!(overridden.selected_gpu_index, Some(0));
    assert_eq!(overridden.cuda_visible_devices.as_deref(), Some("0"));

    let artifacts = required_model_artifacts(&overridden, false);
    assert!(
        artifacts
            .iter()
            .any(|a| a.relative_path == "whisper/ggml-small.bin")
    );
    assert!(
        artifacts
            .iter()
            .any(|a| a.relative_path == "vad/silero_vad.onnx")
    );
    assert!(
        artifacts
            .iter()
            .any(|a| a.relative_path == "llm/Qwen3-8B-Q4_K_M.gguf")
    );
    assert!(
        artifacts
            .iter()
            .any(|a| a.relative_path == "tts/xtts-v2/model.pth")
    );
    assert!(
        artifacts
            .iter()
            .all(|a| a.download_url.starts_with("https://huggingface.co/"))
    );
}
