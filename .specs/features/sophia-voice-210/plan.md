# Plan — Sophia voice 210 Hz

## Observable success

`python .specs/features/sophia-voice-210/prove.py` exits 0 and `judge.py` prints APPROVE with human_score 10/10.

## Checks frozen

| Check | Command |
| --- | --- |
| Spec shape | `validate_spec.py` on `spec.md` |
| Build | `build.py` writes `voices/sophia-amostra.wav` |
| Proof | `prove.py` |
| Opera judge | `judge.py` |

## Build steps

1. Run `build.py` (Thalita native pitch, FFmpeg hiss cut only).
2. Run `prove.py`; if a human gate fails, adjust service pitch in `build.py` only — never PSOLA.
3. Update `voices/sophia.voice.json` and docs with measured Hz.

## The-fool challenge

SEGUIR COM AJUSTE: shifting pulses to 210 Hz would re-open metallic timbre; native generation is the reversible path.
