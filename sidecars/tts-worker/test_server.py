from __future__ import annotations

import base64
import io
import os
from pathlib import Path
import sys
import wave

from fastapi.testclient import TestClient
import numpy as np
import pytest
import soundfile as sf

sys.path.insert(0, str(Path(__file__).resolve().parent))
os.environ.setdefault("ALLOW_SYNTHETIC_FALLBACK", "1")

from engines import TARGET_SAMPLE_RATE, TtsEngine  # noqa: E402
from engines.cosyvoice_engine import CosyVoiceEngine  # noqa: E402
from engines.xtts_engine import XttsEngine  # noqa: E402
from server import create_app  # noqa: E402


def _write_voiced_wav(
    path: Path,
    f0_hz: float = 145.0,
    formant_hz: float = 700.0,
    duration_s: float = 1.2,
    sample_rate: int = 24_000,
) -> bytes:
    num_samples = int(duration_s * sample_rate)
    t = np.arange(num_samples, dtype=np.float64) / sample_rate
    signal = np.zeros(num_samples, dtype=np.float64)
    for k in range(1, 22):
        freq = k * f0_hz
        if freq >= sample_rate * 0.45:
            break
        formant_gain = np.exp(-0.5 * ((freq - formant_hz) / 450.0) ** 2) + 0.25 / (k**1.1)
        signal += formant_gain * np.sin(2.0 * np.pi * freq * t + 0.3 * k)
    envelope = 0.65 + 0.35 * np.sin(2.0 * np.pi * 3.5 * t)
    signal = signal * envelope
    signal = (signal / (np.max(np.abs(signal)) + 1e-9)) * 0.8
    sf.write(str(path), signal.astype(np.float32), sample_rate, subtype="PCM_16", format="WAV")
    return path.read_bytes()


def test_health_returns_expected_schema_and_types(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "1")
    monkeypatch.setenv("PREWARM_ENGINES", "cosyvoice_3,xtts_v2")

    app = create_app()
    with TestClient(app) as client:
        response = client.get("/health")

    assert response.status_code == 200
    payload = response.json()
    assert payload["status"] == "ok"
    assert payload["device"] in ("cuda", "cpu")
    assert isinstance(payload["prewarmed_engines"], list)
    assert payload["prewarmed_engines"] == ["cosyvoice_3", "xtts_v2"]
    assert isinstance(payload["loaded_engines"], list)
    assert isinstance(payload["vram_allocated_mib"], int)
    assert payload["vram_allocated_mib"] >= 0


@pytest.mark.parametrize("engine", ["cosyvoice_3", "xtts_v2"])
def test_synthesize_returns_valid_24khz_mono_riff_wav_for_both_engines(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    engine: str,
) -> None:
    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "1")
    monkeypatch.setenv("COSYVOICE_MODEL_DIR", str(tmp_path / "no-cosyvoice"))
    monkeypatch.setenv("XTTS_MODEL_DIR", str(tmp_path / "no-xtts"))

    ref_wav = tmp_path / "sample_ref.wav"
    _write_voiced_wav(ref_wav, f0_hz=140.0, formant_hz=680.0)

    app = create_app()
    with TestClient(app) as client:
        resp = client.post(
            "/synthesize",
            json={
                "profile_slug": "default_voice",
                "reference_wav_path": str(ref_wav),
                "reference_transcript_en": "Testing local voice cloning reference clip.",
                "engine": engine,
                "spoken_hanzi": "你好世界，今天天气真不错。",
                "seed": 42,
            },
        )

    assert resp.status_code == 200
    assert resp.headers["content-type"].startswith("audio/wav")
    assert resp.headers["x-engine-used"] == engine
    assert resp.headers["x-sample-rate"] == "24000"
    duration_ms = int(resp.headers["x-duration-ms"])
    assert duration_ms >= 500

    raw = resp.content
    assert raw[:4] == b"RIFF"
    assert raw[8:16] == b"WAVEfmt "

    with wave.open(io.BytesIO(raw), "rb") as wf:
        assert wf.getnchannels() == 1
        assert wf.getsampwidth() == 2
        assert wf.getframerate() == 24_000
        num_frames = wf.getnframes()
        assert num_frames >= 12_000

    pcm, sr = sf.read(io.BytesIO(raw), dtype="float32")
    assert sr == 24_000
    assert pcm.ndim == 1
    assert np.all(np.isfinite(pcm))
    peak = float(np.max(np.abs(pcm)))
    rms = float(np.sqrt(np.mean(np.square(pcm))))
    assert 0.05 <= peak <= 1.0
    assert rms >= 0.01


@pytest.mark.parametrize("engine", ["cosyvoice_3", "xtts_v2"])
def test_synthesize_is_deterministic_per_seed_and_varies_across_seed_shifts(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    engine: str,
) -> None:
    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "1")
    monkeypatch.setenv("COSYVOICE_MODEL_DIR", str(tmp_path / "no-cosyvoice"))
    monkeypatch.setenv("XTTS_MODEL_DIR", str(tmp_path / "no-xtts"))

    ref_wav = tmp_path / "ref.wav"
    _write_voiced_wav(ref_wav)

    base_req = {
        "profile_slug": "default_voice",
        "reference_wav_path": str(ref_wav),
        "reference_transcript_en": "Hello there.",
        "engine": engine,
        "spoken_hanzi": "我们下午三点半在咖啡馆见面吧",
    }

    app = create_app()
    with TestClient(app) as client:
        r_seed1_a = client.post("/synthesize", json={**base_req, "seed": 1})
        r_seed1_b = client.post("/synthesize", json={**base_req, "seed": 1})
        r_seed2 = client.post("/synthesize", json={**base_req, "seed": 2})
        r_short = client.post("/synthesize", json={**base_req, "spoken_hanzi": "你好", "seed": 1})

    assert r_seed1_a.status_code == 200
    assert r_seed1_b.status_code == 200
    assert r_seed2.status_code == 200
    assert r_short.status_code == 200

    assert r_seed1_a.content == r_seed1_b.content
    assert r_seed1_a.content != r_seed2.content

    pcm_1, _ = sf.read(io.BytesIO(r_seed1_a.content), dtype="float32")
    pcm_2, _ = sf.read(io.BytesIO(r_seed2.content), dtype="float32")
    pcm_short, _ = sf.read(io.BytesIO(r_short.content), dtype="float32")
    assert pcm_1.shape == pcm_2.shape
    assert not np.allclose(pcm_1, pcm_2)
    assert pcm_1.shape[0] > pcm_short.shape[0]


@pytest.mark.parametrize(
    "bad_payload",
    [
        {
            "profile_slug": "default_voice",
            "reference_wav_path": "/tmp/ref.wav",
            "reference_transcript_en": "Hi",
            "engine": "cosyvoice_3",
            "spoken_hanzi": "今天有3个会议",
            "seed": 0,
        },
        {
            "profile_slug": "default_voice",
            "reference_wav_path": "/tmp/ref.wav",
            "reference_transcript_en": "Hi",
            "engine": "xtts_v2",
            "spoken_hanzi": "2026年十一月",
            "seed": 0,
        },
        {
            "profile_slug": "default_voice",
            "reference_wav_path": "/tmp/ref.wav",
            "reference_transcript_en": "Hi",
            "engine": "unknown_engine",
            "spoken_hanzi": "你好世界",
            "seed": 0,
        },
        {
            "profile_slug": "   ",
            "reference_wav_path": "/tmp/ref.wav",
            "reference_transcript_en": "Hi",
            "engine": "cosyvoice_3",
            "spoken_hanzi": "你好世界",
            "seed": 0,
        },
        {
            "profile_slug": "default_voice",
            "reference_wav_path": "/tmp/ref.wav",
            "reference_transcript_en": "Hi",
            "engine": "cosyvoice_3",
            "spoken_hanzi": "   ",
            "seed": 0,
        },
    ],
)
def test_synthesize_rejects_digits_empty_fields_and_unknown_engines_with_422(
    monkeypatch: pytest.MonkeyPatch,
    bad_payload: dict[str, object],
) -> None:
    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "1")
    app = create_app()
    with TestClient(app) as client:
        resp = client.post("/synthesize", json=bad_payload)
    assert resp.status_code == 422


@pytest.mark.parametrize("engine", ["cosyvoice_3", "xtts_v2"])
def test_synthesize_returns_503_when_synthetic_fallback_disabled_and_weights_absent(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    engine: str,
) -> None:
    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "0")
    monkeypatch.setenv("COSYVOICE_MODEL_DIR", str(tmp_path / "absent-cosyvoice"))
    monkeypatch.setenv("XTTS_MODEL_DIR", str(tmp_path / "absent-xtts"))

    ref_wav = tmp_path / "ref.wav"
    _write_voiced_wav(ref_wav)

    app = create_app()
    with TestClient(app) as client:
        resp = client.post(
            "/synthesize",
            json={
                "profile_slug": "default_voice",
                "reference_wav_path": str(ref_wav),
                "reference_transcript_en": "Reference clip.",
                "engine": engine,
                "spoken_hanzi": "你好世界",
                "seed": 0,
            },
        )
    assert resp.status_code == 503


def test_synthesize_returns_404_when_reference_wav_missing_on_ready_engine(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "0")

    class _ReadyStubEngine:
        engine_id = "cosyvoice_3"

        def is_loaded(self) -> bool:
            return True

        def prewarm(self) -> bool:
            return True

        def can_serve(self) -> bool:
            return True

        def synthesize(
            self,
            spoken_hanzi: str,
            reference_wav_path: str,
            reference_transcript_en: str,
            seed: int,
        ) -> np.ndarray:
            return np.zeros(24_000, dtype=np.float32)

    app = create_app(custom_engines={"cosyvoice_3": _ReadyStubEngine()})
    with TestClient(app) as client:
        resp = client.post(
            "/synthesize",
            json={
                "profile_slug": "default_voice",
                "reference_wav_path": str(tmp_path / "missing_ref.wav"),
                "reference_transcript_en": "Missing reference.",
                "engine": "cosyvoice_3",
                "spoken_hanzi": "你好世界",
                "seed": 0,
            },
        )
    assert resp.status_code == 404


def test_speaker_similarity_scores_identical_voice_high_and_noise_or_shifted_pitch_low(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "1")
    monkeypatch.setenv("SPEAKER_MODEL_DIR", str(tmp_path / "no-speaker-weights"))

    ref_wav_path = tmp_path / "speaker_a.wav"
    ref_bytes = _write_voiced_wav(ref_wav_path, f0_hz=135.0, formant_hz=620.0, duration_s=1.5)

    # Different pitch & formant speaker (high pitch 315 Hz, high formant 2350 Hz)
    diff_pitch_path = tmp_path / "speaker_b_high_pitch.wav"
    diff_pitch_bytes = _write_voiced_wav(
        diff_pitch_path,
        f0_hz=315.0,
        formant_hz=2350.0,
        duration_s=1.5,
    )

    # Broadband white noise WAV
    rng = np.random.default_rng(999)
    noise_pcm = (rng.standard_normal(int(1.5 * 24_000)) * 0.25).astype(np.float32)
    noise_buf = io.BytesIO()
    sf.write(noise_buf, noise_pcm, 24_000, subtype="PCM_16", format="WAV")
    noise_bytes = noise_buf.getvalue()

    app = create_app()
    with TestClient(app) as client:
        r_identical = client.post(
            "/speaker-similarity",
            json={
                "reference_wav_path": str(ref_wav_path),
                "candidate_wav_base64": base64.b64encode(ref_bytes).decode("ascii"),
            },
        )
        r_diff_pitch = client.post(
            "/speaker-similarity",
            json={
                "reference_wav_path": str(ref_wav_path),
                "candidate_wav_base64": base64.b64encode(diff_pitch_bytes).decode("ascii"),
            },
        )
        r_noise = client.post(
            "/speaker-similarity",
            json={
                "reference_wav_path": str(ref_wav_path),
                "candidate_wav_base64": base64.b64encode(noise_bytes).decode("ascii"),
            },
        )
        # Also verify synthesized clone from reference WAV preserves high timbre similarity
        synth_resp = client.post(
            "/synthesize",
            json={
                "profile_slug": "default_voice",
                "reference_wav_path": str(ref_wav_path),
                "reference_transcript_en": "Reference speaker voice.",
                "engine": "cosyvoice_3",
                "spoken_hanzi": "你好，这是克隆语音相似度测试。",
                "seed": 7,
            },
        )
        r_cloned = client.post(
            "/speaker-similarity",
            json={
                "reference_wav_path": str(ref_wav_path),
                "candidate_wav_base64": base64.b64encode(synth_resp.content).decode("ascii"),
            },
        )

    assert r_identical.status_code == 200
    assert r_diff_pitch.status_code == 200
    assert r_noise.status_code == 200
    assert r_cloned.status_code == 200

    id_body = r_identical.json()
    pitch_body = r_diff_pitch.json()
    noise_body = r_noise.json()
    cloned_body = r_cloned.json()

    for body in (id_body, pitch_body, noise_body, cloned_body):
        assert isinstance(body["similarity_bps"], int)
        assert 0 <= body["similarity_bps"] <= 10_000
        assert isinstance(body["backend"], str) and len(body["backend"]) > 0

    assert 9_500 <= id_body["similarity_bps"] <= 10_000
    assert pitch_body["similarity_bps"] <= 6_500
    assert noise_body["similarity_bps"] <= 5_500
    assert id_body["similarity_bps"] - pitch_body["similarity_bps"] >= 3_000
    assert id_body["similarity_bps"] - noise_body["similarity_bps"] >= 4_000
    assert cloned_body["similarity_bps"] >= 7_800


def test_speaker_similarity_rejects_missing_ref_with_404_and_corrupt_base64_with_422(
    tmp_path: Path,
) -> None:
    ref_wav_path = tmp_path / "valid_ref.wav"
    ref_bytes = _write_voiced_wav(ref_wav_path)

    app = create_app()
    with TestClient(app) as client:
        r_missing = client.post(
            "/speaker-similarity",
            json={
                "reference_wav_path": str(tmp_path / "does_not_exist.wav"),
                "candidate_wav_base64": base64.b64encode(ref_bytes).decode("ascii"),
            },
        )
        r_bad_b64 = client.post(
            "/speaker-similarity",
            json={
                "reference_wav_path": str(ref_wav_path),
                "candidate_wav_base64": "not-valid-base64!!!",
            },
        )
        r_bad_wav = client.post(
            "/speaker-similarity",
            json={
                "reference_wav_path": str(ref_wav_path),
                "candidate_wav_base64": base64.b64encode(b"not a wav file").decode("ascii"),
            },
        )

    assert r_missing.status_code == 404
    assert r_bad_b64.status_code == 422
    assert r_bad_wav.status_code == 422


def test_engines_satisfy_protocol_and_speaker_similarity_handles_stereo_48k_and_silence(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "1")
    cosy = CosyVoiceEngine(model_dir=tmp_path / "no-cosy")
    xtts = XttsEngine(model_dir=tmp_path / "no-xtts")
    assert isinstance(cosy, TtsEngine)
    assert isinstance(xtts, TtsEngine)

    ref_24k_path = tmp_path / "ref_24k.wav"
    _write_voiced_wav(ref_24k_path, f0_hz=150.0, formant_hz=720.0, sample_rate=24_000)

    pcm = cosy.synthesize("测试语音合成协议", str(ref_24k_path), "Protocol check.", seed=11)
    assert pcm.dtype == np.float32
    assert pcm.ndim == 1
    assert pcm.size >= TARGET_SAMPLE_RATE // 2
    assert float(np.max(np.abs(pcm))) <= 1.0

    # Same speaker rendered at 48 kHz stereo should still match >= 9500 bps
    ref_48k_mono_path = tmp_path / "ref_48k_mono.wav"
    _write_voiced_wav(ref_48k_mono_path, f0_hz=150.0, formant_hz=720.0, sample_rate=48_000)
    mono_48k, _ = sf.read(str(ref_48k_mono_path), dtype="float32")
    stereo_48k = np.stack([mono_48k, mono_48k * 0.9], axis=1)
    stereo_buf = io.BytesIO()
    sf.write(stereo_buf, stereo_48k, 48_000, subtype="PCM_16", format="WAV")

    # Silent candidate WAV must satisfy 0 <= similarity_bps <= 10000 without NaN
    silent_buf = io.BytesIO()
    sf.write(silent_buf, np.zeros(24_000, dtype=np.float32), 24_000, subtype="PCM_16", format="WAV")

    app = create_app()
    with TestClient(app) as client:
        r_stereo = client.post(
            "/speaker-similarity",
            json={
                "reference_wav_path": str(ref_24k_path),
                "candidate_wav_base64": base64.b64encode(stereo_buf.getvalue()).decode("ascii"),
            },
        )
        r_silent = client.post(
            "/speaker-similarity",
            json={
                "reference_wav_path": str(ref_24k_path),
                "candidate_wav_base64": base64.b64encode(silent_buf.getvalue()).decode("ascii"),
            },
        )

    assert r_stereo.status_code == 200
    assert r_silent.status_code == 200
    stereo_bps = r_stereo.json()["similarity_bps"]
    silent_bps = r_silent.json()["similarity_bps"]
    assert 9_500 <= stereo_bps <= 10_000
    assert 0 <= silent_bps <= 10_000
    assert silent_bps == 0


@pytest.mark.parametrize(
    ("target_language", "engine", "spoken_text"),
    [
        ("es", "xtts_v2", "Hola equipo, nos vemos en cinco minutos."),
        ("fr", "xtts_v2", "Bonjour l'équipe, nous avons une réunion cet après-midi."),
        ("ja", "cosyvoice_3", "今日の午後三時半に会議があります。"),
        ("ja", "xtts_v2", "おはようございます、よろしくお願いします。"),
    ],
)
def test_synthesize_supports_multilingual_spoken_text_and_exposes_languages(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    target_language: str,
    engine: str,
    spoken_text: str,
) -> None:
    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "1")
    monkeypatch.setenv("COSYVOICE_MODEL_DIR", str(tmp_path / "no-cosy"))
    monkeypatch.setenv("XTTS_MODEL_DIR", str(tmp_path / "no-xtts"))

    ref_wav = tmp_path / "polyglot_ref.wav"
    _write_voiced_wav(ref_wav, f0_hz=142.0, formant_hz=690.0)

    app = create_app()
    with TestClient(app) as client:
        health = client.get("/health").json()
        assert "supported_languages" in health
        assert len(health["supported_languages"]) == 12
        assert target_language in health["supported_languages"]

        langs_resp = client.get("/languages")
        assert langs_resp.status_code == 200
        langs_body = langs_resp.json()
        assert len(langs_body["languages"]) == 12

        resp = client.post(
            "/synthesize",
            json={
                "profile_slug": "default_voice",
                "reference_wav_path": str(ref_wav),
                "reference_transcript_en": "Reference speaker clip.",
                "engine": engine,
                "target_language": target_language,
                "spoken_text": spoken_text,
                "seed": 19,
            },
        )

    assert resp.status_code == 200
    assert resp.headers["x-engine-used"] == engine
    assert resp.headers["x-target-language"] == target_language
    pcm, sr = sf.read(io.BytesIO(resp.content), dtype="float32")
    assert sr == 24_000
    assert pcm.ndim == 1
    assert pcm.size >= 12_000


@pytest.mark.parametrize(
    "invalid_multilingual_payload",
    [
        # Unsupported ISO language code -> 422
        {
            "profile_slug": "default_voice",
            "reference_wav_path": "/tmp/ref.wav",
            "reference_transcript_en": "Hi",
            "engine": "xtts_v2",
            "target_language": "tlh",
            "spoken_text": "nuqneH",
            "seed": 0,
        },
        # CosyVoice 3 forced on Spanish (even when spoken_hanzi alias is also sent) -> 422
        {
            "profile_slug": "default_voice",
            "reference_wav_path": "/tmp/ref.wav",
            "reference_transcript_en": "Hi",
            "engine": "cosyvoice_3",
            "target_language": "es",
            "spoken_text": "Hola equipo, nos vemos pronto.",
            "spoken_hanzi": "Hola equipo, nos vemos pronto.",
            "seed": 0,
        },
        # Raw ASCII digits in spoken_text -> 422
        {
            "profile_slug": "default_voice",
            "reference_wav_path": "/tmp/ref.wav",
            "reference_transcript_en": "Hi",
            "engine": "xtts_v2",
            "target_language": "es",
            "spoken_text": "Nos vemos en 5 minutos.",
            "seed": 0,
        },
        # Neither spoken_text nor spoken_hanzi provided -> 422
        {
            "profile_slug": "default_voice",
            "reference_wav_path": "/tmp/ref.wav",
            "reference_transcript_en": "Hi",
            "engine": "xtts_v2",
            "target_language": "fr",
            "seed": 0,
        },
    ],
)
def test_synthesize_rejects_unsupported_language_or_engine_pair_with_422(
    monkeypatch: pytest.MonkeyPatch,
    invalid_multilingual_payload: dict[str, object],
) -> None:
    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "1")
    app = create_app()
    with TestClient(app) as client:
        resp = client.post("/synthesize", json=invalid_multilingual_payload)
    assert resp.status_code == 422


def test_xtts_engine_passes_mapped_xtts_language_code_to_inference_and_cosyvoice_guards_language(
    tmp_path: Path,
) -> None:
    from engines import UnsupportedLanguageError

    ref_wav = tmp_path / "xtts_ref.wav"
    _write_voiced_wav(ref_wav)

    captured_calls: list[tuple[str, str]] = []

    class _FakeXttsModel:
        def get_conditioning_latents(self, audio_path: list[str]) -> tuple[str, str]:
            assert len(audio_path) == 1
            return ("gpt_latent", "spk_emb")

        def inference(
            self,
            text: str,
            language: str,
            gpt_cond_latent: object,
            speaker_embedding: object,
        ) -> dict[str, np.ndarray]:
            assert gpt_cond_latent == "gpt_latent"
            assert speaker_embedding == "spk_emb"
            captured_calls.append((text, language))
            return {"wav": np.ones(24_000, dtype=np.float32) * 0.25}

    xtts = XttsEngine(model_dir=tmp_path / "no-xtts")
    xtts._model = _FakeXttsModel()

    xtts.synthesize("Hola equipo", str(ref_wav), "Hi team", seed=1, target_language="es")
    xtts.synthesize("今天开会", str(ref_wav), "Meeting today", seed=1, target_language="zh")
    assert captured_calls == [("Hola equipo", "es"), ("今天开会", "zh-cn")]

    cosy = CosyVoiceEngine(model_dir=tmp_path / "no-cosy")
    with pytest.raises(UnsupportedLanguageError):
        cosy.synthesize("Hola equipo", str(ref_wav), "Hi team", seed=1, target_language="es")


def test_synthesis_tuning_defaults_and_fallback_speed_scaling(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    from engines import (
        DEFAULT_SYNTHESIS_TUNING,
        SynthesisTuningParams,
        synthesize_deterministic_fallback,
    )

    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "1")
    assert DEFAULT_SYNTHESIS_TUNING == SynthesisTuningParams(
        temperature=0.62,
        top_p=0.80,
        top_k=50,
        repetition_penalty=5.0,
        length_penalty=1.0,
        speed=1.0,
        gpt_cond_len_secs=30,
        gpt_cond_chunk_len_secs=4,
        max_ref_len_secs=60,
        sound_norm_refs=True,
    )

    ref_wav = tmp_path / "speed_ref.wav"
    _write_voiced_wav(ref_wav)

    pcm_none = synthesize_deterministic_fallback(
        "xtts_v2", "今天下午我们一起去公园散步吧", str(ref_wav), "Walk in park", seed=5
    )
    pcm_default = synthesize_deterministic_fallback(
        "xtts_v2",
        "今天下午我们一起去公园散步吧",
        str(ref_wav),
        "Walk in park",
        seed=5,
        tuning=DEFAULT_SYNTHESIS_TUNING,
    )
    pcm_fast = synthesize_deterministic_fallback(
        "xtts_v2",
        "今天下午我们一起去公园散步吧",
        str(ref_wav),
        "Walk in park",
        seed=5,
        tuning=SynthesisTuningParams(speed=1.5),
    )
    pcm_slow = synthesize_deterministic_fallback(
        "xtts_v2",
        "今天下午我们一起去公园散步吧",
        str(ref_wav),
        "Walk in park",
        seed=5,
        tuning=SynthesisTuningParams(speed=0.75),
    )

    assert np.array_equal(pcm_none, pcm_default)
    assert pcm_fast.size < pcm_default.size < pcm_slow.size


def test_xtts_multisample_conditioning_and_tuning_forwarding_via_http(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "0")

    wav_1 = tmp_path / "clip_1.wav"
    wav_2 = tmp_path / "clip_2.wav"
    wav_3 = tmp_path / "clip_3.wav"
    for idx, w in enumerate((wav_1, wav_2, wav_3), start=1):
        _write_voiced_wav(w, f0_hz=135.0 + idx * 5.0, duration_s=1.4)

    cond_calls: list[dict[str, object]] = []
    inf_calls: list[dict[str, object]] = []

    class _DetailedFakeXttsModel:
        def get_conditioning_latents(
            self,
            audio_path: list[str],
            max_ref_length: int = 30,
            gpt_cond_len: int = 6,
            gpt_cond_chunk_len: int = 6,
            sound_norm_refs: bool = False,
        ) -> tuple[str, str]:
            cond_calls.append(
                {
                    "audio_path": list(audio_path),
                    "max_ref_length": max_ref_length,
                    "gpt_cond_len": gpt_cond_len,
                    "gpt_cond_chunk_len": gpt_cond_chunk_len,
                    "sound_norm_refs": sound_norm_refs,
                }
            )
            return ("multi_gpt_latent", "multi_spk_emb")

        def inference(
            self,
            text: str,
            language: str,
            gpt_cond_latent: object,
            speaker_embedding: object,
            **kwargs: object,
        ) -> dict[str, np.ndarray]:
            inf_calls.append(
                {
                    "text": text,
                    "language": language,
                    "gpt_cond_latent": gpt_cond_latent,
                    "speaker_embedding": speaker_embedding,
                    **kwargs,
                }
            )
            return {"wav": np.ones(24_000, dtype=np.float32) * 0.3}

    xtts = XttsEngine(model_dir=tmp_path / "no-xtts")
    xtts._model = _DetailedFakeXttsModel()
    app = create_app(custom_engines={"xtts_v2": xtts})

    with TestClient(app) as client:
        # 1) Default tuning (high-likeness defaults)
        r_default = client.post(
            "/synthesize",
            json={
                "profile_slug": "default_voice",
                "reference_wav_path": str(wav_1),
                "reference_wav_paths": [str(wav_1), str(wav_2), str(wav_3)],
                "reference_transcript_en": "Clip one",
                "reference_transcripts": ["Clip one", "Clip two", "Clip three"],
                "engine": "xtts_v2",
                "spoken_hanzi": "你好，这是多样本语音克隆测试。",
                "seed": 12,
            },
        )
        # 2) Custom tuning override (same reference files + cond params -> hits latent cache!)
        r_custom = client.post(
            "/synthesize",
            json={
                "profile_slug": "default_voice",
                "reference_wav_path": str(wav_1),
                "reference_wav_paths": [str(wav_1), str(wav_2), str(wav_3)],
                "reference_transcript_en": "Clip one",
                "engine": "xtts_v2",
                "spoken_hanzi": "再来一句自定义参数合成。",
                "seed": 13,
                "tuning": {
                    "temperature": 0.45,
                    "top_p": 0.72,
                    "top_k": 35,
                    "repetition_penalty": 6.5,
                    "length_penalty": 1.1,
                    "speed": 1.15,
                },
            },
        )
        # 3) Missing file inside reference_wav_paths -> 404 when ALLOW_SYNTHETIC_FALLBACK=0
        r_missing_multi = client.post(
            "/synthesize",
            json={
                "profile_slug": "default_voice",
                "reference_wav_path": str(wav_1),
                "reference_wav_paths": [str(wav_1), str(tmp_path / "absent_clip.wav")],
                "reference_transcript_en": "Clip one",
                "engine": "xtts_v2",
                "spoken_hanzi": "测试缺失参考音频。",
                "seed": 14,
            },
        )

    assert r_default.status_code == 200
    assert r_default.headers["x-finetuned-used"] == "false"
    assert r_custom.status_code == 200
    assert r_custom.headers["x-finetuned-used"] == "false"
    assert r_missing_multi.status_code == 404

    # Latent computation ran once and was cached on the second call
    assert len(cond_calls) == 1
    assert cond_calls[0]["audio_path"] == [
        str(wav_1.resolve()),
        str(wav_2.resolve()),
        str(wav_3.resolve()),
    ]
    assert cond_calls[0]["gpt_cond_len"] == 30
    assert cond_calls[0]["gpt_cond_chunk_len"] == 4
    assert cond_calls[0]["max_ref_length"] == 60
    assert cond_calls[0]["sound_norm_refs"] is True

    assert len(inf_calls) == 2
    assert inf_calls[0]["temperature"] == pytest.approx(0.62)
    assert inf_calls[0]["top_p"] == pytest.approx(0.80)
    assert inf_calls[0]["top_k"] == 50
    assert inf_calls[0]["repetition_penalty"] == pytest.approx(5.0)
    assert inf_calls[0]["length_penalty"] == pytest.approx(1.0)
    assert inf_calls[0]["speed"] == pytest.approx(1.0)
    assert inf_calls[0]["enable_text_splitting"] is True

    assert inf_calls[1]["temperature"] == pytest.approx(0.45)
    assert inf_calls[1]["top_p"] == pytest.approx(0.72)
    assert inf_calls[1]["top_k"] == 35
    assert inf_calls[1]["repetition_penalty"] == pytest.approx(6.5)
    assert inf_calls[1]["length_penalty"] == pytest.approx(1.1)
    assert inf_calls[1]["speed"] == pytest.approx(1.15)


def test_cosyvoice_multisample_stitching_loudness_and_speed_support(
    tmp_path: Path,
) -> None:
    from engines import SynthesisTuningParams

    clip_a = tmp_path / "cosy_a.wav"
    clip_b = tmp_path / "cosy_b.wav"
    clip_c = tmp_path / "cosy_c_long.wav"
    _write_voiced_wav(clip_a, f0_hz=140.0, duration_s=8.0, sample_rate=24_000)
    _write_voiced_wav(clip_b, f0_hz=145.0, duration_s=7.0, sample_rate=24_000)
    _write_voiced_wav(clip_c, f0_hz=150.0, duration_s=9.0, sample_rate=24_000)

    zero_shot_calls: list[dict[str, object]] = []

    class _FakeCosyModelNoSpeedArg:
        sample_rate = 24_000

        def inference_zero_shot(
            self,
            tts_text: str,
            prompt_text: str,
            prompt_speech_16k: object,
            stream: bool = False,
        ) -> list[dict[str, np.ndarray]]:
            arr = np.asarray(prompt_speech_16k, dtype=np.float32).reshape(-1)
            zero_shot_calls.append(
                {
                    "tts_text": tts_text,
                    "prompt_text": prompt_text,
                    "prompt_samples_16k": arr.size,
                    "silence_gap_rms": float(
                        np.sqrt(np.mean(np.square(arr[8 * 16_000 : 8 * 16_000 + 2_400])))
                    ),
                }
            )
            t = np.arange(24_000, dtype=np.float32) / 24_000.0
            return [{"tts_speech": (0.4 * np.sin(2.0 * np.pi * 180.0 * t)).astype(np.float32)}]

    cosy = CosyVoiceEngine(model_dir=tmp_path / "no-cosy")
    cosy._model = _FakeCosyModelNoSpeedArg()

    pcm_normal = cosy.synthesize(
        "你好世界",
        str(clip_a),
        "First sentence.",
        seed=1,
        reference_wav_paths=[str(clip_a), str(clip_b), str(clip_c)],
        reference_transcripts=["First sentence.", "Second sentence.", "Third overflow sentence."],
        tuning=SynthesisTuningParams(speed=1.0, sound_norm_refs=True),
    )
    pcm_faster = cosy.synthesize(
        "你好世界",
        str(clip_a),
        "First sentence.",
        seed=1,
        reference_wav_paths=[str(clip_a), str(clip_b), str(clip_c)],
        reference_transcripts=["First sentence.", "Second sentence.", "Third overflow sentence."],
        tuning=SynthesisTuningParams(speed=1.5, sound_norm_refs=True),
    )

    # Clips A (8s) + 150ms silence (2400 samples) + Clip B (7s) = 15.15s = 242,400 samples <= 20s;
    # Clip C (9s) would exceed ~20s budget so only A and B are stitched & joined.
    assert len(zero_shot_calls) == 2
    assert zero_shot_calls[0]["prompt_samples_16k"] == (8 * 16_000) + 2_400 + (7 * 16_000)
    assert zero_shot_calls[0]["silence_gap_rms"] == 0.0
    assert zero_shot_calls[0]["prompt_text"] == "First sentence. Second sentence."

    # Fallback time-scale resampling shortened 1.0s (24000 samples) to ~0.67s (~16000 samples)
    assert pcm_normal.size == 24_000
    assert 15_000 <= pcm_faster.size <= 17_000


@pytest.mark.parametrize(
    "bad_tuning",
    [
        {"temperature": 0.04},
        {"temperature": 1.21},
        {"top_p": 0.05},
        {"top_p": 1.05},
        {"top_k": 0},
        {"top_k": 201},
        {"repetition_penalty": 0.9},
        {"repetition_penalty": 15.1},
        {"length_penalty": 0.4},
        {"length_penalty": 2.1},
        {"speed": 0.45},
        {"speed": 2.05},
        {"gpt_cond_len_secs": 2},
        {"gpt_cond_len_secs": 61},
        {"gpt_cond_chunk_len_secs": 1},
        {"gpt_cond_chunk_len_secs": 31},
        {"max_ref_len_secs": 5},
        {"max_ref_len_secs": 121},
    ],
)
def test_synthesize_rejects_out_of_bounds_tuning_values_with_422(
    monkeypatch: pytest.MonkeyPatch,
    bad_tuning: dict[str, object],
) -> None:
    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "1")
    app = create_app()
    with TestClient(app) as client:
        resp = client.post(
            "/synthesize",
            json={
                "profile_slug": "default_voice",
                "reference_wav_path": "/tmp/ref.wav",
                "reference_transcript_en": "Hi",
                "engine": "xtts_v2",
                "spoken_hanzi": "你好世界",
                "seed": 0,
                "tuning": bad_tuning,
            },
        )
    assert resp.status_code == 422


def test_finetune_background_lifecycle_and_synthesize_with_finetuned_checkpoint(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    import time

    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "1")
    monkeypatch.setenv("XTTS_MODEL_DIR", str(tmp_path / "no-base-xtts"))

    sample_1 = tmp_path / "enroll_1.wav"
    sample_2 = tmp_path / "enroll_2.wav"
    _write_voiced_wav(sample_1, f0_hz=138.0, duration_s=2.0)
    _write_voiced_wav(sample_2, f0_hz=142.0, duration_s=2.2)
    ft_out_dir = tmp_path / "default_voice" / "finetuned_xtts"

    app = create_app()
    with TestClient(app) as client:
        # Unknown job_id -> 404
        assert client.get("/finetune/nonexistent-job").status_code == 404

        # Validation checks -> 422
        r_bad_epochs = client.post(
            "/finetune",
            json={
                "job_id": "bad-ep",
                "profile_slug": "default_voice",
                "samples": [{"wav_path": str(sample_1), "transcript_en": "Hello world"}],
                "output_dir": str(ft_out_dir),
                "epochs": 51,
                "learning_rate": 5e-6,
            },
        )
        assert r_bad_epochs.status_code == 422

        # Enqueue valid synthetic fine-tune -> 202 queued
        r_start = client.post(
            "/finetune",
            json={
                "job_id": "ft-job-001",
                "profile_slug": "default_voice",
                "samples": [
                    {"wav_path": str(sample_1), "transcript_en": "Hello world from sample one."},
                    {"wav_path": str(sample_2), "transcript_en": "Second enrolled voice clip."},
                ],
                "output_dir": str(ft_out_dir),
                "epochs": 6,
                "learning_rate": 0.000005,
            },
        )
        assert r_start.status_code == 202
        start_body = r_start.json()
        assert start_body == {
            "job_id": "ft-job-001",
            "profile_slug": "default_voice",
            "status": "queued",
            "epochs": 6,
            "current_epoch": 0,
            "progress_bps": 0,
            "last_loss": None,
            "checkpoint_dir": None,
            "error_message": None,
        }

        # Poll GET /finetune/{job_id} until succeeded
        final_body: dict[str, object] | None = None
        deadline = time.monotonic() + 3.0
        while time.monotonic() < deadline:
            r_poll = client.get("/finetune/ft-job-001")
            assert r_poll.status_code == 200
            body = r_poll.json()
            if body["status"] in ("succeeded", "failed"):
                final_body = body
                break
            time.sleep(0.01)

        assert final_body is not None
        assert final_body["status"] == "succeeded"
        assert final_body["current_epoch"] == 6
        assert final_body["progress_bps"] == 10_000
        assert final_body["last_loss"] == pytest.approx(0.42)
        assert final_body["checkpoint_dir"] == str(ft_out_dir)
        assert final_body["error_message"] is None
        assert (ft_out_dir / "synthetic_finetuned.json").is_file()
        assert (ft_out_dir / "config.json").is_file()

        # Synthesize using the fine-tuned checkpoint directory
        r_synth = client.post(
            "/synthesize",
            json={
                "profile_slug": "default_voice",
                "reference_wav_path": str(sample_1),
                "reference_wav_paths": [str(sample_1), str(sample_2)],
                "reference_transcript_en": "Hello world from sample one.",
                "finetuned_checkpoint_dir": str(ft_out_dir),
                "engine": "xtts_v2",
                "spoken_hanzi": "使用微调模型进行高相似度合成测试。",
                "seed": 77,
            },
        )
        assert r_synth.status_code == 200
        assert r_synth.headers["x-finetuned-used"] == "true"
        assert r_synth.headers["x-engine-used"] == "xtts_v2"
        pcm, sr = sf.read(io.BytesIO(r_synth.content), dtype="float32")
        assert sr == 24_000
        assert pcm.size >= 12_000


def test_finetune_enforces_404_and_503_when_synthetic_fallback_disabled_and_slices_long_audio(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    from engines.xtts_finetuner import slice_long_clip_and_transcript

    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "0")
    monkeypatch.setenv("XTTS_MODEL_DIR", str(tmp_path / "missing-xtts-weights"))

    real_wav = tmp_path / "real_sample.wav"
    _write_voiced_wav(real_wav, duration_s=1.5)

    app = create_app()
    with TestClient(app) as client:
        r_missing_wav = client.post(
            "/finetune",
            json={
                "job_id": "ft-404",
                "profile_slug": "default_voice",
                "samples": [{"wav_path": str(tmp_path / "nope.wav"), "transcript_en": "Missing"}],
                "output_dir": str(tmp_path / "out"),
                "epochs": 4,
                "learning_rate": 5e-6,
            },
        )
        r_missing_weights = client.post(
            "/finetune",
            json={
                "job_id": "ft-503",
                "profile_slug": "default_voice",
                "samples": [{"wav_path": str(real_wav), "transcript_en": "Present clip"}],
                "output_dir": str(tmp_path / "out"),
                "epochs": 4,
                "learning_rate": 5e-6,
            },
        )

    assert r_missing_wav.status_code == 404
    assert r_missing_weights.status_code == 503

    # Verify >11.0s audio valley slicer produces <=10.5s chunks at 22050 Hz with proportional words
    sr = 22_050
    long_wav = np.ones(int(23.0 * sr), dtype=np.float32) * 0.25
    # Insert quiet valleys around 9.2s and 18.4s
    long_wav[int(9.1 * sr) : int(9.3 * sr)] = 0.0
    long_wav[int(18.3 * sr) : int(18.5 * sr)] = 0.0
    words = " ".join(f"word{i}" for i in range(1, 31))
    chunks = slice_long_clip_and_transcript(long_wav, words, sr=sr)
    assert len(chunks) == 3
    for chunk_pcm, chunk_text in chunks:
        assert 0 < chunk_pcm.size <= int(10.5 * sr)
        assert len(chunk_text.strip()) > 0
    rejoined = " ".join(text for _, text in chunks)
    assert rejoined == words


def test_xtts_finetuned_checkpoint_one_slot_lru_cache_evicts_previous_profile(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "0")

    ref_wav = tmp_path / "ref.wav"
    _write_voiced_wav(ref_wav, duration_s=1.2)

    ft_dir_a = tmp_path / "profile_a_ft"
    ft_dir_b = tmp_path / "profile_b_ft"
    for d in (ft_dir_a, ft_dir_b):
        d.mkdir(parents=True)
        (d / "config.json").write_text('{"model": "xtts"}', encoding="utf-8")
        (d / "model.pth").write_bytes(b"checkpoint-bytes")

    loaded_dirs: list[str] = []

    class _StubLoadedModel:
        def __init__(self, label: str) -> None:
            self.label = label

        def get_conditioning_latents(self, audio_path: list[str], **kwargs: object) -> tuple[str, str]:
            return (f"latent:{self.label}", f"spk:{self.label}")

        def inference(
            self,
            text: str,
            language: str,
            gpt_cond_latent: object,
            speaker_embedding: object,
            **kwargs: object,
        ) -> dict[str, np.ndarray]:
            assert gpt_cond_latent == f"latent:{self.label}"
            return {"wav": np.ones(24_000, dtype=np.float32) * 0.2}

    xtts = XttsEngine(model_dir=tmp_path / "base-xtts")

    def _fake_load_from_dir(
        checkpoint_dir: Path,
        vocab_fallback_dir: Path | None = None,
    ) -> tuple[_StubLoadedModel, int]:
        loaded_dirs.append(checkpoint_dir.name)
        return (_StubLoadedModel(checkpoint_dir.name), 24_000)

    monkeypatch.setattr(xtts, "_load_xtts_from_dir", _fake_load_from_dir)

    xtts.synthesize(
        "你好",
        str(ref_wav),
        "Hi",
        seed=1,
        finetuned_checkpoint_dir=str(ft_dir_a),
    )
    # Second call with same dir hits 1-slot cache (no reload)
    xtts.synthesize(
        "再见",
        str(ref_wav),
        "Hi",
        seed=2,
        finetuned_checkpoint_dir=str(ft_dir_a),
    )
    assert loaded_dirs == ["profile_a_ft"]
    assert len(xtts._finetuned_model_cache) == 1

    # Switching to profile_b_ft evicts profile_a_ft so cache stays bounded to 1 slot
    xtts.synthesize(
        "你好",
        str(ref_wav),
        "Hi",
        seed=3,
        finetuned_checkpoint_dir=str(ft_dir_b),
    )
    assert loaded_dirs == ["profile_a_ft", "profile_b_ft"]
    assert len(xtts._finetuned_model_cache) == 1


def test_finetune_manager_rejects_duplicate_active_profile_and_queues_distinct_profiles(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    import threading
    import time

    from engines.xtts_finetuner import XttsFineTuneManager

    monkeypatch.setenv("ALLOW_SYNTHETIC_FALLBACK", "1")
    sample_wav = tmp_path / "sample.wav"
    _write_voiced_wav(sample_wav, duration_s=1.2)

    gate = threading.Event()
    entered_first = threading.Event()
    manager = XttsFineTuneManager(base_model_dir=tmp_path / "no-base")
    orig_synthetic = manager._run_synthetic_finetune

    def _gated_synthetic(**kwargs: object) -> None:
        if kwargs.get("job_id") == "job-a1":
            entered_first.set()
            gate.wait(timeout=2.0)
        orig_synthetic(**kwargs)  # type: ignore[arg-type]

    monkeypatch.setattr(manager, "_run_synthetic_finetune", _gated_synthetic)
    app = create_app(finetune_manager=manager)

    with TestClient(app) as client:
        r_a1 = client.post(
            "/finetune",
            json={
                "job_id": "job-a1",
                "profile_slug": "profile_a",
                "samples": [{"wav_path": str(sample_wav), "transcript_en": "One"}],
                "output_dir": str(tmp_path / "out_a"),
                "epochs": 2,
                "learning_rate": 5e-6,
            },
        )
        assert r_a1.status_code == 202
        assert entered_first.wait(timeout=1.5)

        # Duplicate active profile_a with a new job_id -> 409 Conflict
        r_dup = client.post(
            "/finetune",
            json={
                "job_id": "job-a2",
                "profile_slug": "profile_a",
                "samples": [{"wav_path": str(sample_wav), "transcript_en": "Duplicate"}],
                "output_dir": str(tmp_path / "out_a"),
                "epochs": 2,
                "learning_rate": 5e-6,
            },
        )
        assert r_dup.status_code == 409

        # Distinct profile_b queues cleanly behind _gpu_lock while job-a1 holds GPU
        r_b1 = client.post(
            "/finetune",
            json={
                "job_id": "job-b1",
                "profile_slug": "profile_b",
                "samples": [{"wav_path": str(sample_wav), "transcript_en": "Two"}],
                "output_dir": str(tmp_path / "out_b"),
                "epochs": 2,
                "learning_rate": 5e-6,
            },
        )
        assert r_b1.status_code == 202
        assert client.get("/finetune/job-b1").json()["status"] == "queued"

        # Release gate -> both jobs finish sequentially and succeed
        gate.set()
        deadline = time.monotonic() + 3.0
        while time.monotonic() < deadline:
            sa = client.get("/finetune/job-a1").json()["status"]
            sb = client.get("/finetune/job-b1").json()["status"]
            if sa == "succeeded" and sb == "succeeded":
                break
            time.sleep(0.01)

        assert client.get("/finetune/job-a1").json()["status"] == "succeeded"
        assert client.get("/finetune/job-b1").json()["status"] == "succeeded"


def test_models_dir_env_resolves_windows_and_portable_subdirs_cleanly(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    from engines.xtts_finetuner import XttsFineTuneManager
    from server import SpeakerSimilarityScorer

    portable_models = tmp_path / "VoiceCloneStudio" / "data" / "models" / "tts"
    monkeypatch.setenv("MODELS_DIR", str(portable_models))
    monkeypatch.delenv("XTTS_MODEL_DIR", raising=False)
    monkeypatch.delenv("COSYVOICE_MODEL_DIR", raising=False)
    monkeypatch.delenv("SPEAKER_MODEL_DIR", raising=False)

    assert XttsEngine().model_dir == portable_models / "xtts-v2"
    assert CosyVoiceEngine().model_dir == portable_models / "cosyvoice"
    assert XttsFineTuneManager().base_model_dir == portable_models / "xtts-v2"
    assert SpeakerSimilarityScorer().model_dir == portable_models / "speaker"








