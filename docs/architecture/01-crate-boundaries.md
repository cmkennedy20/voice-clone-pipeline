# 01 — Crate Boundaries & Vertical Slice Rules

## 1. Strict 5-Crate + Binary DAG

Every Cargo manifest in the workspace obeys a strict downward dependency order enforced both by Cargo and by `crates/api/tests/architecture_boundaries.rs`:

```text
bins/{api-server, discord-worker, cli}
  └── crates/api          (Tier 5: Axum v1 routes, AuthenticatedOwner, RFC 7807 ApiError)
        └── crates/features     (Tier 4: Plain `pub async fn` workflows + SQLx queries)
              └── crates/framework    (Tier 3: AppConfig, AppContext, DSP, GPU/HTTP clients, Fake* doubles)
                    └── crates/domain       (Tier 2: 100% sync newtypes, Gate 0, CER verifier, narrow ports)
                          └── crates/common       (Tier 1: 100% sync BasisPoints, Secret<T>, Clock, define_id!)
```

### Placement Decision Table

| Code Artifact | Target Crate & Path | Boundary Invariant |
| :--- | :--- | :--- |
| Generic rate math, redacted secrets, UUIDv7 wrappers, SHA-256 | `crates/common/src/` | Synchronous, zero domain nouns, zero I/O crates. |
| PCM/script newtypes, Gate-0 audio evaluation, 256-bin waveform quantization, CJK Levenshtein CER, narrow port traits | `crates/domain/src/` | Synchronous functional core (`async_trait` signatures allowed solely on port traits in `ports.rs`). Zero `tokio`, `sqlx`, `axum`, `reqwest`, `serenity`, or `whisper-rs`. |
| Environment parsing (`std::env::vars()`), `PgPool`, GPU `Semaphore(1)`, Symphonia/Rubato/Opus/WAV codec, Whisper/Qwen/TTS/Discord clients | `crates/framework/src/` | Implements `domain::ports::*` and provides stateful `Fake*` test doubles behind `#[cfg(any(test, feature = "test-support"))]`. |
| Multi-step orchestration (decode -> Gate 0 -> EN ASR -> Qwen ZH -> seed-shifted TTS + ZH CER -> Opus mux -> DB ledger -> Discord publish) | `crates/features/src/` | Plain `pub async fn` workflows taking `&AppContext`. Zero Axum/HTTP types. |
| Multipart/JSON request parsing, Bearer auth extraction, binary `.ogg`/`.wav` HTTP responses, RFC 7807 Problem Details | `crates/api/src/` | Thin 5–15 line controllers delegating immediately to `features::*`. |
| Process startup, tracing initialization, signal shutdown, CLI argument dispatch | `bins/*/src/main.rs` | `< 100` total lines per `main.rs`; `discord-worker` stays CPU-only and calls `api-server` over HTTP so Whisper loads into RTX 3090 VRAM once. |

## 2. Named Module Roots & File Size Budgets

- **Named module roots:** Pair `foo.rs` with `foo/bar.rs` (for example, `crates/domain/src/audio.rs` + `crates/domain/src/audio/gate.rs`). Never create `mod.rs`.
- **350-line production ceiling:** Every `.rs` file under `crates/**/src/**` must stay `<= 350` lines above `#[cfg(test)]`. Split cohesive submodules early by responsibility (`types.rs`, `gate.rs`, `waveform.rs`, `error.rs`) rather than compressing formatting.
- **100-line entrypoint ceiling:** Every `bins/*/src/main.rs` must stay `< 100` lines.

## 3. Vertical Slice Isolation in `crates/features/src/`

Feature capabilities (`voice_jobs`, `profiles`, `benchmarks`, `desktop`) are sibling vertical slices:

- A file inside `voice_jobs` (`voice_jobs.rs` or `voice_jobs/*.rs`) never imports `crate::profiles` or `crate::benchmarks` (and vice versa).
- When two or more capabilities need the same database read (such as loading an enrolled `voice_profiles` row by slug and verifying `owner_discord_user_id`), place that query in `crates/features/src/shared_queries/`.
- When two capabilities share pure validation or calculation logic, push that logic down into `crates/domain`.

## 4. Compliant Discord User-Anchored Delivery Modes

`domain::ports::DiscordDeliveryTarget` models all four compliant delivery targets without touching personal user session tokens:

1. **`ClientReturnOnly` (Modes A1 & A2):** `POST /api/v1/voice-jobs/render?format=wav|ogg` returns verified `48 kHz` WAV (for macOS BlackHole / Windows VB-Cable virtual microphone live calls) or verified `48 kHz` Ogg/Opus (for iOS Shortcut share sheet or desktop clipboard paste) so transmission originates 100% from the user's real Discord client.
2. **`UserInstallFollowup` (Mode B):** Replies to a `USER_INSTALL` slash interaction (`integration_types: [1]`, `contexts: [0, 1, 2]`), enabling 1-command voice translation in 1:1 DMs, Group DMs, and external servers where no bot is installed.
3. **`ProfileWebhook` (Mode C1):** Executes a channel webhook overriding `username` and `avatar_url` to match the speaker's profile.
4. **`BotVoiceBubble` (Mode C2):** Performs Discord's 3-step attachment slot request -> unauthenticated GCS `PUT` (`Content-Type: audio/ogg`) -> message `POST` with `flags: 8192` (`IS_VOICE_MESSAGE`), `duration_secs`, and base64 256-bin `waveform` (omitting `content` and `embeds` per Discord REST validation rules).
