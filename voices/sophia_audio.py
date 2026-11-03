# SPDX-License-Identifier: GPL-3.0-or-later
"""Post-processing for Sophia neural speech (matches sophia-amostra.wav)."""

from __future__ import annotations

import numpy as np


def biquad(samples: np.ndarray, b: tuple[float, float, float], a: tuple[float, float, float]) -> np.ndarray:
    b0, b1, b2 = b
    a1, a2 = a[1], a[2]
    out = np.empty_like(samples)
    x1 = x2 = y1 = y2 = 0.0
    for i, x0 in enumerate(samples):
        y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        out[i] = y0
        x2, x1 = x1, x0
        y2, y1 = y1, y0
    return out


def coeffs_highpass(sr: int, freq: float, q: float = 0.7071) -> tuple[tuple[float, float, float], tuple[float, float, float]]:
    w0 = 2 * np.pi * freq / sr
    alpha = np.sin(w0) / (2 * q)
    cos = np.cos(w0)
    b0 = (1 + cos) / 2
    b1 = -(1 + cos)
    b2 = (1 + cos) / 2
    a0 = 1 + alpha
    a1 = -2 * cos
    a2 = 1 - alpha
    return (b0 / a0, b1 / a0, b2 / a0), (1.0, a1 / a0, a2 / a0)


def coeffs_peak(sr: int, freq: float, q: float, gain_db: float) -> tuple[tuple[float, float, float], tuple[float, float, float]]:
    a = 10 ** (gain_db / 40.0)
    w0 = 2 * np.pi * freq / sr
    alpha = np.sin(w0) / (2 * q)
    cos = np.cos(w0)
    b0 = 1 + alpha * a
    b1 = -2 * cos
    b2 = 1 - alpha * a
    a0 = 1 + alpha / a
    a1 = -2 * cos
    a2 = 1 - alpha / a
    return (b0 / a0, b1 / a0, b2 / a0), (1.0, a1 / a0, a2 / a0)


def coeffs_highshelf(
    sr: int, freq: float, gain_db: float, slope: float = 0.7
) -> tuple[tuple[float, float, float], tuple[float, float, float]]:
    a = 10 ** (gain_db / 40.0)
    w0 = 2 * np.pi * freq / sr
    alpha = np.sin(w0) / 2 * np.sqrt((a + 1 / a) * (1 / slope - 1) + 2)
    cos = np.cos(w0)
    two = 2 * np.sqrt(a) * alpha
    b0 = a * ((a + 1) + (a - 1) * cos + two)
    b1 = -2 * a * ((a - 1) + (a + 1) * cos)
    b2 = a * ((a + 1) + (a - 1) * cos - two)
    a0 = (a + 1) - (a - 1) * cos + two
    a1 = 2 * ((a - 1) - (a + 1) * cos)
    a2 = (a + 1) - (a - 1) * cos - two
    return (b0 / a0, b1 / a0, b2 / a0), (1.0, a1 / a0, a2 / a0)


def finish(samples: np.ndarray, sr: int) -> np.ndarray:
    """Warm body and clear consonants; same chain as sophia-amostra.wav."""
    samples = samples - np.mean(samples)
    samples = biquad(samples, *coeffs_highpass(sr, 70.0))
    samples = biquad(samples, *coeffs_peak(sr, 220.0, 0.85, 2.2))
    samples = biquad(samples, *coeffs_peak(sr, 950.0, 0.75, 1.0))
    samples = biquad(samples, *coeffs_peak(sr, 1850.0, 1.05, 2.0))
    samples = biquad(samples, *coeffs_peak(sr, 3150.0, 1.15, -2.0))
    samples = biquad(samples, *coeffs_peak(sr, 5200.0, 0.50, 2.4))
    samples = biquad(samples, *coeffs_highshelf(sr, 8000.0, -7.0, 0.7))
    env = np.convolve(np.abs(samples), np.ones(int(0.02 * sr)) / int(0.02 * sr), mode="same")
    speech = samples[env > np.percentile(env, 45)]
    rms = float(np.sqrt(np.mean(speech * speech))) if len(speech) else 0.0
    if rms > 0:
        samples *= 0.23 / rms
    peak = float(np.max(np.abs(samples)))
    if peak > 0.94:
        samples *= 0.94 / peak
    fade = int(0.008 * sr)
    ramp = np.linspace(0.0, 1.0, fade)
    samples[:fade] *= ramp
    samples[-fade:] *= ramp[::-1]
    return samples - np.mean(samples)
