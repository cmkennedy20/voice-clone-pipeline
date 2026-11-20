//! `clap` subcommand definitions and `HTTP` client execution for `doctor`, `enroll`,
//! `translate-clip`, and `bench`.

use std::{
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

use clap::{Args, Parser, Subcommand};
use domain::{
    hardware::recommend_hardware_plan,
    translation::{
        error::TranslationDomainError,
        types::{TargetLanguage, TtsEngineKind},
    },
};
use features::{
    benchmarks::run_rtx3090_suite::{BenchmarkSuiteReport, RunBenchmarkSuiteInput},
    profiles::enroll_profile::EnrollProfileInput,
    shared_queries::profile_loader::VoiceProfileRecord,
};
use framework::{
    config::{AppConfig, ConfigError},
    desktop::probe::probe_system_specs,
};
use reqwest::{
    header::AUTHORIZATION,
    multipart::{Form, Part},
};

/// Typed operational error for `voice-clone-pipeline` CLI subcommands.
#[derive(Debug)]
pub enum CliError {
    Config(ConfigError),
    Io(std::io::Error),
    Http(reqwest::Error),
    Translation(TranslationDomainError),
    InvalidArgument(String),
    ApiStatus {
        label: String,
        status: u16,
        body: String,
    },
    BenchmarkGateFailed(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(err) => write!(f, "configuration error: {err}"),
            Self::Io(err) => write!(f, "filesystem I/O error: {err}"),
            Self::Http(err) => write!(f, "HTTP client error: {err}"),
            Self::Translation(err) => write!(f, "translation parameter error: {err}"),
            Self::InvalidArgument(msg) | Self::BenchmarkGateFailed(msg) => f.write_str(msg),
            Self::ApiStatus {
                label,
                status,
                body,
            } => write!(f, "{label} failed ({status}): {body}"),
        }
    }
}

impl std::error::Error for CliError {}

impl From<ConfigError> for CliError {
    fn from(err: ConfigError) -> Self {
        Self::Config(err)
    }
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<reqwest::Error> for CliError {
    fn from(err: reqwest::Error) -> Self {
        Self::Http(err)
    }
}

impl From<TranslationDomainError> for CliError {
    fn from(err: TranslationDomainError) -> Self {
        Self::Translation(err)
    }
}

/// Containerized operator CLI for `voice-clone-pipeline`.
#[derive(Debug, Parser)]
#[command(name = "cli", about = "Operational & RTX 3090 benchmark CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: CliCommand,
}

#[derive(Debug, Args)]
pub struct EnrollArgs {
    #[arg(long)]
    pub slug: String,
    #[arg(long)]
    pub wav: PathBuf,
    #[arg(long)]
    pub transcript: String,
    #[arg(long)]
    pub display_name: Option<String>,
    #[arg(long, default_value = "cosyvoice_3")]
    pub engine: String,
    #[arg(long, default_value = "zh")]
    pub lang: String,
}

#[derive(Debug, Args)]
pub struct TranslateClipArgs {
    #[arg(long)]
    pub profile: String,
    #[arg(long)]
    pub audio: Option<PathBuf>,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long, default_value = "zh")]
    pub lang: String,
    #[arg(long)]
    pub out: PathBuf,
    #[arg(long, default_value = "ogg")]
    pub format: String,
}

#[derive(Debug, Args)]
pub struct BenchArgs {
    #[arg(long)]
    pub manifest: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
    #[arg(long, default_value = "default_voice")]
    pub profile: String,
    #[arg(long)]
    pub engine: Option<String>,
}

/// Supported CLI subcommands matching `Makefile` and `Justfile` targets.
#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Probe `api-server` `/health/ready` and `NVIDIA` `GPU` availability (`nvidia-smi`).
    Doctor,
    /// Inspect container/host `GPUs`, `VRAM`, `RAM`, disk space, and recommended hardware tier.
    HardwareProbe {
        /// Emit machine-readable JSON output for agents and setup scripts.
        #[arg(long)]
        json: bool,
    },
    /// Enroll a `24 kHz` English reference `WAV` into `PostgreSQL` + shared storage.
    Enroll(EnrollArgs),
    /// Render an English audio file or text prompt into verified cloned `.ogg` or `48 kHz` `.wav`.
    TranslateClip(TranslateClipArgs),
    /// Run the 15-clip + 3-trap `RTX 3090` Layer-7 benchmark suite (`M1–M6`).
    Bench(BenchArgs),
}

fn bearer_header(config: &AppConfig) -> String {
    format!("Bearer {}", config.owner_api_bearer_token.expose_secret())
}

async fn write_output_file(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, bytes).await?;
    Ok(())
}

async fn run_nvidia_smi(args: &[&str]) -> Option<String> {
    let out = tokio::process::Command::new("nvidia-smi")
        .args(args)
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

async fn ensure_success(
    resp: reqwest::Response,
    label: &str,
) -> Result<reqwest::Response, CliError> {
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(CliError::ApiStatus {
            label: label.to_owned(),
            status,
            body,
        });
    }
    Ok(resp)
}

async fn run_doctor(http: &reqwest::Client, base_url: &str) -> Result<(), CliError> {
    ensure_success(
        http.get(format!("{base_url}/health/ready")).send().await?,
        "readiness probe",
    )
    .await?;
    match run_nvidia_smi(&[
        "--query-gpu=name,memory.used,memory.total",
        "--format=csv,noheader",
    ])
    .await
    {
        Some(gpu) => tracing::info!(gpu = %gpu, "doctor: api-server ready & CUDA GPU detected"),
        None => {
            tracing::info!("doctor: api-server ready (nvidia-smi binary not present in container)");
        }
    }
    Ok(())
}

async fn run_enroll(
    http: &reqwest::Client,
    base_url: &str,
    config: &AppConfig,
    args: &EnrollArgs,
) -> Result<(), CliError> {
    let input = EnrollProfileInput {
        slug: args.slug.clone(),
        display_name: args
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&args.slug)
            .to_owned(),
        avatar_url: None,
        reference_audio_bytes: tokio::fs::read(&args.wav).await?,
        reference_transcript_en: args.transcript.clone(),
        preferred_engine: TtsEngineKind::from_str(args.engine.trim())?,
        default_target_language: TargetLanguage::from_str(args.lang.trim())?,
        additional_samples: Vec::new(),
        tuning: None,
    };
    let resp = http
        .post(format!("{base_url}/api/v1/profiles"))
        .header(AUTHORIZATION, bearer_header(config))
        .json(&input)
        .send()
        .await?;
    let created: VoiceProfileRecord = ensure_success(resp, "profile enrollment")
        .await?
        .json()
        .await?;
    tracing::info!(
        profile_id = %created.id,
        slug = %created.slug,
        engine = %created.preferred_engine,
        lang = %created.default_target_language,
        "enrolled voice profile"
    );
    Ok(())
}

async fn build_translate_form(args: &TranslateClipArgs) -> Result<Form, CliError> {
    let mut form = Form::new();
    let mut has_input = false;

    if let Some(audio_path) = &args.audio {
        let file_name = audio_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("clip.wav")
            .to_owned();
        let part = Part::bytes(tokio::fs::read(audio_path).await?)
            .file_name(file_name)
            .mime_str("application/octet-stream")?;
        form = form.part("audio", part);
        has_input = true;
    }

    if let Some(text) = args
        .text
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        form = form.text("source_text", text.to_owned());
        has_input = true;
    }

    if !has_input {
        return Err(CliError::InvalidArgument(
            "provide either --audio <path> or --text <english_text>".to_owned(),
        ));
    }
    Ok(form)
}

async fn run_translate_clip(
    http: &reqwest::Client,
    base_url: &str,
    config: &AppConfig,
    args: &TranslateClipArgs,
) -> Result<(), CliError> {
    let fmt = args.format.trim();
    if fmt != "ogg" && fmt != "wav" {
        return Err(CliError::InvalidArgument(format!(
            "unsupported format `{fmt}`: expected `ogg` or `wav`"
        )));
    }
    let target_lang = TargetLanguage::from_str(args.lang.trim())?;
    let form = build_translate_form(args).await?;

    let resp = http
        .post(format!("{base_url}/api/v1/voice-jobs/render"))
        .query(&[
            ("profile", args.profile.as_str()),
            ("lang", target_lang.code()),
            ("format", fmt),
        ])
        .header(AUTHORIZATION, bearer_header(config))
        .multipart(form)
        .send()
        .await?;
    let resp = ensure_success(resp, "translate-clip").await?;
    let cer_bps = resp
        .headers()
        .get("x-cer-bps")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_owned();
    let engine_used = resp
        .headers()
        .get("x-engine-used")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_owned();
    let rendered = resp.bytes().await?;
    write_output_file(&args.out, &rendered).await?;
    tracing::info!(
        output = %args.out.display(),
        bytes = rendered.len(),
        lang = target_lang.code(),
        engine = %engine_used,
        cer_bps = %cer_bps,
        "wrote rendered voice clip"
    );
    Ok(())
}

async fn run_bench(
    http: &reqwest::Client,
    base_url: &str,
    config: &AppConfig,
    args: &BenchArgs,
) -> Result<(), CliError> {
    let peak_vram_mib_override =
        run_nvidia_smi(&["--query-gpu=memory.used", "--format=csv,noheader,nounits"])
            .await
            .and_then(|raw| {
                raw.lines()
                    .filter_map(|l| l.trim().parse::<u32>().ok())
                    .max()
            });
    let input = RunBenchmarkSuiteInput {
        profile_slug: args.profile.clone(),
        engine_override: args
            .engine
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(TtsEngineKind::from_str)
            .transpose()?,
        manifest_json: tokio::fs::read_to_string(&args.manifest).await?,
        peak_vram_mib_override,
    };
    let resp = http
        .post(format!("{base_url}/api/v1/benchmarks/run"))
        .header(AUTHORIZATION, bearer_header(config))
        .json(&input)
        .send()
        .await?;
    let report: BenchmarkSuiteReport = ensure_success(resp, "benchmark suite")
        .await?
        .json()
        .await?;
    write_output_file(&args.out, report.scorecard_markdown.as_bytes()).await?;
    tracing::info!(
        run_id = %report.run_id,
        engine = %report.engine_tested,
        mean_cer_bps = report.mean_cer_bps.value(),
        speaker_sim_bps = report.mean_speaker_sim_bps.value(),
        p95_latency_ms = report.p95_latency_ms,
        passed = report.passed_all_gates,
        "completed RTX 3090 benchmark suite"
    );
    if !report.passed_all_gates {
        return Err(CliError::BenchmarkGateFailed(format!(
            "benchmark run {} failed M1-M6 gates (see {})",
            report.run_id,
            args.out.display()
        )));
    }
    Ok(())
}

/// Dispatches the parsed [`Cli`] command against `api-server`.
pub async fn execute(cli: &Cli, config: &AppConfig) -> Result<(), CliError> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()?;
    let base_url = config.api_server_internal_url.trim().trim_end_matches('/');
    match &cli.command {
        CliCommand::Doctor => run_doctor(&http, base_url).await,
        CliCommand::HardwareProbe { json } => {
            let specs = probe_system_specs(&config.storage_dir);
            let plan = recommend_hardware_plan(&specs, None, None);
            let payload = serde_json::json!({
                "specs": specs,
                "recommended_plan": plan,
            });
            if *json {
                if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
                    println!("{pretty}");
                }
            } else {
                tracing::info!(
                    tier = %plan.tier,
                    gpu = ?plan.selected_gpu_name,
                    vram_mib = plan.selected_gpu_vram_mib,
                    whisper = %plan.whisper_model_filename,
                    llm = %plan.llm_model_filename,
                    "probed container hardware profile"
                );
            }
            Ok(())
        }
        CliCommand::Enroll(args) => run_enroll(&http, base_url, config, args).await,
        CliCommand::TranslateClip(args) => run_translate_clip(&http, base_url, config, args).await,
        CliCommand::Bench(args) => run_bench(&http, base_url, config, args).await,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use clap::Parser;

    use super::{Cli, CliCommand};

    #[test]
    fn parses_justfile_and_polyglot_text_cli_invocations() {
        assert!(matches!(
            Cli::try_parse_from(["cli", "doctor"]).unwrap().command,
            CliCommand::Doctor
        ));

        let enroll = Cli::try_parse_from([
            "cli",
            "enroll",
            "--slug",
            "default_voice",
            "--wav",
            "/storage/voice_profiles/default.wav",
            "--transcript",
            "Studio clip.",
        ])
        .unwrap();
        assert!(matches!(
            enroll.command,
            CliCommand::Enroll(a) if a.slug == "default_voice"
                && a.wav == Path::new("/storage/voice_profiles/default.wav")
                && a.engine == "cosyvoice_3"
                && a.lang == "zh"
        ));

        let translate_audio = Cli::try_parse_from([
            "cli",
            "translate-clip",
            "--profile",
            "default_voice",
            "--audio",
            "/tmp/in.wav",
            "--out",
            "/tmp/out.wav",
            "--format",
            "wav",
        ])
        .unwrap();
        assert!(matches!(
            translate_audio.command,
            CliCommand::TranslateClip(a) if a.format == "wav"
                && a.lang == "zh"
                && a.audio.as_deref() == Some(Path::new("/tmp/in.wav"))
                && a.text.is_none()
        ));

        let translate_text = Cli::try_parse_from([
            "cli",
            "translate-clip",
            "--profile",
            "default_voice",
            "--text",
            "See you in five minutes!",
            "--lang",
            "es",
            "--out",
            "/tmp/es.ogg",
        ])
        .unwrap();
        assert!(matches!(
            translate_text.command,
            CliCommand::TranslateClip(a) if a.lang == "es"
                && a.audio.is_none()
                && a.text.as_deref() == Some("See you in five minutes!")
        ));

        let bench = Cli::try_parse_from([
            "cli",
            "bench",
            "--manifest",
            "/evals/golden_manifest.json",
            "--out",
            "/storage/reports/scorecard.md",
        ])
        .unwrap();
        assert!(
            matches!(bench.command, CliCommand::Bench(a) if a.profile == "default_voice" && a.engine.is_none())
        );
    }
}
