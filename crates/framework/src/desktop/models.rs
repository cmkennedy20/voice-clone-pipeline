//! Pure-Rust model readiness inspector and resumable `HTTP Range` downloader (`416`-safe).

use std::path::Path;

use domain::hardware::ModelArtifactSpec;
use reqwest::header::{AUTHORIZATION, RANGE};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use super::DesktopError;

/// On-disk status of a single model weight artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelFileStatus {
    pub component: String,
    pub relative_path: String,
    pub full_path: String,
    pub required: bool,
    pub present: bool,
    pub bytes_on_disk: u64,
    pub approx_size_mib: u64,
}

/// Aggregate model readiness summary for CLI (`models status --json`) and Studio UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelsStatusReport {
    pub models_dir: String,
    pub all_required_present: bool,
    pub missing_required_count: usize,
    pub files: Vec<ModelFileStatus>,
}

/// Summary returned after `models pull` completes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelsPullReport {
    pub models_dir: String,
    pub downloaded_files: usize,
    pub skipped_existing_files: usize,
    pub total_bytes_written: u64,
}

/// Inspects `models_dir` synchronously against `artifacts`.
#[must_use]
pub fn inspect_models_status(
    models_dir: &Path,
    artifacts: &[ModelArtifactSpec],
) -> ModelsStatusReport {
    let mut files = Vec::with_capacity(artifacts.len());
    let mut missing_required_count = 0_usize;

    for spec in artifacts {
        let dest = models_dir.join(&spec.relative_path);
        let meta = std::fs::metadata(&dest).ok();
        let bytes_on_disk = meta.as_ref().map_or(0, std::fs::Metadata::len);
        let present = meta.is_some_and(|m| m.is_file() && m.len() > 0);
        if spec.required && !present {
            missing_required_count += 1;
        }
        files.push(ModelFileStatus {
            component: spec.component.clone(),
            relative_path: spec.relative_path.clone(),
            full_path: dest.to_string_lossy().into_owned(),
            required: spec.required,
            present,
            bytes_on_disk,
            approx_size_mib: spec.approx_size_mib,
        });
    }

    ModelsStatusReport {
        models_dir: models_dir.to_string_lossy().into_owned(),
        all_required_present: missing_required_count == 0,
        missing_required_count,
        files,
    }
}

async fn send_download_request(
    http: &reqwest::Client,
    url: &str,
    hf_token: Option<&str>,
    range_start: u64,
) -> Result<reqwest::Response, reqwest::Error> {
    let mut req = http.get(url);
    if let Some(tok) = hf_token.map(str::trim).filter(|s| !s.is_empty()) {
        req = req.header(AUTHORIZATION, format!("Bearer {tok}"));
    }
    if range_start > 0 {
        req = req.header(RANGE, format!("bytes={range_start}-"));
    }
    req.send().await
}

async fn download_single_artifact(
    http: &reqwest::Client,
    models_dir: &Path,
    spec: &ModelArtifactSpec,
    hf_token: Option<&str>,
) -> Result<u64, DesktopError> {
    let dest = models_dir.join(&spec.relative_path);
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part_path = dest.with_extension(format!(
        "{}.part",
        dest.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("download")
    ));

    let mut existing_len = tokio::fs::metadata(&part_path)
        .await
        .ok()
        .map_or(0, |m| m.len());

    let mut resp = send_download_request(http, &spec.download_url, hf_token, existing_len).await?;
    if existing_len > 0 && resp.status().as_u16() == 416 {
        let _ = tokio::fs::remove_file(&part_path).await;
        existing_len = 0;
        resp = send_download_request(http, &spec.download_url, hf_token, 0).await?;
    }

    let status = resp.status();
    if !status.is_success() {
        let detail = resp.text().await.unwrap_or_default();
        return Err(DesktopError::DownloadStatus {
            artifact: spec.relative_path.clone(),
            status: status.as_u16(),
            detail,
        });
    }

    let append_mode = existing_len > 0 && status.as_u16() == 206;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append_mode)
        .truncate(!append_mode)
        .open(&part_path)
        .await?;

    let mut written = 0_u64;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk).await?;
        written = written.saturating_add(u64::try_from(chunk.len()).unwrap_or(0));
    }
    file.flush().await?;
    drop(file);

    if dest.is_file() && cfg!(windows) {
        let _ = tokio::fs::remove_file(&dest).await;
    }
    tokio::fs::rename(&part_path, &dest).await?;
    Ok(written)
}

/// Downloads any missing artifacts into `models_dir` with `HTTP Range` resume and `.part` atomic rename.
pub async fn pull_missing_models(
    http: &reqwest::Client,
    models_dir: &Path,
    artifacts: &[ModelArtifactSpec],
    hf_token: Option<&str>,
) -> Result<ModelsPullReport, DesktopError> {
    let mut downloaded_files = 0_usize;
    let mut skipped_existing_files = 0_usize;
    let mut total_bytes_written = 0_u64;

    for spec in artifacts {
        let dest = models_dir.join(&spec.relative_path);
        let already_ok = tokio::fs::metadata(&dest)
            .await
            .is_ok_and(|m| m.is_file() && m.len() > 0);
        if already_ok {
            skipped_existing_files += 1;
            continue;
        }

        tracing::info!(
            component = %spec.component,
            relative_path = %spec.relative_path,
            approx_size_mib = spec.approx_size_mib,
            "downloading model weight artifact"
        );
        let bytes = download_single_artifact(http, models_dir, spec, hf_token).await?;
        downloaded_files += 1;
        total_bytes_written = total_bytes_written.saturating_add(bytes);
    }

    Ok(ModelsPullReport {
        models_dir: models_dir.to_string_lossy().into_owned(),
        downloaded_files,
        skipped_existing_files,
        total_bytes_written,
    })
}
