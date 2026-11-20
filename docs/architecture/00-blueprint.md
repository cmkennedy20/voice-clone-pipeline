# Local English-to-Mandarin Voice Cloning Pipeline — 100% Dockerized 5-Crate Agent-First Rust Blueprint

## High-Level Summary & Document Map

This blueprint specifies a **100% Docker-contained** local **English-to-Mandarin voice cloning content pipeline** running on your **[NVIDIA GeForce RTX 3090 (24 GB GDDR6X)](https://www.nvidia.com/en-us/geforce/graphics-cards/30-series/rtx-3090-3090ti/)** server using the **5-Crate Agent-First Modular Rust Architecture** (Rust Edition 2024, Tokio, Axum, SQLx, `thiserror`, `tracing`). Every lifecycle step—Hugging Face weight pulls, SQLx migrations, the 7-layer verification harness (`docker compose --profile verify`), GPU hardware benchmarks, the LAN/Tailscale HTTP API, and the Discord worker—executes inside Docker containers with **zero Rust, CUDA, or C/Opus packages required on the host OS**.

To support messages coming from your **personal Discord user account** without violating the [Discord Community Guidelines against automating user accounts](https://discord.com/guidelines), the architecture avoids raw user-token scraping and implements **three compliant user-anchored delivery modes**: (1) a **Client Audio Return Endpoint (`POST /api/v1/voice-jobs/render`)** feeding [BlackHole](https://existential.audio/blackhole/) (macOS) / [VB-Cable](https://vb-audio.com/Cable/) (Windows) or iOS/macOS file share so voice calls and file drops come **100% from your real personal account** with zero `APP` badge; (2) a **[Discord `USER_INSTALL` Personal App](https://docs.discord.com/developers/interactions/receiving-and-responding)** (`integration_types: [1]`, `contexts: [0, 1, 2]`) usable inside any 1:1 DM, Group DM, or external server without admin rights; and (3) **[Webhook Profile Overrides](https://docs.discord.com/developers/resources/webhook)** / native [`IS_VOICE_MESSAGE` (`flags: 8192`)](https://docs.discord.com/developers/resources/message) waveform bubbles.

| Section | Architectural Scope | Primary Engineering Deliverable |
| :--- | :--- | :--- |
| **1. Architecture & User-Account Modes** | Maps the 5-crate one-way dependency graph and compares the 3 compliant personal-account Discord delivery paths. | Protects your personal Discord account while enabling real-account voice/chat + universal DM/server reach. |
| **2. RTX 3090 VRAM & Domain Guards** | Co-locates all inference models inside `~19.8 GB / 24.0 GB` VRAM (`~3.3s` E2E for 10s audio) and neutralizes Mandarin tone/digit traps. | Single-load [`whisper-rs`](https://crates.io/crates/whisper-rs), [`Qwen3-8B`](https://huggingface.co/Qwen/Qwen3-8B), and hot-swappable [`CosyVoice`](https://github.com/QwenAudio/CosyVoice) / [`XTTS-v2`](https://huggingface.co/coqui/XTTS-v2). |
| **3. 5-Crate Workspace Tree & SQL** | Complete workspace tree (`src/`, 5-tier `tests/`, `docs/`, `bins/`) with zero `mod.rs` files and `<= 350 LOC` ceiling. | Unambiguous file paths for AI coding agents + Postgres `voice_profiles`, `voice_jobs`, & `benchmark_runs` ledger. |
| **4. Canonical Tier Code Templates** | Drop-in Rust 2024 code across `common`, `domain`, `framework`, `features`, and `api` (`/v1/voice-jobs/render`). | Plain `pub fn` `BasisPoints` engines + plain `pub async fn` workflows + 3-mode Discord delivery. |
| **5. 100% Containerized Docker Stack** | Multi-stage [`cargo-chef`](https://github.com/lukemathwalker/cargo-chef) CUDA `Dockerfile` + unified `docker-compose.yml` (`runtime`, `verify`, `cli` profiles). | Zero host dependencies: auto-pulls weights, migrates DB, runs `just verify` in tmpfs DB, and serves GPU API. |
| **6. 7-Layer Testing & Validation** | Drop-in [`proptest`](https://crates.io/crates/proptest), golden matrices, real Symphonia Opus round-trip, [`wiremock`](https://crates.io/crates/wiremock), `#[sqlx::test]`, `oneshot` HTTP, and boundary scanner. | Blocks mocked tautological tests, horizontal feature imports, GCS header bugs, and unverified TTS hallucinations. |
| **7. 5-Phase Build & 6-Metric Suite** | Sequential gated agent phases (`Phase 1 -> Phase 5`), `Justfile`, `AGENTS.md`, and 6-metric RTX 3090 scorecard (`M1–M6`). | Proves `CER <= 1,000 bps` (`10.00%`), speaker similarity `>= 7,800 bps`, and `P95 <= 5,000 ms` inside Docker. |

## 1. Architecture, Crate Boundaries & Personal Discord Account Strategy

### 1.1 Compiler-Enforced 5-Crate Dependency Graph
Dependencies flow strictly top-to-bottom (`bins -> api -> features -> framework -> domain -> common`). `crates/common` and `crates/domain` remain 100% synchronous and pure (zero `tokio`, `sqlx`, `axum`, `reqwest`, `serenity`, or `whisper-rs`):

```mermaid
graph TD
    subgraph DockerHost["NVIDIA RTX 3090 Host (Docker Engine + NVIDIA Container Toolkit ONLY)"]
        subgraph Tier6["Tier 6: Thin Container Entrypoints (<100 LOC each)"]
            BIN_API["bins/api-server (:8080 LAN/Tailscale)"]
            BIN_BOT["bins/discord-worker (Gateway + Interactions)"]
            BIN_CLI["bins/cli (doctor | enroll | bench)"]
        end

        subgraph Tier5["Tier 5: HTTP Transport (crates/api)"]
            API["Axum v1 Controllers (5-15 LOC)<br/>POST /v1/voice-jobs | POST /v1/voice-jobs/render (raw .ogg/.wav)<br/>AuthenticatedOwner Extractor + RFC 7807 ApiError"]
        end

        subgraph Tier4["Tier 4: Imperative Shell (crates/features)"]
            FEAT["Plain `pub async fn` Workflows<br/>voice_jobs::translate_clip | profiles::enroll_profile | benchmarks::run_suite<br/>Direct SQLx Queries + shared_queries/"]
        end

        subgraph Tier3["Tier 3: Infrastructure & Concrete Adapters (crates/framework)"]
            FW["AppConfig (sole env::var reader) + AppContext (PgPool, GPU Semaphore)<br/>DSP (Symphonia/Rubato/Opus) + Whisper/Qwen/TTS/Discord Clients + Fake* Test Doubles"]
        end

        subgraph Tier2["Tier 2: Functional Core (crates/domain — 100% Sync, Zero I/O)"]
            DOM["Validated Newtypes (Pcm16kMono, MandarinSpokenScript, DiscordWaveform)<br/>Pure Engines: audio::gate, audio::waveform, translation::verifier (CER in BasisPoints)<br/>Narrow External Traits: ports.rs"]
        end

        subgraph Tier1["Tier 1: Generic Utilities (crates/common — 100% Sync, Zero Business Terms)"]
            COM["Clock, define_id!, BasisPoints, Secret<T>, PageParams, sha256"]
        end
    end

    BIN_API --> API
    BIN_BOT --> FEAT
    BIN_CLI --> FEAT
    API --> FEAT
    FEAT --> FW
    FW --> DOM
    DOM --> COM
```

### 1.2 Personal Discord User Account Trade-Offs & Supported Delivery Modes

Per the [Discord Community Guidelines](https://discord.com/guidelines) and [Discord Developer Policy (`discord-api-docs` #491)](https://github.com/discord/discord-api-docs/issues/491), automating REST uploads directly through a personal user session token is prohibited and triggers automated anti-abuse account bans. Instead, `crates/api` and `crates/framework` implement **three compliant modes** so you get real personal-account voice/uploads via a local audio bridge plus universal slash access across DMs and servers:

| Delivery Mode | Appears From Your Real User Account? | Works in Private 1:1 DMs & External Servers? | Renders `flags: 8192` Waveform Bubble? | Mechanism & Ideal Workflow |
| :--- | :--- | :--- | :--- | :--- |
| **Mode A1: Virtual Mic Loopback (`BlackHole` / `VB-Cable`)** | **Yes (`100%` real user account, zero `APP` badge)** | **Yes (any voice call, DM call, or Stage)** | Live voice stream | Mac/PC hotkey posts mic audio to Docker `POST /api/v1/voice-jobs/render?format=wav` and plays the `48 kHz` Mandarin WAV into [BlackHole](https://existential.audio/blackhole/) or [VB-Cable](https://vb-audio.com/Cable/) set as Discord's input mic (`~3.3s` latency). |
| **Mode A2: iOS Shortcut / Mac Clipboard Return** | **Yes (`100%` real user account, zero `APP` badge)** | **Yes (every DM, Group DM, & Server)** | Inline audio card on [Desktop](https://www.airdroid.com/app-tips/send-voice-messages-on-discord-pc/); shareable clip on Mobile | `POST /api/v1/voice-jobs/render?format=ogg` returns verified Ogg/Opus directly to an iOS Shortcut (Share -> Discord) or copies the file onto your macOS clipboard (`Cmd+V` -> `Enter` in Discord). |
| **Mode B: `USER_INSTALL` Personal Discord App** | Shows `"You used /voice_clone"` + your icon (`APP` tag) | **Yes (`GUILD=0`, `BOT_DM=1`, `PRIVATE_CHANNEL=2` with zero server bot needed)** | Playable audio attachment + optional spoiler Pinyin | Installed onto your **personal Discord profile** via [Discord User-Installable Apps (`USER_INSTALL`)](https://docs.discord.com/developers/interactions/receiving-and-responding) for 1-command use inside private DMs or external servers. |
| **Mode C: Bot / Profile-Matched Channel Webhook** | Matches your `username` + `avatar_url` (`APP`/`BOT` tag) | Servers where bot/webhook is added | **Yes (true native `flags: 8192` mobile/desktop waveform bubble)** | Uses the 3-step [`IS_VOICE_MESSAGE` (`flags: 8192`)](https://docs.discord.com/developers/resources/message) uploader or [Execute Webhook profile fields](https://docs.discord.com/developers/resources/webhook) inside your own servers. |

## 2. RTX 3090 (24 GB) Co-Resident VRAM Budget

All GPU containers share your [NVIDIA GeForce RTX 3090 (`24 GB` GDDR6X, `935.8 GB/s`)](https://www.thefpsreview.com/gpu-family/nvidia-geforce-rtx-3090-gpu-family-specifications/) via [NVIDIA Container Toolkit](https://docs.nvidia.com/ai-enterprise/deployment/vmware/latest/docker.html), keeping every model co-resident (`~19.8 GB` active, `~4.2 GB` free):

| Stage | Container & Crate Module | Pinned Model | VRAM | 10s Clip Latency | Verified Benchmark Basis |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Gate 0 & DSP** | `api-server` (`framework::dsp`) | [`silero-vad-rust`](https://crates.io/crates/silero-vad-rust) ([`ort`](https://github.com/pykeio/ort)) + [`symphonia`](https://crates.io/crates/symphonia) + [`rubato`](https://docs.rs/rubato) | `0.0 GB` (CPU) | `~15 ms` | Trims dead air and rejects `<400 ms` or clipped audio before acquiring GPU semaphore. |
| **Stage 1 (EN ASR)** | `api-server` (`framework::clients::whisper_cuda`) | [`whisper-rs`](https://crates.io/crates/whisper-rs) `ggml-large-v3` (`FP16`) | `~4.2 GB` | `~0.80 s` | **`RTF 0.08` (`12.5x` real-time)** on RTX 3090 ([GigaGPU Whisper Benchmark](https://gigagpu.com/whisper-large-v3-on-rtx-3090-benchmark/)). |
| **Stage 2 (ZH LLM)** | `qwen-translator` ([Qwen `llama.cpp` CUDA](https://qwen.readthedocs.io/en/latest/run_locally/llama.cpp.html)) | [`Qwen/Qwen3-8B`](https://huggingface.co/Qwen/Qwen3-8B) (`Q8_0` GGUF, `/no_think`) | `~9.2 GB` | `~0.35 s` | Near-lossless `Q8_0` fits in 24 GB ([Spheron Qwen3 Guide](https://www.spheron.network/blog/run-qwen3-locally-gpu-requirements-2026/)) and expands digits into spoken *kǒuyǔ* (口语). |
| **Stage 3 (TTS Clone)** | `tts-clone-sidecar` (`domain::ports::VoiceSynthesizer`) | [`QwenAudio/CosyVoice`](https://github.com/QwenAudio/CosyVoice) (`3.0`) **or** [`coqui/XTTS-v2`](https://huggingface.co/coqui/XTTS-v2) (`FP16`) | `~4.3 GB` | `~1.65 s` | [`XTTS-v2`](https://gigagpu.com/xtts-v2-vram-requirements/) runs **`5.6x` real-time** on RTX 3090 ([GigaGPU XTTS Benchmark](https://gigagpu.com/coqui-xtts-v2-on-rtx-3090-benchmark/)); [`CosyVoice`](https://cosyvoice.org/zero-shot-voice-cloning) preserves Mandarin 4-tone sandhi ([2026 Voice Cloning Eval](https://rarebuildsoftware.com/blog/best-open-source-voice-cloning-2026)). |
| **Stage 4 (ZH Verify)** | `api-server` (reuses Stage 1 [`whisper-rs`](https://crates.io/crates/whisper-rs) `lang="zh"`) | `ggml-large-v3` (Shared context) | `0.0 GB` | `~0.65 s` | Independent reverse-ASR check enforces `CER <= 1,200 bps` (`12.00%`) with up to 3 seed-shifted retries. |
| **Stage 5 (Opus)** | `api-server` (`framework::dsp::opus_and_wav`) | [`ogg-opus`](https://lib.rs/crates/ogg-opus) (`48 kHz` Mono VoIP Opus + 256-bin waveform) | `0.0 GB` (CPU) | `~25 ms` | Prevents unplayable non-Opus voice bubbles ([Hermes Agent Issue #123680](https://github.com/NousResearch/hermes-agent/issues/123680)). |
| **CUDA Buffer** | KV cache (`ctx = 4096`), cuBLAS workspaces | — | `~2.1 GB` | — | **Total Peak: `~19.8 GB / 24.0 GB`** (`4.2 GB` free margin). |

*(Single-Process GPU Ownership: `api-server` owns the `whisper-rs` CUDA context and single-GPU `Semaphore(1)`, while `discord-worker` runs as a `<50 MB` CPU container forwarding Discord interactions to `http://api-server:8080/api/v1/voice-jobs`—loading Whisper into VRAM once and saving `4.2 GB`.)*

## 3. Complete 5-Crate Workspace Directory Tree & SQLx Schema

### 3.1 Workspace File Tree (Zero `mod.rs` Files, `<= 350 LOC` Source Ceiling)

```text
discord-mandarin-voice/
├── Cargo.toml                              # Workspace manifest: [workspace.dependencies] & strict [workspace.lints]
├── Cargo.lock
├── Dockerfile                              # Multi-stage cargo-chef + CUDA 12.6 build, verify-runner, api-server, discord-worker, cli
├── docker-compose.yml                      # 100% Dockerized stack: model-puller, postgres, db-migrate, qwen, tts, api, bot, verify, cli
├── rust-toolchain.toml                     # Pinned stable Rust + rustfmt + clippy (used inside Dockerfile)
├── rustfmt.toml
├── clippy.toml                             # Bans std::env::var outside crates/framework/src/config.rs
├── deny.toml                               # Supply-chain & license check (bans openssl -> rustls)
├── Justfile                                # Delegates 100% of commands (`check`, `lint`, `verify`, `verify-gpu`) to Docker Compose
├── AGENTS.md                               # Root operational contract for AI coding agents
├── .env.example
├── .config/
│   └── nextest.toml
├── docs/
│   └── architecture/
│       ├── 01-crate-boundaries.md
│       ├── 02-testing-strategy.md
│       └── 03-verification-and-lints.md
├── migrations/
│   └── 202611170001_init_voice_pipeline.sql
├── evals/
│   └── golden_manifest.json                # 15 golden English test utterances + 3 Gate-0 trap specs
├── sidecars/
│   └── tts-worker/                         # Isolated Python PyTorch CUDA container (CosyVoice 3.0 + XTTS-v2 + Speaker Sim)
│       ├── Dockerfile
│       ├── server.py                       # /health, /synthesize, /speaker-similarity (ECAPA-TDNN / CampPlus)
│       ├── test_server.py                  # Pytest schema & tensor-shape validation suite
│       └── engines/
│           ├── cosyvoice_engine.py
│           └── xtts_engine.py
├── bins/
│   ├── api-server/
│   │   ├── Cargo.toml
│   │   └── src/main.rs                     # <100 LOC: AppConfig + Telemetry + Whisper/GPU AppContext + Axum serve (:8080)
│   ├── discord-worker/
│   │   ├── Cargo.toml
│   │   └── src/main.rs                     # <100 LOC: CPU-only Poise USER_INSTALL + DM relay forwarding to `api-server`
│   └── cli/
│       ├── Cargo.toml
│       └── src/main.rs                     # <100 LOC: Containerized CLI (`doctor`, `enroll`, `translate-clip`, `bench`)
└── crates/
    ├── common/                             # TIER 1: Pure generic utilities (Zero business terms, zero async/I/O)
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── basis_points.rs             # `BasisPoints(u32)` exact integer rate/error math + inline unit tests
    │       ├── clock.rs                    # `Clock` trait (`SystemClock`)
    │       ├── digest.rs                   # Deterministic SHA-256 hex helper
    │       ├── id.rs                       # `define_id!` typed UUIDv7 wrapper macro
    │       ├── pagination.rs               # `PageParams`, `Paged<T>`
    │       └── secret.rs                   # `Secret<T>` redacting tokens in Debug/Display + redaction unit tests
    ├── domain/                             # TIER 2: Pure Functional Core & Engines (100% Sync, Zero async/DB/HTTP)
    │   ├── Cargo.toml                      # `[dev-dependencies] proptest = "1.6"`
    │   ├── src/
    │   │   ├── lib.rs
    │   │   ├── ports.rs                    # Narrow external traits: AudioCodec, ASR, LLM, TTS, DiscordPublisher
    │   │   ├── audio.rs                    # Named module root (NO mod.rs)
    │   │   ├── audio/
    │   │   │   ├── types.rs                # `Pcm16kMono`, `Pcm24kMono`, `RenderedAudioBundle`, `DiscordWaveform`
    │   │   │   ├── gate.rs                 # Pure `evaluate_audio_gate` + inline boundary tests
    │   │   │   ├── waveform.rs             # Pure `quantize_256_waveform_bins` -> base64 preview + proptests
    │   │   │   └── error.rs                # `AudioDomainError` (`thiserror`)
    │   │   ├── translation.rs              # Named module root (NO mod.rs)
    │   │   └── translation/
    │   │       ├── types.rs                # `MandarinSpokenScript` (bans raw ASCII digits), `TtsEngineKind`
    │   │       ├── verifier.rs             # Pure Hanzi Levenshtein CER calculator & `BasisPoints` gate + proptests
    │   │       └── error.rs                # `TranslationDomainError` (`thiserror`)
    │   └── tests/
    │       ├── audio_gate_matrix.rs        # 20-case table-driven boundary suite for Gate 0 & 256-bin waveforms
    │       └── cer_verifier_matrix.rs      # 25-case table-driven Mandarin CER & digit-rejection golden suite
    ├── framework/                          # TIER 3: Concrete Infra, Config, DB Pool, DSP & External Clients
    │   ├── Cargo.toml                      # `[dev-dependencies] wiremock = "0.6"`
    │   ├── src/
    │   │   ├── lib.rs
    │   │   ├── config.rs                   # Typed `AppConfig` (ONLY file allowed to read `std::env::var`)
    │   │   ├── context.rs                  # Clonable `AppContext` (`PgPool`, `Semaphore`, `Clock`, ports)
    │   │   ├── telemetry.rs
    │   │   ├── db.rs
    │   │   ├── db/
    │   │   │   ├── pool.rs
    │   │   │   └── pg_errors.rs
    │   │   ├── auth.rs
    │   │   ├── auth/
    │   │   │   └── owner_token.rs          # Bearer API key & `DISCORD_OWNER_USER_ID` check
    │   │   ├── dsp.rs                      # Concrete Symphonia + Silero VAD + Rubato + Opus 48kHz / WAV muxer
    │   │   ├── dsp/
    │   │   │   ├── decode_and_vad.rs
    │   │   │   └── opus_and_wav.rs
    │   │   ├── clients.rs                  # Concrete GPU/REST clients + stateful `Fake*` test doubles
    │   │   ├── clients/
    │   │   │   ├── whisper_cuda.rs         # `whisper-rs` CUDA (EN transcribe + ZH back-transcribe)
    │   │   │   ├── qwen_llama.rs           # Local `llama-server` Qwen3-8B structured JSON client
    │   │   │   ├── tts_sidecar.rs          # HTTP client to local CosyVoice / XTTS-v2 worker
    │   │   │   └── discord_publisher.rs    # Bot `flags: 8192`, `USER_INSTALL` followup, & Profile Webhook
    │   │   ├── testing.rs                  # Gated behind `#[cfg(any(test, feature = "test-support"))]`
    │   │   └── testing/
    │   │       ├── test_context.rs         # `FixedClock` + `build_test_context(PgPool)` + `FakeCallCounters`
    │   │       └── audio_fixtures.rs       # Deterministic synthetic 16kHz/24kHz sine, speech-like, silent & clipped WAVs
    │   └── tests/
    │       ├── opus_and_wav_roundtrip.rs   # Real Symphonia demux verification of encoded 48kHz Ogg/Opus & WAV
    │       └── discord_and_sidecar_wiremock.rs # Verifies 3-step GCS header rules, flags=8192, & HTTP 503 retries
    ├── features/                           # TIER 4: Transport-Agnostic Async Workflows & Direct SQLx (Imperative Shell)
    │   ├── Cargo.toml
    │   ├── src/
    │   │   ├── lib.rs
    │   │   ├── error.rs                    # `AppError` (`Validation`, `NotFound`, `Conflict`, `Forbidden`, `Internal`)
    │   │   ├── shared_queries.rs           # SQLx queries shared across 2+ workflows
    │   │   ├── shared_queries/
    │   │   │   └── profile_loader.rs
    │   │   ├── profiles.rs
    │   │   ├── profiles/
    │   │   │   ├── enroll_profile.rs
    │   │   │   └── list_profiles.rs
    │   │   ├── voice_jobs.rs
    │   │   ├── voice_jobs/
    │   │   │   ├── translate_clip.rs       # Core E2E workflow returning `VoiceJobResult` (metadata + `.ogg`/`.wav` bytes)
    │   │   │   └── get_voice_job.rs        # Direct read SQL projection -> `VoiceJobView`
    │   │   ├── benchmarks.rs
    │   │   └── benchmarks/
    │   │       └── run_rtx3090_suite.rs    # Runs 15 golden clips & writes 6-metric `scorecard.md` + DB row
    │   └── tests/
    │       ├── profiles/
    │       │   └── enroll_profile_test.rs  # `#[sqlx::test]` duplicate slug & WAV validation checks
    │       └── voice_jobs/
    │           └── translate_clip_test.rs  # `#[sqlx::test]` retry self-healing, exhausted rollback, auth, & GPU semaphore
    └── api/                                # TIER 5: Thin Axum HTTP Controllers, Extractors & RFC 7807 Errors
        ├── Cargo.toml
        ├── src/
        │   ├── lib.rs                      # `pub fn build_router(ctx: AppContext) -> axum::Router`
        │   ├── error.rs                    # Maps `AppError` -> HTTP StatusCode + RFC 7807 Problem Details JSON
        │   ├── extractors.rs               # `AuthenticatedOwner` extractor
        │   ├── middleware.rs
        │   ├── middleware/
        │   │   └── request_id.rs
        │   ├── routes.rs
        │   ├── controllers.rs
        │   └── controllers/
        │       ├── health_controller.rs
        │       ├── v1.rs
        │       └── v1/
        │           ├── profiles_controller.rs
        │           ├── voice_jobs_controller.rs # `/v1/voice-jobs` (JSON) & `/v1/voice-jobs/render` (binary stream)
        │           └── benchmarks_controller.rs
        └── tests/
            ├── architecture_boundaries.rs  # <50ms check: zero mod.rs, pure common/domain, <=350 LOC, zero sibling imports
            └── v1/
                └── voice_jobs_api_test.rs  # In-memory `tower::ServiceExt::oneshot` black-box RFC 7807 & magic-byte tests
```

### 3.2 SQLx Migration (`migrations/202611170001_init_voice_pipeline.sql`)

```sql
CREATE TABLE voice_profiles (
    id UUID PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    avatar_url TEXT,
    reference_wav_path TEXT NOT NULL,
    reference_transcript_en TEXT NOT NULL,
    preferred_engine TEXT NOT NULL CHECK (preferred_engine IN ('cosyvoice_3', 'xtts_v2')),
    owner_discord_user_id BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE voice_jobs (
    id UUID PRIMARY KEY,
    profile_id UUID NOT NULL REFERENCES voice_profiles(id),
    delivery_mode TEXT NOT NULL CHECK (delivery_mode IN ('client_return_only', 'user_install_followup', 'profile_webhook', 'bot_voice_bubble')),
    source_sha256 TEXT NOT NULL,
    english_transcript TEXT NOT NULL,
    spoken_hanzi TEXT NOT NULL,
    pinyin TEXT NOT NULL,
    english_back_gloss TEXT NOT NULL,
    back_transcribed_zh TEXT NOT NULL,
    engine_used TEXT NOT NULL,
    cer_bps INT NOT NULL CHECK (cer_bps >= 0),
    attempts_used SMALLINT NOT NULL CHECK (attempts_used >= 1),
    duration_ms INT NOT NULL,
    total_latency_ms INT NOT NULL,
    output_ogg_path TEXT NOT NULL,
    discord_channel_id BIGINT,
    discord_message_id BIGINT,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE benchmark_runs (
    id UUID PRIMARY KEY,
    engine_tested TEXT NOT NULL,
    clips_evaluated SMALLINT NOT NULL,
    gate0_accuracy_bps INT NOT NULL,
    digit_hygiene_bps INT NOT NULL,
    mean_cer_bps INT NOT NULL,
    mean_speaker_sim_bps INT NOT NULL,
    format_compliance_bps INT NOT NULL,
    p95_latency_ms INT NOT NULL,
    peak_vram_mib INT NOT NULL,
    passed_all_gates BOOLEAN NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_voice_jobs_profile_created ON voice_jobs (profile_id, created_at DESC);
```

## 4. Canonical Code Templates (Multi-Mode Delivery & Audio Render Endpoint)

### 4.1 Domain Delivery Modes & Narrow Ports (`crates/domain/src/ports.rs`)

```rust
use common::secret::Secret;
use serde::{Deserialize, Serialize};
use crate::{
    audio::types::{Pcm16kMono, Pcm24kMono, RawAudioMetrics, RenderedAudioBundle},
    translation::types::{MandarinSpokenScript, TtsEngineKind},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum DiscordDeliveryTarget {
    /// Mode A: Returns raw .ogg / 48kHz .wav bytes directly to Mac BlackHole or iOS Shortcut
    /// so playback/upload happens 100% from the user's real personal Discord client.
    ClientReturnOnly,
    /// Mode B: Responds to a USER_INSTALL interaction (`contexts = [0, 1, 2]`)
    /// usable inside private 1:1 DMs, Group DMs, or external servers without adding a bot.
    UserInstallFollowup {
        application_id: u64,
        interaction_token: Secret<String>,
        attach_spoiler_pinyin: bool,
    },
    /// Mode C1: Posts via channel webhook with custom `username` + `avatar_url`.
    ProfileWebhook {
        webhook_url: Secret<String>,
        username: String,
        avatar_url: Option<String>,
    },
    /// Mode C2: Posts a native `IS_VOICE_MESSAGE` (`flags: 8192`) waveform bubble via Bot REST API.
    BotVoiceBubble {
        channel_id: u64,
        reply_to_message_id: Option<u64>,
    },
}

#[derive(Debug, Clone)]
pub struct TranslatedMandarin {
    pub spoken_hanzi: MandarinSpokenScript,
    pub pinyin: String,
    pub english_back_gloss: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ExternalPortError {
    #[error("audio codec failure: {0}")]
    AudioCodec(String),
    #[error("ASR inference failure: {0}")]
    Asr(String),
    #[error("LLM translation failure: {0}")]
    Translation(String),
    #[error("TTS sidecar synthesis failure: {0}")]
    Tts(String),
    #[error("Discord publish failure: {0}")]
    Discord(String),
}

pub trait AudioCodecEngine: Send + Sync {
    fn decode_and_measure(&self, raw_bytes: &[u8]) -> Result<(Pcm16kMono, RawAudioMetrics), ExternalPortError>;
    fn encode_bundle(&self, pcm_24k: &Pcm24kMono) -> Result<RenderedAudioBundle, ExternalPortError>;
}

#[async_trait::async_trait]
pub trait SpeechRecognizer: Send + Sync {
    async fn transcribe_english(&self, pcm: &Pcm16kMono) -> Result<String, ExternalPortError>;
    async fn transcribe_mandarin(&self, pcm: &Pcm24kMono) -> Result<String, ExternalPortError>;
}

#[async_trait::async_trait]
pub trait MandarinTranslator: Send + Sync {
    async fn translate_english_to_spoken_zh(&self, english: &str) -> Result<TranslatedMandarin, ExternalPortError>;
}

#[async_trait::async_trait]
pub trait VoiceSynthesizer: Send + Sync {
    async fn synthesize_cloned_zh(
        &self,
        profile_slug: &str,
        reference_wav_path: &str,
        reference_transcript_en: &str,
        engine: TtsEngineKind,
        script: &MandarinSpokenScript,
        seed: u64,
    ) -> Result<Pcm24kMono, ExternalPortError>;
}

#[async_trait::async_trait]
pub trait DiscordPublisher: Send + Sync {
    async fn publish(
        &self,
        target: &DiscordDeliveryTarget,
        bundle: &RenderedAudioBundle,
        translated: &TranslatedMandarin,
    ) -> Result<Option<u64>, ExternalPortError>;
}
```

### 4.2 Thin Axum v1 Controller Returning Verified Audio Bytes (`crates/api`)

```rust
// crates/api/src/controllers/v1/voice_jobs_controller.rs
use axum::{
    Json, Router,
    body::Body,
    extract::{Multipart, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
    routing::post,
};
use domain::ports::DiscordDeliveryTarget;
use features::voice_jobs::translate_clip::{TranslateClipInput, VoiceJobReceipt, translate_clip};
use framework::context::AppContext;
use serde::Deserialize;

use crate::{error::ApiError, extractors::AuthenticatedOwner};

#[derive(Debug, Deserialize)]
pub struct RenderQuery {
    pub profile: Option<String>,
    pub format: Option<String>, // "ogg" (default) or "wav" (for BlackHole / VB-Cable virtual mic)
}

pub fn routes() -> Router<AppContext> {
    Router::new()
        .route("/", post(post_voice_job))
        .route("/render", post(post_render_audio_for_user_client))
}

async fn post_voice_job(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    Json(body): Json<TranslateClipInput>,
) -> Result<(StatusCode, Json<VoiceJobReceipt>), ApiError> {
    let out = translate_clip(&ctx, owner.discord_user_id, body).await?;
    Ok((StatusCode::CREATED, Json(out.receipt)))
}

async fn post_render_audio_for_user_client(
    State(ctx): State<AppContext>,
    owner: AuthenticatedOwner,
    Query(q): Query<RenderQuery>,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    let field = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(format!("invalid multipart upload: {e}")))?
        .ok_or_else(|| ApiError::BadRequest("missing audio file part".into()))?;
    let audio_bytes = field
        .bytes()
        .await
        .map_err(|e| ApiError::BadRequest(format!("failed reading audio bytes: {e}")))?
        .to_vec();

    let out = translate_clip(
        &ctx,
        owner.discord_user_id,
        TranslateClipInput {
            profile_slug: q.profile.unwrap_or_else(|| "default_voice".into()),
            audio_bytes,
            delivery: DiscordDeliveryTarget::ClientReturnOnly,
        },
    )
    .await?;

    let (mime, payload) = if q.format.as_deref() == Some("wav") {
        ("audio/wav", out.wav_48k_bytes)
    } else {
        ("audio/ogg", out.ogg_opus_bytes)
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(
            "x-cer-bps",
            HeaderValue::from_str(&out.receipt.cer_bps.value().to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("0")),
        )
        .body(Body::from(payload))
        .map_err(|e| ApiError::Internal(format!("response build error: {e}")))
}
```

## 5. 100% Dockerized Build, Verification & Runtime Stack

### 5.1 Multi-Stage `Dockerfile` (`cargo-chef` + CUDA 12.6 + `verify-runner` + Slim Targets)
Uses [`LukeMathWalker/cargo-chef`](https://github.com/lukemathwalker/cargo-chef) so dependencies compile once and stay cached in Docker layers, providing dedicated targets for `verify-runner`, `db-migrate`, `gpu-runtime`, and `discord-worker`:

```dockerfile
# syntax=docker/dockerfile:1.7
FROM nvidia/cuda:12.6.3-devel-ubuntu24.04 AS rust-cuda-base
ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH="/usr/local/cargo/bin:${PATH}"

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential clang libclang-dev cmake pkg-config libssl-dev \
    libopus-dev ca-certificates curl git ffmpeg \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
    --default-toolchain stable -c rustfmt -c clippy \
    && cargo install --locked cargo-chef cargo-nextest cargo-deny sqlx-cli

WORKDIR /app

FROM rust-cuda-base AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM rust-cuda-base AS cacher
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --all-targets --all-features --recipe-path recipe.json

FROM rust-cuda-base AS verify-runner
COPY --from=cacher /app/target /app/target
COPY --from=cacher /usr/local/cargo /usr/local/cargo
COPY . .
CMD ["bash", "-euo", "pipefail", "-c", "\
    cargo fmt --all -- --check && \
    cargo clippy --workspace --all-targets --all-features -- -D warnings && \
    cargo test -p common -p domain && \
    cargo test -p api --test architecture_boundaries && \
    cargo nextest run --workspace --all-targets && \
    cargo deny check && \
    echo 'ALL DOCKERIZED ARCHITECTURE, LINT, AND TEST GATES PASSED.'"]

FROM rust-cuda-base AS builder
COPY --from=cacher /app/target /app/target
COPY --from=cacher /usr/local/cargo /usr/local/cargo
COPY . .
RUN cargo build --release --bins

FROM debian:bookworm-slim AS db-migrate
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
COPY --from=rust-cuda-base /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx
WORKDIR /app
COPY migrations ./migrations
ENTRYPOINT ["sqlx", "migrate", "run"]

FROM nvidia/cuda:12.6.3-runtime-ubuntu24.04 AS gpu-runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libopus0 curl ffmpeg && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/api-server /usr/local/bin/api-server
COPY --from=builder /app/target/release/cli /usr/local/bin/cli

FROM debian:bookworm-slim AS discord-worker
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/discord-worker /usr/local/bin/discord-worker
ENTRYPOINT ["/usr/local/bin/discord-worker"]
```

### 5.2 Unified `docker-compose.yml` (Auto Model Puller + Migrations + Runtime + Verify + CLI)

```yaml
services:
  model-puller:
    image: python:3.12-slim
    command:
      - bash
      - -c
      - |
        pip install --no-cache-dir -q huggingface_hub &&
        python3 - << 'PY'
        from huggingface_hub import hf_hub_download, snapshot_download
        hf_hub_download("ggerganov/whisper.cpp", "ggml-large-v3.bin", local_dir="/models/whisper")
        hf_hub_download("Qwen/Qwen3-8B-GGUF", "Qwen3-8B-Q8_0.gguf", local_dir="/models/llm")
        snapshot_download("coqui/XTTS-v2", local_dir="/models/tts/xtts-v2")
        PY
    volumes:
      - ./models:/models

  postgres:
    image: postgres:16-alpine
    restart: unless-stopped
    environment:
      POSTGRES_USER: voice_pipe
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:-local_voice_dev}
      POSTGRES_DB: voice_pipe
    volumes:
      - pg_voice_data:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U voice_pipe -d voice_pipe"]
      interval: 5s
      timeout: 3s
      retries: 5

  db-migrate:
    build:
      context: .
      target: db-migrate
    depends_on:
      postgres:
        condition: service_healthy
    environment:
      DATABASE_URL: postgres://voice_pipe:${POSTGRES_PASSWORD:-local_voice_dev}@postgres:5432/voice_pipe

  qwen-translator:
    image: ghcr.io/ggml-org/llama.cpp:server-cuda
    restart: unless-stopped
    depends_on:
      model-puller:
        condition: service_completed_successfully
    command:
      - "-m"
      - "/models/Qwen3-8B-Q8_0.gguf"
      - "--host"
      - "0.0.0.0"
      - "--port"
      - "8081"
      - "-ngl"
      - "99"
      - "-c"
      - "4096"
      - "--parallel"
      - "1"
    volumes:
      - ./models/llm:/models:ro
    deploy:
      resources:
        reservations:
          devices:
            - driver: nvidia
              count: 1
              capabilities: [gpu]
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://localhost:8081/health"]
      interval: 5s
      timeout: 3s
      retries: 10

  tts-clone-sidecar:
    build:
      context: ./sidecars/tts-worker
    restart: unless-stopped
    depends_on:
      model-puller:
        condition: service_completed_successfully
    environment:
      CUDA_VISIBLE_DEVICES: "0"
      PREWARM_ENGINES: "cosyvoice_3,xtts_v2"
    volumes:
      - ./models/tts:/models:ro
      - ./local_storage:/storage
    deploy:
      resources:
        reservations:
          devices:
            - driver: nvidia
              count: 1
              capabilities: [gpu]
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://localhost:8082/health"]
      interval: 5s
      timeout: 3s
      retries: 15

  api-server:
    build:
      context: .
      target: gpu-runtime
    entrypoint: ["/usr/local/bin/api-server"]
    restart: unless-stopped
    ports:
      - "${API_BIND_PORT:-8080}:8080"
    depends_on:
      db-migrate:
        condition: service_completed_successfully
      qwen-translator:
        condition: service_healthy
      tts-clone-sidecar:
        condition: service_healthy
    environment:
      DATABASE_URL: postgres://voice_pipe:${POSTGRES_PASSWORD:-local_voice_dev}@postgres:5432/voice_pipe
      OWNER_API_BEARER_TOKEN: ${OWNER_API_BEARER_TOKEN}
      DISCORD_BOT_TOKEN: ${DISCORD_BOT_TOKEN}
      DISCORD_OWNER_USER_ID: ${DISCORD_OWNER_USER_ID}
      WHISPER_MODEL_PATH: /models/whisper/ggml-large-v3.bin
      TRANSLATOR_BASE_URL: http://qwen-translator:8081
      TTS_SIDECAR_URL: http://tts-clone-sidecar:8082
      STORAGE_DIR: /storage
      MAX_CER_BPS: "1200"
      MAX_TTS_ATTEMPTS: "3"
    volumes:
      - ./models/whisper:/models/whisper:ro
      - ./local_storage:/storage
    deploy:
      resources:
        reservations:
          devices:
            - driver: nvidia
              count: 1
              capabilities: [gpu]
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://localhost:8080/health/ready"]
      interval: 10s
      timeout: 3s
      retries: 5

  discord-worker:
    build:
      context: .
      target: discord-worker
    restart: unless-stopped
    depends_on:
      api-server:
        condition: service_healthy
    environment:
      API_SERVER_INTERNAL_URL: http://api-server:8080
      OWNER_API_BEARER_TOKEN: ${OWNER_API_BEARER_TOKEN}
      DISCORD_BOT_TOKEN: ${DISCORD_BOT_TOKEN}
      DISCORD_APPLICATION_ID: ${DISCORD_APPLICATION_ID}
      DISCORD_OWNER_USER_ID: ${DISCORD_OWNER_USER_ID}

  postgres-test:
    image: postgres:16-alpine
    profiles: ["verify"]
    tmpfs:
      - /var/lib/postgresql/data
    environment:
      POSTGRES_USER: test_user
      POSTGRES_PASSWORD: test_password
      POSTGRES_DB: voice_pipe_test
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U test_user -d voice_pipe_test"]
      interval: 2s
      timeout: 2s
      retries: 5

  verify-runner:
    build:
      context: .
      target: verify-runner
    profiles: ["verify"]
    depends_on:
      postgres-test:
        condition: service_healthy
    environment:
      DATABASE_URL: postgres://test_user:test_password@postgres-test:5432/voice_pipe_test
    volumes:
      - cargo_verify_cache:/app/target

  cli:
    build:
      context: .
      target: gpu-runtime
    entrypoint: ["/usr/local/bin/cli"]
    profiles: ["cli"]
    depends_on:
      api-server:
        condition: service_healthy
    environment:
      DATABASE_URL: postgres://voice_pipe:${POSTGRES_PASSWORD:-local_voice_dev}@postgres:5432/voice_pipe
      API_SERVER_INTERNAL_URL: http://api-server:8080
      OWNER_API_BEARER_TOKEN: ${OWNER_API_BEARER_TOKEN}
      STORAGE_DIR: /storage
    volumes:
      - ./local_storage:/storage
      - ./evals:/evals:ro

volumes:
  pg_voice_data:
  cargo_verify_cache:
```

## 6. Comprehensive 7-Layer Testing, Validation & Boundary Architecture

To prevent coding agents from shipping tautological mock tests, every feature slice must satisfy **all seven automated verification layers** inside Docker (`just verify` + `just verify-gpu`):

| Layer | Location | Backing Harness | Speed | Exact Bugs & Regressions Caught |
| :--- | :--- | :--- | :--- | :--- |
| **L1. Inline Unit + Property Tests** | `crates/{common,domain}/src/**` | `cargo test` + [`proptest`](https://crates.io/crates/proptest) | `<15 ms` | Off-by-one `BasisPoints` rounding, unexpanded ASCII digits/symbols in `MandarinSpokenScript`, waveform panics on arbitrary PCM lengths. |
| **L2. Domain Golden Matrices** | `crates/domain/tests/*.rs` | Table-driven Rust suites | `<40 ms` | Mandarin homophone vs. hallucination CER thresholds, Gate 0 clipping (`50 bps`) and duration (`400..=120,000 ms`) edge cases. |
| **L3. Real Codec + Wiremock Contracts** | `crates/framework/tests/*.rs` | [`symphonia`](https://crates.io/crates/symphonia) + [`wiremock`](https://crates.io/crates/wiremock) | `~200 ms` | Broken 48 kHz RFC 7845 Opus framing, leaked `Authorization` header on GCS pre-signed `PUT` (`HTTP 403`), illegal `content` field with `flags: 8192` (`HTTP 400`). |
| **L4. Stateful Workflow + SQLx Tests** | `crates/features/tests/**/*.rs` | `#[sqlx::test]` (`postgres-test` tmpfs) | `~350 ms` | Seed-shifted TTS self-healing retry logic, exhausted-retry rollback (ensuring zero corrupted uploads), non-owner `403`, `Semaphore(1)` GPU contention. |
| **L5. Black-Box HTTP API Tests** | `crates/api/tests/v1/*.rs` | `tower::ServiceExt::oneshot` | `~250 ms` | `401` bearer auth, `422` RFC 7807 `application/problem+json` schema, `/render` `OggS` and `RIFF` binary magic headers + `x-cer-bps` header. |
| **L6. Architecture & Hygiene Scanner** | `crates/api/tests/architecture_boundaries.rs` | AST/filesystem test + `cargo-deny` | `<50 ms` | Forbidden `mod.rs` files, `>350 LOC` files, async/I/O crates inside `common`/`domain`, horizontal sibling feature imports, `std::env::var` leaks, banned `openssl`. |
| **L7. Live RTX 3090 GPU & Audio Gate** | `bins/cli` (`just verify-gpu`) | Real Whisper + Qwen3 + TTS + `ffprobe` | `~55 s` | Real Mandarin tonal intelligibility (`CER <= 1,000 bps`), ECAPA-TDNN voice clone similarity (`>= 7,800 bps`), `P95 <= 5,000 ms`, VRAM `<= 21.5 GB`. |

### 6.1 Layer 1 — Property-Based Domain Invariants (`crates/domain/src/translation/verifier.rs`)
Uses [`proptest`](https://crates.io/crates/proptest) so arbitrary CJK strings, emojis, and punctuation never panic or violate metric symmetry:

```rust
#[cfg(test)]
mod proptests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn identical_hanzi_always_scores_zero_cer_regardless_of_punctuation(
            cjk in "[\\u4e00-\\u9fa5]{1,80}",
            punct in "[，。！？、\\s]{0,10}"
        ) {
            let decorated = format!("{punct}{cjk}{punct}");
            let verdict = evaluate_cer_gate(&cjk, &decorated, BasisPoints::ZERO);
            prop_assert!(verdict.passed);
            prop_assert_eq!(verdict.cer_bps, BasisPoints::ZERO);
        }

        #[test]
        fn spoken_script_rejects_any_embedded_ascii_digit(
            prefix in "[\\u4e00-\\u9fa5]{1,20}",
            digit in "[0-9]",
            suffix in "[\\u4e00-\\u9fa5]{1,20}"
        ) {
            let raw = format!("{prefix}{digit}{suffix}");
            prop_assert!(MandarinSpokenScript::parse(&raw).is_err());
        }
    }
}
```

### 6.2 Layer 3 — Real Codec Demux & Discord Wiremock Contract Tests (`crates/framework/tests/`)
Never mock the Opus encoder or guess Discord's 3-step REST contract—verify both deterministically in `verify-runner`:

```rust
// crates/framework/tests/discord_and_sidecar_wiremock.rs
#![allow(clippy::unwrap_used, clippy::expect_used)]

use common::secret::Secret;
use domain::ports::{DiscordDeliveryTarget, DiscordPublisher, TranslatedMandarin};
use domain::translation::types::MandarinSpokenScript;
use framework::{
    clients::discord_publisher::HttpDiscordPublisher,
    dsp::opus_and_wav::RealAudioCodecEngine,
    testing::audio_fixtures::synthetic_24k_pcm,
};
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{header, header_exists, method, path},
};

#[tokio::test]
async fn bot_voice_bubble_omits_auth_on_gcs_put_and_sets_flag_8192_without_content() {
    let server = MockServer::start().await;
    let codec = RealAudioCodecEngine::new();
    let bundle = codec.encode_bundle(&synthetic_24k_pcm(1_500)).unwrap();

    // 1. Pre-signed upload slot request
    Mock::given(method("POST"))
        .and(path("/api/v10/channels/42/attachments"))
        .and(header("authorization", "Bot test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "attachments": [{
                "upload_url": format!("{}/gcs-upload/slot-0", server.uri()),
                "upload_filename": "gcs/voice-message.ogg"
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    // 2. GCS PUT must have audio/ogg and NO Discord Authorization header (otherwise GCS 403s)
    Mock::given(method("PUT"))
        .and(path("/gcs-upload/slot-0"))
        .and(header("content-type", "audio/ogg"))
        .respond_with(|req: &wiremock::Request| {
            assert!(
                !req.headers.contains_key("authorization"),
                "GCS pre-signed PUT must NOT leak Bot Authorization header"
            );
            assert_eq!(&req.body[0..4], b"OggS", "Uploaded bytes must be real Ogg container");
            ResponseTemplate::new(200)
        })
        .expect(1)
        .mount(&server)
        .await;

    // 3. Final message POST must set flags=8192 and omit `content`/`embeds`
    Mock::given(method("POST"))
        .and(path("/api/v10/channels/42/messages"))
        .and(header_exists("authorization"))
        .respond_with(|req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            assert_eq!(body["flags"], 8192);
            assert!(body.get("content").is_none(), "flags=8192 payload cannot include content");
            assert!(body["attachments"][0]["waveform"].as_str().unwrap().len() >= 4);
            ResponseTemplate::new(200).set_body_json(json!({ "id": "99887766" }))
        })
        .expect(1)
        .mount(&server)
        .await;

    let publisher = HttpDiscordPublisher::with_base_url(server.uri(), Secret::new("test-token".into()));
    let msg_id = publisher
        .publish(
            &DiscordDeliveryTarget::BotVoiceBubble { channel_id: 42, reply_to_message_id: None },
            &bundle,
            &TranslatedMandarin {
                spoken_hanzi: MandarinSpokenScript::parse("你好世界").unwrap(),
                pinyin: "nǐ hǎo shì jiè".into(),
                english_back_gloss: "Hello world".into(),
            },
        )
        .await
        .unwrap();

    assert_eq!(msg_id, Some(99_887_766));
}
```

### 6.3 Layer 4 & Layer 5 — Stateful Retry Rollback (`#[sqlx::test]`) & Black-Box HTTP (`oneshot`)

```rust
// crates/features/tests/voice_jobs/translate_clip_test.rs
#![allow(clippy::unwrap_used, clippy::expect_used)]

use domain::ports::DiscordDeliveryTarget;
use features::{error::AppError, voice_jobs::translate_clip::{TranslateClipInput, translate_clip}};
use framework::testing::{audio_fixtures::synthetic_speech_wav, test_context::build_test_context};
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrations = "../../migrations")]
async fn aborts_without_publishing_or_db_commit_when_all_3_tts_seeds_fail_cer(pool: PgPool) {
    let (ctx, counters) = build_test_context_with_always_mumbling_tts(pool);
    let owner_id: i64 = 777_888_999;

    sqlx::query(
        "INSERT INTO voice_profiles (id, slug, display_name, reference_wav_path, reference_transcript_en, preferred_engine, owner_discord_user_id, created_at)
         VALUES ($1, 'default_voice', 'Alex', '/tmp/ref.wav', 'Reference clip.', 'cosyvoice_3', $2, now())"
    )
    .bind(Uuid::now_v7())
    .bind(owner_id)
    .execute(&ctx.db)
    .await
    .unwrap();

    let err = translate_clip(
        &ctx,
        u64::try_from(owner_id).unwrap(),
        TranslateClipInput {
            profile_slug: "default_voice".into(),
            audio_bytes: synthetic_speech_wav(),
            delivery: DiscordDeliveryTarget::BotVoiceBubble { channel_id: 42, reply_to_message_id: None },
        },
    )
    .await
    .unwrap_err();

    assert!(matches!(err, AppError::Internal(_)));
    assert_eq!(counters.tts_calls(), 3, "Must attempt all 3 seed shifts");
    assert_eq!(counters.discord_publishes(), 0, "Must NEVER publish unverified audio to Discord");

    let saved_jobs: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM voice_jobs")
        .fetch_one(&ctx.db)
        .await
        .unwrap();
    assert_eq!(saved_jobs.0, 0);
}
```

### 6.4 Layer 6 — Upgraded Architecture & Horizontal-Import Scanner (`crates/api/tests/architecture_boundaries.rs`)
Enforces all four structural laws in `<50ms`: (1) zero `mod.rs`, (2) zero async/I/O crates in `common`/`domain`, (3) `<= 350 LOC` production source ceiling, and (4) **zero horizontal imports between sibling capabilities in `crates/features/src/`**:

```rust
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    fs,
    path::{Path, PathBuf},
};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
}

#[test]
fn forbids_legacy_mod_rs_files_across_workspace() {
    let root = workspace_root();
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files);
    collect_rs_files(&root.join("bins"), &mut files);

    let violations: Vec<_> = files
        .into_iter()
        .filter(|p| p.file_name().and_then(|n| n.to_str()) == Some("mod.rs"))
        .collect();
    assert!(violations.is_empty(), "Forbidden mod.rs files detected: {violations:?}");
}

#[test]
fn enforces_pure_dependencies_in_common_and_domain() {
    let root = workspace_root();
    let forbidden = [
        "tokio", "sqlx", "axum", "reqwest", "tower", "hyper", "serenity", "poise", "songbird", "whisper-rs",
    ];
    for crate_name in ["common", "domain"] {
        let cargo_toml =
            fs::read_to_string(root.join("crates").join(crate_name).join("Cargo.toml")).unwrap();
        for banned in forbidden {
            assert!(
                !cargo_toml.contains(banned),
                "crates/{crate_name}/Cargo.toml must stay 100% pure and cannot depend on `{banned}`"
            );
        }
    }
}

#[test]
fn forbids_horizontal_imports_between_sibling_feature_capabilities() {
    let features_src = workspace_root().join("crates/features/src");
    let capabilities = ["voice_jobs", "profiles", "benchmarks"];

    for &cap in &capabilities {
        let cap_dir = features_src.join(cap);
        let mut files = Vec::new();
        collect_rs_files(&cap_dir, &mut files);

        for file in files {
            let content = fs::read_to_string(&file).unwrap();
            for &sibling in &capabilities {
                if sibling != cap {
                    let banned_import = format!("crate::{sibling}");
                    assert!(
                        !content.contains(&banned_import),
                        "Horizontal sibling feature import `{banned_import}` forbidden in {file:?}; move shared SQL to `shared_queries/` or pure logic to `crates/domain`."
                    );
                }
            }
        }
    }
}

#[test]
fn enforces_production_source_file_line_ceiling() {
    let root = workspace_root();
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files);

    let mut oversized = Vec::new();
    for file in files {
        if file.components().any(|c| c.as_os_str() == "src") {
            let content = fs::read_to_string(&file).unwrap();
            let prod_code = content.split("#[cfg(test)]").next().unwrap_or(&content);
            let line_count = prod_code.lines().count();
            if line_count > 350 {
                oversized.push((file, line_count));
            }
        }
    }
    assert!(oversized.is_empty(), "Production src/ files exceeded 350 LOC limit: {oversized:?}");
}
```

### 6.5 Docker-First `Justfile` & Root `AGENTS.md`

```makefile
set shell := ["bash", "-euo", "pipefail", "-c"]

# Boot the entire RTX 3090 production stack in Docker (pulls models, runs migrations, starts API + bot)
up:
    docker compose up -d --build

# Fast pure unit + proptest + golden matrix + architecture boundary checks inside Docker (<3s warm)
test-fast:
    docker compose --profile verify run --rm --no-deps verify-runner \
      bash -euo pipefail -c "cargo test -p common -p domain && cargo test -p api --test architecture_boundaries"

# Format + strict zero-warning Clippy + Python sidecar pytest inside Docker
lint:
    docker compose --profile verify run --rm --no-deps verify-runner \
      bash -euo pipefail -c "cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings"

# MANDATORY CI/AGENT GATE: Runs all 6 non-GPU test layers (fmt, clippy, proptest, wiremock, tmpfs sqlx, oneshot HTTP, cargo-deny)
verify:
    docker compose --profile verify run --rm verify-runner

# Enroll a 15-second 24kHz reference WAV into the Docker stack
enroll slug wav_name transcript:
    docker compose --profile cli run --rm cli enroll \
      --slug "{{slug}}" \
      --wav "/storage/voice_profiles/{{wav_name}}" \
      --transcript "{{transcript}}"

# LAYER 7 LIVE HARDWARE GATE: Runs CUDA readiness probe + 15-clip A/B benchmark & persists benchmark_runs row
verify-gpu:
    docker compose --profile cli run --rm cli doctor
    docker compose --profile cli run --rm cli bench --manifest /evals/golden_manifest.json --out /storage/reports/scorecard.md
```

```markdown
# AGENTS.md — Operational Contract for AI Coding Agents

## 1. 100% Dockerized Verification Commands (Never Assume Host `cargo` Exists)
- Fast unit + proptest + boundary check: `just test-fast`
- Mandatory pre-completion gate: `just verify` (executes Layers 1–6 inside `verify-runner` against tmpfs Postgres)
- Live RTX 3090 GPU benchmark: `just verify-gpu` (executes Layer 7 hardware + audio quality suite)

## 2. 5-Crate One-Way Dependency Graph (`bins -> api -> features -> framework -> domain -> common`)
- `crates/common/src/`: Generic sync utilities (`BasisPoints`, `Clock`, `Secret<T>`, `define_id!`).
- `crates/domain/src/`: 100% sync functional core (`MandarinSpokenScript`, `audio::gate`, `audio::waveform`, `translation::verifier` CER in `BasisPoints`, narrow `ports.rs`).
- `crates/framework/src/`: `AppConfig` (sole `std::env::var` reader), `AppContext`, `PgPool`, Symphonia/Opus/WAV DSP, Whisper/Qwen/TTS/Discord clients (`USER_INSTALL`, Profile Webhook, `flags: 8192`), `Fake*` test doubles (`test-support`).
- `crates/features/src/`: Plain `pub async fn` workflows + private single-use SQLx queries (`shared_queries/` for 2+ workflows).
- `crates/api/src/controllers/v1/`: Thin 5–15 line Axum handlers (`/v1/voice-jobs` + `/v1/voice-jobs/render`) + RFC 7807 `ApiError`.
- `bins/{api-server, discord-worker, cli}/src/main.rs`: `<100 LOC` container entrypoints.

## 3. Non-Negotiable Testing & Code Prohibitions
- NEVER mark a task complete until `just verify` exits 0 with new tests covering the touched tier (Domain matrix/proptest, Wiremock/Opus round-trip, `#[sqlx::test]`, and `oneshot` HTTP).
- NEVER implement prohibited Discord user-token scraping; use `ClientReturnOnly` (`/v1/voice-jobs/render`), `USER_INSTALL` interactions, Profile Webhooks, or Bot `flags: 8192`.
- NEVER create `mod.rs` files, horizontal sibling imports in `crates/features`, or `src/*.rs` files exceeding 350 non-test lines.
- NEVER call `.unwrap()`, `.expect()`, `panic!()`, or `std::env::var` in production code, and NEVER silence lints with `#[allow(...)]` in `src/`.
```

## 7. Sequential 5-Phase Dockerized Rollout & 6-Metric RTX 3090 Release Gate

### 7.1 Gated Agent Implementation Phases
1. **Phase 1 — Docker Toolchain, Workspace Skeleton & Layer-6 Boundary Gate:** Commit `Dockerfile`, `docker-compose.yml`, `Justfile`, workspace `Cargo.toml` lints, `clippy.toml`, `rustfmt.toml`, `deny.toml`, `migrations/`, and `crates/api/tests/architecture_boundaries.rs`. Gate: `just verify` exits `0` inside Docker.
2. **Phase 2 — `crates/common` & `crates/domain` + Layers 1–2 (`proptest` & Golden Matrices):** Build `BasisPoints`, `Secret<T>`, `MandarinSpokenScript` (digit/symbol guard), `audio::gate`, `audio::waveform`, `translation::verifier` (CJK Levenshtein CER), inline `proptest` suites, and `crates/domain/tests/{audio_gate_matrix,cer_verifier_matrix}.rs`. Gate: `just test-fast` exits `0`.
3. **Phase 3 — `crates/framework` & `tts-worker` + Layer 3 (Codec Round-Trip & Wiremock):** Build `AppConfig`, `AppContext`, `PgPool`, Opus/WAV DSP (`opus_and_wav_roundtrip.rs`), `whisper-rs` CUDA adapter, `Qwen3-8B` client, `tts-worker` (`CosyVoice 3.0` + `XTTS-v2` + `pytest`), `DiscordPublisher` (`discord_and_sidecar_wiremock.rs`), and stateful `Fake*` doubles. Gate: `just verify` exits `0`.
4. **Phase 4 — `crates/features`, `crates/api` (`/render`) & Thin `bins/*` + Layers 4–5 (`#[sqlx::test]` & `oneshot`):** Implement `translate_clip`, `enroll_profile`, and `run_rtx3090_suite` with tmpfs Postgres integration tests (retry self-healing, rollback, auth, GPU `Semaphore(1)`), thin Axum controllers with RFC 7807 + `OggS`/`RIFF` `oneshot` tests, and `<100 LOC` `bins/*`. Gate: `just verify` exits `0`.
5. **Phase 5 — Layer 7 Live RTX 3090 Calibration & Personal Discord Setup:** Enable **User Install** (`integration_types: [1]`, `contexts: [0, 1, 2]`) on your private Discord app, boot `docker compose up -d --build` on your RTX 3090 server, enroll your 15s `24 kHz` WAV voice clip (`just enroll`), and require `just verify-gpu` to pass all 6 scorecard gates below before live use.

### 7.2 Layer 7 Six-Metric Containerized RTX 3090 Release Scorecard (`just verify-gpu`)

| # | Metric | Exact Automated Check Across 15 Golden Utterances + 3 Traps | Hard Pass Gate (`BasisPoints` / Latency) |
| :--- | :--- | :--- | :--- |
| **M1** | **Gate 0 Trap Rejection** | Silent (`<400 ms`), clipped (`>50 bps`), and runaway (`>120 s`) traps rejected pre-GPU | **`10,000 bps` (`100.00%`)** (`0` GPU calls leaked) |
| **M2** | **Spoken Hanzi Digit Hygiene** | `MandarinSpokenScript::parse` confirms `0` unexpanded ASCII digits `[0-9]` or `$`/`%` symbols | **`10,000 bps` (`100.00%`)** clean spoken Hanzi |
| **M3** | **Mandarin Round-Trip CER** | Mean CJK Levenshtein error rate (`evaluate_cer_gate`) via `whisper-rs` (`lang = "zh"`) back-transcription | **`Mean <= 1,000 bps` (`10.00%`)**, max clip `<= 1,200 bps` |
| **M4** | **Speaker Clone Similarity** | Cosine similarity (ECAPA-TDNN / CampPlus in `tts-clone-sidecar`) vs. enrolled `24 kHz` English reference WAV | **`>= 7,800 bps` (`78.00%`)** |
| **M5** | **Dual-Format Spec (`ffprobe`)** | `ffprobe` + Symphonia verify `48 kHz` mono Opus `.ogg` (waveform `1..=256` bytes, drift `<40 ms`) & `48 kHz` `.wav` | **`10,000 bps` (`100.00%`)** (`15/15` clips valid) |
| **M6** | **E2E Latency & Single-Load VRAM** | `P95` latency for 10s clip via `POST /api/v1/voice-jobs/render` + peak `nvidia-smi` across all GPU containers | **`P95 <= 5,000 ms`** & **`Peak VRAM <= 21.5 GB`** |
