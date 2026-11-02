// SPDX-License-Identifier: GPL-3.0-or-later
use crate::config::Profile;
use serde::{Deserialize, Serialize};

pub const VECTORS: &[(&str, f64, f64)] = &[
    ("STARTLE", -0.2, 1.0),
    ("WATCH", 0.0, 0.3),
    ("WANDER", 0.25, 0.55),
    ("CHEER", 0.9, 0.8),
    ("SETTLE", 0.6, -0.45),
    ("DROWSE", 0.0, -0.8),
    ("ANGRY", -0.9, 0.9),
    ("SAD", -0.9, -0.6),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Emotion {
    Happy,
    Content,
    Watching,
    Curious,
    Angry,
    Sad,
    Startled,
    Dramatic,
    Sleeping,
}
impl Emotion {
    pub fn name(self) -> &'static str {
        match self {
            Self::Happy => "feliz",
            Self::Content => "contente",
            Self::Watching => "observando",
            Self::Curious => "curioso",
            Self::Angry => "bravo",
            Self::Sad => "triste",
            Self::Startled => "assustado",
            Self::Dramatic => "dramatico",
            Self::Sleeping => "dormindo",
        }
    }
}
pub fn classify(v: f64, a: f64) -> Emotion {
    if v < -0.2 {
        if a > 0.15 {
            Emotion::Angry
        } else {
            Emotion::Sad
        }
    } else if v > 0.18 {
        if a > 0.3 {
            Emotion::Happy
        } else {
            Emotion::Content
        }
    } else if a > 0.22 {
        Emotion::Curious
    } else {
        Emotion::Watching
    }
}
pub struct EmotionState {
    pub valence: f64,
    pub arousal: f64,
    pub current: Emotion,
    pub since: f64,
    overlay: Option<(Emotion, f64, u8)>,
    startled: bool,
}
impl Default for EmotionState {
    fn default() -> Self {
        Self {
            valence: 0.0,
            arousal: 0.0,
            current: Emotion::Watching,
            since: 0.0,
            overlay: None,
            startled: false,
        }
    }
}
impl EmotionState {
    pub fn force(&mut self, e: Emotion, seconds: f64, now: f64, priority: u8) -> bool {
        if self
            .overlay
            .is_some_and(|(_, until, p)| now < until && priority < p)
        {
            return false;
        }
        self.overlay = Some((e, now + seconds, priority));
        true
    }
    pub fn clear(&mut self) {
        self.overlay = None;
    }
    pub fn update(&mut self, hz: impl Fn(&str) -> f64, dt: f64, p: &Profile, now: f64) -> Emotion {
        let (mut v, mut a, mut weight) = (0.0, 0.0, 2.0);
        // Decision motors contribute neutral mass in the original EmotionState.
        for &(name, tv, ta) in VECTORS {
            let w = (hz(name) - 0.5).max(0.0);
            v += w * tv;
            a += w * ta;
            weight += w;
        }
        for name in [
            "SENTAR",
            "FUGIR",
            "CARTAZ",
            "CANTO",
            "TRAVESSURA",
            "PAPEL",
            "ACEITAR",
        ] {
            weight += (hz(name) - 0.5).max(0.0);
        }
        let k = 1.0 - (-dt / p.emotion_tau).exp();
        self.valence += (v / weight - self.valence) * k;
        self.arousal += (a / weight - self.arousal) * k;
        let startle = hz("STARTLE");
        if startle >= 7.0 && !self.startled {
            self.force(Emotion::Startled, 1.2, now, 40);
            self.startled = true;
        } else if startle < 2.1 {
            self.startled = false;
        }
        let wanted = if let Some((e, until, _)) = self.overlay {
            if now < until {
                e
            } else {
                self.overlay = None;
                classify(self.valence, self.arousal)
            }
        } else {
            classify(self.valence, self.arousal)
        };
        if wanted != self.current
            && (now - self.since >= p.emotion_dwell
                || matches!(wanted, Emotion::Startled | Emotion::Dramatic))
        {
            self.current = wanted;
            self.since = now;
        }
        self.current
    }
}
