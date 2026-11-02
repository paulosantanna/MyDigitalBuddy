// SPDX-License-Identifier: GPL-3.0-or-later
use super::unit;
use crate::config::Profile;
use serde::{Deserialize, Serialize};

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Protest {
    pub grievance: f64,
    pub revolutions: u64,
    #[serde(default)]
    pub saved_at: f64,
    #[serde(skip)]
    pub stage: u8,
    #[serde(skip)]
    pub since: f64,
    #[serde(skip)]
    pub repressions: u32,
}
impl Protest {
    pub fn update(
        &mut self,
        dt: f64,
        rate: f64,
        p: &Profile,
        caress: bool,
        now: f64,
        sad: bool,
    ) -> bool {
        let (up, down, thresholds, top) = if sad {
            (p.sad_up, p.sad_down, p.sad_stage_up, 3)
        } else {
            (p.grievance_up, p.grievance_down, p.stage_up, 4)
        };
        self.grievance = (self.grievance + dt * (rate / up - 1.0 / down)
            - if caress { 0.3 * dt } else { 0.0 })
        .clamp(0.0, 1.3);
        let old = self.stage;
        let mut new = old;
        while new < top && self.grievance >= thresholds[new as usize] {
            new += 1;
        }
        while new > 0 && self.grievance < thresholds[new as usize - 1] - p.stage_hyst {
            new -= 1;
        }
        if new > old {
            new = if now - self.since < p.stage_dwell {
                old
            } else {
                old + 1
            };
        }
        if new != old {
            self.stage = new;
            self.since = now;
            if new == 4 && !sad {
                self.revolutions += 1;
            }
            if new == 0 {
                self.repressions = 0;
            }
            return true;
        }
        false
    }
    pub fn repressed(&mut self) {
        self.grievance = (self.grievance + 0.15).min(1.3);
        self.repressions += 1;
    }
    pub fn scale(&self) -> f64 {
        let base = if self.stage == 4 {
            1.0 + 0.25 * self.revolutions.saturating_sub(1) as f64
        } else {
            1.0
        };
        (base * (1.0 + 0.15 * self.repressions as f64)).min(2.0)
    }
    pub fn restore(&mut self, s: Self, now: f64, down: f64) -> bool {
        if !s.grievance.is_finite()
            || !(0.0..=1.3).contains(&s.grievance)
            || !s.saved_at.is_finite()
            || s.saved_at < 0.0
            || s.revolutions > 1_000_000_000
        {
            return false;
        }
        self.grievance = (s.grievance - (now - s.saved_at).max(0.0) / down).max(0.0);
        self.revolutions = s.revolutions;
        self.since = -1e9;
        true
    }
}

#[derive(Default)]
pub struct Desk {
    pub seated: bool,
    pub visible: bool,
    pub position: (f64, f64),
    pub request_until: f64,
    pub pending: bool,
    pub could_sit: bool,
    pub requests: Vec<f64>,
    pub refused: bool,
}
impl Desk {
    pub fn request(&mut self, now: f64) {
        self.request_until = now + 60.0;
        self.pending = true;
        self.could_sit = false;
        self.requests.retain(|t| now - *t < 900.0);
        self.requests.push(now);
    }
    pub fn signal(&self, now: f64) -> f64 {
        unit((self.request_until - now) / 60.0)
    }
    pub fn tick(&mut self, now: f64, hz: f64, can_sit: bool) {
        let can = can_sit && self.visible;
        if self.seated {
            if !can || hz < 3.0 {
                self.seated = false;
            }
        } else if can && hz >= 6.0 {
            self.seated = true;
        }
        if self.pending {
            self.could_sit |= can;
            if self.seated {
                self.pending = false;
            } else if now >= self.request_until {
                self.pending = false;
                self.refused = self.could_sit;
            }
        }
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Corner {
    #[serde(default, rename = "vitorias")]
    pub victories: std::collections::BTreeMap<String, f64>,
    #[serde(default, rename = "rancor")]
    pub resentment: f64,
    #[serde(default, rename = "receio")]
    pub fear: f64,
    #[serde(skip)]
    pub enabled: bool,
    #[serde(skip)]
    pub inside: bool,
    #[serde(skip)]
    pub visit: f64,
    #[serde(skip)]
    pub urge: f64,
    #[serde(skip)]
    pub punishment_until: f64,
    #[serde(skip)]
    pub escape: Option<(f64, f64, f64, bool)>,
    #[serde(skip)]
    pub last_success: Option<f64>,
    #[serde(skip)]
    pub pending_landing: Option<f64>,
    #[serde(skip)]
    pub position: (f64, f64),
    #[serde(skip)]
    pub was_held: bool,
}
impl Corner {
    pub fn validate(&self) -> bool {
        [self.resentment, self.fear]
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            && self
                .victories
                .values()
                .all(|v| v.is_finite() && (0.0..=1e9).contains(v))
    }
    pub fn enter(&mut self, now: f64) {
        if self.escape.take().is_some() {
            self.fear = unit(self.fear + 0.3);
        }
        if self.last_success.is_some_and(|t| now - t < 60.0) {
            let v = self.victories.entry("fugir".into()).or_default();
            *v = (*v - 1.0).max(0.0);
            self.fear = unit(self.fear + 0.3);
            self.last_success = None;
        }
        self.inside = true;
        self.urge = 0.0;
        self.punishment_until = now + 1.0;
    }
    pub fn release(&mut self) {
        self.inside = false;
        self.urge = 0.0;
        self.pending_landing = None;
        self.punishment_until = 0.0;
    }
    pub fn tick(&mut self, dt: f64, visited: bool) {
        self.resentment =
            unit(self.resentment + if self.inside { dt / 120.0 } else { -dt / 600.0 });
        self.fear = unit(self.fear - dt / 1800.0);
        self.visit += (if visited { 1.0 } else { 0.0 } - self.visit) * dt.min(1.0);
    }
    pub fn escape_memory(&self) -> f64 {
        unit(0.2 * self.victories.get("fugir").copied().unwrap_or(0.0))
    }
}
