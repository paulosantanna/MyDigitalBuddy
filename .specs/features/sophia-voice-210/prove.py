"""Proof for VOICE-01..04 plus the Judge human gates. Exit 0 only when all pass."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

from metrics import measure

ROOT = Path(__file__).resolve().parents[3]


def main() -> None:
    metrics = measure()
    spec_gates = {
        "format": metrics["format_ok"],
        "pitch": 207.0 <= metrics["pitch_hz"] <= 213.0,
        "human_top": metrics["pitch_p90_hz"] <= 285.0,
        "metal": metrics["presence_db"] <= -18.0,
        "hiss": metrics["hiss_db"] <= -19.0,
        "fluency": metrics["gap_ms"] <= 580.0 and metrics["stalls"] == 0,
        "headroom": metrics["peak"] <= 0.97 and metrics["silence_db"] <= -80.0,
    }
    print(json.dumps({"metrics": metrics, "spec_gates": spec_gates}, ensure_ascii=False))
    if not all(spec_gates.values()):
        raise SystemExit(1)
    judge = subprocess.run(
        [sys.executable, str(Path(__file__).with_name("judge.py"))],
        cwd=Path(__file__).parent,
        capture_output=True,
        text=True,
    )
    if judge.returncode != 0:
        print(judge.stdout)
        print(judge.stderr, file=sys.stderr)
        raise SystemExit(1)
    print(judge.stdout)


if __name__ == "__main__":
    main()
