# SPDX-License-Identifier: GPL-3.0-or-later
"""Synthesize any Sophia line with Thalita + post-processing from sophia.voice.json."""

from __future__ import annotations

import asyncio
import json
import subprocess
import sys
import wave
from pathlib import Path

import numpy as np

from sophia_audio import finish

HERE = Path(__file__).resolve().parent
CFG = HERE / "sophia.voice.json"


def load_wav(path: Path) -> tuple[int, np.ndarray]:
    with wave.open(str(path), "rb") as handle:
        sr = handle.getframerate()
        samples = np.frombuffer(handle.readframes(handle.getnframes()), np.int16).astype(np.float64) / 32768.0
    return sr, samples


def save_wav(path: Path, sr: int, samples: np.ndarray) -> None:
    pcm = np.clip(np.round(samples * 32767.0), -32768, 32767).astype(np.int16)
    with wave.open(str(path), "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(sr)
        handle.writeframes(pcm.tobytes())


async def synthesize(text: str, destination: Path, cfg: dict) -> None:
    import edge_tts

    mp3 = destination.with_suffix(".mp3")
    raw = destination.with_suffix(".raw.wav")
    comm = edge_tts.Communicate(
        text,
        cfg["voice"],
        rate=cfg["rate"],
        volume=cfg["volume"],
        pitch=cfg["pitch"],
    )
    await comm.save(str(mp3))
    subprocess.run(
        [
            "ffmpeg",
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            str(mp3),
            "-ar",
            "24000",
            "-ac",
            "1",
            str(raw),
        ],
        check=True,
    )
    mp3.unlink(missing_ok=True)
    sr, samples = load_wav(raw)
    cooked = finish(samples, sr)
    save_wav(destination, sr, cooked)
    raw.unlink(missing_ok=True)


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: sophia-speak.py TEXT_FILE OUT.wav")
    text_path = Path(sys.argv[1])
    out_path = Path(sys.argv[2])
    text = text_path.read_text(encoding="utf-8").strip()
    if not text:
        raise SystemExit("empty text")
    cfg = json.loads(CFG.read_text(encoding="utf-8"))
    if cfg.get("engine") != "edge-tts":
        raise SystemExit("sophia.voice.json must use edge-tts")
    asyncio.run(synthesize(text, out_path, cfg))


if __name__ == "__main__":
    main()
