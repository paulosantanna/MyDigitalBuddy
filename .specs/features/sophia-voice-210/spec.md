# Sophia voice at 210 Hz

## Problem Statement

The sample `voices/sophia-amostra.wav` must sound like a living human alto, not metal, not a robot, and not a cartoon. Paulo fixed the target at 210 Hz. The greeting stays the same, the speech must keep moving, and the hiss must stay down.

## Goals

- [ ] Median voiced pitch is 210 Hz within 3 Hz
- [ ] The phrase top stays out of the cartoon register
- [ ] The 3–4 kHz band is not boosted, and the band above 8 kHz sits below the vowel body
- [ ] The opera judge reports 10/10 human gates

## Out of Scope

| Feature | Reason |
| --- | --- |
| Replacing the in-app Windows voice | The sample is the preview file only |
| A new sentence | The greeting in `src/dialogue.rs` stays the spoken line |
| Pulse shifting or PSOLA to force pitch | That path sounded metallic in earlier takes |

---

## Assumptions & Open Questions

| Assumption / decision | Chosen default | Rationale | Confirmed? |
| --- | --- | --- | --- |
| Spoken line | Oi! Eu sou a Sophia. Tudo bem sim. Pode falar comigo que eu guardo. | Matches `voice_greet` for Sophia | y |
| Reach 210 Hz | Generate Thalita near +9 Hz service offset; no pulse shift | Native take keeps human timbre | y |
| Hiss cut | FFmpeg high-pass 70 Hz and shelf −7 dB at 8 kHz | Moving de-esser pumped | y |
| Metallic band | No boost between 2.5 and 4 kHz | That boost was the metallic defect | y |
| Human proof | `judge.py` ten gates before `prove.py` exits 0 | Separates spec from opera ear | y |

**Open questions:** none - all resolved or logged above.

---

## User Stories

### P1: A human Sophia sample at 210 Hz ⭐ MVP

**User Story**: As Paulo, I want the Sophia sample at 210 Hz so that it sounds like a person speaking, not synthesis.

**Why P1**: The file is the voice reference he listens to.

**Acceptance Criteria**:

1. The sample SHALL be mono, 16-bit, 24000 Hz PCM, with a median voiced fundamental from 207 Hz to 213 Hz.
2. WHEN the sample is measured against its vowel body THEN the system SHALL keep 2500–4000 Hz at or below −18 dB and 8000–11000 Hz at or below −19 dB.
3. The sample SHALL contain no loud 40 ms frame pair correlated above 0.992, and no internal pause above 580 ms.
4. IF the rendered peak would pass 0.97 THEN the system SHALL hold the peak at or below 0.94 and leave the pause floor at or below −80 dB.
5. WHEN the opera judge runs THEN every human gate in `judge.py` SHALL pass.

**Independent Test**: `python .specs/features/sophia-voice-210/prove.py` exits 0.

---

## Edge Cases

- IF pulse shifting would be needed to hit 210 Hz THEN the system SHALL lower the native Thalita pitch instead.
- WHEN a clause break is already in the generated phrase THEN the system SHALL keep that pause and SHALL not insert another.

---

## Requirement Traceability

| Requirement ID | Story | Phase | Status |
| --- | --- | --- | --- |
| VOICE-01 | P1: A human Sophia sample at 210 Hz | Execute | Verified |
| VOICE-02 | P1: A human Sophia sample at 210 Hz | Execute | Verified |
| VOICE-03 | P1: A human Sophia sample at 210 Hz | Execute | Verified |
| VOICE-04 | P1: A human Sophia sample at 210 Hz | Execute | Verified |
| VOICE-05 | P1: A human Sophia sample at 210 Hz | Execute | Verified |

**Coverage:** 5 total, 5 mapped to the proof command, 0 unmapped.

## Sources

- Session request on 2026-11-03: 210 Hz, human timbre, forbid metallic, robot, or cartoon color.
- `src/dialogue.rs` `voice_greet` for Sophia.
