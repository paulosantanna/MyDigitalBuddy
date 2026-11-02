// SPDX-License-Identifier: GPL-3.0-or-later
use super::unit;
use crate::config::Profile;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Drives {
    pub energy: f64,
    pub stimulation: f64,
    pub contact: f64,
    #[serde(default)]
    pub saved_at: f64,
}
impl Default for Drives {
    fn default() -> Self {
        Self {
            energy: 1.0,
            stimulation: 0.6,
            contact: 0.0,
            saved_at: 0.0,
        }
    }
}

impl Drives {
    pub fn stimulate(&mut self, event: &str, scale: f64) {
        let amount = match event {
            "app" => 0.08,
            "novelty" | "drag" => 0.2,
            "poke" => 0.15,
            "throw" => 0.35,
            "mischief" => 0.25,
            "caress" => 0.05,
            _ => 0.0,
        };
        self.stimulation = unit(self.stimulation + amount * scale);
    }
    pub fn touched(&mut self, event: &str, scale: f64) {
        let amount = match event {
            "poke" => 0.15,
            "drag" | "throw" => 0.1,
            "caress" => 0.25,
            "dismiss" => 0.05,
            _ => 0.0,
        };
        self.contact = unit(self.contact - amount * scale);
    }
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        dt: f64,
        p: &Profile,
        asleep: bool,
        moving: f64,
        arousal: f64,
        away: bool,
        engaged: bool,
        confined: bool,
        visited: bool,
    ) {
        self.energy = unit(
            self.energy
                + if asleep {
                    dt / p.sleep_recover
                } else {
                    -dt * (1.0 + 1.5 * moving + arousal.max(0.0)) / p.energy_span
                },
        );
        if engaged && !asleep {
            self.stimulation += (0.6 - self.stimulation) * (dt / 20.0).min(1.0);
        } else {
            self.stimulation =
                unit(self.stimulation - dt * if confined { 3.0 } else { 1.0 } / p.stim_decay);
        }
        let lack = if away {
            0.3
        } else if confined && !visited {
            2.0
        } else {
            1.0
        };
        self.contact = unit(self.contact + dt / p.contact_span * lack);
    }
    pub fn channels(&self) -> BTreeMap<String, f64> {
        [
            ("BOREDOM", unit((0.35 - self.stimulation) / 0.35)),
            ("LONELY", unit((self.contact - 0.5) / 0.5)),
            ("FATIGUE", unit((0.5 - self.energy) / 0.5)),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect()
    }
    pub fn sleepy(&self) -> bool {
        self.energy < 0.18
    }
    pub fn rested(&self) -> bool {
        self.energy >= 0.6
    }
    pub fn restore(&mut self, candidate: Self, p: &Profile, now: f64) -> bool {
        if ![candidate.energy, candidate.stimulation, candidate.contact]
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            || !candidate.saved_at.is_finite()
            || candidate.saved_at < 0.0
        {
            return false;
        }
        let elapsed = (now - candidate.saved_at).max(0.0);
        *self = candidate;
        self.energy = unit(self.energy + elapsed / p.sleep_recover);
        self.stimulation = unit(self.stimulation - elapsed / p.stim_decay);
        self.contact = unit(self.contact + 0.3 * elapsed / p.contact_span);
        true
    }
}
