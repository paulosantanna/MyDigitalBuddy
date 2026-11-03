"""Generate voices/sophia-amostra.wav at native 210 Hz. No pulse shifting."""

from __future__ import annotations

import asyncio
import importlib.util
import subprocess
import wave
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[3]
DEST = ROOT / "voices" / "sophia-amostra.wav"
TEXT = "Oi! Eu sou a Sophia. Tudo bem sim. Pode falar comigo que eu guardo."
VOICE = "pt-BR-ThalitaMultilingualNeural"
RATE = "+4%"
WORK = Path(r"C:\Users\paulo\AppData\Local\Temp\sophia210-build")
WORK.mkdir(exist_ok=True)
HELPER = Path(r"C:\Users\paulo\AppData\Local\Temp\sophia_humanize.py")

spec = importlib.util.spec_from_file_location("sh", HELPER)
sh = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sh)


def load(path: Path):
    with wave.open(str(path), "rb") as handle:
        sr = handle.getframerate()
        samples = np.frombuffer(handle.readframes(handle.getnframes()), np.int16).astype(np.float64) / 32768.0
    return sr, samples


def save(path: Path, sr: int, samples: np.ndarray) -> None:
    pcm = np.clip(np.round(samples * 32767.0), -32768, 32767).astype(np.int16)
    with wave.open(str(path), "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(sr)
        handle.writeframes(pcm.tobytes())


def median_f0(samples: np.ndarray, sr: int) -> float:
    win = int(0.04 * sr)
    hop = int(0.01 * sr)
    hann = np.hanning(win)
    found = []
    for start in range(0, len(samples) - win, hop):
        frame = samples[start : start + win] - np.mean(samples[start : start + win])
        if np.sqrt(np.mean(frame * frame)) < 0.03:
            continue
        frame = frame * hann
        corr = np.correlate(frame, frame, mode="full")[len(frame) - 1 :]
        low, high = int(sr / 420), int(sr / 130)
        lag = low + int(np.argmax(corr[low:high]))
        if corr[lag] / (corr[0] + 1e-12) < 0.45:
            continue
        found.append(sr / lag)
    if len(found) < 20:
        raise RuntimeError("too few voiced frames")
    return float(np.median(found))


def finish(samples: np.ndarray, sr: int) -> np.ndarray:
    """Warm velvet body, clear formants, wide mark above 5 kHz; no 2.5–4 kHz boost."""
    samples = samples - np.mean(samples)
    samples = sh.biquad(samples, *sh.coeffs_highpass(sr, 70.0))
    samples = sh.biquad(samples, *sh.coeffs_peak(sr, 220.0, 0.85, 2.2))
    samples = sh.biquad(samples, *sh.coeffs_peak(sr, 950.0, 0.75, 1.0))
    samples = sh.biquad(samples, *sh.coeffs_peak(sr, 1850.0, 1.05, 2.0))
    samples = sh.biquad(samples, *sh.coeffs_peak(sr, 3150.0, 1.15, -2.0))
    samples = sh.biquad(samples, *sh.coeffs_peak(sr, 5200.0, 0.50, 2.4))
    samples = sh.biquad(samples, *sh.coeffs_highshelf(sr, 8000.0, -7.0, 0.7))
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


async def synthesize(pitch_hz: str, stem: str) -> Path:
    import edge_tts

    mp3 = WORK / f"{stem}.mp3"
    wav = WORK / f"{stem}.raw.wav"
    comm = edge_tts.Communicate(TEXT, VOICE, rate=RATE, volume="+10%", pitch=pitch_hz)
    await comm.save(str(mp3))
    subprocess.run(
        ["ffmpeg", "-y", "-hide_banner", "-loglevel", "error", "-i", str(mp3), "-ar", "24000", "-ac", "1", str(wav)],
        check=True,
    )
    return wav


async def pick_pitch() -> str:
    candidates = [f"{hz:+d}Hz" for hz in (9, 10, 11, 8, 12)]
    best = None
    for pitch in candidates:
        try:
            raw = await synthesize(pitch, pitch.replace("+", "p").replace("-", "m"))
        except Exception:
            continue
        sr, samples = load(raw)
        cooked = finish(samples, sr)
        measured = median_f0(cooked, sr)
        err = abs(measured - 210.0)
        if best is None or err < best[0]:
            best = (err, pitch)
        if err <= 1.2:
            return pitch
    if best is None:
        raise SystemExit("no Thalita take reached 210 Hz")
    return best[1]


async def main() -> None:
    pitch = await pick_pitch()
    raw = await synthesize(pitch, "final")
    sr, samples = load(WORK / "final.raw.wav")
    finished = finish(samples, sr)
    save(DEST, sr, finished)
    print(
        {
            "dest": str(DEST),
            "service_pitch": pitch,
            "measured_hz": round(median_f0(finished, sr), 2),
        }
    )


if __name__ == "__main__":
    asyncio.run(main())
