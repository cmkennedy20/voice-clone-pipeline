from __future__ import annotations

import inspect
import math
import os
from pathlib import Path
import sys
import tempfile
from typing import Any

import numpy as np
from scipy import signal
import soundfile as sf

from engines import (
    COSYVOICE_SUPPORTED_LANGUAGES,
    DEFAULT_SYNTHESIS_TUNING,
    EngineUnavailableError,
    SynthesisTuningParams,
    TARGET_SAMPLE_RATE,
    UnsupportedLanguageError,
    allow_synthetic_fallback,
    default_models_subdir,
    normalize_and_resample_24k_mono,
    rms_normalize_mono,
    seed_torch_and_cuda,
    synthesize_deterministic_fallback,
    time_scale_pcm,
)

_COSY_PROMPT_SR: int = 16_000
_COSY_MAX_PROMPT_SAMPLES: int = 20 * _COSY_PROMPT_SR
_COSY_SILENCE_GAP_SAMPLES: int = int(round(0.150 * _COSY_PROMPT_SR))


def _load_clip_16k_mono(path: str, sound_norm: bool) -> np.ndarray:
    wav_np, sr = sf.read(path, dtype="float32")
    wav_np = np.nan_to_num(np.asarray(wav_np, dtype=np.float32), nan=0.0, posinf=0.0, neginf=0.0)
    if wav_np.ndim == 2:
        if wav_np.shape[0] <= 8 and wav_np.shape[1] > wav_np.shape[0]:
            wav_np = np.mean(wav_np, axis=0, dtype=np.float32)
        else:
            wav_np = np.mean(wav_np, axis=1, dtype=np.float32)
    elif wav_np.ndim > 2:
        wav_np = wav_np.reshape(-1).astype(np.float32)

    if wav_np.size == 0:
        return np.zeros(0, dtype=np.float32)

    orig_sr = int(sr)
    if orig_sr != _COSY_PROMPT_SR:
        g = math.gcd(_COSY_PROMPT_SR, orig_sr)
        wav_np = signal.resample_poly(
            wav_np, _COSY_PROMPT_SR // g, orig_sr // g
        ).astype(np.float32)

    if sound_norm:
        wav_np = rms_normalize_mono(wav_np)
    return wav_np


def _stitch_reference_clips_16k(
    pairs: list[tuple[str, str]],
    sound_norm: bool,
) -> tuple[np.ndarray, str]:
    stitched_parts: list[np.ndarray] = []
    selected_transcripts: list[str] = []
    total_samples = 0

    for wav_path, transcript in pairs:
        clip_16k = _load_clip_16k_mono(wav_path, sound_norm=sound_norm)
        if clip_16k.size == 0:
            continue

        if not stitched_parts:
            if clip_16k.size > _COSY_MAX_PROMPT_SAMPLES:
                keep_ratio = _COSY_MAX_PROMPT_SAMPLES / float(clip_16k.size)
                clip_16k = clip_16k[:_COSY_MAX_PROMPT_SAMPLES]
                words = transcript.strip().split()
                if words:
                    keep_words = max(1, int(round(len(words) * keep_ratio)))
                    transcript = " ".join(words[:keep_words])
            stitched_parts.append(clip_16k)
            total_samples = clip_16k.size
            if transcript.strip():
                selected_transcripts.append(transcript.strip())
            continue

        needed = _COSY_SILENCE_GAP_SAMPLES + clip_16k.size
        if total_samples + needed > _COSY_MAX_PROMPT_SAMPLES:
            continue

        stitched_parts.append(np.zeros(_COSY_SILENCE_GAP_SAMPLES, dtype=np.float32))
        stitched_parts.append(clip_16k)
        total_samples += needed
        if transcript.strip():
            selected_transcripts.append(transcript.strip())

    if not stitched_parts:
        raise EngineUnavailableError("Reference audio clips produced zero samples for CosyVoice")

    joined_pcm = (
        stitched_parts[0]
        if len(stitched_parts) == 1
        else np.concatenate(stitched_parts, axis=0)
    )
    joined_transcript = " ".join(selected_transcripts)
    return joined_pcm.astype(np.float32), joined_transcript


def _to_prompt_tensor_or_array(pcm_16k: np.ndarray) -> Any:
    try:
        import torch  # type: ignore[import-not-found]

        return torch.from_numpy(pcm_16k).unsqueeze(0)
    except Exception:
        return pcm_16k.reshape(1, -1)


class CosyVoiceEngine:
    engine_id: str = "cosyvoice_3"

    def __init__(self, model_dir: str | Path | None = None) -> None:
        resolved = (
            model_dir
            or os.environ.get("COSYVOICE_MODEL_DIR", "").strip()
            or default_models_subdir("cosyvoice")
        )
        self.model_dir = Path(resolved)
        self._model: Any | None = None

    def weights_present(self) -> bool:
        if not self.model_dir.is_dir():
            return False
        markers = (
            "cosyvoice.yaml",
            "cosyvoice2.yaml",
            "cosyvoice3.yaml",
            "llm.pt",
            "flow.pt",
            "hift.pt",
        )
        if any((self.model_dir / name).is_file() for name in markers):
            return True
        return any(self.model_dir.glob("*.pt")) or any(self.model_dir.glob("*.safetensors"))

    def is_loaded(self) -> bool:
        return self._model is not None

    def can_serve(self) -> bool:
        return self.is_loaded() or self.weights_present() or allow_synthetic_fallback()

    def prewarm(self) -> bool:
        if not self.weights_present():
            return allow_synthetic_fallback()
        self._ensure_model_loaded()
        return True

    def _ensure_model_loaded(self) -> Any:
        if self._model is not None:
            return self._model
        if not self.weights_present():
            raise EngineUnavailableError(
                f"CosyVoice 3.0 weights absent at {self.model_dir}"
            )

        cosy_src_env = os.environ.get("COSYVOICE_SRC_DIR", "").strip()
        candidate_paths = []
        if cosy_src_env:
            candidate_paths.append(cosy_src_env)
            candidate_paths.append(str(Path(cosy_src_env) / "third_party" / "Matcha-TTS"))
        candidate_paths.extend(
            (
                "/opt/CosyVoice",
                "/opt/CosyVoice/third_party/Matcha-TTS",
                str(self.model_dir),
            )
        )
        for candidate_path in candidate_paths:
            if os.path.isdir(candidate_path) and candidate_path not in sys.path:
                sys.path.insert(0, candidate_path)

        try:
            from cosyvoice.cli.cosyvoice import AutoModel  # type: ignore[import-not-found]

            self._model = AutoModel(model_dir=str(self.model_dir))
            return self._model
        except ImportError:
            pass

        try:
            from cosyvoice.cli.cosyvoice import CosyVoice2  # type: ignore[import-not-found]

            if (self.model_dir / "cosyvoice2.yaml").is_file() or (
                self.model_dir / "cosyvoice3.yaml"
            ).is_file():
                self._model = CosyVoice2(str(self.model_dir), fp16=True)
                return self._model
        except Exception:
            pass

        try:
            from cosyvoice.cli.cosyvoice import CosyVoice  # type: ignore[import-not-found]

            self._model = CosyVoice(str(self.model_dir), fp16=True)
            return self._model
        except Exception as exc:
            raise EngineUnavailableError(
                f"Failed to load CosyVoice 3.0 from {self.model_dir}: {exc}"
            ) from exc

    def synthesize(
        self,
        spoken_hanzi: str,
        reference_wav_path: str,
        reference_transcript_en: str,
        seed: int,
        target_language: str = "zh",
        *,
        reference_wav_paths: list[str] | None = None,
        reference_transcripts: list[str] | None = None,
        tuning: SynthesisTuningParams | None = None,
        finetuned_checkpoint_dir: str | None = None,
    ) -> np.ndarray:
        _ = finetuned_checkpoint_dir
        norm_lang = target_language.strip().lower()
        if norm_lang not in COSYVOICE_SUPPORTED_LANGUAGES:
            raise UnsupportedLanguageError(
                f"CosyVoice 3.0 only supports {sorted(COSYVOICE_SUPPORTED_LANGUAGES)}, got '{target_language}'"
            )

        seed_torch_and_cuda(seed)
        eff_tuning = tuning or DEFAULT_SYNTHESIS_TUNING

        if self._model is None and not self.weights_present():
            if allow_synthetic_fallback():
                return synthesize_deterministic_fallback(
                    engine_id=self.engine_id,
                    spoken_hanzi=spoken_hanzi,
                    reference_wav_path=reference_wav_path,
                    reference_transcript_en=reference_transcript_en,
                    seed=seed,
                    target_language=norm_lang,
                    reference_wav_paths=reference_wav_paths,
                    tuning=tuning,
                )
            raise EngineUnavailableError(
                f"CosyVoice 3.0 weights not found at {self.model_dir} and ALLOW_SYNTHETIC_FALLBACK!=1"
            )

        model = self._ensure_model_loaded()
        explicit_paths = (
            [p.strip() for p in reference_wav_paths if p and p.strip()]
            if reference_wav_paths
            else []
        )
        if explicit_paths:
            if reference_transcripts is not None:
                pairs = [
                    (
                        p,
                        reference_transcripts[idx]
                        if idx < len(reference_transcripts)
                        else "",
                    )
                    for idx, p in enumerate(explicit_paths)
                ]
            else:
                pairs = [
                    (p, reference_transcript_en if idx == 0 else "")
                    for idx, p in enumerate(explicit_paths)
                ]
        else:
            pairs = [(reference_wav_path, reference_transcript_en)]

        stitched_16k, joined_transcript = _stitch_reference_clips_16k(
            pairs, sound_norm=bool(eff_tuning.sound_norm_refs)
        )

        use_zero_shot = bool(joined_transcript.strip()) and hasattr(model, "inference_zero_shot")
        target_fn = (
            model.inference_zero_shot if use_zero_shot else model.inference_cross_lingual
        )
        sig = inspect.signature(target_fn)
        has_var_kw = any(
            p.kind == inspect.Parameter.VAR_KEYWORD for p in sig.parameters.values()
        )
        accepts_speed = has_var_kw or ("speed" in sig.parameters)
        expects_prompt_wav_path = "prompt_wav" in sig.parameters

        speed_val = float(eff_tuning.speed)
        speed_passed_natively = False
        extra_kwargs: dict[str, Any] = {}
        if has_var_kw or "stream" in sig.parameters:
            extra_kwargs["stream"] = False
        if abs(speed_val - 1.0) > 1e-6 and accepts_speed:
            extra_kwargs["speed"] = speed_val
            speed_passed_natively = True

        tmp_wav_path: str | None = None
        try:
            if expects_prompt_wav_path:
                with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as tmp_f:
                    tmp_wav_path = tmp_f.name
                sf.write(
                    tmp_wav_path,
                    stitched_16k,
                    _COSY_PROMPT_SR,
                    subtype="PCM_16",
                    format="WAV",
                )
                prompt_arg: Any = tmp_wav_path
            else:
                prompt_arg = _to_prompt_tensor_or_array(stitched_16k)

            if use_zero_shot:
                generator = target_fn(
                    spoken_hanzi,
                    joined_transcript,
                    prompt_arg,
                    **extra_kwargs,
                )
            else:
                generator = target_fn(
                    spoken_hanzi,
                    prompt_arg,
                    **extra_kwargs,
                )

            chunks: list[np.ndarray] = []
            for item in generator:
                speech_tensor = item["tts_speech"]
                if hasattr(speech_tensor, "detach"):
                    chunk_np = speech_tensor.detach().cpu().numpy().astype(np.float32)
                else:
                    chunk_np = np.asarray(speech_tensor, dtype=np.float32)
                chunks.append(chunk_np.reshape(-1))
        finally:
            if tmp_wav_path is not None:
                try:
                    os.unlink(tmp_wav_path)
                except OSError:
                    pass

        if not chunks:
            raise EngineUnavailableError("CosyVoice 3.0 produced zero audio chunks")

        full_pcm = np.concatenate(chunks, axis=0)
        if abs(speed_val - 1.0) > 1e-6 and not speed_passed_natively:
            full_pcm = time_scale_pcm(full_pcm, speed_val)
        model_sr = int(getattr(model, "sample_rate", TARGET_SAMPLE_RATE))
        return normalize_and_resample_24k_mono(full_pcm, model_sr)
