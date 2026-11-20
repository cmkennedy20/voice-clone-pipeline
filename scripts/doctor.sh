#!/usr/bin/env bash
# ==============================================================================
# Voice Clone Pipeline & Studio — WSL2 / Linux Readiness Doctor (doctor.sh)
# ==============================================================================
# Verifies WSL2 Docker Desktop integration, NVIDIA WSL2 GPU paravirtualization
# (/usr/lib/wsl/lib/nvidia-smi & /dev/dxg), .env configuration, downloaded
# HuggingFace weights, and live HTTP readiness (/health/ready + Studio UI).
#
# Usage:
#   make doctor
#   make doctor ARGS="--json"
# ==============================================================================
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

ENV_FILE=".env"
JSON_OUTPUT=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --env-file) ENV_FILE="$2"; shift 2 ;;
    --json) JSON_OUTPUT=true; shift ;;
    -h|--help)
      echo "Usage: bash scripts/doctor.sh [--env-file .env] [--json]"
      exit 0
      ;;
    *) echo "Unknown argument: $1" >&2; exit 2 ;;
  esac
done

read_env_key() {
  local key="$1"
  [[ -f "$ENV_FILE" ]] || return 1
  local line val
  line="$(grep -E "^[[:space:]]*${key}=" "$ENV_FILE" | tail -n1 || true)"
  [[ -n "$line" ]] || return 1
  val="${line#*=}"
  val="${val%$'\r'}"
  if [[ "$val" == \"*\" && "$val" == *\" ]]; then val="${val:1:${#val}-2}"; fi
  if [[ "$val" == \'*\' && "$val" == *\' ]]; then val="${val:1:${#val}-2}"; fi
  printf '%s' "$val"
}

IS_WSL2=false
if [[ -f /proc/sys/fs/binfmt_misc/WSLInterop ]] || grep -qiE "microsoft|wsl" /proc/version 2>/dev/null; then
  IS_WSL2=true
fi

DOCKER_OK=false
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
  DOCKER_OK=true
fi

ENV_OK=false
OWNER_TOKEN_OK=false
if [[ -f "$ENV_FILE" ]]; then
  ENV_OK=true
  tok="$(read_env_key OWNER_API_BEARER_TOKEN || true)"
  if [[ -n "$tok" && "$tok" != replace-with-* && "$tok" != "dev-owner-bearer-token-change-me" ]]; then
    OWNER_TOKEN_OK=true
  fi
fi

COMPUTE_MODE="$(read_env_key COMPUTE_MODE || echo "gpu")"
MODELS_DIR="$(read_env_key STUDIO_MODELS_DIR || echo "./models")"
WHISPER_FILE="$(read_env_key WHISPER_MODEL_FILE || echo "ggml-large-v3.bin")"
LLM_FILE="$(read_env_key LLM_MODEL_FILE || echo "Qwen3-8B-Q8_0.gguf")"
WEB_PORT="$(read_env_key WEB_UI_PORT || echo "3000")"
TLS_PORT="$(read_env_key WEB_UI_TLS_PORT || echo "3443")"
API_PORT="$(read_env_key API_BIND_PORT || echo "8080")"
LAN_IP="$(read_env_key LAN_HOST_IP || echo "127.0.0.1")"

GPU_VISIBLE=false
if command -v nvidia-smi >/dev/null 2>&1 && nvidia-smi -L >/dev/null 2>&1; then
  GPU_VISIBLE=true
elif [[ -x "/usr/lib/wsl/lib/nvidia-smi" ]] && /usr/lib/wsl/lib/nvidia-smi -L >/dev/null 2>&1; then
  GPU_VISIBLE=true
fi

MODELS_OK=false
if [[ -f "${MODELS_DIR}/whisper/${WHISPER_FILE}" && -f "${MODELS_DIR}/llm/${LLM_FILE}" ]]; then
  MODELS_OK=true
fi

API_HEALTHY=false
if command -v curl >/dev/null 2>&1 && curl -fsS --max-time 3 "http://127.0.0.1:${API_PORT}/health/ready" >/dev/null 2>&1; then
  API_HEALTHY=true
fi

WEB_HEALTHY=false
if command -v curl >/dev/null 2>&1 && curl -fsS --max-time 3 "http://127.0.0.1:${WEB_PORT}/" >/dev/null 2>&1; then
  WEB_HEALTHY=true
fi

OVERALL_READY=false
if [[ "$DOCKER_OK" == "true" && "$ENV_OK" == "true" && "$OWNER_TOKEN_OK" == "true" && "$API_HEALTHY" == "true" && "$WEB_HEALTHY" == "true" ]]; then
  OVERALL_READY=true
fi

if [[ "$JSON_OUTPUT" == "true" ]]; then
  python3 - "$OVERALL_READY" "$IS_WSL2" "$DOCKER_OK" "$GPU_VISIBLE" "$ENV_OK" "$OWNER_TOKEN_OK" \
    "$MODELS_OK" "$API_HEALTHY" "$WEB_HEALTHY" "$COMPUTE_MODE" "$WEB_PORT" "$TLS_PORT" "$API_PORT" "$LAN_IP" <<'PY'
import json, sys
(
    ready, is_wsl2, docker_ok, gpu_ok, env_ok, token_ok,
    models_ok, api_ok, web_ok, compute_mode, web_port, tls_port, api_port, lan_ip,
) = sys.argv[1:]
remediations = []
if docker_ok != "true":
    if is_wsl2 == "true":
        remediations.append("Start Docker Desktop on Windows and enable Settings -> Resources -> WSL Integration for this distro.")
    else:
        remediations.append("Start dockerd and verify 'docker compose version'.")
if compute_mode == "gpu" and gpu_ok != "true":
    remediations.append("NVIDIA GPU not visible in WSL2; install Windows NVIDIA driver or run 'make setup ARGS=\"--tier cpu --non-interactive\"'.")
if env_ok != "true" or token_ok != "true":
    remediations.append("Run 'make setup' to initialize .env and secrets.")
if models_ok != "true":
    remediations.append("Run 'make pull-models' or 'make up' to download HuggingFace model weights.")
if api_ok != "true" or web_ok != "true":
    remediations.append("Run 'make up' and inspect 'make logs' until api-server and web-ui report healthy.")
print(json.dumps({
    "ready": ready == "true",
    "wsl2": is_wsl2 == "true",
    "compute_mode": compute_mode,
    "checks": {
        "docker_daemon": docker_ok == "true",
        "nvidia_gpu_visible": gpu_ok == "true",
        "env_file": env_ok == "true",
        "owner_token_configured": token_ok == "true",
        "models_downloaded": models_ok == "true",
        "api_ready": api_ok == "true",
        "web_ui_ready": web_ok == "true",
    },
    "urls": {
        "windows_browser_http": f"http://localhost:{web_port}",
        "lan_https_mic": f"https://{lan_ip}:{tls_port}",
        "api_ready": f"http://localhost:{api_port}/health/ready",
        "agent_manifest": f"http://localhost:{api_port}/api/v1/agent/manifest",
    },
    "remediations": remediations,
}, indent=2))
PY
else
  printf '\n  Voice Clone Studio · WSL2 / Docker System Doctor (%s mode)\n' "$COMPUTE_MODE"
  printf '  ──────────────────────────────────────────────────────────\n'
  printf '  [%s] Docker daemon & Compose v2\n' "$( [[ "$DOCKER_OK" == "true" ]] && echo "OK" || echo "!!" )"
  if [[ "$COMPUTE_MODE" == "gpu" ]]; then
    printf '  [%s] NVIDIA CUDA GPU visible in WSL2\n' "$( [[ "$GPU_VISIBLE" == "true" ]] && echo "OK" || echo "!!" )"
  fi
  printf '  [%s] Environment file (%s) & owner secret\n' "$( [[ "$ENV_OK" == "true" && "$OWNER_TOKEN_OK" == "true" ]] && echo "OK" || echo "!!" )" "$ENV_FILE"
  printf '  [%s] Model weights (%s / %s)\n' "$( [[ "$MODELS_OK" == "true" ]] && echo "OK" || echo "--" )" "$WHISPER_FILE" "$LLM_FILE"
  printf '  [%s] API server ready   (http://localhost:%s/health/ready)\n' "$( [[ "$API_HEALTHY" == "true" ]] && echo "OK" || echo "--" )" "$API_PORT"
  printf '  [%s] Windows Browser UI (http://localhost:%s)\n\n' "$( [[ "$WEB_HEALTHY" == "true" ]] && echo "OK" || echo "--" )" "$WEB_PORT"
fi
