# Sophia voice, human pitch

## Problem Statement

The raised sample sounded like a cartoon voice. Paulo asked for 250 Hz and allowed a lower pitch whenever a higher one would still sound like animation. The greeting stays the same, the pulses are not shifted, the speech keeps moving, and the hiss stays down.

## Goals

- [ ] Median voiced pitch is at or below 252 Hz and at or above 230 Hz
- [ ] The top of the phrase, the 90th percentile, stays at or below 320 Hz
- [ ] The 3–4 kHz band is not boosted, and the band above 8 kHz sits below the vowel body

## Out of Scope

| Feature | Reason |
| --- | --- |
| Replacing the in-app Windows voice | The sample is the preview file only |
| A new sentence | The greeting in `src/dialogue.rs` stays the spoken line |
| Publishing a GitHub review | There is no pull request |

---

## Assumptions & Open Questions

| Assumption / decision | Chosen default | Rationale | Confirmed? |
| --- | --- | --- | --- |
| Spoken line | Oi! Eu sou a Sophia. Tudo bem sim. Pode falar comigo que eu guardo. | Whisper on the original sample matches `voice_greet` for Sophia | y |
| How to reach a human pitch | Generate Thalita at +22%. The saved file measures 245 Hz, and the 90th percentile is 301 Hz. The pulses are not shifted | 250 Hz was the request. This take sits under it, and the top of the phrase stays out of the cartoon register | y |
| Hiss cut | Static shelf, −7 dB at 8 kHz, no moving de-esser | A riding de-esser was heard as pumping | y |
| Metallic band | No boost between 2.5 and 4 kHz | That boost was the metallic defect | y |

**Open questions:** none - all resolved or logged above.

---

## User Stories

### P1: A human Sophia sample ⭐ MVP

**User Story**: As Paulo, I want the Sophia sample below a cartoon pitch so that it sounds like a person.

**Why P1**: The file is the voice reference he listens to.

**Acceptance Criteria**:

1. The sample SHALL be mono, 16-bit, 24000 Hz PCM, with a median voiced fundamental from 230 Hz to 252 Hz and a 90th percentile at or below 320 Hz.
2. WHEN the sample is measured against its vowel body THEN the system SHALL keep 2500–4000 Hz at or below −18 dB and 8000–11000 Hz at or below −19 dB.
3. The sample SHALL contain no loud 40 ms frame pair correlated above 0.992, and no internal pause above 520 ms.
4. IF the rendered peak would pass 0.97 THEN the system SHALL hold the peak at or below 0.94 and leave the pause floor at or below −80 dB.

**Independent Test**: `python .specs/features/sophia-voice-380/prove.py` exits 0.

---

## Edge Cases

- IF a higher pitch would push the 90th percentile above 320 Hz THEN the system SHALL use a lower native take instead of shifting pulses.
- WHEN a clause break is already in the generated phrase THEN the system SHALL keep that pause and SHALL not insert another.

---

## Requirement Traceability

| Requirement ID | Story | Phase | Status |
| --- | --- | --- | --- |
| VOICE-01 | P1: A human Sophia sample | Execute | Verified |
| VOICE-02 | P1: A human Sophia sample | Execute | Verified |
| VOICE-03 | P1: A human Sophia sample | Execute | Verified |
| VOICE-04 | P1: A human Sophia sample | Execute | Verified |

**Coverage:** 4 total, 4 mapped to the proof command, 0 unmapped.

## Sources

- Session request on 2026-11-03: 250 Hz, and lower still if that pitch would sound like a cartoon.
- `src/dialogue.rs` `voice_greet` for Sophia.
