# AGENTS.md — Operational Contract

## Execution Loop & Completion Criteria

Run every build, lint, migration, and test step through Docker Compose inside WSL2 / Linux via `Makefile` (`make ...`) or `Justfile` (`just ...`). Never assume `cargo`, `nvcc`, `python`, `node`, or `libopus` exist on the host OS.

1. **Orient on the target tier** before editing code, following the context pointers below when crossing crate boundaries, adding test seams, or troubleshooting lints.
2. **Drive changes test-first (`red -> green -> refactor`)** at the touched tier's public interface using independent worked examples, boundary literals, or protocol invariants.
3. **Run fast feedback** during iteration:
   ```bash
   make test-fast      # (or: just test-fast)
   ```
   *Criterion:* `common` + `domain` unit/property/golden suites and `crates/api/tests/architecture_boundaries.rs` exit `0`.
4. **Run the mandatory pre-completion gate** before declaring any task done:
   ```bash
   make verify         # (or: just verify)
   ```
   *Exhaustive completion criterion:* `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `common`/`domain` tests, `architecture_boundaries`, `cargo nextest` across workspace targets against `postgres-test` (`tmpfs`), `cargo deny check`, `tts-sidecar-test` (`pytest`), and `web-ui-test` (`tsc --noEmit`, `vitest`, `vite build`) all exit `0`, with every modified behavior covered at its tier seam.
5. **Run the live RTX 3090 hardware release gate** whenever calibrating inference models, prompts, or audio synthesis:
   ```bash
   just verify-gpu
   ```
   *Criterion:* `cli doctor` and `cli bench` pass all six `M1–M6` thresholds in `evals/golden_manifest.json` and persist a passing `benchmark_runs` row.

## 5-Crate One-Way Dependency Graph

Dependencies point strictly downward: `bins/* -> api -> features -> framework -> domain -> common`.

- `crates/common/src/`: Synchronous generic primitives (`BasisPoints`, `Clock`, `Secret<T>`, `define_id!`, `PageParams`, `sha256`). Zero business vocabulary, zero async/I/O crates.
- `crates/domain/src/`: Synchronous functional core (`Pcm16kMono`, `MandarinSpokenScript`, `audio::gate`, `audio::waveform`, `translation::verifier` CER in `BasisPoints`, narrow external traits in `ports.rs`). Zero async runtime, DB, or HTTP dependencies.
- `crates/framework/src/`: Concrete infrastructure (`AppConfig` reading `std::env::vars()`, `AppContext` holding `PgPool` and single-GPU `Semaphore(1)`, Symphonia/Rubato/Opus/WAV DSP, Whisper/Qwen/TTS/Discord adapters, and `test-support` `Fake*` doubles).
- `crates/features/src/`: Transport-agnostic `pub async fn` workflows (`voice_jobs`, `profiles`, `benchmarks`) executing direct SQLx queries. Place SQL shared by two or more capabilities in `shared_queries/`.
- `crates/api/src/`: Thin 5–15 line Axum v1 handlers (`/api/v1/voice-jobs`, `/api/v1/voice-jobs/render`), `AuthenticatedOwner` extractor, and RFC 7807 `ApiError`.
- `bins/{api-server,discord-worker,cli}/src/main.rs`: Container entrypoints (`< 100` lines each).

## Target Engineering Rules

- **Module layout:** Declare module roots as named files (`audio.rs` + `audio/gate.rs`). Zero `mod.rs` files anywhere in `crates/` or `bins/`.
- **Source size budgets:** Keep every file in `crates/**/src/**` at `<= 350` lines prior to `#[cfg(test)]`, and every `bins/*/src/main.rs` under `100` lines.
- **Total error handling:** Propagate fallible paths with typed `Result<T, E>` (`thiserror`). Production code in `src/` contains zero `.unwrap()`, `.expect()`, `panic!()`, `todo!()`, `unimplemented!()`, `dbg!()`, `unsafe`, or `#[allow(...)]` / `#![allow(...)]` attributes.
- **Centralized configuration:** Read process environment variables exclusively inside `crates/framework/src/config.rs` via `std::env::vars()`. `std::env::var` and `std::env::var_os` are banned workspace-wide.
- **Vertical capability isolation:** Keep `voice_jobs`, `profiles`, and `benchmarks` inside `crates/features/src/` independent of one another; promote shared SQL to `shared_queries/` and pure business logic to `crates/domain`.
- **Compliant Discord user-account delivery:** Deliver audio strictly via `ClientReturnOnly` (`POST /api/v1/voice-jobs/render` feeding BlackHole/VB-Cable or iOS/macOS share), `UserInstallFollowup` (`integration_types: [1]`, `contexts: [0, 1, 2]`), `ProfileWebhook`, or `BotVoiceBubble` (`flags: 8192`). Never automate personal Discord user session tokens.

## Context Pointers (Disclosed Architecture Reference)

- **Crate placement, vertical feature slices, or Discord delivery modes** -> read [`docs/architecture/01-crate-boundaries.md`](docs/architecture/01-crate-boundaries.md).
- **Writing unit/property tests, codec round-trips, Wiremock contracts, `#[sqlx::test]`, or `oneshot` API tests** -> read [`docs/architecture/02-testing-strategy.md`](docs/architecture/02-testing-strategy.md).
- **Troubleshooting Clippy/Cargo-deny failures, `architecture_boundaries.rs`, or Docker CUDA stub vs. runtime linking** -> read [`docs/architecture/03-verification-and-lints.md`](docs/architecture/03-verification-and-lints.md).
- **Full SQL schema, canonical Rust port signatures, VRAM budget, or `M1–M6` release scorecard** -> read [`docs/architecture/00-blueprint.md`](docs/architecture/00-blueprint.md).
- **WSL2 on Windows Docker setup, `Makefile` targets, `.env` hardware parameters, or unattended agent onboarding** -> read [`AGENT_INSTALL.md`](AGENT_INSTALL.md).
