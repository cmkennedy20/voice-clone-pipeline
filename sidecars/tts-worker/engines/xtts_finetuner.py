from __future__ import annotations

from dataclasses import dataclass
import gc
import json
import logging
import math
import os
from pathlib import Path
import random
import shutil
import threading
import time
from typing import Any, Literal

import numpy as np
from scipy import signal
import soundfile as sf

from engines import allow_synthetic_fallback, default_models_subdir, rms_normalize_mono

logger = logging.getLogger("tts_worker.finetune")

FINETUNE_SAMPLE_RATE: int = 22_050
FinetuneStatus = Literal["queued", "running", "succeeded", "failed"]


class DuplicateFinetuneError(RuntimeError):
    """Raised when a profile already has an active fine-tuning job running or queued."""


@dataclass(frozen=True)
class FinetuneSample:
    wav_path: str
    transcript_en: str


@dataclass
class FinetuneJobState:
    job_id: str
    profile_slug: str
    status: FinetuneStatus
    epochs: int
    current_epoch: int = 0
    progress_bps: int = 0
    last_loss: float | None = None
    checkpoint_dir: str | None = None
    error_message: str | None = None

    def to_dict(self) -> dict[str, object]:
        return {
            "job_id": self.job_id,
            "profile_slug": self.profile_slug,
            "status": self.status,
            "epochs": self.epochs,
            "current_epoch": self.current_epoch,
            "progress_bps": max(0, min(10_000, int(self.progress_bps))),
            "last_loss": self.last_loss,
            "checkpoint_dir": self.checkpoint_dir,
            "error_message": self.error_message,
        }


def _truncate_words_to_char_limit(text: str, max_chars: int = 185) -> str:
    cleaned = " ".join(text.strip().split())
    if len(cleaned) <= max_chars:
        return cleaned
    words = cleaned.split(" ")
    kept: list[str] = []
    current_len = 0
    for word in words:
        added = len(word) if not kept else len(word) + 1
        if current_len + added > max_chars:
            break
        kept.append(word)
        current_len += added
    return " ".join(kept) if kept else cleaned[:max_chars]


def _find_low_energy_split_index(
    wav: np.ndarray,
    lo_idx: int,
    hi_idx: int,
    sr: int,
) -> int:
    lo = max(1, min(lo_idx, wav.size - 1))
    hi = max(lo + 1, min(hi_idx, wav.size))
    win = max(256, int(round(0.050 * sr)))
    hop = max(128, win // 2)
    if hi - lo <= win:
        return (lo + hi) // 2

    best_center = (lo + hi) // 2
    best_energy = float("inf")
    for start in range(lo, max(lo + 1, hi - win + 1), hop):
        seg = wav[start : start + win]
        energy = float(np.dot(seg, seg))
        if energy < best_energy:
            best_energy = energy
            best_center = start + (win // 2)
    return best_center


def slice_long_clip_and_transcript(
    wav_22k: np.ndarray,
    transcript: str,
    sr: int = FINETUNE_SAMPLE_RATE,
    trigger_sec: float = 11.0,
    max_chunk_sec: float = 10.5,
    min_chunk_sec: float = 2.0,
) -> list[tuple[np.ndarray, str]]:
    arr = np.asarray(wav_22k, dtype=np.float32).reshape(-1)
    cleaned_text = " ".join(transcript.strip().split()) or "hello"
    if arr.size == 0:
        return []

    trigger_samples = int(round(trigger_sec * sr))
    max_chunk_samples = int(round(max_chunk_sec * sr))
    min_chunk_samples = int(round(min_chunk_sec * sr))

    if arr.size <= trigger_samples:
        if arr.size > max_chunk_samples:
            arr = arr[:max_chunk_samples]
        return [(arr, cleaned_text)]

    chunks: list[tuple[np.ndarray, str]] = []
    rem_wav = arr
    rem_words = cleaned_text.split(" ")

    while rem_wav.size > max_chunk_samples:
        if rem_wav.size > max_chunk_samples + min_chunk_samples:
            hi = max_chunk_samples
            lo = max(min_chunk_samples, int(round(max_chunk_samples * 0.50)))
        else:
            hi = max(min_chunk_samples + 1, rem_wav.size - min_chunk_samples)
            lo = max(min_chunk_samples, int(round(rem_wav.size * 0.35)))
            if lo >= hi:
                lo = max(1, hi // 2)

        split_idx = _find_low_energy_split_index(rem_wav, lo, hi, sr=sr)
        split_idx = max(1, min(split_idx, max_chunk_samples, rem_wav.size - 1))

        if len(rem_words) > 1:
            ratio = split_idx / float(rem_wav.size)
            n_take = max(1, min(len(rem_words) - 1, int(round(len(rem_words) * ratio))))
            chunk_text = " ".join(rem_words[:n_take])
            rem_words = rem_words[n_take:]
        else:
            chunk_text = rem_words[0] if rem_words else "hello"

        chunks.append((rem_wav[:split_idx].copy(), chunk_text))
        rem_wav = rem_wav[split_idx:]

    if rem_wav.size > 0:
        tail_text = " ".join(rem_words) if rem_words else "hello"
        chunks.append((rem_wav.copy(), tail_text))

    return chunks


def _load_and_resample_22k_mono(wav_path: str) -> np.ndarray:
    raw_pcm, orig_sr = sf.read(wav_path, dtype="float32")
    arr = np.nan_to_num(np.asarray(raw_pcm, dtype=np.float32), nan=0.0, posinf=0.0, neginf=0.0)
    if arr.ndim == 2:
        if arr.shape[0] <= 8 and arr.shape[1] > arr.shape[0]:
            arr = np.mean(arr, axis=0, dtype=np.float32)
        else:
            arr = np.mean(arr, axis=1, dtype=np.float32)
    elif arr.ndim > 2:
        arr = arr.reshape(-1).astype(np.float32)

    if arr.size == 0:
        return np.zeros(0, dtype=np.float32)

    sr_int = int(orig_sr)
    if sr_int != FINETUNE_SAMPLE_RATE:
        g = math.gcd(FINETUNE_SAMPLE_RATE, sr_int)
        arr = signal.resample_poly(
            arr, FINETUNE_SAMPLE_RATE // g, sr_int // g
        ).astype(np.float32)

    arr = rms_normalize_mono(arr, target_rms=0.12, max_peak=0.92)
    min_samples = int(round(0.60 * FINETUNE_SAMPLE_RATE))
    if arr.size < min_samples:
        arr = np.pad(arr, (0, min_samples - arr.size))
    return arr.astype(np.float32)


class XttsFineTuneManager:
    REQUIRED_BASE_FILES: tuple[str, ...] = (
        "model.pth",
        "dvae.pth",
        "mel_stats.pth",
        "vocab.json",
        "config.json",
    )

    def __init__(self, base_model_dir: str | Path | None = None) -> None:
        resolved = (
            base_model_dir
            or os.environ.get("XTTS_MODEL_DIR", "").strip()
            or default_models_subdir("xtts-v2")
        )
        self.base_model_dir = Path(resolved)
        self._state_lock = threading.Lock()
        self._gpu_lock = threading.Lock()
        self._jobs: dict[str, FinetuneJobState] = {}

    def base_weights_present(self) -> bool:
        if not self.base_model_dir.is_dir():
            return False
        return all((self.base_model_dir / name).is_file() for name in self.REQUIRED_BASE_FILES)

    def get_job(self, job_id: str) -> dict[str, object] | None:
        with self._state_lock:
            state = self._jobs.get(job_id)
            return state.to_dict() if state is not None else None

    def submit_job(
        self,
        job_id: str,
        profile_slug: str,
        samples: list[FinetuneSample],
        output_dir: str,
        epochs: int = 6,
        learning_rate: float = 5e-6,
    ) -> dict[str, object]:
        with self._state_lock:
            existing = self._jobs.get(job_id)
            if existing is not None:
                return existing.to_dict()

            for active_job in self._jobs.values():
                if active_job.profile_slug == profile_slug and active_job.status in (
                    "queued",
                    "running",
                ):
                    raise DuplicateFinetuneError(
                        f"Profile '{profile_slug}' already has an active fine-tuning job '{active_job.job_id}'"
                    )

            state = FinetuneJobState(
                job_id=job_id,
                profile_slug=profile_slug,
                status="queued",
                epochs=int(epochs),
                current_epoch=0,
                progress_bps=0,
                last_loss=None,
                checkpoint_dir=None,
                error_message=None,
            )
            self._jobs[job_id] = state
            initial_snapshot = state.to_dict()

        worker = threading.Thread(
            target=self._execute_job,
            args=(job_id, profile_slug, list(samples), output_dir, int(epochs), float(learning_rate)),
            name=f"xtts-ft-{job_id}",
            daemon=True,
        )
        worker.start()
        return initial_snapshot

    def _update_job(
        self,
        job_id: str,
        *,
        status: FinetuneStatus | None = None,
        current_epoch: int | None = None,
        progress_bps: int | None = None,
        last_loss: float | None = None,
        checkpoint_dir: str | None = None,
        error_message: str | None = None,
    ) -> None:
        with self._state_lock:
            state = self._jobs.get(job_id)
            if state is None:
                return
            if status is not None:
                state.status = status
            if current_epoch is not None:
                state.current_epoch = current_epoch
            if progress_bps is not None:
                state.progress_bps = max(0, min(10_000, int(progress_bps)))
            if last_loss is not None:
                state.last_loss = float(last_loss)
            if checkpoint_dir is not None:
                state.checkpoint_dir = checkpoint_dir
            if error_message is not None:
                state.error_message = error_message

    def _execute_job(
        self,
        job_id: str,
        profile_slug: str,
        samples: list[FinetuneSample],
        output_dir: str,
        epochs: int,
        learning_rate: float,
    ) -> None:
        with self._gpu_lock:
            self._update_job(job_id, status="running", progress_bps=100)
            try:
                if self.base_weights_present():
                    self._run_real_pytorch_finetune(
                        job_id=job_id,
                        profile_slug=profile_slug,
                        samples=samples,
                        output_dir=output_dir,
                        epochs=epochs,
                        learning_rate=learning_rate,
                    )
                elif allow_synthetic_fallback():
                    self._run_synthetic_finetune(
                        job_id=job_id,
                        profile_slug=profile_slug,
                        samples=samples,
                        output_dir=output_dir,
                        epochs=epochs,
                        learning_rate=learning_rate,
                    )
                else:
                    raise RuntimeError(
                        f"XTTS-v2 fine-tuning weights absent at {self.base_model_dir}"
                    )
            except Exception as exc:
                logger.exception("XTTS fine-tune job %s failed", job_id)
                self._update_job(
                    job_id,
                    status="failed",
                    error_message=str(exc),
                )

    def _run_synthetic_finetune(
        self,
        job_id: str,
        profile_slug: str,
        samples: list[FinetuneSample],
        output_dir: str,
        epochs: int,
        learning_rate: float,
    ) -> None:
        step_sleep = min(0.01, 0.05 / max(1, epochs))
        for ep in range(1, epochs + 1):
            time.sleep(step_sleep)
            sim_loss = round(0.92 - (0.50 * (ep / float(epochs))), 4)
            bps = int(round((ep / float(epochs)) * 9_500.0))
            self._update_job(
                job_id,
                current_epoch=ep,
                progress_bps=bps,
                last_loss=sim_loss,
            )

        out_path = Path(output_dir)
        out_path.mkdir(parents=True, exist_ok=True)
        staging = out_path.parent / f".{out_path.name}.tmp.{job_id}"
        staging.mkdir(parents=True, exist_ok=True)
        try:
            (staging / "config.json").write_text(
                json.dumps(
                    {
                        "model": "xtts",
                        "synthetic": True,
                        "profile_slug": profile_slug,
                    },
                    indent=2,
                ),
                encoding="utf-8",
            )
            (staging / "synthetic_finetuned.json").write_text(
                json.dumps(
                    {
                        "job_id": job_id,
                        "profile_slug": profile_slug,
                        "epochs": epochs,
                        "learning_rate": learning_rate,
                        "sample_count": len(samples),
                        "last_loss": 0.42,
                    },
                    indent=2,
                ),
                encoding="utf-8",
            )
            for item in staging.iterdir():
                os.replace(str(item), str(out_path / item.name))
        finally:
            shutil.rmtree(staging, ignore_errors=True)

        self._update_job(
            job_id,
            status="succeeded",
            current_epoch=epochs,
            progress_bps=10_000,
            last_loss=0.42,
            checkpoint_dir=str(out_path),
        )

    def _run_real_pytorch_finetune(
        self,
        job_id: str,
        profile_slug: str,
        samples: list[FinetuneSample],
        output_dir: str,
        epochs: int,
        learning_rate: float,
    ) -> None:
        import torch  # type: ignore[import-not-found]
        import torch.nn as nn  # type: ignore[import-not-found]
        import torch.nn.functional as F  # type: ignore[import-not-found]
        from TTS.tts.configs.xtts_config import XttsConfig  # type: ignore[import-not-found]
        from TTS.tts.layers.xtts.trainer.gpt_trainer import (  # type: ignore[import-not-found]
            GPTArgs,
            GPTTrainer,
            GPTTrainerConfig,
        )
        from TTS.tts.models.xtts import XttsAudioConfig  # type: ignore[import-not-found]

        trainer_model: Any = None
        optimizer: Any = None
        scaler: Any = None
        prepared_items: list[tuple[Any, Any]] = []
        batch: dict[str, Any] = {}
        xtts_state: dict[str, Any] = {}
        loss_text: Any = None
        loss_mel: Any = None
        total_loss: Any = None
        out_path = Path(output_dir)
        staging_dir = out_path.parent / f".{out_path.name}.tmp.{job_id}"

        try:
            raw_chunks: list[tuple[np.ndarray, str]] = []
            for sample in samples:
                wav_22k = _load_and_resample_22k_mono(sample.wav_path)
                if wav_22k.size == 0:
                    continue
                sliced = slice_long_clip_and_transcript(
                    wav_22k,
                    sample.transcript_en,
                    sr=FINETUNE_SAMPLE_RATE,
                    trigger_sec=11.0,
                    max_chunk_sec=10.5,
                    min_chunk_sec=2.0,
                )
                raw_chunks.extend(sliced)

            if not raw_chunks:
                raise ValueError("No valid non-empty audio chunks could be prepared from samples")

            base_xtts_cfg = XttsConfig()
            base_xtts_cfg.load_json(str(self.base_model_dir / "config.json"))
            base_args = base_xtts_cfg.model_args

            max_cond_len = 132_300  # 6.0s at 22050 Hz
            min_cond_len = 66_150   # 3.0s at 22050 Hz

            model_args = GPTArgs(
                max_conditioning_length=max_cond_len,
                min_conditioning_length=min_cond_len,
                debug_loading_failures=False,
                max_wav_length=255_995,
                max_text_length=200,
                mel_norm_file=str(self.base_model_dir / "mel_stats.pth"),
                dvae_checkpoint=str(self.base_model_dir / "dvae.pth"),
                xtts_checkpoint=str(self.base_model_dir / "model.pth"),
                tokenizer_file=str(self.base_model_dir / "vocab.json"),
                gpt_layers=int(getattr(base_args, "gpt_layers", 30)),
                gpt_n_model_channels=int(getattr(base_args, "gpt_n_model_channels", 1024)),
                gpt_n_heads=int(getattr(base_args, "gpt_n_heads", 16)),
                gpt_max_audio_tokens=int(getattr(base_args, "gpt_max_audio_tokens", 605)),
                gpt_max_text_tokens=int(getattr(base_args, "gpt_max_text_tokens", 402)),
                gpt_max_prompt_tokens=int(getattr(base_args, "gpt_max_prompt_tokens", 70)),
                gpt_num_audio_tokens=int(getattr(base_args, "gpt_num_audio_tokens", 1026)),
                gpt_start_audio_token=int(getattr(base_args, "gpt_start_audio_token", 1024)),
                gpt_stop_audio_token=int(getattr(base_args, "gpt_stop_audio_token", 1025)),
                gpt_code_stride_len=int(getattr(base_args, "gpt_code_stride_len", 1024)),
                gpt_use_masking_gt_prompt_approach=True,
                gpt_use_perceiver_resampler=bool(
                    getattr(base_args, "gpt_use_perceiver_resampler", True)
                ),
            )
            audio_config = XttsAudioConfig(
                sample_rate=FINETUNE_SAMPLE_RATE,
                dvae_sample_rate=FINETUNE_SAMPLE_RATE,
                output_sample_rate=24_000,
            )
            trainer_cfg = GPTTrainerConfig(
                output_path=str(staging_dir),
                model_args=model_args,
                audio=audio_config,
                batch_size=1,
                eval_batch_size=1,
                num_loader_workers=0,
                lr=float(learning_rate),
                optimizer="AdamW",
                optimizer_wd_only_on_weights=True,
                optimizer_params={"betas": [0.9, 0.96], "eps": 1e-8, "weight_decay": 1e-2},
                test_sentences=[],
            )

            device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
            trainer_model = GPTTrainer.init_from_config(trainer_cfg)
            trainer_model.to(device)
            trainer_model.eval()

            # Freeze DVAE, HiFiGAN decoder, and mel extractors so only GPT trains in VRAM
            for submod_name in (
                "dvae",
                "torch_mel_spectrogram_style_encoder",
                "torch_mel_spectrogram_dvae",
            ):
                submod = getattr(trainer_model, submod_name, None)
                if submod is not None:
                    submod.requires_grad_(False)
            if hasattr(trainer_model.xtts, "hifigan_decoder"):
                trainer_model.xtts.hifigan_decoder.requires_grad_(False)

            trainer_model.xtts.gpt.requires_grad_(True)
            trainer_model.xtts.gpt.train()

            tokenizer = trainer_model.xtts.tokenizer
            fallback_tokens = [
                t for t in tokenizer.encode("hello world", lang="en") if t not in (0, 1)
            ] or [2]

            prepared_items: list[tuple[Any, Any]] = []
            for chunk_pcm, chunk_text in raw_chunks:
                bounded_text = _truncate_words_to_char_limit(chunk_text, max_chars=185)
                encoded = [
                    int(tok)
                    for tok in tokenizer.encode(bounded_text, lang="en")
                    if int(tok) not in (0, 1)
                ][:185]
                if not encoded:
                    encoded = list(fallback_tokens)
                wav_tensor = torch.from_numpy(chunk_pcm[:255_000]).float()
                tok_tensor = torch.tensor(encoded, dtype=torch.long)
                prepared_items.append((wav_tensor, tok_tensor))

            norm_cls = (
                nn.LayerNorm,
                nn.GroupNorm,
                nn.BatchNorm1d,
                nn.BatchNorm2d,
                nn.InstanceNorm1d,
                nn.InstanceNorm2d,
            )
            emb_cls = (nn.Embedding, nn.EmbeddingBag)
            decay_params: list[Any] = []
            no_decay_params: list[Any] = []
            seen_params: set[int] = set()
            for mod in trainer_model.xtts.gpt.modules():
                for pname, param in mod.named_parameters(recurse=False):
                    if not param.requires_grad or id(param) in seen_params:
                        continue
                    seen_params.add(id(param))
                    if pname.endswith(".bias") or pname == "bias" or isinstance(mod, norm_cls + emb_cls):
                        no_decay_params.append(param)
                    else:
                        decay_params.append(param)

            param_groups = [
                group
                for group in (
                    {"params": decay_params, "weight_decay": 1e-2},
                    {"params": no_decay_params, "weight_decay": 0.0},
                )
                if group["params"]
            ]
            optimizer = torch.optim.AdamW(
                param_groups,
                lr=float(learning_rate),
                betas=(0.9, 0.96),
                eps=1e-8,
            )

            use_amp = device.type == "cuda"
            try:
                scaler = torch.amp.GradScaler("cuda", enabled=use_amp)
            except Exception:
                scaler = torch.cuda.amp.GradScaler(enabled=use_amp)

            total_steps = max(1, epochs * len(prepared_items))
            last_valid_loss: float | None = None
            text_ce_w = float(getattr(model_args, "gpt_loss_text_ce_weight", 0.01))
            mel_ce_w = float(getattr(model_args, "gpt_loss_mel_ce_weight", 1.0))

            for epoch_idx in range(epochs):
                rng = random.Random(1000 + epoch_idx)
                order = list(range(len(prepared_items)))
                rng.shuffle(order)
                epoch_losses: list[float] = []

                for step_pos, item_idx in enumerate(order):
                    wav_t, tok_t = prepared_items[item_idx]
                    cond_src_wav = (
                        prepared_items[order[(step_pos + 1) % len(order)]][0]
                        if len(prepared_items) > 1
                        else wav_t
                    )

                    src_len = int(cond_src_wav.numel())
                    if src_len >= min_cond_len:
                        slice_len = min(src_len, rng.randint(min_cond_len, max_cond_len))
                        start_off = rng.randint(0, max(0, src_len - slice_len))
                        cond_slice = cond_src_wav[start_off : start_off + slice_len]
                    else:
                        cond_slice = cond_src_wav
                    cond_actual_len = int(cond_slice.numel())
                    cond_padded = F.pad(cond_slice, (0, max(0, max_cond_len - cond_actual_len)))[
                        :max_cond_len
                    ]

                    batch: dict[str, Any] = {
                        "padded_text": tok_t.unsqueeze(0).to(device),
                        "text_lengths": torch.tensor([tok_t.numel()], dtype=torch.long, device=device),
                        "wav": wav_t.view(1, 1, -1).to(device),
                        "wav_lengths": torch.tensor([wav_t.numel()], dtype=torch.long, device=device),
                        "conditioning": cond_padded.view(1, 1, 1, max_cond_len).to(device),
                        "cond_idxs": None,
                        "cond_lens": torch.tensor([cond_actual_len], dtype=torch.long, device=device),
                    }
                    batch = trainer_model.format_batch_on_device(batch)

                    optimizer.zero_grad(set_to_none=True)
                    with torch.amp.autocast(
                        device_type=device.type,
                        dtype=torch.float16,
                        enabled=use_amp,
                    ):
                        loss_text, loss_mel, _ = trainer_model.forward(
                            batch["text_inputs"],
                            batch["text_lengths"],
                            batch["audio_codes"],
                            batch["wav_lengths"],
                            batch["cond_mels"],
                            batch["cond_idxs"],
                            batch["cond_lens"],
                        )
                        total_loss = (loss_text * text_ce_w) + (loss_mel * mel_ce_w)

                    if torch.isfinite(total_loss):
                        scaler.scale(total_loss).backward()
                        scaler.unscale_(optimizer)
                        torch.nn.utils.clip_grad_norm_(
                            trainer_model.xtts.gpt.parameters(),
                            max_norm=1.0,
                        )
                        scaler.step(optimizer)
                        scaler.update()
                        loss_val = float(total_loss.detach().cpu().item())
                        epoch_losses.append(loss_val)
                        last_valid_loss = loss_val

                    done_steps = epoch_idx * len(prepared_items) + (step_pos + 1)
                    step_bps = max(100, min(9_500, int(round((done_steps / float(total_steps)) * 9_500.0))))
                    self._update_job(
                        job_id,
                        progress_bps=step_bps,
                        last_loss=last_valid_loss,
                    )

                mean_epoch_loss = (
                    sum(epoch_losses) / len(epoch_losses)
                    if epoch_losses
                    else last_valid_loss
                )
                self._update_job(
                    job_id,
                    current_epoch=epoch_idx + 1,
                    last_loss=mean_epoch_loss,
                )

            # Export fine-tuned XTTS checkpoint atomically in Xtts.load_checkpoint format
            out_path.mkdir(parents=True, exist_ok=True)
            staging_dir.mkdir(parents=True, exist_ok=True)

            ignore_prefixes = (
                "torch_mel_spectrogram_style_encoder.",
                "torch_mel_spectrogram_dvae.",
                "dvae.",
                "gpt.gpt_inference.",
                "gpt.gpt.wte.",
            )
            xtts_state: dict[str, Any] = {}
            for raw_key, tensor in trainer_model.xtts.state_dict().items():
                clean_key = raw_key[5:] if raw_key.startswith("xtts.") else raw_key
                if clean_key.startswith(ignore_prefixes):
                    continue
                xtts_state[clean_key] = tensor.detach().cpu()

            torch.save({"model": xtts_state}, str(staging_dir / "model.pth"))
            shutil.copy2(str(self.base_model_dir / "config.json"), str(staging_dir / "config.json"))
            shutil.copy2(str(self.base_model_dir / "vocab.json"), str(staging_dir / "vocab.json"))

            base_speakers = self.base_model_dir / "speakers_xtts.pth"
            if base_speakers.is_file():
                staged_speakers = staging_dir / "speakers_xtts.pth"
                try:
                    os.symlink(str(base_speakers.resolve()), str(staged_speakers))
                except OSError:
                    shutil.copy2(str(base_speakers), str(staged_speakers))

            for staged_item in list(staging_dir.iterdir()):
                dest_item = out_path / staged_item.name
                if dest_item.is_symlink() or dest_item.is_file():
                    try:
                        dest_item.unlink()
                    except OSError:
                        pass
                os.replace(str(staged_item), str(dest_item))

            self._update_job(
                job_id,
                status="succeeded",
                current_epoch=epochs,
                progress_bps=10_000,
                last_loss=last_valid_loss if last_valid_loss is not None else 0.0,
                checkpoint_dir=str(out_path),
            )
        finally:
            shutil.rmtree(staging_dir, ignore_errors=True)
            prepared_items.clear()
            batch.clear()
            xtts_state.clear()
            loss_text = None
            loss_mel = None
            total_loss = None
            del trainer_model, optimizer, scaler
            gc.collect()
            try:
                if torch.cuda.is_available():
                    torch.cuda.empty_cache()
            except Exception:
                pass
