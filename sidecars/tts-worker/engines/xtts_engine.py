from __future__ import annotations

from collections import OrderedDict
import gc
import inspect
import os
from pathlib import Path
from typing import Any

import numpy as np

from engines import (
    DEFAULT_SYNTHESIS_TUNING,
    EngineUnavailableError,
    SUPPORTED_LANGUAGE_MAP,
    SynthesisTuningParams,
    TARGET_SAMPLE_RATE,
    UnsupportedLanguageError,
    allow_synthetic_fallback,
    default_models_subdir,
    normalize_and_resample_24k_mono,
    seed_torch_and_cuda,
    synthesize_deterministic_fallback,
)


def _filter_callable_kwargs(fn: Any, candidate_kwargs: dict[str, Any]) -> dict[str, Any]:
    sig = inspect.signature(fn)
    if any(p.kind == inspect.Parameter.VAR_KEYWORD for p in sig.parameters.values()):
        return candidate_kwargs
    return {k: v for k, v in candidate_kwargs.items() if k in sig.parameters}


def _free_cuda_cache() -> None:
    gc.collect()
    try:
        import torch  # type: ignore[import-not-found]

        if torch.cuda.is_available():
            torch.cuda.empty_cache()
    except Exception:
        pass


class XttsEngine:
    engine_id: str = "xtts_v2"

    def __init__(self, model_dir: str | Path | None = None) -> None:
        resolved = (
            model_dir
            or os.environ.get("XTTS_MODEL_DIR", "").strip()
            or default_models_subdir("xtts-v2")
        )
        self.model_dir = Path(resolved)
        self._model: Any | None = None
        self._sample_rate: int = TARGET_SAMPLE_RATE
        self._finetuned_model_cache: OrderedDict[tuple[str, int], tuple[Any, int]] = OrderedDict()
        self._latent_cache: dict[tuple[Any, ...], tuple[Any, Any]] = {}

    def weights_present(self) -> bool:
        if not self.model_dir.is_dir():
            return False
        if not (self.model_dir / "config.json").is_file():
            return False
        return (
            (self.model_dir / "model.pth").is_file()
            or any(self.model_dir.glob("*.pth"))
            or any(self.model_dir.glob("*.safetensors"))
        )

    def is_loaded(self) -> bool:
        return self._model is not None or bool(self._finetuned_model_cache)

    def can_serve(self) -> bool:
        return self.is_loaded() or self.weights_present() or allow_synthetic_fallback()

    def prewarm(self) -> bool:
        if not self.weights_present():
            return allow_synthetic_fallback()
        self._ensure_model_loaded()
        return True

    def _load_xtts_from_dir(
        self,
        checkpoint_dir: Path,
        vocab_fallback_dir: Path | None = None,
    ) -> tuple[Any, int]:
        try:
            import torch  # type: ignore[import-not-found]
            from TTS.tts.configs.xtts_config import XttsConfig  # type: ignore[import-not-found]
            from TTS.tts.models.xtts import Xtts  # type: ignore[import-not-found]

            config = XttsConfig()
            config.load_json(str(checkpoint_dir / "config.json"))

            vocab_path: str | None = None
            if (checkpoint_dir / "vocab.json").is_file():
                vocab_path = str(checkpoint_dir / "vocab.json")
            elif vocab_fallback_dir is not None and (vocab_fallback_dir / "vocab.json").is_file():
                vocab_path = str(vocab_fallback_dir / "vocab.json")

            speaker_file_path: str | None = None
            if (checkpoint_dir / "speakers_xtts.pth").is_file():
                speaker_file_path = str(checkpoint_dir / "speakers_xtts.pth")
            elif (
                vocab_fallback_dir is not None
                and (vocab_fallback_dir / "speakers_xtts.pth").is_file()
            ):
                speaker_file_path = str(vocab_fallback_dir / "speakers_xtts.pth")

            model = Xtts.init_from_config(config)
            load_kwargs: dict[str, Any] = {
                "checkpoint_dir": str(checkpoint_dir),
                "eval": True,
            }
            if vocab_path is not None:
                load_kwargs["vocab_path"] = vocab_path
            if speaker_file_path is not None:
                load_kwargs["speaker_file_path"] = speaker_file_path
            model.load_checkpoint(config, **load_kwargs)
            if torch.cuda.is_available():
                model.cuda()
            sr = int(getattr(config.audio, "output_sample_rate", TARGET_SAMPLE_RATE))
            return model, sr
        except Exception as exc:
            raise EngineUnavailableError(
                f"Failed to load XTTS-v2 from {checkpoint_dir}: {exc}"
            ) from exc

    def _ensure_model_loaded(self) -> Any:
        if self._model is not None:
            return self._model
        if not self.weights_present():
            raise EngineUnavailableError(f"XTTS-v2 weights absent at {self.model_dir}")

        model, sr = self._load_xtts_from_dir(self.model_dir)
        self._model = model
        self._sample_rate = sr
        return self._model

    def _evict_finetuned_models(self) -> None:
        if not self._finetuned_model_cache:
            return
        evicted_keys = set(self._finetuned_model_cache.keys())
        self._finetuned_model_cache.clear()
        stale_latent_keys = [
            key for key in self._latent_cache if len(key) > 0 and key[0] in evicted_keys
        ]
        for key in stale_latent_keys:
            del self._latent_cache[key]
        _free_cuda_cache()

    def _resolve_active_model(
        self,
        finetuned_checkpoint_dir: str | None,
    ) -> tuple[Any, int, tuple[str, int]] | None:
        if finetuned_checkpoint_dir and finetuned_checkpoint_dir.strip():
            ft_dir = Path(finetuned_checkpoint_dir.strip())
            if ft_dir.is_dir() and (ft_dir / "model.pth").is_file() and (
                ft_dir / "config.json"
            ).is_file():
                resolved_ft = str(ft_dir.resolve())
                mtime_ns = (ft_dir / "model.pth").stat().st_mtime_ns
                ft_key = (resolved_ft, mtime_ns)
                cached_entry = self._finetuned_model_cache.get(ft_key)
                if cached_entry is not None:
                    self._finetuned_model_cache.move_to_end(ft_key)
                    return cached_entry[0], cached_entry[1], ft_key

                # 1-slot LRU eviction to keep RTX 3090 VRAM bounded
                self._evict_finetuned_models()
                ft_model, ft_sr = self._load_xtts_from_dir(
                    ft_dir, vocab_fallback_dir=self.model_dir
                )
                self._finetuned_model_cache[ft_key] = (ft_model, ft_sr)
                return ft_model, ft_sr, ft_key

        if self._model is not None:
            return self._model, self._sample_rate, ("base", 0)
        if not self.weights_present():
            return None
        base_model = self._ensure_model_loaded()
        return base_model, self._sample_rate, ("base", 0)

    def _try_load_precomputed_latents(
        self,
        model_key: tuple[str, int],
        finetuned_checkpoint_dir: str | None,
        has_override_refs: bool,
    ) -> tuple[Any, Any] | None:
        if has_override_refs or not finetuned_checkpoint_dir:
            return None
        latents_file = Path(finetuned_checkpoint_dir.strip()) / "conditioning_latents.pt"
        if not latents_file.is_file():
            return None
        cache_key = (model_key, "precomputed_latents", latents_file.stat().st_mtime_ns)
        cached = self._latent_cache.get(cache_key)
        if cached is not None:
            return cached
        try:
            import torch  # type: ignore[import-not-found]

            device = "cuda" if torch.cuda.is_available() else "cpu"
            try:
                loaded = torch.load(str(latents_file), map_location=device, weights_only=True)
            except TypeError:
                loaded = torch.load(str(latents_file), map_location=device)
            if isinstance(loaded, dict) and "gpt_cond_latent" in loaded and "speaker_embedding" in loaded:
                pair = (loaded["gpt_cond_latent"], loaded["speaker_embedding"])
            elif isinstance(loaded, (tuple, list)) and len(loaded) == 2:
                pair = (loaded[0], loaded[1])
            else:
                return None
            self._latent_cache[cache_key] = pair
            return pair
        except Exception:
            return None

    def _get_latents(
        self,
        model: Any,
        model_key: tuple[str, int],
        reference_wav_path: str,
        reference_wav_paths: list[str] | None,
        tuning: SynthesisTuningParams,
        finetuned_checkpoint_dir: str | None,
    ) -> tuple[Any, Any]:
        explicit_paths = (
            [p.strip() for p in reference_wav_paths if p and p.strip()]
            if reference_wav_paths
            else []
        )
        precomputed = self._try_load_precomputed_latents(
            model_key=model_key,
            finetuned_checkpoint_dir=finetuned_checkpoint_dir,
            has_override_refs=bool(explicit_paths),
        )
        if precomputed is not None:
            return precomputed

        raw_paths = explicit_paths if explicit_paths else [reference_wav_path]
        resolved_paths = [str(Path(p).resolve()) for p in raw_paths]
        path_stat_sig = tuple(
            (rp, Path(rp).stat().st_mtime_ns if Path(rp).exists() else 0)
            for rp in resolved_paths
        )
        chunk_len_secs = min(int(tuning.gpt_cond_chunk_len_secs), int(tuning.gpt_cond_len_secs))
        cache_key = (
            model_key,
            path_stat_sig,
            int(tuning.gpt_cond_len_secs),
            chunk_len_secs,
            int(tuning.max_ref_len_secs),
            bool(tuning.sound_norm_refs),
        )
        cached = self._latent_cache.get(cache_key)
        if cached is not None:
            return cached

        cond_kwargs = _filter_callable_kwargs(
            model.get_conditioning_latents,
            {
                "audio_path": resolved_paths,
                "max_ref_length": int(tuning.max_ref_len_secs),
                "gpt_cond_len": int(tuning.gpt_cond_len_secs),
                "gpt_cond_chunk_len": chunk_len_secs,
                "sound_norm_refs": bool(tuning.sound_norm_refs),
            },
        )
        gpt_cond_latent, speaker_embedding = model.get_conditioning_latents(**cond_kwargs)
        self._latent_cache[cache_key] = (gpt_cond_latent, speaker_embedding)
        return gpt_cond_latent, speaker_embedding

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
        _ = reference_transcripts
        norm_lang = target_language.strip().lower()
        xtts_code = SUPPORTED_LANGUAGE_MAP.get(norm_lang)
        if xtts_code is None:
            raise UnsupportedLanguageError(
                f"XTTS-v2 does not support target_language '{target_language}'"
            )

        seed_torch_and_cuda(seed)
        eff_tuning = tuning or DEFAULT_SYNTHESIS_TUNING

        resolved_model = self._resolve_active_model(finetuned_checkpoint_dir)
        if resolved_model is None:
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
                f"XTTS-v2 weights not found at {self.model_dir} and ALLOW_SYNTHETIC_FALLBACK!=1"
            )

        model, active_sr, model_key = resolved_model
        gpt_cond_latent, speaker_embedding = self._get_latents(
            model=model,
            model_key=model_key,
            reference_wav_path=reference_wav_path,
            reference_wav_paths=reference_wav_paths,
            tuning=eff_tuning,
            finetuned_checkpoint_dir=finetuned_checkpoint_dir,
        )
        inf_kwargs = _filter_callable_kwargs(
            model.inference,
            {
                "text": spoken_hanzi,
                "language": xtts_code,
                "gpt_cond_latent": gpt_cond_latent,
                "speaker_embedding": speaker_embedding,
                "temperature": float(eff_tuning.temperature),
                "top_p": float(eff_tuning.top_p),
                "top_k": int(eff_tuning.top_k),
                "repetition_penalty": float(eff_tuning.repetition_penalty),
                "length_penalty": float(eff_tuning.length_penalty),
                "speed": float(eff_tuning.speed),
                "enable_text_splitting": True,
            },
        )
        output = model.inference(**inf_kwargs)
        raw_wav = output["wav"] if isinstance(output, dict) else output
        if hasattr(raw_wav, "detach"):
            wav_np = raw_wav.detach().cpu().numpy().astype(np.float32)
        else:
            wav_np = np.asarray(raw_wav, dtype=np.float32)
        return normalize_and_resample_24k_mono(wav_np, active_sr)
