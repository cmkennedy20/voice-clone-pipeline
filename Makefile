# ==============================================================================
# Voice Clone Pipeline & Studio — Self-Contained WSL2 / Linux Makefile
# ==============================================================================
# Designed to run inside WSL2 on Windows (with Docker Desktop WSL2 integration
# or native dockerd) or Linux. Zero host Rust, Python, Node, or CUDA SDK needed.
#
# Quick start:
#   make setup          # Probe GPU VRAM / RAM / LAN IP & write .env
#   make up             # Compile containers, pull models, migrate DB & boot app
#   make doctor         # Verify container health & print http://localhost:3000
# ==============================================================================

SHELL := /usr/bin/env bash
.SHELLFLAGS := -euo pipefail -c
.DEFAULT_GOAL := help

ENV_FILE ?= .env
COMPUTE_MODE := $(shell if [ -f "$(ENV_FILE)" ]; then grep -E '^[[:space:]]*COMPUTE_MODE=' "$(ENV_FILE)" | tail -n1 | cut -d= -f2- | tr -d "\"'\r "; else echo gpu; fi)
ifeq ($(strip $(COMPUTE_MODE)),)
  COMPUTE_MODE := gpu
endif

ifeq ($(strip $(COMPUTE_MODE)),cpu)
  COMPOSE := docker compose --env-file $(ENV_FILE) -f docker-compose.yml
else
  COMPOSE := docker compose --env-file $(ENV_FILE) -f docker-compose.yml -f docker-compose.gpu.yml
endif

COMPOSE_VERIFY := docker compose -f docker-compose.yml --profile verify

.PHONY: help setup pull-models build up down restart logs status doctor setup-discord \
        enroll test-fast verify-sidecar verify-web lint verify verify-gpu

help: ## Show all WSL2/Docker setup, build, model download, Discord, and test targets
	@printf "\n  Voice Clone Pipeline & Studio — WSL2 Docker Task Runner (mode: %s)\n" "$(COMPUTE_MODE)"
	@printf "  ─────────────────────────────────────────────────────────────────────────────\n"
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z0-9_-]+:.*## / {printf "  make %-16s %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@printf "\n"

setup: ## Probe WSL2 GPU/VRAM/RAM & Windows LAN IP, create ./models, and write .env
	bash scripts/setup.sh --env-file "$(ENV_FILE)" $(ARGS)

$(ENV_FILE):
	bash scripts/setup.sh --non-interactive --env-file "$(ENV_FILE)"

pull-models: $(ENV_FILE) ## Download hardware-matched Whisper, VAD, Qwen3, XTTS-v2 & CosyVoice models
	$(COMPOSE) run --rm model-puller

build: $(ENV_FILE) ## Compile all Rust, llama.cpp, Python TTS, and React Web UI Docker images locally
	$(COMPOSE) build

up: $(ENV_FILE) ## Compile, pull models, run SQLx migrations, and start the full stack in Docker
	$(COMPOSE) up -d --build
	@bash scripts/doctor.sh --env-file "$(ENV_FILE)" || true

down: ## Stop and remove running stack containers (preserves DB, models, and voice profiles)
	docker compose -f docker-compose.yml down

restart: $(ENV_FILE) ## Recreate/restart running services after .env edits
	$(COMPOSE) up -d

logs: ## Tail container logs (optional filter: `make logs SERVICE=api-server`)
	docker compose -f docker-compose.yml logs -f --tail=150 $(SERVICE)

status: ## Show container health and port mappings
	docker compose -f docker-compose.yml ps

doctor: ## Run end-to-end WSL2 & HTTP readiness check (`make doctor ARGS="--json"`)
	bash scripts/doctor.sh --env-file "$(ENV_FILE)" $(ARGS)

setup-discord: ## Configure 1-on-1 Discord bot credentials in .env & hot-reload discord-worker
	bash scripts/setup-discord.sh $(ARGS)

enroll: $(ENV_FILE) ## Enroll a 24kHz WAV (`make enroll SLUG=my_voice WAV=sample.wav TRANSCRIPT="Hello..."`)
	@test -n "$(SLUG)" || (echo "Usage: make enroll SLUG=my_voice WAV=sample.wav TRANSCRIPT=\"...\"" >&2 && exit 2)
	@test -n "$(WAV)" || (echo "Missing WAV=<filename inside ./local_storage/voice_profiles/>" >&2 && exit 2)
	@test -n "$(TRANSCRIPT)" || (echo "Missing TRANSCRIPT=\"...\"" >&2 && exit 2)
	$(COMPOSE) --profile cli run --rm cli enroll \
		--slug "$(SLUG)" \
		--wav "/storage/voice_profiles/$(WAV)" \
		--transcript "$(TRANSCRIPT)"

test-fast: ## Fast pure unit + proptest + golden matrix + architecture boundary checks in Docker
	$(COMPOSE_VERIFY) run --rm --no-deps verify-runner \
		bash -euo pipefail -c "cargo test -p common -p domain && cargo test -p api --test architecture_boundaries"

verify-sidecar: ## Build and run Python TTS sidecar pytest schema & tensor-shape suite in Docker
	$(COMPOSE_VERIFY) run --rm --build tts-sidecar-test

verify-web: ## Build and run React + Redux Studio UI typecheck, Vitest suite, and bundle build
	$(COMPOSE_VERIFY) run --rm --build web-ui-test

lint: ## Run rustfmt, zero-warning Clippy, Python sidecar pytest, and Web UI checks in Docker
	$(COMPOSE_VERIFY) run --rm --no-deps verify-runner \
		bash -euo pipefail -c "cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings"
	$(MAKE) verify-sidecar
	$(MAKE) verify-web

verify: ## Full non-GPU verification gate (Rust fmt + Clippy + SQLx nextest + cargo-deny + sidecar + web)
	$(COMPOSE_VERIFY) run --rm --build verify-runner
	$(MAKE) verify-sidecar
	$(MAKE) verify-web

verify-gpu: $(ENV_FILE) ## Live GPU readiness probe + 15-clip A/B benchmark scorecard
	$(COMPOSE) --profile cli run --rm cli doctor
	$(COMPOSE) --profile cli run --rm cli bench --manifest /evals/golden_manifest.json --out /storage/reports/scorecard.md
