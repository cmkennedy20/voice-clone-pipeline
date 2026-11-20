# syntax=docker/dockerfile:1.7
FROM nvidia/cuda:12.2.2-devel-ubuntu22.04 AS cuda-122-devel
FROM nvidia/cuda:12.2.2-runtime-ubuntu22.04 AS cuda-122-runtime

FROM ubuntu:24.04 AS rust-cuda-base
ARG CUDA_ARCHITECTURES="75;80;86;89"
COPY --from=cuda-122-devel /usr/local/cuda-12.2 /usr/local/cuda-12.2
RUN ln -s /usr/local/cuda-12.2 /usr/local/cuda \
    && echo /usr/local/cuda/lib64 > /etc/ld.so.conf.d/cuda-12-2.conf \
    && ldconfig

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    CUDA_HOME=/usr/local/cuda \
    CUDA_PATH=/usr/local/cuda \
    CUDAHOSTCXX=/usr/bin/g++-12 \
    CMAKE_CUDA_HOST_COMPILER=/usr/bin/g++-12 \
    PATH="/usr/local/cuda/bin:/usr/local/cargo/bin:${PATH}" \
    CMAKE_CUDA_ARCHITECTURES="${CUDA_ARCHITECTURES}" \
    LIBRARY_PATH="/usr/local/cuda/lib64/stubs:/usr/local/cuda/lib64"

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    gcc-12 \
    g++-12 \
    clang \
    libclang-dev \
    cmake \
    pkg-config \
    libssl-dev \
    libopus-dev \
    ca-certificates \
    curl \
    git \
    ffmpeg \
    python3 \
    python3-pytest \
    python3-pip \
    && rm -rf /var/lib/apt/lists/* \
    && ln -sf /usr/local/cuda/lib64/stubs/libcuda.so /usr/local/cuda/lib64/stubs/libcuda.so.1

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
    --default-toolchain stable -c rustfmt -c clippy \
    && cargo install --locked cargo-chef cargo-nextest cargo-deny \
    && cargo install --locked sqlx-cli --no-default-features --features rustls,postgres

WORKDIR /app

FROM rust-cuda-base AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM rust-cuda-base AS cacher
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --all-targets --all-features --recipe-path recipe.json
RUN cargo chef cook --all-targets --all-features --recipe-path recipe.json

FROM rust-cuda-base AS verify-runner
ENV LD_LIBRARY_PATH="/usr/local/cuda/lib64/stubs:/usr/local/cuda/lib64"
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
ARG COMPUTE_BACKEND=cuda
COPY --from=cacher /app/target /app/target
COPY --from=cacher /usr/local/cargo /usr/local/cargo
COPY . .
RUN if [ "${COMPUTE_BACKEND}" = "cpu" ]; then \
        cargo build --release -p api-server -p cli --bin api-server --bin cli; \
    else \
        cargo build --release -p api-server -p cli --bin api-server --bin cli --features cuda; \
    fi \
    && cargo build --release -p discord-worker --bin discord-worker \
    && mkdir -p /out/lib \
    && find /app/target/release -maxdepth 1 -name 'libonnxruntime*.so*' -exec cp -a {} /out/lib/ \;

FROM ubuntu:24.04 AS db-migrate
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3t64 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=rust-cuda-base /usr/local/cargo/bin/sqlx /usr/local/bin/sqlx
WORKDIR /app
COPY migrations ./migrations
ENTRYPOINT ["sqlx", "migrate", "run"]

FROM ubuntu:24.04 AS gpu-runtime
ENV DEBIAN_FRONTEND=noninteractive \
    NVIDIA_VISIBLE_DEVICES=all \
    NVIDIA_DRIVER_CAPABILITIES=compute,utility \
    CUDA_HOME=/usr/local/cuda \
    PATH="/usr/local/cuda/bin:${PATH}"
COPY --from=cuda-122-runtime /usr/local/cuda-12.2 /usr/local/cuda-12.2
RUN ln -s /usr/local/cuda-12.2 /usr/local/cuda \
    && echo /usr/local/cuda/lib64 > /etc/ld.so.conf.d/cuda-12-2.conf \
    && apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libopus0 \
    libssl3t64 \
    curl \
    ffmpeg \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /out/lib/ /usr/local/lib/
RUN ldconfig
COPY --from=builder /app/target/release/api-server /usr/local/bin/api-server
COPY --from=builder /app/target/release/cli /usr/local/bin/cli

FROM ubuntu:24.04 AS discord-worker
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3t64 \
    libopus0 \
    curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /out/lib/ /usr/local/lib/
RUN ldconfig
COPY --from=builder /app/target/release/discord-worker /usr/local/bin/discord-worker
ENTRYPOINT ["/usr/local/bin/discord-worker"]
