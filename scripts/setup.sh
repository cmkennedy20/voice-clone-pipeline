#!/usr/bin/env bash
# ==============================================================================
# Voice Clone Pipeline & Studio — WSL2 on Windows / Linux Bootstrap (setup.sh)
# ==============================================================================
# Designed for Ubuntu/Debian running inside WSL2 on Windows (with Docker Desktop
# WSL2 integration or native dockerd inside WSL2), as well as native Linux.
#
# Responsibilities:
#   1. Detects WSL2 environment, checks WSL2 VM RAM (/proc/meminfo) & /mnt/c 9P
#      mount performance, and verifies Docker + Docker Compose v2 integration.
#   2. Probes NVIDIA GPUs via `nvidia-smi` or WSL2's `/usr/lib/wsl/lib/nvidia-smi`
#      (plus `/dev/dxg`), selecting the highest-VRAM GPU on multi-GPU desktops.
#   3. Maps VRAM to an optimal model tier (`enthusiast` >=16GB, `mainstream` 8-15GB,
#      `entry` 4-7GB, or `cpu` fallback when no NVIDIA GPU is available).
#   4. Resolves the Windows host's real LAN IPv4 address (for LAN HTTPS mic on :3443,
#      while `http://localhost:3000` works directly in Windows Chrome/Edge!).
#   5. Creates `./models` & `./local_storage` bind-mount directories and writes
#      `.env` idempotently WITHOUT overwriting existing secrets.
#
# Usage:
#   make setup
#   make setup ARGS="--non-interactive --json"
#   bash scripts/setup.sh --tier mainstream --non-interactive --json
# ==============================================================================
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

ENV_FILE=".env"
TIER_OVERRIDE="auto"
GPU_ID_OVERRIDE=""
MODELS_DIR_OVERRIDE=""
STORAGE_DIR_OVERRIDE=""
NON_INTERACTIVE=false
JSON_OUTPUT=false

usage() {
  cat <<'EOF'
Usage: bash scripts/setup.sh [options]

Options:
  --non-interactive       Never prompt; auto-detect WSL2 hardware, ports, and secrets
  --env-file <path>       Target dotenv file path (default: .env)
  --tier <tier>           Hardware tier: auto | enthusiast | mainstream | entry | cpu
  --gpu-id <id>           Explicit NVIDIA GPU index (0) or UUID (GPU-...)
  --models-dir <path>     Host directory inside WSL2 for ~6-18 GB of weights (default: ./models)
  --storage-dir <path>    Host directory inside WSL2 for profiles & renders (default: ./local_storage)
  --json                  Emit machine-readable JSON summary to stdout
  -h, --help              Show this help text
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --non-interactive) NON_INTERACTIVE=true; shift ;;
    --env-file) ENV_FILE="$2"; shift 2 ;;
    --tier) TIER_OVERRIDE="$2"; shift 2 ;;
    --gpu-id) GPU_ID_OVERRIDE="$2"; shift 2 ;;
    --models-dir) MODELS_DIR_OVERRIDE="$2"; shift 2 ;;
    --storage-dir) STORAGE_DIR_OVERRIDE="$2"; shift 2 ;;
    --json) JSON_OUTPUT=true; NON_INTERACTIVE=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

WARNINGS=()

log_info() {
  if [[ "$JSON_OUTPUT" == "false" ]]; then
    printf '  [setup] %s\n' "$*"
  fi
}

log_warn() {
  WARNINGS+=("$*")
  if [[ "$JSON_OUTPUT" == "false" ]]; then
    printf '  [warn]  %s\n' "$*" >&2
  fi
}

read_env_key() {
  local key="$1" file="${2:-$ENV_FILE}"
  [[ -f "$file" ]] || return 1
  local line val
  line="$(grep -E "^[[:space:]]*${key}=" "$file" | tail -n1 || true)"
  [[ -n "$line" ]] || return 1
  val="${line#*=}"
  val="${val%$'\r'}"
  if [[ "$val" == \"*\" && "$val" == *\" ]]; then
    val="${val:1:${#val}-2}"
  elif [[ "$val" == \'*\' && "$val" == *\' ]]; then
    val="${val:1:${#val}-2}"
  fi
  printf '%s' "$val"
}

upsert_env_key() {
  local key="$1" val="$2" file="${3:-$ENV_FILE}"
  local tmp formatted
  formatted="$val"
  if [[ "$val" =~ [[:space:]\;\$\#\"\'] ]]; then
    formatted="\"${val//\"/\\\"}\""
  fi
  tmp="$(mktemp)"
  if [[ -f "$file" ]]; then
    awk -v k="$key" -v fv="$formatted" '
      BEGIN { done = 0 }
      $0 ~ "^[[:space:]]*" k "=" {
        if (!done) { print k "=" fv; done = 1 }
        next
      }
      { print }
      END {
        if (!done) { print k "=" fv }
      }
    ' "$file" > "$tmp"
  else
    printf '%s=%s\n' "$key" "$formatted" > "$tmp"
  fi
  cat "$tmp" > "$file"
  rm -f "$tmp"
}

gen_hex() {
  local bytes="${1:-32}"
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex "$bytes"
  elif [[ -r /dev/urandom ]]; then
    od -An -N"$bytes" -tx1 /dev/urandom | tr -d ' \n'
  else
    python3 -c "import secrets; print(secrets.token_hex($bytes))"
  fi
}

# 1. Detect WSL2 environment, filesystem mount speed, and WSL2 VM RAM cap
IS_WSL2=false
if [[ -f /proc/sys/fs/binfmt_misc/WSLInterop ]] || grep -qiE "microsoft|wsl" /proc/version 2>/dev/null; then
  IS_WSL2=true
fi

RAM_TOTAL_MIB=0
if [[ -r /proc/meminfo ]]; then
  RAM_TOTAL_MIB="$(awk '/^MemTotal:/ {print int($2 / 1024)}' /proc/meminfo)"
fi

if [[ "$IS_WSL2" == "true" ]]; then
  log_info "Detected WSL2 on Windows (WSL2 VM RAM: ${RAM_TOTAL_MIB} MiB)."
  if [[ "$REPO_ROOT" =~ ^/mnt/[a-zA-Z]/ ]]; then
    log_warn "Repo is mounted on Windows DrvFs ($REPO_ROOT). For 10x faster Docker builds & model loading in WSL2, clone onto native Linux ext4 (e.g. ~/voice-clone-pipeline) or pass --models-dir \$HOME/.cache/voice-clone-models."
  fi
  if (( RAM_TOTAL_MIB > 0 && RAM_TOTAL_MIB < 10240 )); then
    log_warn "WSL2 VM memory is capped at ${RAM_TOTAL_MIB} MiB (<10 GiB). Consider adding '[wsl2]\nmemory=16GB' in %USERPROFILE%\\.wslconfig on Windows and running 'wsl --shutdown'."
  fi
fi

detect_lan_ip() {
  local ip=""
  # On WSL2, query the Windows host's real Wi-Fi/Ethernet IPv4 rather than WSL2's internal 172.x NAT subnet.
  if [[ "$IS_WSL2" == "true" ]] && command -v powershell.exe >/dev/null 2>&1; then
    ip="$(powershell.exe -NoProfile -NonInteractive -Command \
      "(Get-NetIPAddress -AddressFamily IPv4 | Where-Object { \$_.IPAddress -notlike '127.*' -and \$_.IPAddress -notlike '169.254.*' -and \$_.InterfaceAlias -notmatch 'WSL|vEthernet|Docker|Loopback' } | Select-Object -First 1).IPAddress" \
      2>/dev/null | tr -d '\r\n ' || true)"
  fi
  if [[ -z "$ip" ]] && command -v ip >/dev/null 2>&1; then
    ip="$(ip route get 1.1.1.1 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="src") {print $(i+1); exit}}' || true)"
  fi
  if [[ -z "$ip" ]] && command -v hostname >/dev/null 2>&1; then
    ip="$(hostname -I 2>/dev/null | awk '{print $1}' || true)"
  fi
  printf '%s' "${ip:-127.0.0.1}"
}

# 2. Check Docker CLI + Compose v2 inside WSL2
DOCKER_OK=false
COMPOSE_OK=false
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  DOCKER_OK=true
fi
if command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
  COMPOSE_OK=true
fi

if [[ "$DOCKER_OK" == "false" || "$COMPOSE_OK" == "false" ]]; then
  if [[ "$IS_WSL2" == "true" ]]; then
    log_warn "Docker is not reachable inside WSL2. Start Docker Desktop on Windows and enable Settings -> Resources -> WSL Integration for this WSL2 distro."
  else
    log_warn "Docker daemon or 'docker compose' v2 is not running."
  fi
fi

# 3. Seed ENV_FILE from .env.example if absent
ENV_CREATED=false
if [[ ! -f "$ENV_FILE" ]]; then
  mkdir -p "$(dirname "$ENV_FILE")"
  (umask 077 && cp .env.example "$ENV_FILE")
  ENV_CREATED=true
  log_info "Created $ENV_FILE from .env.example (mode 0600)."
else
  log_info "Updating existing $ENV_FILE (preserving configured secrets & custom settings)."
fi

# 4. Probe NVIDIA GPUs (checks PATH + WSL2's /usr/lib/wsl/lib/nvidia-smi)
NVIDIA_SMI_BIN=""
if command -v nvidia-smi >/dev/null 2>&1; then
  NVIDIA_SMI_BIN="nvidia-smi"
elif [[ -x "/usr/lib/wsl/lib/nvidia-smi" ]]; then
  NVIDIA_SMI_BIN="/usr/lib/wsl/lib/nvidia-smi"
elif command -v nvidia-smi.exe >/dev/null 2>&1; then
  NVIDIA_SMI_BIN="nvidia-smi.exe"
fi

DETECTED_GPU_NAME="none"
DETECTED_GPU_VRAM_MIB=0
DETECTED_GPU_ID="0"
DETECTED_GPU_COUNT=0

if [[ -n "$NVIDIA_SMI_BIN" ]]; then
  while IFS=',' read -r idx uuid name vram; do
    idx="$(printf '%s' "$idx" | tr -d ' \r')"
    uuid="$(printf '%s' "$uuid" | tr -d ' \r')"
    name="$(printf '%s' "$name" | sed 's/^[[:space:]]*//;s/[[:space:]\r]*$//')"
    vram="$(printf '%s' "$vram" | tr -dc '0-9')"
    [[ -n "$vram" ]] || vram=0
    DETECTED_GPU_COUNT=$((DETECTED_GPU_COUNT + 1))
    if (( vram > DETECTED_GPU_VRAM_MIB )); then
      DETECTED_GPU_VRAM_MIB="$vram"
      DETECTED_GPU_NAME="$name"
      if [[ "$DETECTED_GPU_COUNT" -gt 1 && -n "$uuid" ]]; then
        DETECTED_GPU_ID="$uuid"
      else
        DETECTED_GPU_ID="${idx:-0}"
      fi
    fi
  done < <("$NVIDIA_SMI_BIN" --query-gpu=index,uuid,name,memory.total --format=csv,noheader,nounits 2>/dev/null || true)
fi

if [[ "$IS_WSL2" == "true" && -e /dev/dxg && "$DETECTED_GPU_COUNT" -eq 0 ]]; then
  log_warn "WSL2 DirectX GPU device (/dev/dxg) exists, but nvidia-smi returned no NVIDIA GPUs. Falling back to CPU mode unless an NVIDIA Windows driver is installed."
fi

# 5. Select hardware tier
SELECTED_TIER="$TIER_OVERRIDE"
if [[ "$SELECTED_TIER" == "auto" ]]; then
  if (( DETECTED_GPU_VRAM_MIB >= 15872 )); then
    SELECTED_TIER="enthusiast"
  elif (( DETECTED_GPU_VRAM_MIB >= 7936 )); then
    SELECTED_TIER="mainstream"
  elif (( DETECTED_GPU_VRAM_MIB >= 3840 )); then
    SELECTED_TIER="entry"
  else
    SELECTED_TIER="cpu"
  fi
fi

EXISTING_GPU_ID="$(read_env_key NVIDIA_GPU_ID || true)"
if [[ -n "$GPU_ID_OVERRIDE" ]]; then
  FINAL_GPU_ID="$GPU_ID_OVERRIDE"
elif [[ "$ENV_CREATED" == "false" && -n "$EXISTING_GPU_ID" && "$EXISTING_GPU_ID" != "0" ]]; then
  FINAL_GPU_ID="$EXISTING_GPU_ID"
else
  FINAL_GPU_ID="$DETECTED_GPU_ID"
fi

case "$SELECTED_TIER" in
  enthusiast)
    COMPUTE_MODE="gpu"
    COMPUTE_BACKEND="cuda"
    TORCH_COMPUTE="cu121"
    WHISPER_USE_GPU="true"
    TTS_CUDA_VISIBLE_DEVICES="0"
    WHISPER_MODEL_FILE="ggml-large-v3.bin"
    LLM_HF_REPO="Qwen/Qwen3-8B-GGUF"
    LLM_MODEL_FILE="Qwen3-8B-Q8_0.gguf"
    LLM_GPU_LAYERS="99"
    PULL_COSYVOICE="true"
    PREWARM_ENGINES="cosyvoice_3,xtts_v2"
    ;;
  mainstream)
    COMPUTE_MODE="gpu"
    COMPUTE_BACKEND="cuda"
    TORCH_COMPUTE="cu121"
    WHISPER_USE_GPU="true"
    TTS_CUDA_VISIBLE_DEVICES="0"
    WHISPER_MODEL_FILE="ggml-medium.bin"
    LLM_HF_REPO="Qwen/Qwen3-8B-GGUF"
    LLM_MODEL_FILE="Qwen3-8B-Q4_K_M.gguf"
    LLM_GPU_LAYERS="99"
    PULL_COSYVOICE="true"
    PREWARM_ENGINES="xtts_v2"
    ;;
  entry)
    COMPUTE_MODE="gpu"
    COMPUTE_BACKEND="cuda"
    TORCH_COMPUTE="cu121"
    WHISPER_USE_GPU="true"
    TTS_CUDA_VISIBLE_DEVICES="0"
    WHISPER_MODEL_FILE="ggml-small.bin"
    LLM_HF_REPO="Qwen/Qwen3-4B-GGUF"
    LLM_MODEL_FILE="Qwen3-4B-Q4_K_M.gguf"
    LLM_GPU_LAYERS="28"
    PULL_COSYVOICE="false"
    PREWARM_ENGINES="xtts_v2"
    ;;
  cpu)
    COMPUTE_MODE="cpu"
    COMPUTE_BACKEND="cpu"
    TORCH_COMPUTE="cpu"
    WHISPER_USE_GPU="false"
    TTS_CUDA_VISIBLE_DEVICES=""
    WHISPER_MODEL_FILE="ggml-small.bin"
    LLM_HF_REPO="Qwen/Qwen3-4B-GGUF"
    LLM_MODEL_FILE="Qwen3-4B-Q4_K_M.gguf"
    LLM_GPU_LAYERS="0"
    PULL_COSYVOICE="false"
    PREWARM_ENGINES=""
    ;;
  *)
    echo "Invalid --tier '$SELECTED_TIER' (expected auto|enthusiast|mainstream|entry|cpu)" >&2
    exit 2
    ;;
esac

if [[ "$ENV_CREATED" == "true" || "$TIER_OVERRIDE" != "auto" ]]; then
  upsert_env_key COMPUTE_MODE "$COMPUTE_MODE"
  upsert_env_key COMPUTE_BACKEND "$COMPUTE_BACKEND"
  upsert_env_key TORCH_COMPUTE "$TORCH_COMPUTE"
  upsert_env_key WHISPER_USE_GPU "$WHISPER_USE_GPU"
  upsert_env_key TTS_CUDA_VISIBLE_DEVICES "$TTS_CUDA_VISIBLE_DEVICES"
  upsert_env_key WHISPER_MODEL_FILE "$WHISPER_MODEL_FILE"
  upsert_env_key WHISPER_MODEL_PATH "/models/whisper/${WHISPER_MODEL_FILE}"
  upsert_env_key LLM_HF_REPO "$LLM_HF_REPO"
  upsert_env_key LLM_MODEL_FILE "$LLM_MODEL_FILE"
  upsert_env_key LLM_GPU_LAYERS "$LLM_GPU_LAYERS"
  upsert_env_key PULL_COSYVOICE "$PULL_COSYVOICE"
  upsert_env_key PREWARM_ENGINES "$PREWARM_ENGINES"
elif [[ -z "$(read_env_key COMPUTE_MODE || true)" ]]; then
  upsert_env_key COMPUTE_MODE "$COMPUTE_MODE"
fi

upsert_env_key NVIDIA_GPU_ID "$FINAL_GPU_ID"

# 6. Configure WSL2/Linux host directories & create bind-mount folders cleanly
if [[ -n "$MODELS_DIR_OVERRIDE" ]]; then
  upsert_env_key STUDIO_MODELS_DIR "$MODELS_DIR_OVERRIDE"
fi
if [[ -n "$STORAGE_DIR_OVERRIDE" ]]; then
  upsert_env_key STUDIO_STORAGE_DIR "$STORAGE_DIR_OVERRIDE"
fi

MODELS_DIR="$(read_env_key STUDIO_MODELS_DIR || echo "./models")"
STORAGE_DIR="$(read_env_key STUDIO_STORAGE_DIR || echo "./local_storage")"
[[ -n "$MODELS_DIR" ]] || MODELS_DIR="./models"
[[ -n "$STORAGE_DIR" ]] || STORAGE_DIR="./local_storage"

mkdir -p \
  "$MODELS_DIR/whisper" \
  "$MODELS_DIR/vad" \
  "$MODELS_DIR/llm" \
  "$MODELS_DIR/tts/xtts-v2" \
  "$MODELS_DIR/tts/cosyvoice" \
  "$STORAGE_DIR/voice_profiles" \
  "$STORAGE_DIR/renders" \
  "$STORAGE_DIR/reports"

# 7. Populate LAN IP & non-placeholder secrets without clobbering existing credentials
EXISTING_LAN_IP="$(read_env_key LAN_HOST_IP || true)"
if [[ -z "$EXISTING_LAN_IP" || "$EXISTING_LAN_IP" == "127.0.0.1" ]]; then
  DETECTED_LAN_IP="$(detect_lan_ip)"
  upsert_env_key LAN_HOST_IP "$DETECTED_LAN_IP"
  FINAL_LAN_IP="$DETECTED_LAN_IP"
else
  FINAL_LAN_IP="$EXISTING_LAN_IP"
fi

EXISTING_PG_PASS="$(read_env_key POSTGRES_PASSWORD || true)"
if [[ "$ENV_CREATED" == "true" && ( -z "$EXISTING_PG_PASS" || "$EXISTING_PG_PASS" == "local_voice_dev" ) ]]; then
  NEW_PG_PASS="$(gen_hex 16)"
  upsert_env_key POSTGRES_PASSWORD "$NEW_PG_PASS"
  upsert_env_key DATABASE_URL "postgres://voice_pipe:${NEW_PG_PASS}@postgres:5432/voice_pipe"
fi

EXISTING_OWNER_TOKEN="$(read_env_key OWNER_API_BEARER_TOKEN || true)"
if [[ -z "$EXISTING_OWNER_TOKEN" || "$EXISTING_OWNER_TOKEN" == replace-with-* || "$EXISTING_OWNER_TOKEN" == "dev-owner-bearer-token-change-me" ]]; then
  NEW_OWNER_TOKEN="$(gen_hex 32)"
  upsert_env_key OWNER_API_BEARER_TOKEN "$NEW_OWNER_TOKEN"
  log_info "Generated fresh 32-byte OWNER_API_BEARER_TOKEN in $ENV_FILE."
fi

WEB_PORT="$(read_env_key WEB_UI_PORT || echo "3000")"
TLS_PORT="$(read_env_key WEB_UI_TLS_PORT || echo "3443")"
API_PORT="$(read_env_key API_BIND_PORT || echo "8080")"
EFFECTIVE_MODE="$(read_env_key COMPUTE_MODE || echo "$COMPUTE_MODE")"

WARNINGS_JOINED=""
if (( ${#WARNINGS[@]} > 0 )); then
  WARNINGS_JOINED="$(printf '%s\n' "${WARNINGS[@]}")"
fi

if [[ "$JSON_OUTPUT" == "true" ]]; then
  python3 - "$ENV_FILE" "$IS_WSL2" "$RAM_TOTAL_MIB" "$SELECTED_TIER" "$EFFECTIVE_MODE" \
    "$DETECTED_GPU_NAME" "$DETECTED_GPU_VRAM_MIB" "$FINAL_GPU_ID" "$MODELS_DIR" "$STORAGE_DIR" \
    "$FINAL_LAN_IP" "$WEB_PORT" "$TLS_PORT" "$API_PORT" "$DOCKER_OK" "$COMPOSE_OK" "$WARNINGS_JOINED" <<'PY'
import json, sys
(
    env_file, is_wsl2, ram_mib, tier, compute_mode,
    gpu_name, gpu_vram_mib, gpu_id, models_dir, storage_dir,
    lan_ip, web_port, tls_port, api_port, docker_ok, compose_ok, warnings_raw,
) = sys.argv[1:]
warnings = [w for w in warnings_raw.splitlines() if w.strip()]
print(json.dumps({
    "status": "configured",
    "env_file": env_file,
    "wsl2_detected": is_wsl2 == "true",
    "ram_total_mib": int(ram_mib),
    "tier": tier,
    "compute_mode": compute_mode,
    "detected_gpu": {
        "name": gpu_name,
        "vram_mib": int(gpu_vram_mib),
        "selected_gpu_id": gpu_id,
    },
    "docker_ready": docker_ok == "true" and compose_ok == "true",
    "paths": {
        "models_dir": models_dir,
        "storage_dir": storage_dir,
    },
    "urls": {
        "windows_browser_http": f"http://localhost:{web_port}",
        "lan_https_mic": f"https://{lan_ip}:{tls_port}",
        "api_base": f"http://localhost:{api_port}",
    },
    "warnings": warnings,
    "next_command": "make up",
}, indent=2))
PY
else
  log_info "Hardware tier: ${SELECTED_TIER} (compute_mode=${EFFECTIVE_MODE}, gpu='${DETECTED_GPU_NAME}', vram=${DETECTED_GPU_VRAM_MIB} MiB, id=${FINAL_GPU_ID})"
  log_info "Directories ready: models=${MODELS_DIR} · storage=${STORAGE_DIR}"
  log_info "Studio URLs once started:"
  log_info "  • Windows Browser (Mic ready): http://localhost:${WEB_PORT}"
  log_info "  • LAN Devices HTTPS (Mic):     https://${FINAL_LAN_IP}:${TLS_PORT}"
  log_info "  • REST API:                    http://localhost:${API_PORT}"
  log_info "Next step: run 'make up'"
fi
