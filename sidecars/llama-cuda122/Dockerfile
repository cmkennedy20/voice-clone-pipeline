# syntax=docker/dockerfile:1.7
FROM nvidia/cuda:12.2.2-devel-ubuntu22.04 AS build
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    cmake \
    git \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && ln -sf /usr/local/cuda/lib64/stubs/libcuda.so /usr/local/cuda/lib64/stubs/libcuda.so.1

WORKDIR /src
RUN git clone --depth 1 https://github.com/ggml-org/llama.cpp.git .

ARG COMPUTE_BACKEND=cuda
ARG CUDA_ARCHITECTURES="75;80;86;89"
RUN export LD_LIBRARY_PATH="/usr/local/cuda/lib64/stubs:${LD_LIBRARY_PATH:-}" \
    && export LIBRARY_PATH="/usr/local/cuda/lib64/stubs:${LIBRARY_PATH:-}" \
    && GGML_CUDA_FLAG=$( [ "${COMPUTE_BACKEND}" = "cpu" ] && echo "OFF" || echo "ON" ) \
    && cmake -B build \
      -DGGML_CUDA="${GGML_CUDA_FLAG}" \
      -DCMAKE_CUDA_ARCHITECTURES="${CUDA_ARCHITECTURES}" \
      -DCMAKE_EXE_LINKER_FLAGS="-Wl,-rpath-link,/usr/local/cuda/lib64/stubs" \
      -DCMAKE_SHARED_LINKER_FLAGS="-Wl,-rpath-link,/usr/local/cuda/lib64/stubs" \
      -DLLAMA_CURL=OFF \
      -DLLAMA_BUILD_TESTS=OFF \
      -DLLAMA_BUILD_EXAMPLES=OFF \
      -DLLAMA_BUILD_SERVER=ON \
      -DBUILD_SHARED_LIBS=ON \
      -DCMAKE_BUILD_TYPE=Release \
    && cmake --build build --target llama-server -j "$(nproc)" \
    && mkdir -p /out/bin /out/lib \
    && cp build/bin/llama-server /out/bin/llama-server \
    && find build/bin -name '*.so*' -exec cp -a {} /out/lib/ \;

FROM nvidia/cuda:12.2.2-runtime-ubuntu22.04 AS runtime
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libgomp1 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/bin/llama-server /app/llama-server
COPY --from=build /out/lib/ /usr/local/lib/
RUN ldconfig
WORKDIR /app
ENTRYPOINT ["/app/llama-server"]
