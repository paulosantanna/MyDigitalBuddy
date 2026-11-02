// SPDX-License-Identifier: GPL-3.0-or-later
use super::unit;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conditioning {
    #[serde(rename = "V", default)]
    pub values: BTreeMap<String, f64>,
    #[serde(skip)]
    active: BTreeMap<String, bool>,
    #[serde(skip)]
    ended: Vec<(String, f64, bool)>,
}
impl Default for Conditioning {
    fn default() -> Self {
        Self {
            values: ["PRESS", "APPROACH", "PLACA"]
                .into_iter()
                .map(|k| (k.into(), 0.0))
                .collect(),
            active: BTreeMap::new(),
            ended: Vec::new(),
        }
    }
}
fn window(cue: &str) -> f64 {
    if cue == "PLACA" { 4.0 } else { 1.5 }
}
impl Conditioning {
    pub fn set_cue(&mut self, cue: &str, on: bool, now: f64) {
        if !["PRESS", "APPROACH", "PLACA"].contains(&cue) {
            return;
        }
        if on {
            self.active.entry(cue.into()).or_insert(false);
        } else if let Some(hit) = self.active.remove(cue) {
            self.ended.push((cue.into(), now, hit));
        }
    }
    pub fn outcome(&mut self, now: f64, cues: &[&str]) {
        for (cue, hit) in &mut self.active {
            if cues.contains(&cue.as_str()) {
                *hit = true;
            }
        }
        for (cue, end, hit) in &mut self.ended {
            if cues.contains(&cue.as_str()) && now - *end <= window(cue) {
                *hit = true;
            }
        }
    }
    pub fn tick(&mut self, now: f64, rate: f64) {
        let mut pending = Vec::new();
        for (cue, end, hit) in self.ended.drain(..) {
            if now - end < window(&cue) {
                pending.push((cue, end, hit));
                continue;
            }
            let v = self.values.entry(cue).or_default();
            *v = unit(*v + rate * (if hit { 1.0 } else { 0.0 } - *v));
        }
        self.ended = pending;
    }
    pub fn strength(&self, cues: &[&str]) -> f64 {
        cues.iter()
            .filter(|c| self.active.contains_key(**c))
            .map(|c| self.values.get(*c).copied().unwrap_or(0.0))
            .fold(0.0, f64::max)
    }
    pub fn validate(&mut self) {
        self.values.retain(|k, v| {
            ["PRESS", "APPROACH", "PLACA"].contains(&k.as_str())
                && v.is_finite()
                && (0.0..=1.0).contains(v)
        });
    }
}
