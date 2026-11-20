# syntax=docker/dockerfile:1.7
FROM python:3.11-slim

ENV DEBIAN_FRONTEND=noninteractive \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    NVIDIA_VISIBLE_DEVICES=all \
    NVIDIA_DRIVER_CAPABILITIES=compute,utility \
    COQUI_TOS_AGREED=1 \
    PYTHONPATH="/opt/CosyVoice:/opt/CosyVoice/third_party/Matcha-TTS"

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    curl \
    espeak-ng \
    ffmpeg \
    git \
    libsndfile1 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY requirements.txt ./requirements.txt
RUN pip install --no-cache-dir -r requirements.txt

ARG INSTALL_EXTRA_TTS_PACKAGES=false
ARG TORCH_COMPUTE=cu121
RUN if [ "${INSTALL_EXTRA_TTS_PACKAGES}" = "true" ]; then \
        if [ "${TORCH_COMPUTE}" = "cpu" ]; then \
            TORCH_SPEC="torch==2.4.1"; \
            AUDIO_SPEC="torchaudio==2.4.1"; \
            TORCH_INDEX="https://download.pytorch.org/whl/cpu"; \
        else \
            TORCH_SPEC="torch==2.4.1+cu121"; \
            AUDIO_SPEC="torchaudio==2.4.1+cu121"; \
            TORCH_INDEX="https://download.pytorch.org/whl/cu121"; \
        fi && \
        printf '%s\n' \
            "${TORCH_SPEC}" \
            "${AUDIO_SPEC}" \
            "numpy==1.26.4" \
            "coqui-tts-trainer==0.2.2" \
            "coqpit-config>=0.1.1,<0.2.0" \
            "transformers>=4.43.0,<=4.46.2" \
            > /etc/pip-constraints.txt && \
        pip install --no-cache-dir --retries 10 --timeout 120 \
            --index-url "${TORCH_INDEX}" \
            "${TORCH_SPEC}" \
            "${AUDIO_SPEC}"; \
    fi

RUN if [ "${INSTALL_EXTRA_TTS_PACKAGES}" = "true" ]; then \
        TORCH_EXTRA_INDEX="https://download.pytorch.org/whl/cu121"; \
        ONNX_PKG="onnxruntime-gpu==1.22.0"; \
        if [ "${TORCH_COMPUTE}" = "cpu" ]; then \
            TORCH_EXTRA_INDEX="https://download.pytorch.org/whl/cpu"; \
            ONNX_PKG="onnxruntime==1.22.0"; \
        fi && \
        echo "setuptools<81.0.0" >> /etc/pip-constraints.txt && \
        PIP_CONSTRAINT=/etc/pip-constraints.txt pip install --no-cache-dir --retries 10 --timeout 120 \
            --extra-index-url "${TORCH_EXTRA_INDEX}" \
            "setuptools<81.0.0" \
            "numpy==1.26.4" \
            "coqui-tts==0.25.3" \
            "coqui-tts-trainer==0.2.2" \
            "coqpit-config>=0.1.1,<0.2.0" \
            "pypinyin>=0.50.0" \
            "jieba>=0.42.1" \
            "cutlet>=0.4.0" \
            "fugashi>=1.3.2" \
            "unidic-lite>=1.0.8" \
            "hangul-romanize>=0.1.0" \
            "speechbrain==1.0.3" \
            "transformers>=4.43.0,<=4.46.2" \
            "einops>=0.8.0" \
            "conformer==0.3.2" \
            "${ONNX_PKG}" \
            "modelscope>=1.20.0" \
            "hyperpyyaml>=1.2.2" \
            "omegaconf>=2.3.0" \
            "diffusers==0.31.0" \
            "hydra-core>=1.3.2" \
            "inflect>=7.3.1" \
            "librosa==0.10.2.post1" \
            "matplotlib>=3.8.0,<3.11.0" \
            "openai-whisper==20240930" \
            "x-transformers==1.44.4" && \
        git clone --depth 1 --recursive https://github.com/FunAudioLLM/CosyVoice.git /opt/CosyVoice && \
        python3 -c "import torch, torchaudio, TTS; print('Verified ABI:', torch.__version__, torchaudio.__version__)"; \
    fi

COPY engines ./engines
COPY server.py test_server.py ./

EXPOSE 8082

CMD ["uvicorn", "server:app", "--host", "0.0.0.0", "--port", "8082"]
