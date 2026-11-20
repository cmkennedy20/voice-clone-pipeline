from __future__ import annotations

import base64
import binascii
from contextlib import asynccontextmanager
import inspect
import io
import logging
import os
from pathlib import Path
import re
from typing import Any, AsyncIterator, Literal
import wave

from fastapi import FastAPI, HTTPException, Response
import numpy as np
from pydantic import BaseModel, Field, field_validator, model_validator
import soundfile as sf

from engines import (
    COSYVOICE_SUPPORTED_LANGUAGES,
    DEFAULT_SYNTHESIS_TUNING,
    EngineUnavailableError,
    SUPPORTED_LANGUAGE_MAP,
    SynthesisTuningParams,
    TARGET_SAMPLE_RATE,
    TtsEngine,
    UnsupportedLanguageError,
    allow_synthetic_fallback,
    default_models_subdir,
    detect_device_and_vram_mib,
    extract_logmel24_cepstral_embedding,
    normalize_and_resample_24k_mono,
)
from engines.cosyvoice_engine import CosyVoiceEngine
from engines.xtts_engine import XttsEngine
from engines.xtts_finetuner import (
    DuplicateFinetuneError,
    FinetuneSample,
    XttsFineTuneManager,
)

logger = logging.getLogger("tts_worker")
_ASCII_DIGIT_RE = re.compile(r"[0-9]")


class TuningPayload(BaseModel):
    temperature: float = Field(default=DEFAULT_SYNTHESIS_TUNING.temperature, ge=0.05, le=1.20)
    top_p: float = Field(default=DEFAULT_SYNTHESIS_TUNING.top_p, ge=0.10, le=1.00)
    top_k: int = Field(default=DEFAULT_SYNTHESIS_TUNING.top_k, ge=1, le=200)
    repetition_penalty: float = Field(
        default=DEFAULT_SYNTHESIS_TUNING.repetition_penalty, ge=1.00, le=15.00
    )
    length_penalty: float = Field(
        default=DEFAULT_SYNTHESIS_TUNING.length_penalty, ge=0.50, le=2.00
    )
    speed: float = Field(default=DEFAULT_SYNTHESIS_TUNING.speed, ge=0.50, le=2.00)
    gpt_cond_len_secs: int = Field(
        default=DEFAULT_SYNTHESIS_TUNING.gpt_cond_len_secs, ge=3, le=60
    )
    gpt_cond_chunk_len_secs: int = Field(
        default=DEFAULT_SYNTHESIS_TUNING.gpt_cond_chunk_len_secs, ge=2, le=30
    )
    max_ref_len_secs: int = Field(
        default=DEFAULT_SYNTHESIS_TUNING.max_ref_len_secs, ge=6, le=120
    )
    sound_norm_refs: bool = Field(default=DEFAULT_SYNTHESIS_TUNING.sound_norm_refs)

    @model_validator(mode="before")
    @classmethod
    def _strip_explicit_nones(cls, data: Any) -> Any:
        if isinstance(data, dict):
            return {k: v for k, v in data.items() if v is not None}
        return data

    def to_domain(self) -> SynthesisTuningParams:
        return SynthesisTuningParams(
            temperature=float(self.temperature),
            top_p=float(self.top_p),
            top_k=int(self.top_k),
            repetition_penalty=float(self.repetition_penalty),
            length_penalty=float(self.length_penalty),
            speed=float(self.speed),
            gpt_cond_len_secs=int(self.gpt_cond_len_secs),
            gpt_cond_chunk_len_secs=int(self.gpt_cond_chunk_len_secs),
            max_ref_len_secs=int(self.max_ref_len_secs),
            sound_norm_refs=bool(self.sound_norm_refs),
        )


class SynthesizeRequest(BaseModel):
    profile_slug: str
    reference_wav_path: str
    reference_transcript_en: str
    engine: Literal["cosyvoice_3", "xtts_v2"]
    target_language: str = "zh"
    spoken_text: str | None = None
    spoken_hanzi: str | None = None
    seed: int = 0
    reference_wav_paths: list[str] | None = None
    reference_transcripts: list[str] | None = None
    finetuned_checkpoint_dir: str | None = None
    tuning: TuningPayload | None = None

    @field_validator("profile_slug", "reference_wav_path")
    @classmethod
    def _require_non_empty(cls, value: str) -> str:
        trimmed = value.strip()
        if not trimmed:
            raise ValueError("must not be empty")
        return trimmed

    @field_validator("reference_wav_paths")
    @classmethod
    def _validate_reference_wav_paths(cls, value: list[str] | None) -> list[str] | None:
        if value is None:
            return None
        cleaned: list[str] = []
        for item in value:
            trimmed = item.strip()
            if not trimmed:
                raise ValueError("reference_wav_paths entries must not be empty")
            cleaned.append(trimmed)
        return cleaned or None

    @field_validator("finetuned_checkpoint_dir")
    @classmethod
    def _normalize_finetuned_dir(cls, value: str | None) -> str | None:
        if value is None:
            return None
        trimmed = value.strip()
        return trimmed if trimmed else None

    @field_validator("target_language")
    @classmethod
    def _validate_target_language(cls, value: str) -> str:
        normalized = value.strip().lower()
        if normalized == "zh-cn":
            normalized = "zh"
        if normalized not in SUPPORTED_LANGUAGE_MAP:
            raise ValueError(
                f"Unsupported target_language '{value}'; expected one of {list(SUPPORTED_LANGUAGE_MAP)}"
            )
        return normalized

    @model_validator(mode="after")
    def _validate_spoken_payload_and_engine(self) -> SynthesizeRequest:
        candidates: list[str] = []
        for field_name, raw in (
            ("spoken_text", self.spoken_text),
            ("spoken_hanzi", self.spoken_hanzi),
        ):
            if raw is not None:
                trimmed = raw.strip()
                if not trimmed:
                    raise ValueError(f"{field_name} must not be empty")
                if _ASCII_DIGIT_RE.search(trimmed):
                    raise ValueError(f"{field_name} must not contain raw ASCII digits [0-9]")
                candidates.append(trimmed)

        if not candidates:
            raise ValueError("either spoken_text or spoken_hanzi must be provided")

        if self.spoken_text is not None:
            self.spoken_text = self.spoken_text.strip()
        if self.spoken_hanzi is not None:
            self.spoken_hanzi = self.spoken_hanzi.strip()

        if (
            self.engine == "cosyvoice_3"
            and self.target_language not in COSYVOICE_SUPPORTED_LANGUAGES
        ):
            raise ValueError(
                f"Engine 'cosyvoice_3' does not support target_language '{self.target_language}'"
            )

        return self

    @property
    def resolved_spoken_text(self) -> str:
        if self.spoken_text:
            return self.spoken_text
        if self.spoken_hanzi:
            return self.spoken_hanzi
        return ""


class FinetuneSamplePayload(BaseModel):
    wav_path: str
    transcript_en: str

    @field_validator("wav_path", "transcript_en")
    @classmethod
    def _require_non_empty(cls, value: str) -> str:
        trimmed = value.strip()
        if not trimmed:
            raise ValueError("must not be empty")
        return trimmed


class FinetuneRequest(BaseModel):
    job_id: str
    profile_slug: str
    samples: list[FinetuneSamplePayload] = Field(min_length=1)
    output_dir: str
    epochs: int = Field(default=6, ge=1, le=50)
    learning_rate: float = Field(default=5e-6, ge=1e-7, le=1e-3)

    @field_validator("job_id", "profile_slug", "output_dir")
    @classmethod
    def _require_non_empty(cls, value: str) -> str:
        trimmed = value.strip()
        if not trimmed:
            raise ValueError("must not be empty")
        return trimmed


class SpeakerSimilarityRequest(BaseModel):
    reference_wav_path: str
    candidate_wav_base64: str

    @field_validator("reference_wav_path", "candidate_wav_base64")
    @classmethod
    def _require_non_empty(cls, value: str) -> str:
        trimmed = value.strip()
        if not trimmed:
            raise ValueError("must not be empty")
        return trimmed


class SpeakerSimilarityScorer:
    """Computes speaker cosine similarity in BasisPoints (0..=10000)."""

    def __init__(self, model_dir: str | Path | None = None) -> None:
        resolved = (
            model_dir
            or os.environ.get("SPEAKER_MODEL_DIR", "").strip()
            or default_models_subdir("speaker")
        )
        self.model_dir = Path(resolved)
        self._ecapa_classifier: Any | None = None
        self._load_attempted: bool = False

    def _try_get_neural_classifier(self) -> Any | None:
        if self._load_attempted:
            return self._ecapa_classifier
        self._load_attempted = True

        if not self.model_dir.is_dir():
            return None
        has_weights = (
            (self.model_dir / "hyperparams.yaml").is_file()
            or any(self.model_dir.glob("*.ckpt"))
            or any(self.model_dir.glob("*.pt"))
        )
        if not has_weights:
            return None

        try:
            import torch  # type: ignore[import-not-found]
            from speechbrain.inference.speaker import EncoderClassifier  # type: ignore[import-not-found]

            device = "cuda" if torch.cuda.is_available() else "cpu"
            self._ecapa_classifier = EncoderClassifier.from_hparams(
                source=str(self.model_dir),
                savedir=str(self.model_dir),
                run_opts={"device": device},
            )
            return self._ecapa_classifier
        except Exception as exc:
            logger.warning("Falling back to logmel24_cepstral speaker scorer: %s", exc)
            return None

    def score_bps(
        self,
        ref_pcm: np.ndarray,
        ref_sr: int,
        cand_pcm: np.ndarray,
        cand_sr: int,
    ) -> tuple[int, str]:
        classifier = self._try_get_neural_classifier()
        if classifier is not None:
            try:
                from scipy import signal
                import torch  # type: ignore[import-not-found]

                ref_16k = normalize_and_resample_24k_mono(ref_pcm, ref_sr)
                cand_16k = normalize_and_resample_24k_mono(cand_pcm, cand_sr)
                ref_16k = signal.resample_poly(ref_16k, 2, 3).astype(np.float32)
                cand_16k = signal.resample_poly(cand_16k, 2, 3).astype(np.float32)

                with torch.no_grad():
                    emb_ref = classifier.encode_batch(torch.from_numpy(ref_16k).unsqueeze(0))
                    emb_cand = classifier.encode_batch(torch.from_numpy(cand_16k).unsqueeze(0))
                    u = emb_ref.reshape(-1).detach().cpu().numpy().astype(np.float64)
                    v = emb_cand.reshape(-1).detach().cpu().numpy().astype(np.float64)

                denom = (np.linalg.norm(u) * np.linalg.norm(v)) + 1e-9
                cos_sim = float(np.dot(u, v) / denom)
                bps = int(round(max(0.0, min(1.0, cos_sim)) * 10_000.0))
                return max(0, min(10_000, bps)), "ecapa_tdnn"
            except Exception as exc:
                logger.warning("ECAPA-TDNN inference failed, using cepstral fallback: %s", exc)

        emb_ref = extract_logmel24_cepstral_embedding(ref_pcm, ref_sr)
        emb_cand = extract_logmel24_cepstral_embedding(cand_pcm, cand_sr)
        norm_product = float(np.linalg.norm(emb_ref) * np.linalg.norm(emb_cand))
        if norm_product < 1e-12:
            return 0, "logmel24_cepstral"

        cos_sim = float(np.dot(emb_ref, emb_cand) / norm_product)
        bps = int(round(max(0.0, min(1.0, cos_sim)) * 10_000.0))
        return max(0, min(10_000, bps)), "logmel24_cepstral"


def _encode_pcm16_riff_wav(pcm_24k_mono: np.ndarray) -> bytes:
    clipped = np.clip(np.asarray(pcm_24k_mono, dtype=np.float32), -1.0, 1.0)
    pcm_int16 = np.round(clipped * 32767.0).astype("<i2")
    buf = io.BytesIO()
    with wave.open(buf, "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(TARGET_SAMPLE_RATE)
        wf.writeframes(pcm_int16.tobytes())
    return buf.getvalue()


def _parse_prewarm_list() -> list[str]:
    raw = os.environ.get("PREWARM_ENGINES", "")
    return [item.strip() for item in raw.split(",") if item.strip()]


def _build_language_catalog() -> list[dict[str, object]]:
    entries: list[dict[str, object]] = []
    for code, xtts_code in SUPPORTED_LANGUAGE_MAP.items():
        engines = (
            ["cosyvoice_3", "xtts_v2"]
            if code in COSYVOICE_SUPPORTED_LANGUAGES
            else ["xtts_v2"]
        )
        entries.append(
            {
                "code": code,
                "xtts_code": xtts_code,
                "supported_engines": engines,
            }
        )
    return entries


def _invoke_engine_synthesize(engine: TtsEngine, req: SynthesizeRequest) -> np.ndarray:
    sig = inspect.signature(engine.synthesize)
    params = sig.parameters
    has_var_kw = any(p.kind == inspect.Parameter.VAR_KEYWORD for p in params.values())

    kwargs: dict[str, Any] = {
        "spoken_hanzi": req.resolved_spoken_text,
        "reference_wav_path": req.reference_wav_path,
        "reference_transcript_en": req.reference_transcript_en,
        "seed": req.seed,
    }
    optional_args: dict[str, Any] = {
        "target_language": req.target_language,
        "reference_wav_paths": req.reference_wav_paths,
        "reference_transcripts": req.reference_transcripts,
        "tuning": req.tuning.to_domain() if req.tuning is not None else None,
        "finetuned_checkpoint_dir": req.finetuned_checkpoint_dir,
    }
    for key, val in optional_args.items():
        if has_var_kw or key in params:
            kwargs[key] = val
    return engine.synthesize(**kwargs)


def create_app(
    custom_engines: dict[str, TtsEngine] | None = None,
    finetune_manager: XttsFineTuneManager | None = None,
) -> FastAPI:
    registry: dict[str, TtsEngine] = custom_engines or {
        CosyVoiceEngine.engine_id: CosyVoiceEngine(),
        XttsEngine.engine_id: XttsEngine(),
    }
    ft_manager = finetune_manager or XttsFineTuneManager()
    speaker_scorer = SpeakerSimilarityScorer()
    prewarmed_engines: list[str] = []

    @asynccontextmanager
    async def lifespan(_app: FastAPI) -> AsyncIterator[None]:
        prewarmed_engines.clear()
        for engine_name in _parse_prewarm_list():
            engine = registry.get(engine_name)
            if engine is None:
                logger.warning("Unknown engine in PREWARM_ENGINES: %s", engine_name)
                continue
            try:
                if engine.prewarm():
                    prewarmed_engines.append(engine_name)
            except Exception as exc:
                logger.warning("Prewarm skipped for %s: %s", engine_name, exc)
        yield

    fastapi_app = FastAPI(
        title="Polyglot Voice Cloning TTS Sidecar",
        version="0.3.0",
        lifespan=lifespan,
    )

    @fastapi_app.get("/health")
    def get_health() -> dict[str, object]:
        device, vram_mib = detect_device_and_vram_mib()
        loaded = [name for name, eng in registry.items() if eng.is_loaded()]
        return {
            "status": "ok",
            "device": device,
            "prewarmed_engines": list(prewarmed_engines),
            "loaded_engines": loaded,
            "supported_languages": list(SUPPORTED_LANGUAGE_MAP.keys()),
            "vram_allocated_mib": vram_mib,
        }

    @fastapi_app.get("/languages")
    def get_languages() -> dict[str, object]:
        return {"languages": _build_language_catalog()}

    @fastapi_app.post("/synthesize")
    def post_synthesize(req: SynthesizeRequest) -> Response:
        engine = registry.get(req.engine)
        if engine is None:
            raise HTTPException(status_code=422, detail=f"Unsupported engine: {req.engine}")

        has_ft_weights = (
            req.engine == "xtts_v2"
            and bool(req.finetuned_checkpoint_dir)
            and (Path(str(req.finetuned_checkpoint_dir)) / "model.pth").is_file()
            and (Path(str(req.finetuned_checkpoint_dir)) / "config.json").is_file()
        )
        if not engine.can_serve() and not has_ft_weights:
            raise HTTPException(
                status_code=503,
                detail=f"Engine '{req.engine}' weights are not loaded on disk.",
            )

        if not allow_synthetic_fallback():
            if not Path(req.reference_wav_path).is_file():
                raise HTTPException(
                    status_code=404,
                    detail=f"Reference WAV not found at '{req.reference_wav_path}'.",
                )
            for extra_path in req.reference_wav_paths or []:
                if not Path(extra_path).is_file():
                    raise HTTPException(
                        status_code=404,
                        detail=f"Reference WAV not found at '{extra_path}'.",
                    )

        engine_used = req.engine
        try:
            pcm_24k = _invoke_engine_synthesize(engine, req)
        except UnsupportedLanguageError as exc:
            raise HTTPException(status_code=422, detail=str(exc)) from exc
        except EngineUnavailableError as exc:
            fallback = registry.get("xtts_v2") if req.engine != "xtts_v2" else None
            if fallback is not None and (fallback.is_loaded() or getattr(fallback, "weights_present", lambda: False)()):
                logger.warning(
                    "Engine '%s' unavailable (%s); falling back to xtts_v2",
                    req.engine,
                    exc,
                )
                try:
                    pcm_24k = _invoke_engine_synthesize(fallback, req)
                    engine_used = "xtts_v2"
                except Exception as fallback_exc:
                    logger.exception("Fallback engine xtts_v2 also failed")
                    raise HTTPException(
                        status_code=503,
                        detail=f"Primary ({req.engine}) failed: {exc}; fallback (xtts_v2) failed: {fallback_exc}",
                    ) from fallback_exc
            else:
                raise HTTPException(status_code=503, detail=str(exc)) from exc
        except Exception as exc:
            fallback = registry.get("xtts_v2") if req.engine != "xtts_v2" else None
            if fallback is not None and (fallback.is_loaded() or getattr(fallback, "weights_present", lambda: False)()):
                logger.warning(
                    "Engine '%s' failed (%s); falling back to xtts_v2",
                    req.engine,
                    exc,
                )
                try:
                    pcm_24k = _invoke_engine_synthesize(fallback, req)
                    engine_used = "xtts_v2"
                except Exception as fallback_exc:
                    logger.exception("Fallback engine xtts_v2 also failed")
                    raise HTTPException(
                        status_code=503,
                        detail=f"Primary ({req.engine}) failed: {exc}; fallback (xtts_v2) failed: {fallback_exc}",
                    ) from fallback_exc
            else:
                logger.exception("TTS synthesis failed on %s", req.engine)
                raise HTTPException(status_code=503, detail=str(exc)) from exc

        if pcm_24k.ndim != 1 or pcm_24k.size == 0 or not np.all(np.isfinite(pcm_24k)):
            raise HTTPException(status_code=500, detail="Engine produced invalid PCM tensor")

        wav_bytes = _encode_pcm16_riff_wav(pcm_24k)
        duration_ms = int(round((pcm_24k.size * 1000.0) / TARGET_SAMPLE_RATE))

        return Response(
            content=wav_bytes,
            status_code=200,
            media_type="audio/wav",
            headers={
                "x-engine-used": engine_used,
                "x-target-language": req.target_language,
                "x-sample-rate": str(TARGET_SAMPLE_RATE),
                "x-duration-ms": str(duration_ms),
                "x-finetuned-used": "true" if req.finetuned_checkpoint_dir else "false",
            },
        )

    @fastapi_app.post("/finetune", status_code=202)
    def post_finetune(req: FinetuneRequest) -> dict[str, object]:
        if not allow_synthetic_fallback():
            for sample in req.samples:
                if not Path(sample.wav_path).is_file():
                    raise HTTPException(
                        status_code=404,
                        detail=f"Sample WAV not found at '{sample.wav_path}'.",
                    )
            if not ft_manager.base_weights_present():
                raise HTTPException(
                    status_code=503,
                    detail=f"Base XTTS-v2 fine-tuning weights not found at '{ft_manager.base_model_dir}'.",
                )

        domain_samples = [
            FinetuneSample(wav_path=s.wav_path, transcript_en=s.transcript_en)
            for s in req.samples
        ]
        try:
            return ft_manager.submit_job(
                job_id=req.job_id,
                profile_slug=req.profile_slug,
                samples=domain_samples,
                output_dir=req.output_dir,
                epochs=req.epochs,
                learning_rate=req.learning_rate,
            )
        except DuplicateFinetuneError as exc:
            raise HTTPException(status_code=409, detail=str(exc)) from exc

    @fastapi_app.get("/finetune/{job_id}")
    def get_finetune_status(job_id: str) -> dict[str, object]:
        state = ft_manager.get_job(job_id)
        if state is None:
            raise HTTPException(
                status_code=404,
                detail=f"Fine-tune job '{job_id}' not found.",
            )
        return state

    @fastapi_app.post("/speaker-similarity")
    def post_speaker_similarity(req: SpeakerSimilarityRequest) -> dict[str, object]:
        ref_path = Path(req.reference_wav_path)
        if not ref_path.is_file():
            raise HTTPException(
                status_code=404,
                detail=f"Reference WAV not found at '{req.reference_wav_path}'.",
            )

        try:
            cand_bytes = base64.b64decode(req.candidate_wav_base64, validate=True)
        except (binascii.Error, ValueError) as exc:
            raise HTTPException(
                status_code=422,
                detail=f"Invalid candidate_wav_base64 encoding: {exc}",
            ) from exc

        try:
            ref_pcm, ref_sr = sf.read(str(ref_path), dtype="float32")
        except Exception as exc:
            raise HTTPException(
                status_code=422,
                detail=f"Failed reading reference WAV: {exc}",
            ) from exc

        try:
            cand_pcm, cand_sr = sf.read(io.BytesIO(cand_bytes), dtype="float32")
        except Exception as exc:
            raise HTTPException(
                status_code=422,
                detail=f"Failed decoding candidate WAV bytes: {exc}",
            ) from exc

        if ref_pcm.size == 0 or cand_pcm.size == 0:
            raise HTTPException(status_code=422, detail="Audio payload must not be empty")

        similarity_bps, backend = speaker_scorer.score_bps(
            ref_pcm=ref_pcm,
            ref_sr=int(ref_sr),
            cand_pcm=cand_pcm,
            cand_sr=int(cand_sr),
        )
        return {
            "similarity_bps": similarity_bps,
            "backend": backend,
        }

    return fastapi_app


app = create_app()
