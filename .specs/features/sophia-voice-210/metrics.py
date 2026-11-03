"""Shared measurements for Sophia voice proofs and the opera judge."""

from __future__ import annotations

import wave
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[3]
WAV = ROOT / "voices" / "sophia-amostra.wav"


def load(path: Path = WAV):
    with wave.open(str(path), "rb") as handle:
        info = (
            handle.getnchannels(),
            handle.getsampwidth(),
            handle.getframerate(),
            handle.getnframes(),
        )
        samples = np.frombuffer(handle.readframes(info[3]), np.int16).astype(np.float64) / 32768.0
    return info, samples


def envelope(samples: np.ndarray, sr: int) -> np.ndarray:
    win = max(1, int(0.02 * sr))
    return np.convolve(np.abs(samples), np.ones(win) / win, mode="same")


def voiced_f0(samples: np.ndarray, sr: int) -> np.ndarray:
    win = int(0.04 * sr)
    hop = int(0.01 * sr)
    hann = np.hanning(win)
    found = []
    for start in range(0, len(samples) - win, hop):
        frame = samples[start : start + win]
        frame = frame - frame.mean()
        if np.sqrt(np.mean(frame * frame)) < 0.03:
            continue
        frame = frame * hann
        corr = np.correlate(frame, frame, mode="full")
        corr = corr[len(corr) // 2 :]
        low = int(sr / 420)
        high = int(sr / 130)
        lag = low + int(np.argmax(corr[low:high]))
        if corr[lag] / (corr[0] + 1e-12) < 0.45:
            continue
        found.append(sr / lag)
    if len(found) < 30:
        raise RuntimeError("too few voiced frames")
    return np.array(found)


def relative_band(samples: np.ndarray, sr: int, low: int, high: int) -> float:
    env = envelope(samples, sr)
    speech = samples[env > np.percentile(env, 40)]
    size = 4096
    window = np.hanning(size)
    power = None
    count = 0
    for start in range(0, len(speech) - size, size // 2):
        frame = np.abs(np.fft.rfft(speech[start : start + size] * window)) ** 2
        power = frame if power is None else power + frame
        count += 1
    power /= count
    freqs = np.fft.rfftfreq(size, 1 / sr)
    ref = np.mean(power[(freqs >= 300) & (freqs < 800)]) + 1e-20
    mask = (freqs >= low) & (freqs < high)
    return float(10 * np.log10((np.mean(power[mask]) + 1e-20) / ref))


def max_gap_ms(samples: np.ndarray, sr: int) -> float:
    env = envelope(samples, sr)
    loud = env > 0.03
    idx = np.flatnonzero(loud)
    if len(idx) < 2:
        return 0.0
    gaps = []
    run = 0
    for index in range(int(idx[0]), int(idx[-1])):
        if not loud[index]:
            run += 1
        elif run:
            gaps.append(run)
            run = 0
    return 1000.0 * max(gaps or [0]) / sr


def frozen(samples: np.ndarray, sr: int) -> int:
    size = int(0.04 * sr)
    hop = size // 2
    worst = 0
    for start in range(0, len(samples) - size - hop, hop):
        left = samples[start : start + size]
        right = samples[start + hop : start + hop + size]
        if np.sqrt(np.mean(left * left)) < 0.04:
            continue
        if np.corrcoef(left, right)[0, 1] > 0.992:
            worst += 1
    return worst


def wobble_cents(track: np.ndarray) -> float:
    deltas = 1200.0 * np.log2((track[1:] + 1e-9) / (track[:-1] + 1e-9))
    micro = deltas[np.abs(deltas) <= 35.0]
    if len(micro) < 12:
        return 999.0
    return float(np.std(micro))


def micro_jitter_cents(track: np.ndarray) -> float:
    return wobble_cents(track)


def periodicity(samples: np.ndarray, sr: int) -> float:
    win = int(0.025 * sr)
    step = int(0.010 * sr)
    window = np.hanning(win)
    strengths = []
    hop = int(0.01 * sr)
    for start in range(0, len(samples) - win, step):
        frame = samples[start : start + win]
        frame = frame - np.mean(frame)
        if np.sqrt(np.mean(frame * frame)) < 0.03:
            continue
        frame = frame * window
        corr = np.correlate(frame, frame, mode="full")
        corr = corr[len(corr) // 2 :]
        low, high = int(sr / 420), int(sr / 130)
        lag = low + int(np.argmax(corr[low:high]))
        if corr[lag] / (corr[0] + 1e-12) < 0.45:
            continue
        local = corr[lag - 3 : lag + 4]
        peak = lag - 3 + int(np.argmax(local))
        strengths.append(corr[peak] / (corr[0] + 1e-12))
    if len(strengths) < 20:
        return 0.0
    return float(np.mean(strengths))


def metallic_narrowness(samples: np.ndarray, sr: int) -> float:
    low = relative_band(samples, sr, 2500, 3200)
    high = relative_band(samples, sr, 3200, 4500)
    return low - high


def measure(path: Path = WAV) -> dict:
    info, samples = load(path)
    sr = info[2]
    env = envelope(samples, sr)
    quiet = samples[env <= np.percentile(env, 12)]
    silence = 20.0 * np.log10(float(np.sqrt(np.mean(quiet * quiet))) + 1e-12)
    pitch_track = voiced_f0(samples, sr)
    pitch = float(np.median(pitch_track))
    top = float(np.percentile(pitch_track, 90))
    presence = relative_band(samples, sr, 2500, 4000)
    hiss = relative_band(samples, sr, 8000, 11000)
    gap = max_gap_ms(samples, sr)
    stalls = frozen(samples, sr)
    peak = float(np.max(np.abs(samples)))
    wobble = wobble_cents(pitch_track)
    jitter = micro_jitter_cents(pitch_track)
    lock = periodicity(samples, sr)
    narrow = metallic_narrowness(samples, sr)
    return {
        "pitch_hz": round(pitch, 2),
        "pitch_p90_hz": round(top, 2),
        "presence_db": round(presence, 2),
        "hiss_db": round(hiss, 2),
        "gap_ms": round(gap, 1),
        "stalls": stalls,
        "silence_db": round(float(silence), 1),
        "peak": round(peak, 3),
        "wobble_cents": round(wobble, 2),
        "jitter_cents": round(jitter, 2),
        "periodicity": round(lock, 3),
        "metal_narrow_db": round(narrow, 2),
        "duration_s": round(info[3] / sr, 2),
        "format_ok": info[:3] == (1, 2, 24000) and 5.2 <= info[3] / sr <= 6.5,
    }
