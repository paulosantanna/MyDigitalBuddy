// SPDX-License-Identifier: GPL-3.0-or-later
use std::collections::BTreeMap;
pub const REFERENCE: &[(&str, f64)] = &[
    ("WANDER", 4.0),
    ("WATCH", 6.0),
    ("CHEER", 6.0),
    ("STARTLE", 6.0),
    ("DROWSE", 2.0),
    ("ANGRY", 8.0),
    ("SAD", 6.0),
    ("SETTLE", 3.0),
];
#[derive(Default)]
pub struct MotorTone {
    pub r: BTreeMap<String, f64>,
}
impl MotorTone {
    pub fn get(&self, k: &str) -> f64 {
        self.r.get(k).copied().unwrap_or(0.0)
    }
    pub fn update(&mut self, hz: impl Fn(&str) -> f64, dt: f64) {
        let k = 1.0 - (-dt / 0.3).exp();
        for &(name, ref_hz) in REFERENCE {
            let r = self.r.entry(name.into()).or_default();
            *r += (1.0 - (-hz(name).max(0.0) / ref_hz).exp() - *r) * k;
        }
    }
    pub fn vigor(&self) -> f64 {
        (1.0 - 0.7 * self.get("DROWSE")) * (1.0 - 0.6 * self.get("SAD"))
    }
    pub fn drive(&self) -> f64 {
        (self.get("WANDER") + 0.5 * self.get("CHEER") + 0.35 * self.get("SETTLE")).min(1.0)
    }
    pub fn speed(&self) -> f64 {
        (20.0 + 260.0 * self.get("WANDER") + 140.0 * self.get("CHEER") + 60.0 * self.get("SETTLE"))
            * self.vigor()
    }
    pub fn hop_chance(&self) -> f64 {
        (0.03 + 0.6 * self.get("CHEER") + 0.3 * self.get("ANGRY")) * self.vigor()
    }
    pub fn hop_power(&self) -> f64 {
        250.0 + 550.0 * self.get("CHEER") + 200.0 * self.get("ANGRY")
    }
    pub fn startle_power(&self) -> f64 {
        300.0 + 800.0 * self.get("STARTLE")
    }
}

pub struct MotorDecoder {
    pub current: Option<String>,
    since: f64,
}
impl Default for MotorDecoder {
    fn default() -> Self {
        Self {
            current: None,
            since: f64::NEG_INFINITY,
        }
    }
}
impl MotorDecoder {
    pub fn update(&mut self, rates: &BTreeMap<String, f64>, now: f64) -> bool {
        let mut candidate = rates
            .iter()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .filter(|(_, v)| **v >= 6.0)
            .map(|(k, _)| k.clone());
        let old_hz = self
            .current
            .as_ref()
            .and_then(|k| rates.get(k))
            .copied()
            .unwrap_or(0.0);
        if candidate.is_none() && old_hz >= 3.0 {
            candidate = self.current.clone();
        }
        if rates.get("STARTLE").is_some_and(|v| *v >= 6.0) {
            candidate = Some("STARTLE".into());
        }
        if candidate == self.current {
            return false;
        }
        if candidate.as_deref() != Some("STARTLE") {
            if now - self.since < 1.2 {
                return false;
            }
            if let Some(c) = &candidate
                && self.current.is_some()
                && old_hz >= 3.0
                && rates[c] < 1.3 * old_hz
            {
                return false;
            }
        }
        self.current = candidate;
        self.since = now;
        true
    }
}
