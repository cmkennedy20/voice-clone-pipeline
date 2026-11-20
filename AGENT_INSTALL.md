# AGENT_INSTALL.md — Self-Contained WSL2 on Windows Install & Configuration Contract

This repository is **100% self-contained** — zero external CI/CD pipelines, zero GitHub Actions, and zero host Rust/Python/Node/CUDA SDK installs are required. Run every command inside **WSL2 on Windows** (Ubuntu/Debian) using the root [`Makefile`](Makefile) and documented scripts in [`scripts/`](scripts/).

---

## 1. WSL2 Host Prerequisites & Recommended Placement

1. **Clone onto the native WSL2 Linux filesystem (`~/...`, NOT `/mnt/c/...`):**
   ```bash
   cd ~
   git clone https://github.com/cmkennedy20/voice-clone-pipeline.git
   cd ~/voice-clone-pipeline
   ```
   *(Keeping the repo and `./models` on WSL2 ext4 instead of `/mnt/c` avoids slow Windows-to-Linux 9P VirtioFS overhead during Docker builds and multi-GB model loading.)*
2. **Docker in WSL2:** Ensure **Docker Desktop for Windows** is running with **Settings -> Resources -> WSL Integration** enabled for your WSL2 distro (or native `dockerd` + `docker-compose-plugin` inside WSL2). Verify with `docker compose version`.
3. **Optional — NVIDIA GPU Acceleration in WSL2:** Install a standard Windows NVIDIA Game Ready / Studio driver on Windows. WSL2 automatically exposes `/dev/dxg` and `/usr/lib/wsl/lib/nvidia-smi` inside Linux — do **not** install a separate Linux `.run` GPU driver inside WSL2. Machines without an NVIDIA GPU automatically configure `COMPUTE_MODE=cpu`.
4. **WSL2 Memory Check (`>= 12–16 GB` recommended):** Windows defaults WSL2 VM memory to 50% of host RAM. If `free -m` inside WSL2 shows `< 10000` MiB on a 16–32 GB PC, create `%USERPROFILE%\.wslconfig` on Windows (`[wsl2]` / `memory=16GB` / `localhostForwarding=true`) and run `wsl --shutdown` once before building.

---

## 2. Unattended Agent Bootstrap, Compilation, Model Pull & Startup

Run sequentially inside WSL2 from the repository root:

```bash
# 1. Probe WSL2 GPU/VRAM/RAM & Windows LAN IP, create ./models & ./local_storage, and write .env
make setup ARGS="--non-interactive --json"

# 2. Resumably download tier-matched HuggingFace weights (Whisper, Silero VAD, Qwen3, XTTS-v2, CosyVoice)
make pull-models

# 3. Compile all Docker images locally, run SQLx migrations, and start the Web Studio + API stack
make up

# 4. Verify end-to-end container & HTTP readiness
make doctor ARGS="--json"
```

- **Completion criterion:** `make doctor ARGS="--json"` exits `0` and reports `"ready": true` (`http://127.0.0.1:8080/health/ready` and `http://127.0.0.1:3000/` both return `200 OK`).
- **Open in Windows Browser:** On the Windows desktop, open **`http://localhost:3000`** in Chrome or Edge (`localhost` is a W3C Secure Context forwarded automatically from WSL2, so live microphone voice enrollment works immediately with zero TLS prompts).

---

## 3. Hardware Auto-Tiering & `.env` Customization Guide

[`scripts/setup.sh`](scripts/setup.sh) inspects `nvidia-smi` (including `/usr/lib/wsl/lib/nvidia-smi`), selects the highest-VRAM NVIDIA GPU on multi-GPU desktops, and writes matching settings into `.env` (fully documented in [`.env.example`](.env.example)):

| Detected Hardware | Tier (`--tier`) | `COMPUTE_MODE` | `WHISPER_MODEL_FILE` | `LLM_MODEL_FILE` | `PULL_COSYVOICE` | `PREWARM_ENGINES` | Approx. Disk |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **NVIDIA `>= 16 GB` VRAM** (RTX 3090 / 4080 / 4090 / 5090) | `enthusiast` | `gpu` | `ggml-large-v3.bin` | `Qwen3-8B-Q8_0.gguf` | `true` | `cosyvoice_3,xtts_v2` | ~18 GB |
| **NVIDIA `8–15 GB` VRAM** (RTX 3060 12G / 3070 / 4060 / 4070) | `mainstream` | `gpu` | `ggml-medium.bin` | `Qwen3-8B-Q4_K_M.gguf` | `true` | `xtts_v2` | ~12 GB |
| **NVIDIA `4–7 GB` VRAM** (RTX 2060 / 3050 / Laptop 6 GB) | `entry` | `gpu` | `ggml-small.bin` | `Qwen3-4B-Q4_K_M.gguf` | `false` | `xtts_v2` | ~5.5 GB |
| **CPU-Only / AMD / Intel** (`>= 16 GB` system RAM) | `cpu` | `cpu` | `ggml-small.bin` | `Qwen3-4B-Q4_K_M.gguf` | `false` | *(empty)* | ~5.5 GB |

### Common `.env` Overrides for User Systems
- **Place ~6–18 GB of models in a custom directory or secondary drive (`D:`):**
  ```bash
  make setup ARGS="--non-interactive --models-dir /mnt/d/VoiceCloneModels --storage-dir ./local_storage"
  ```
- **Resolve port conflicts (`3000`, `3443`, `8080`):**
  Update `WEB_UI_PORT`, `WEB_UI_TLS_PORT`, or `API_BIND_PORT` in `.env`, then run `make restart`.
- **Pin a specific GPU on a multi-GPU Windows PC:**
  Pass `ARGS="--non-interactive --gpu-id <index-or-UUID>"` to `make setup` (or edit `NVIDIA_GPU_ID` in `.env`) and run `make restart`.
- **Force a lighter tier or CPU mode:**
  Run `make setup ARGS="--non-interactive --tier mainstream"` (or `--tier cpu`), then `make up`.
- **Access Microphone Studio from another phone/laptop on Wi-Fi:**
  `scripts/setup.sh` queries `powershell.exe` via WSL interop to detect the Windows PC's Wi-Fi/Ethernet IP (`LAN_HOST_IP`). Note that accessing `:3443` from *other* LAN devices on Windows WSL2 either uses Windows 11 `networkingMode=mirrored` in `.wslconfig` or a `netsh interface portproxy` forward; on the Windows host itself, `http://localhost:3000` works with zero extra networking setup.

---

## 4. Optional Discord 1-on-1 Bot Pairing (Zero Container Rebuild)

`discord-worker` stands by cleanly when Discord is unconfigured so `make up` succeeds immediately on a fresh clone. When the user wants Discord `/speak`, `/voice_clone`, and 1-on-1 DM voice translation:

- **Option A — Studio Web UI (`http://localhost:3000`):** Open **Tab 5 (`History & Runtime Config`)** -> **`Pair Discord Bot`**, paste `Application ID`, `Bot Token`, and `Owner User ID`, and click **Save Discord Config & Start Bot** (`discord-worker` hot-connects automatically within 3 seconds!).
- **Option B — Unattended Agent CLI:**
  ```bash
  make setup-discord ARGS="--app-id <17_TO_20_DIGIT_APP_ID> --bot-token <BOT_TOKEN> --owner-id <17_TO_20_DIGIT_USER_ID> --json"
  ```
- **Option C — Interactive Human Wizard (auto-opens Windows browser via `explorer.exe` / `wslview`):**
  ```bash
  make setup-discord
  ```

Share both returned OAuth2 URLs (`user_install_url` and `bot_dm_url`) with the user so they can authorize `/speak` + `/voice_clone` on their Discord account and open their 1-on-1 Bot DM.

---

## 5. `Makefile` Target Reference

| Command | Purpose |
| :--- | :--- |
| `make help` | Print all self-contained WSL2/Docker targets and current `COMPUTE_MODE` |
| `make setup` | Probe WSL2 GPU/VRAM/RAM & Windows LAN IP, create folders, write `.env` |
| `make pull-models` | Resumably download HuggingFace weights (`Whisper`, `VAD`, `Qwen3`, `XTTS`, `CosyVoice`) |
| `make build` | Compile all Rust, `llama.cpp`, Python TTS, and React Web UI Docker images locally |
| `make up` | Build images, pull models, run SQLx migrations, and start the full stack |
| `make doctor` | Run end-to-end readiness check (`make doctor ARGS="--json"` for agents) |
| `make setup-discord` | Run interactive or `--json` Discord 1-on-1 bot setup & hot-reload worker |
| `make logs` | Tail container logs (`make logs SERVICE=api-server` to filter) |
| `make status` | Show container status and host port mappings |
| `make restart` | Apply `.env` updates to running containers |
| `make down` | Stop containers cleanly (preserves PostgreSQL volume, models, and profiles) |
| `make verify` | Run full Rust (`fmt`, `clippy`, `nextest`, `cargo-deny`), Python `pytest`, and Web `vitest` gate |
