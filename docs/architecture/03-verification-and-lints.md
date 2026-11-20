# 03 — Dockerized Verification, Lints & CUDA Build Hygiene

## 1. Docker Verification Commands (`Justfile`)

All toolchains (`rustc`, `clippy`, `rustfmt`, `cargo-nextest`, `cargo-deny`, `sqlx-cli`, `clang`, `cmake`, `libopus-dev`, `ffmpeg`, `pytest`) are pinned inside `Dockerfile` and `sidecars/tts-worker/Dockerfile`:

| Command | Container Target | Execution Scope |
| :--- | :--- | :--- |
| `just test-fast` | `verify-runner` (`--no-deps`) | Runs `cargo test -p common -p domain` and `cargo test -p api --test architecture_boundaries` in `< 3s` warm. |
| `just verify-sidecar` | `tts-sidecar-test` | Builds `./sidecars/tts-worker` and executes `pytest -v` on CPU without requiring GPU weights. |
| `just lint` | `verify-runner` + `tts-sidecar-test` | Enforces `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and sidecar `pytest`. |
| `just verify` | `verify-runner` + `postgres-test` + `tts-sidecar-test` | **Mandatory pre-completion gate:** executes formatting, Clippy (`--all-features`), pure domain tests, boundary scanner, `cargo nextest` across all workspace crates against ephemeral `tmpfs` Postgres 16, `cargo deny check`, and sidecar `pytest`. |
| `just verify-gpu` | `cli` (`gpu-runtime`) | Runs `cli doctor` + `cli bench` against live RTX 3090 containers (`api-server`, `qwen-translator`, `tts-clone-sidecar`). |

## 2. CUDA Stub Linking vs. Runtime Driver Binding

`whisper-rs` (`cuda` feature) compiles CUDA kernels (`CMAKE_CUDA_ARCHITECTURES="86"` for RTX 3090 Ampere) and links against `libcuda.so.1` during `docker build`, when the NVIDIA host driver is intentionally absent:

- **Build & CPU CI stages (`rust-cuda-base` / `verify-runner`):** `rust-cuda-base` sets `LIBRARY_PATH="/usr/local/cuda/lib64/stubs"` and symlinks `/usr/local/cuda/lib64/stubs/libcuda.so.1 -> libcuda.so` for link-time resolution. Only `verify-runner` sets `LD_LIBRARY_PATH="/usr/local/cuda/lib64/stubs:/usr/local/cuda/lib64"` so `--all-features` Clippy/test binaries load dynamically on CPU-only runners.
- **Production GPU stage (`gpu-runtime`):** Uses `nvidia/cuda:12.6.3-runtime-ubuntu24.04` with **no** CUDA stubs in `LD_LIBRARY_PATH`, ensuring NVIDIA Container Toolkit mounts and binds the host RTX 3090's real `libcuda.so.1` at container startup.
- **Ubuntu 24.04 ABI parity (`db-migrate`, `discord-worker`):** Because `rust-cuda-base` compiles Rust binaries against Ubuntu 24.04 (`glibc 2.39`, `libssl3t64`), CPU runtime stages use `FROM ubuntu:24.04` with `ca-certificates` and `libssl3t64` to guarantee exact shared-library ABI compatibility.

## 3. Compiler, Clippy, Supply-Chain & Structural Enforcement

1. **Workspace lints (`Cargo.toml`):** `unsafe_code = "forbid"`, `unused_must_use = "deny"`, `clippy::all = "deny"`, `clippy::pedantic = "warn"` (promoted to hard error via `-D warnings`), plus `deny` on `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, `dbg_macro`, and `disallowed_methods`.
2. **Environment isolation (`clippy.toml` + `architecture_boundaries.rs`):** `std::env::var` and `std::env::var_os` are disallowed workspace-wide. Only `crates/framework/src/config.rs` reads environment state via `std::env::vars()`. `clippy.toml` enables `allow-unwrap-in-tests`, `allow-expect-in-tests`, and `allow-panic-in-tests` so test code never needs `#[allow(...)]`.
3. **Supply-chain & TLS purity (`deny.toml`):** `cargo deny check` verifies OSI-approved licenses, crates.io-only registry provenance, RustSec advisories, and bans `openssl` / `openssl-sys` across Rust dependencies in favor of `rustls`.
4. **Structural boundary scanner (`crates/api/tests/architecture_boundaries.rs`):** Fails in `< 50 ms` on any `mod.rs` file, impure dependency in `common`/`domain`, upward dependency violation, horizontal import between `features` capabilities, `> 350` non-test lines in `crates/**/src/**`, `>= 100` lines in `bins/*/src/main.rs`, `#[allow(` / `#![allow(` in `src/`, or direct `env::var(` / `env::var_os(` call.
