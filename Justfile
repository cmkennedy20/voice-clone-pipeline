set shell := ["bash", "-euo", "pipefail", "-c"]

# Probe GPUs/VRAM/LAN IP, create ./models & ./local_storage, and write .env
setup *args:
    bash ./scripts/setup.sh {{args}}

# Interactive 5-stage wizard (or --app-id/--bot-token/--owner-id) to configure Discord 1-on-1 bot
setup-discord *args:
    ./scripts/setup-discord.sh {{args}}

# Boot the stack in Docker (automatically selects GPU overlay or CPU mode from .env)
up:
    make up

# Stop running containers while preserving database, models, and enrolled voice profiles
down:
    make down

# Run end-to-end readiness diagnostics (--json supported)
doctor *args:
    bash ./scripts/doctor.sh {{args}}

# Fast pure unit + proptest + golden matrix + architecture boundary checks inside Docker (<3s warm)
test-fast:
    docker compose --profile verify run --rm --no-deps verify-runner \
      bash -euo pipefail -c "cargo test -p common -p domain && cargo test -p api --test architecture_boundaries"

# Build and run the Python TTS sidecar pytest schema & tensor-shape suite inside Docker
verify-sidecar:
    docker compose --profile verify run --rm --build tts-sidecar-test

# Build and run the React + Redux Studio UI typecheck, Vitest suite, and bundle build inside Docker
verify-web:
    docker compose --profile verify run --rm --build web-ui-test

# Format + strict zero-warning Clippy + Python sidecar pytest + Web UI checks inside Docker
lint:
    docker compose --profile verify run --rm --no-deps verify-runner \
      bash -euo pipefail -c "cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings"
    just verify-sidecar
    just verify-web

# MANDATORY CI/AGENT GATE: Runs all non-GPU test layers + sidecar pytest + React/Redux Studio UI verification
verify:
    docker compose --profile verify run --rm --build verify-runner
    just verify-sidecar
    just verify-web

# Enroll a 15-second 24kHz reference WAV into the Docker stack
enroll slug wav_name transcript:
    make enroll SLUG="{{slug}}" WAV="{{wav_name}}" TRANSCRIPT="{{transcript}}"

# LAYER 7 LIVE HARDWARE GATE: Runs CUDA readiness probe + 15-clip A/B benchmark & persists benchmark_runs row
verify-gpu:
    make verify-gpu
