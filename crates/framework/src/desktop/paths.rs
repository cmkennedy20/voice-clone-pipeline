//! Deterministic path layout for installed (`%LOCALAPPDATA%\\VoiceCloneStudio`) and portable
//! (`<bundle>\\data`) deployments using typed [`DesktopLauncherEnv`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::DesktopLauncherEnv;

/// Canonical directory and binary layout for a `VoiceCloneStudio` installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopPaths {
    pub bundle_root: PathBuf,
    pub data_root: PathBuf,
    pub config_dir: PathBuf,
    pub env_file: PathBuf,
    pub state_file: PathBuf,
    pub stop_signal_file: PathBuf,
    pub pgdata_dir: PathBuf,
    pub models_dir: PathBuf,
    pub storage_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub web_dist_dir: PathBuf,
    pub tts_worker_dir: PathBuf,
    pub evals_dir: PathBuf,
    pub runtime_pgsql_bin: PathBuf,
    pub runtime_llama_dir: PathBuf,
    pub runtime_python_dir: PathBuf,
    pub runtime_ffmpeg_dir: PathBuf,
    pub portable: bool,
}

impl DesktopPaths {
    /// Ensures all mutable runtime directories (`config`, `models`, `storage`, `logs`) exist.
    pub fn ensure_mutable_dirs(&self) -> Result<(), std::io::Error> {
        for dir in [
            &self.data_root,
            &self.config_dir,
            &self.models_dir,
            &self.storage_dir,
            &self.logs_dir,
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    /// Resolves a sibling binary (`api-server`, `discord-worker`, `cli`) inside `bin/` or beside launcher.
    #[must_use]
    pub fn resolve_rust_binary(&self, stem: &str) -> PathBuf {
        let exe_name = if cfg!(windows) {
            format!("{stem}.exe")
        } else {
            stem.to_owned()
        };
        for candidate in [
            self.bundle_root.join("bin").join(&exe_name),
            self.bundle_root.join(&exe_name),
            self.bundle_root
                .join("target")
                .join("debug")
                .join(&exe_name),
            self.bundle_root
                .join("target")
                .join("release")
                .join(&exe_name),
        ] {
            if candidate.is_file() {
                return candidate;
            }
        }
        self.bundle_root.join("bin").join(exe_name)
    }

    /// Resolves `llama-server.exe` / `llama-server` inside `runtime/llama` or `PATH`.
    #[must_use]
    pub fn resolve_llama_server(&self) -> PathBuf {
        let name = if cfg!(windows) {
            "llama-server.exe"
        } else {
            "llama-server"
        };
        let bundled = self.runtime_llama_dir.join(name);
        if bundled.is_file() {
            bundled
        } else {
            PathBuf::from(name)
        }
    }

    /// Resolves the standalone `Python` interpreter inside `runtime/python` or `PATH`.
    #[must_use]
    pub fn resolve_python(&self) -> PathBuf {
        for candidate in [
            self.runtime_python_dir.join("python.exe"),
            self.runtime_python_dir.join("bin").join("python3"),
            self.runtime_python_dir.join("Scripts").join("python.exe"),
        ] {
            if candidate.is_file() {
                return candidate;
            }
        }
        if cfg!(windows) {
            PathBuf::from("python.exe")
        } else {
            PathBuf::from("python3")
        }
    }
}

/// Discovers the bundle root directory from the running executable path.
#[must_use]
pub fn detect_bundle_root() -> PathBuf {
    let Ok(exe) = std::env::current_exe() else {
        return PathBuf::from(".");
    };
    let Some(exe_dir) = exe.parent() else {
        return PathBuf::from(".");
    };
    if exe_dir.file_name().and_then(|n| n.to_str()) == Some("bin")
        && let Some(parent) = exe_dir.parent()
    {
        return parent.to_path_buf();
    }
    exe_dir.to_path_buf()
}

/// Pure deterministic resolver mapping `bundle_root` + [`DesktopLauncherEnv`] to [`DesktopPaths`].
#[must_use]
pub fn resolve_desktop_paths(
    bundle_root: &Path,
    launcher_env: &DesktopLauncherEnv,
    force_portable: bool,
) -> DesktopPaths {
    let marker_portable = bundle_root.join("portable.mode").is_file();
    let portable = force_portable || launcher_env.portable_flag || marker_portable;

    let data_root = if let Some(explicit_home) = &launcher_env.home_override {
        explicit_home.clone()
    } else if portable {
        bundle_root.join("data")
    } else if let Some(local_appdata) = &launcher_env.local_appdata {
        local_appdata.join("VoiceCloneStudio")
    } else if let Some(xdg_data) = &launcher_env.xdg_data_home {
        xdg_data.join("voice-clone-studio")
    } else if let Some(home) = &launcher_env.user_home {
        home.join(".voice-clone-studio")
    } else {
        bundle_root.join("data")
    };

    let bundled_models = bundle_root.join("models");
    let models_dir = if bundled_models.is_dir() {
        bundled_models
    } else {
        data_root.join("models")
    };

    let config_dir = data_root.join("config");
    let runtime_dir = bundle_root.join("runtime");

    DesktopPaths {
        bundle_root: bundle_root.to_path_buf(),
        env_file: config_dir.join("studio.env"),
        state_file: config_dir.join("supervisor-state.json"),
        stop_signal_file: config_dir.join("supervisor.stop"),
        pgdata_dir: data_root.join("pgdata"),
        models_dir,
        storage_dir: data_root.join("storage"),
        logs_dir: data_root.join("logs"),
        config_dir,
        data_root,
        web_dist_dir: bundle_root.join("web").join("dist"),
        tts_worker_dir: bundle_root.join("sidecars").join("tts-worker"),
        evals_dir: bundle_root.join("evals"),
        runtime_pgsql_bin: runtime_dir.join("pgsql").join("bin"),
        runtime_llama_dir: runtime_dir.join("llama"),
        runtime_python_dir: runtime_dir.join("python"),
        runtime_ffmpeg_dir: runtime_dir.join("ffmpeg"),
        portable,
    }
}
