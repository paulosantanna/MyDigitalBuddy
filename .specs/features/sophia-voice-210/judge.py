"""The Judge — opera ear. Exit 0 only when the voice reads 100% human, not synthetic."""

from __future__ import annotations

import json
import sys
from pathlib import Path

from metrics import measure

# Ten human gates. All must pass for APPROVE (10/10 human).
GATES = {
    "pitch_210": lambda m: 207.0 <= m["pitch_hz"] <= 213.0,
    "no_cartoon_top": lambda m: m["pitch_p90_hz"] <= 285.0,
    "no_metallic_presence": lambda m: m["presence_db"] <= -18.0,
    "no_synthetic_hiss": lambda m: m["hiss_db"] <= -19.0,
    "speech_moves": lambda m: m["stalls"] == 0 and m["gap_ms"] <= 580.0,
    "living_wobble": lambda m: 1.0 <= m["wobble_cents"] <= 22.0,
    "living_jitter": lambda m: 1.0 <= m["jitter_cents"] <= 22.0,
    "harmonic_body": lambda m: m["periodicity"] >= 0.68,
    "no_narrow_metal_spike": lambda m: m["metal_narrow_db"] <= 8.0 or m["presence_db"] <= -20.0,
    "clean_capture": lambda m: m["format_ok"] and m["peak"] <= 0.97 and m["silence_db"] <= -80.0,
}


def main() -> None:
    metrics = measure()
    results = {name: bool(check(metrics)) for name, check in GATES.items()}
    score = sum(results.values())
    report = {
        "verdict": "APPROVE" if score == len(GATES) else "REQUEST_CHANGES",
        "human_score": f"{score}/{len(GATES)}",
        "metrics": metrics,
        "gates": results,
        "role": "Opera coach with absolute pitch — synthetic, metallic, or cartoon timbre fails.",
    }
    print(json.dumps(report, ensure_ascii=False, indent=2))
    if score != len(GATES):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
