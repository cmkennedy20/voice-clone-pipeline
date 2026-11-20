//! Filesystem loader, path-traversal sanitizer, and `MIME` resolver for bundled Studio `SPA` assets.

use std::path::{Component, Path, PathBuf};

/// Resolved static asset payload ready for `HTTP` transmission.
#[derive(Debug, Clone)]
pub struct LoadedSpaAsset {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
    pub cache_control: &'static str,
}

/// Classification of non-200 `SPA` lookup outcomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaAssetError {
    PathTraversal,
    NotFound,
}

fn content_type_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn sanitize_relative_path(uri_path: &str) -> Result<PathBuf, SpaAssetError> {
    let trimmed = uri_path.trim_start_matches('/');
    if trimmed.is_empty() {
        return Ok(PathBuf::from("index.html"));
    }
    let mut clean = PathBuf::new();
    for comp in Path::new(trimmed).components() {
        match comp {
            Component::Normal(seg) => clean.push(seg),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(SpaAssetError::PathTraversal);
            }
        }
    }
    Ok(if clean.as_os_str().is_empty() {
        PathBuf::from("index.html")
    } else {
        clean
    })
}

/// Loads a static `SPA` asset or `index.html` client-route fallback from `dist_dir`.
pub async fn load_spa_asset(
    dist_dir: Option<&Path>,
    req_path: &str,
) -> Result<LoadedSpaAsset, SpaAssetError> {
    if req_path.starts_with("/api/")
        || req_path.starts_with("/v1/")
        || req_path.starts_with("/health")
    {
        return Err(SpaAssetError::NotFound);
    }
    let Some(root) = dist_dir else {
        return Err(SpaAssetError::NotFound);
    };

    let rel_path = sanitize_relative_path(req_path)?;
    let candidate = root.join(&rel_path);
    if let Ok(bytes) = tokio::fs::read(&candidate).await {
        let cache_control = if req_path.starts_with("/assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return Ok(LoadedSpaAsset {
            bytes,
            content_type: content_type_for_path(&candidate),
            cache_control,
        });
    }

    if rel_path.extension().is_none() {
        let index_path = root.join("index.html");
        if let Ok(bytes) = tokio::fs::read(&index_path).await {
            return Ok(LoadedSpaAsset {
                bytes,
                content_type: "text/html; charset=utf-8",
                cache_control: "no-cache",
            });
        }
    }

    Err(SpaAssetError::NotFound)
}
