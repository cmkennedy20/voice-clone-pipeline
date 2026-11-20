from __future__ import annotations

from dataclasses import dataclass
import hashlib
import math
import os
from pathlib import Path
from typing import Protocol, runtime_checkable

import numpy as np
from scipy import fft as sp_fft, ndimage, signal
import soundfile as sf

TARGET_SAMPLE_RATE: int = 24_000
_STRIP_PUNCT = set("，。！？、；：“”‘’（）《》,.!?;:'\"()[]{}-—…¿¡«»،؟।॥")

SUPPORTED_LANGUAGE_MAP: dict[str, str] = {
    "zh": "zh-cn",
    "es": "es",
    "fr": "fr",
    "de": "de",
    "it": "it",
    "pt": "pt",
    "ja": "ja",
    "ko": "ko",
    "ru": "ru",
    "ar": "ar",
    "hi": "hi",
    "en": "en",
}

COSYVOICE_SUPPORTED_LANGUAGES: frozenset[str] = frozenset({"zh", "ja", "ko", "en"})


@dataclass(frozen=True)
class SynthesisTuningParams:
    temperature: float = 0.62
    top_p: float = 0.80
    top_k: int = 50
    repetition_penalty: float = 5.0
    length_penalty: float = 1.0
    speed: float = 1.0
    gpt_cond_len_secs: int = 30
    gpt_cond_chunk_len_secs: int = 4
    max_ref_len_secs: int = 60
    sound_norm_refs: bool = True


DEFAULT_SYNTHESIS_TUNING = SynthesisTuningParams()


class EngineUnavailableError(RuntimeError):
    """Raised when requested TTS model weights are absent and synthetic fallback is disabled."""


class UnsupportedLanguageError(ValueError):
    """Raised when a TTS engine does not support the requested target language."""


@runtime_checkable
class TtsEngine(Protocol):
    engine_id: str

    def is_loaded(self) -> bool: ...

    def prewarm(self) -> bool: ...

    def can_serve(self) -> bool: ...

    def synthesize(
        self,
        spoken_hanzi: str,
        reference_wav_path: str,
        reference_transcript_en: str,
        seed: int,
        target_language: str = "zh",
    ) -> np.ndarray: ...


def allow_synthetic_fallback() -> bool:
    return os.environ.get("ALLOW_SYNTHETIC_FALLBACK", "0").strip() == "1"


def default_models_subdir(subdir: str) -> Path:
    """Resolves `<MODELS_DIR>/<subdir>` cross-platform (Windows `C:\\...` & Linux `/models`)."""
    base = os.environ.get("MODELS_DIR", "/models").strip() or "/models"
    return Path(base) / subdir


def detect_device_and_vram_mib() -> tuple[str, int]:
    try:
        import torch  # type: ignore[import-not-found]

        if torch.cuda.is_available():
            allocated_bytes = int(torch.cuda.memory_allocated())
            return "cuda", allocated_bytes // (1024 * 1024)
    except Exception:
        pass
    return "cpu", 0


def seed_torch_and_cuda(seed: int) -> None:
    bounded_seed = int(seed) & 0x7FFFFFFFFFFFFFFF
    try:
        import torch  # type: ignore[import-not-found]

        torch.manual_seed(bounded_seed)
        if torch.cuda.is_available():
            torch.cuda.manual_seed_all(bounded_seed)
    except Exception:
        pass


def normalize_and_resample_24k_mono(
    pcm: np.ndarray,
    orig_sr: int,
    target_peak: float | None = None,
) -> np.ndarray:
    arr = np.asarray(pcm, dtype=np.float32)
    arr = np.nan_to_num(arr, nan=0.0, posinf=0.0, neginf=0.0)

    if arr.ndim == 2:
        if arr.shape[0] <= 8 and arr.shape[1] > arr.shape[0]:
            arr = np.mean(arr, axis=0, dtype=np.float32)
        else:
            arr = np.mean(arr, axis=1, dtype=np.float32)
    elif arr.ndim > 2:
        arr = arr.reshape(-1).astype(np.float32)

    if arr.size == 0:
        return np.zeros(0, dtype=np.float32)

    if int(orig_sr) != TARGET_SAMPLE_RATE:
        g = math.gcd(TARGET_SAMPLE_RATE, int(orig_sr))
        up = TARGET_SAMPLE_RATE // g
        down = int(orig_sr) // g
        arr = signal.resample_poly(arr, up, down).astype(np.float32)

    peak = float(np.max(np.abs(arr)))
    if target_peak is not None and peak > 1e-8:
        arr = arr * (float(target_peak) / peak)
    elif peak > 1.0:
        arr = arr * (0.98 / peak)

    return np.clip(arr, -1.0, 1.0).astype(np.float32)


def rms_normalize_mono(
    pcm: np.ndarray,
    target_rms: float = 0.12,
    max_peak: float = 0.95,
) -> np.ndarray:
    arr = np.asarray(pcm, dtype=np.float32).reshape(-1)
    arr = np.nan_to_num(arr, nan=0.0, posinf=0.0, neginf=0.0)
    if arr.size == 0:
        return arr
    rms = float(np.sqrt(np.mean(np.square(arr))))
    if rms > 1e-6:
        arr = arr * (float(target_rms) / rms)
    peak = float(np.max(np.abs(arr)))
    if peak > max_peak and peak > 1e-8:
        arr = arr * (float(max_peak) / peak)
    return np.clip(arr, -1.0, 1.0).astype(np.float32)


def time_scale_pcm(pcm: np.ndarray, speed: float) -> np.ndarray:
    arr = np.asarray(pcm, dtype=np.float32).reshape(-1)
    eff_speed = float(np.clip(speed, 0.50, 2.00))
    if arr.size == 0 or abs(eff_speed - 1.0) <= 1e-6:
        return arr

    target_len = max(1, int(round(arr.size / eff_speed)))
    n_fft = 1024
    hop = 256
    if arr.size < n_fft * 2:
        up = 1000
        down = max(1, int(round(1000.0 * eff_speed)))
        g = math.gcd(up, down)
        scaled = signal.resample_poly(arr, up // g, down // g).astype(np.float32)
        return scaled[:target_len] if scaled.size >= target_len else np.pad(scaled, (0, target_len - scaled.size))

    _, _, zxx = signal.stft(arr, nperseg=n_fft, noverlap=n_fft - hop)
    n_bins, n_frames = zxx.shape
    if n_frames < 2:
        return arr

    time_steps = np.arange(0.0, n_frames - 1, eff_speed, dtype=np.float64)
    if time_steps.size == 0:
        time_steps = np.array([0.0], dtype=np.float64)

    omega = 2.0 * np.pi * hop * np.arange(n_bins, dtype=np.float64) / n_fft
    phase_acc = np.angle(zxx[:, 0]).astype(np.float64)
    out_stft = np.zeros((n_bins, time_steps.size), dtype=np.complex128)

    for out_idx, step in enumerate(time_steps):
        idx0 = int(np.floor(step))
        idx1 = min(idx0 + 1, n_frames - 1)
        alpha = step - idx0
        mag = (1.0 - alpha) * np.abs(zxx[:, idx0]) + alpha * np.abs(zxx[:, idx1])
        out_stft[:, out_idx] = mag * np.exp(1j * phase_acc)
        dphase = np.angle(zxx[:, idx1]) - np.angle(zxx[:, idx0]) - omega
        dphase -= 2.0 * np.pi * np.round(dphase / (2.0 * np.pi))
        phase_acc += omega + dphase

    _, stretched = signal.istft(out_stft, nperseg=n_fft, noverlap=n_fft - hop)
    stretched_f32 = np.asarray(stretched, dtype=np.float32)
    if stretched_f32.size >= target_len:
        stretched_f32 = stretched_f32[:target_len]
    else:
        stretched_f32 = np.pad(stretched_f32, (0, target_len - stretched_f32.size))
    return stretched_f32.astype(np.float32)


def _estimate_f0_and_envelope(ref_pcm_24k: np.ndarray) -> tuple[float, np.ndarray]:
    n_fft = 1024
    freqs = np.fft.rfftfreq(n_fft, d=1.0 / TARGET_SAMPLE_RATE)

    if ref_pcm_24k.size < 2048 or float(np.max(np.abs(ref_pcm_24k))) < 1e-5:
        default_env = (
            np.exp(-0.5 * ((freqs - 520.0) / 180.0) ** 2)
            + 0.65 * np.exp(-0.5 * ((freqs - 1480.0) / 320.0) ** 2)
            + 0.35 * np.exp(-0.5 * ((freqs - 2550.0) / 420.0) ** 2)
            + 0.05
        )
        return 150.0, default_env.astype(np.float64)

    win_len = min(4096, ref_pcm_24k.size)
    hop = max(512, win_len // 2)
    best_seg = ref_pcm_24k[:win_len]
    best_energy = -1.0
    for start in range(0, max(1, ref_pcm_24k.size - win_len + 1), hop):
        seg = ref_pcm_24k[start : start + win_len]
        energy = float(np.dot(seg, seg))
        if energy > best_energy:
            best_energy = energy
            best_seg = seg

    centered = (best_seg - np.mean(best_seg)).astype(np.float64)
    centered *= np.hanning(centered.size)
    corr = signal.correlate(centered, centered, mode="full")[centered.size - 1 :]
    min_lag = int(TARGET_SAMPLE_RATE / 340.0)
    max_lag = min(centered.size - 1, int(TARGET_SAMPLE_RATE / 75.0))
    if max_lag > min_lag and corr[0] > 1e-9:
        search = corr[min_lag : max_lag + 1]
        peak_idx = int(np.argmax(search)) + min_lag
        f0_hz = float(TARGET_SAMPLE_RATE / peak_idx) if corr[peak_idx] / corr[0] >= 0.20 else 150.0
    else:
        f0_hz = 150.0

    _, _, zxx = signal.stft(ref_pcm_24k, fs=TARGET_SAMPLE_RATE, nperseg=n_fft, noverlap=768)
    mag = np.abs(zxx)
    frame_power = np.mean(mag * mag, axis=0)
    max_power = float(np.max(frame_power)) if frame_power.size > 0 else 0.0
    if max_power > 1e-10:
        active = frame_power >= (max_power * 0.02)
        mag_active = mag[:, active] if np.any(active) else mag
    else:
        mag_active = mag

    mean_mag = np.mean(mag_active, axis=1) + 1e-6
    smooth_log = ndimage.gaussian_filter1d(np.log(mean_mag), sigma=4.0, mode="nearest")
    env = np.exp(smooth_log)
    env /= float(np.mean(env)) + 1e-8
    return f0_hz, env.astype(np.float64)


def synthesize_deterministic_fallback(
    engine_id: str,
    spoken_hanzi: str,
    reference_wav_path: str,
    reference_transcript_en: str,
    seed: int,
    target_language: str = "zh",
    reference_wav_paths: list[str] | None = None,
    tuning: SynthesisTuningParams | None = None,
) -> np.ndarray:
    spoken_units = [ch for ch in spoken_hanzi if not ch.isspace() and ch not in _STRIP_PUNCT]
    n_syllables = max(1, len(spoken_units))
    duration_s = float(np.clip(0.34 + 0.15 * n_syllables, 0.55, 30.0))
    if tuning is not None and abs(tuning.speed - 1.0) > 1e-6:
        eff_speed = float(np.clip(tuning.speed, 0.50, 2.00))
        duration_s = float(np.clip(duration_s / eff_speed, 0.30, 60.0))
    num_samples = int(round(duration_s * TARGET_SAMPLE_RATE))

    candidate_paths = (
        [p for p in reference_wav_paths if p and p.strip()]
        if reference_wav_paths and len(reference_wav_paths) > 1
        else ([reference_wav_path] if reference_wav_path else [])
    )
    max_ref_samples = (
        int(tuning.max_ref_len_secs * TARGET_SAMPLE_RATE)
        if tuning is not None
        else int(DEFAULT_SYNTHESIS_TUNING.max_ref_len_secs * TARGET_SAMPLE_RATE)
    )
    loaded_refs: list[np.ndarray] = []
    for cand in candidate_paths:
        cand_path = Path(cand)
        if cand_path.is_file():
            try:
                raw_ref, ref_sr = sf.read(str(cand_path), dtype="float32")
                norm_ref = normalize_and_resample_24k_mono(raw_ref, int(ref_sr))
                if len(candidate_paths) > 1 and norm_ref.size > max_ref_samples:
                    norm_ref = norm_ref[:max_ref_samples]
                if len(candidate_paths) > 1 and (tuning is None or tuning.sound_norm_refs):
                    norm_ref = rms_normalize_mono(norm_ref)
                if norm_ref.size > 0:
                    loaded_refs.append(norm_ref)
            except Exception:
                continue
    ref_pcm = np.concatenate(loaded_refs, axis=0) if loaded_refs else np.zeros(0, dtype=np.float32)

    f0_hz, target_env = _estimate_f0_and_envelope(ref_pcm)

    lang_suffix = "" if target_language == "zh" else f"|{target_language}"
    digest = hashlib.blake2b(
        f"{engine_id}|{spoken_hanzi}|{reference_transcript_en}|{int(seed)}{lang_suffix}".encode(
            "utf-8"
        ),
        digest_size=16,
    ).digest()
    rng_seed = int.from_bytes(digest, byteorder="little", signed=False)
    rng = np.random.default_rng(rng_seed)

    t = np.arange(num_samples, dtype=np.float64) / TARGET_SAMPLE_RATE
    ctrl_points = rng.uniform(-0.06, 0.06, size=n_syllables + 2)
    ctrl_points -= np.mean(ctrl_points)
    ctrl_x = np.linspace(0.0, duration_s, num=ctrl_points.size)
    contour = np.interp(t, ctrl_x, ctrl_points)
    vibrato_phase = float(rng.uniform(0.0, 2.0 * np.pi))
    inst_f0 = f0_hz * (1.0 + contour + 0.012 * np.sin(2.0 * np.pi * 5.3 * t + vibrato_phase))
    base_phase = 2.0 * np.pi * np.cumsum(inst_f0) / TARGET_SAMPLE_RATE

    max_harmonic = min(38, max(6, int(11_000.0 // max(f0_hz, 60.0))))
    harmonic_phases = rng.uniform(0.0, 2.0 * np.pi, size=max_harmonic)
    excitation = np.zeros(num_samples, dtype=np.float64)
    for k in range(1, max_harmonic + 1):
        excitation += (1.0 / (k**0.85)) * np.sin(k * base_phase + harmonic_phases[k - 1])
    excitation += 0.015 * rng.standard_normal(num_samples)

    syllable_rate = n_syllables / duration_s
    syl_mod = 0.62 + 0.38 * (0.5 - 0.5 * np.cos(2.0 * np.pi * syllable_rate * t))
    fade_len = min(int(0.025 * TARGET_SAMPLE_RATE), num_samples // 4)
    if fade_len > 1:
        fade = 0.5 - 0.5 * np.cos(np.linspace(0.0, np.pi, fade_len))
        syl_mod[:fade_len] *= fade
        syl_mod[-fade_len:] *= fade[::-1]
    excitation *= syl_mod

    n_fft = 1024
    _, _, z_src = signal.stft(excitation, fs=TARGET_SAMPLE_RATE, nperseg=n_fft, noverlap=768)
    src_mag = np.mean(np.abs(z_src), axis=1) + 1e-6
    src_env = np.exp(ndimage.gaussian_filter1d(np.log(src_mag), sigma=4.0, mode="nearest"))
    src_env /= float(np.mean(src_env)) + 1e-8

    transfer = np.clip(target_env / (src_env + 1e-4), 0.05, 20.0)
    z_shaped = z_src * transfer[:, np.newaxis]
    _, shaped = signal.istft(z_shaped, fs=TARGET_SAMPLE_RATE, nperseg=n_fft, noverlap=768)
    shaped = shaped[:num_samples]
    if shaped.size < num_samples:
        shaped = np.pad(shaped, (0, num_samples - shaped.size))

    return normalize_and_resample_24k_mono(shaped.astype(np.float32), TARGET_SAMPLE_RATE, target_peak=0.82)


def _hz_to_mel(hz: np.ndarray | float) -> np.ndarray | float:
    return 2595.0 * np.log10(1.0 + np.asarray(hz) / 700.0)


def _mel_to_hz(mel: np.ndarray | float) -> np.ndarray | float:
    return 700.0 * (10.0 ** (np.asarray(mel) / 2595.0) - 1.0)


def _build_mel24_filterbank(n_fft: int = 1024, n_mels: int = 24) -> np.ndarray:
    n_bins = n_fft // 2 + 1
    mel_min = float(_hz_to_mel(60.0))
    mel_max = float(_hz_to_mel(10_500.0))
    mel_points = np.linspace(mel_min, mel_max, n_mels + 2)
    hz_points = np.asarray(_mel_to_hz(mel_points), dtype=np.float64)
    fft_freqs = np.fft.rfftfreq(n_fft, d=1.0 / TARGET_SAMPLE_RATE)

    fb = np.zeros((n_mels, n_bins), dtype=np.float64)
    for m in range(n_mels):
        f_left, f_center, f_right = hz_points[m], hz_points[m + 1], hz_points[m + 2]
        left_slope = (fft_freqs - f_left) / max(f_center - f_left, 1e-6)
        right_slope = (f_right - fft_freqs) / max(f_right - f_center, 1e-6)
        fb[m] = np.maximum(0.0, np.minimum(left_slope, right_slope))
        area = float(np.sum(fb[m]))
        if area > 1e-9:
            fb[m] /= area
    return fb


_MEL24_FB = _build_mel24_filterbank(1024, 24)


def extract_logmel24_cepstral_embedding(pcm: np.ndarray, orig_sr: int) -> np.ndarray:
    wav = normalize_and_resample_24k_mono(pcm, orig_sr)
    if wav.size < 512:
        wav = np.pad(wav, (0, 512 - wav.size))

    rms = float(np.sqrt(np.mean(np.square(wav))))
    if rms < 1e-7:
        return np.zeros(64, dtype=np.float64)
    wav_norm = (wav.astype(np.float64) / rms) * 0.1

    n_fft = 1024
    _, _, zxx = signal.stft(wav_norm, fs=TARGET_SAMPLE_RATE, nperseg=n_fft, noverlap=512)
    power = np.abs(zxx) ** 2
    frame_energy = np.sum(power, axis=0)
    peak_energy = float(np.max(frame_energy)) if frame_energy.size > 0 else 0.0
    if peak_energy > 1e-12:
        active = frame_energy >= (0.02 * peak_energy)
        power_active = power[:, active] if np.any(active) else power
    else:
        power_active = power

    mel_power = _MEL24_FB @ power_active
    log_mel = np.log(mel_power + 1e-10)
    mean_log_mel = np.mean(log_mel, axis=1)
    centered_mel = mean_log_mel - float(np.mean(mean_log_mel))
    mel_unit = centered_mel / (float(np.linalg.norm(centered_mel)) + 1e-8)

    cepstral = sp_fft.dct(centered_mel, type=2, norm="ortho")[1:17]
    cep_unit = cepstral / (float(np.linalg.norm(cepstral)) + 1e-8)

    win = min(2048, wav_norm.size)
    hop = max(512, win // 2)
    min_lag = int(TARGET_SAMPLE_RATE / 400.0)
    max_lag = min(win - 1, int(TARGET_SAMPLE_RATE / 75.0))
    pitch_bins = np.zeros(24, dtype=np.float64)
    if max_lag > min_lag + 4:
        lag_edges = np.geomspace(min_lag, max_lag, 25)
        accum = np.zeros(max_lag - min_lag + 1, dtype=np.float64)
        n_frames = 0
        for start in range(0, max(1, wav_norm.size - win + 1), hop):
            seg = wav_norm[start : start + win]
            e = float(np.dot(seg, seg))
            if e < 1e-6:
                continue
            seg_w = (seg - np.mean(seg)) * np.hanning(seg.size)
            corr = signal.correlate(seg_w, seg_w, mode="full")[seg.size - 1 :]
            if corr[0] > 1e-9:
                accum += np.maximum(0.0, corr[min_lag : max_lag + 1] / corr[0])
                n_frames += 1
        if n_frames > 0:
            accum /= n_frames
            lags = np.arange(min_lag, max_lag + 1, dtype=np.float64)
            for b in range(24):
                mask = (lags >= lag_edges[b]) & (lags <= lag_edges[b + 1] + 0.5)
                if np.any(mask):
                    pitch_bins[b] = float(np.max(accum[mask]))

    pitch_norm = float(np.linalg.norm(pitch_bins))
    if pitch_norm > 1e-6:
        pitch_feat = (pitch_bins / pitch_norm) * min(1.0, pitch_norm * 1.5)
    else:
        pitch_feat = np.full(24, -0.20, dtype=np.float64)

    embedding = np.concatenate([mel_unit * 0.45, cep_unit * 0.35, pitch_feat * 0.45])
    total_norm = float(np.linalg.norm(embedding))
    if total_norm > 1e-9:
        embedding /= total_norm
    return embedding
