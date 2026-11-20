# 02 — 7-Layer Testing Strategy & Seam Discipline

## 1. The 7-Layer Verification Pyramid

Every change is verified at the narrowest deterministic layer that owns the behavior, culminating in `just verify` (Layers 1–6 + Python sidecar `pytest`) and `just verify-gpu` (Layer 7 on RTX 3090):

| Layer | Suite Location | Runner & Backing Seam | Target Invariants |
| :--- | :--- | :--- | :--- |
| **L1. Inline Unit + Property** | `crates/{common,domain}/src/**` | `cargo test` + `proptest` | `BasisPoints` rounding/clamping, `Secret<T>` Debug/Display redaction, `MandarinSpokenScript` ASCII-digit/symbol rejection, arbitrary-length PCM 256-bin waveform safety, CJK CER symmetry. |
| **L2. Domain Golden Matrices** | `crates/domain/tests/*.rs` | Table-driven pure Rust tests | 20+ Gate-0 boundary cases (`399 ms` vs `400 ms`, `50 bps` vs `51 bps` clipping, `120,000 ms` vs `120,001 ms`) and 25+ Mandarin CER & punctuation-stripping cases. |
| **L3. Real Codec + Wiremock** | `crates/framework/tests/*.rs` + `sidecars/tts-worker/test_server.py` | Symphonia demux + `wiremock` + `pytest` | Encoded `48 kHz` Ogg/Opus (`OggS`) & WAV (`RIFF`) demux cleanly with `< 40 ms` duration drift; GCS `PUT` omits Discord `Authorization` header; `flags: 8192` payload omits `content`/`embeds`; sidecar HTTP 503 retries; Python FastAPI tensor/schema contracts. |
| **L4. Stateful Workflow + DB** | `crates/features/tests/**/*.rs` | `#[sqlx::test]` against `postgres-test` (`tmpfs`) | Seed-shifted TTS self-healing (`attempt 1` fails CER -> `attempt 2` passes), 3-attempt exhaustion aborts with `0` Discord publishes and `0` committed `voice_jobs` rows, non-owner `Forbidden`, `Semaphore(1)` serialization. |
| **L5. Black-Box HTTP API** | `crates/api/tests/v1/*.rs` | `tower::ServiceExt::oneshot` | Missing/invalid Bearer token returns `401`; Gate-0 rejection returns `422` `application/problem+json` (RFC 7807); `/v1/voice-jobs/render` returns `OggS` or `RIFF` magic bytes and `x-cer-bps` header. |
| **L6. Architecture Scanner** | `crates/api/tests/architecture_boundaries.rs` + `cargo deny` | Filesystem/token test (`< 50 ms`) | Zero `mod.rs`, pure `common`/`domain` manifests, one-way crate DAG, zero horizontal sibling feature imports, `<= 350` src LOC, `< 100` bin LOC, zero `#[allow(` in `src/`, zero `env::var(`, banned `openssl`. |
| **L7. Live RTX 3090 Gate** | `bins/cli` (`just verify-gpu`) | Real CUDA Whisper + Qwen3-8B + CosyVoice/XTTS + `ffprobe` | Evaluates `evals/golden_manifest.json` (15 clips + 3 traps) against `M1–M6` thresholds and writes `scorecard.md` + `benchmark_runs` row. |

## 2. Non-Tautological Seam Rules

1. **Compute expected values independently:** Assert against hand-calculated worked literals, RFC magic bytes (`b"OggS"`, `b"RIFF"`), and protocol-mandated JSON shapes rather than copying production formulas into test assertions.
2. **Never mock pure engines or local codecs:** Always run the real `domain::audio::gate`, `domain::audio::waveform`, `domain::translation::verifier`, and `framework::dsp` (Symphonia/Rubato/Opus/WAV) implementations inside L3–L5 tests.
3. **Use stateful `Fake*` doubles only at external GPU/network boundaries:** `crates/framework/src/testing/` exposes deterministic `FakeSpeechRecognizer`, `FakeMandarinTranslator`, `FakeVoiceSynthesizer` (supporting scripted per-attempt seed responses), `FakeDiscordPublisher`, `FixedClock`, and `FakeCallCounters` so L4 and L5 tests verify retry state machines and side-effect ordering without a physical GPU.
4. **Verify failure-path non-effects:** Every rollback or Gate-0 rejection test must assert not only the returned error variant, but also that downstream call counters (`tts_calls`, `discord_publishes`) and database row counts (`SELECT COUNT(*) FROM voice_jobs`) match exact invariants.
