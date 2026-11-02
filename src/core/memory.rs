// SPDX-License-Identifier: GPL-3.0-or-later
use super::rng::Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    #[serde(rename = "quando")]
    pub when: String,
    #[serde(rename = "contexto")]
    pub context: String,
    #[serde(rename = "acao")]
    pub action: String,
    #[serde(rename = "resultado")]
    pub result: String,
    #[serde(rename = "valencia")]
    pub valence: f64,
}
impl Episode {
    pub fn valid(&self) -> bool {
        !self.context.is_empty()
            && !self.action.is_empty()
            && self.context.len() < 300
            && self.action.len() < 100
            && self.valence.is_finite()
            && (-1.0..=1.0).contains(&self.valence)
    }
}

#[derive(Clone, Debug)]
pub struct OpenEpisode {
    pub id: String,
    pub when: String,
    pub context: String,
    pub action: String,
    pub start: f64,
    pub thing: Option<u64>,
    pub attention: bool,
}

pub type Stats = BTreeMap<String, [f64; 2]>;
#[derive(Default, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(rename = "tabela", default)]
    pub table: BTreeMap<String, Stats>,
    #[serde(rename = "geral", default)]
    pub totals: Stats,
}
impl Preferences {
    pub fn validate(&mut self) {
        fn valid(v: &[f64; 2]) -> bool {
            v[0].is_finite() && v[0] >= 0.0 && v[1].is_finite() && (-1.0..=1.0).contains(&v[1])
        }
        self.table
            .retain(|k, v| !k.is_empty() && k.len() < 300 && v.values().all(valid));
        self.totals.retain(|k, v| !k.is_empty() && valid(v));
    }
}

#[derive(Default)]
pub struct Memory {
    pub preferences: Preferences,
    pub recent: Vec<Episode>,
    pub open: Vec<OpenEpisode>,
    last: Option<(String, f64, String)>,
    counter: u64,
}

pub fn dismiss_valence(elapsed: f64, patience: f64) -> f64 {
    -0.8 + 0.6 * (elapsed / patience).min(1.0)
}

impl Memory {
    pub fn estimate(&self, ctx: &str, action: &str) -> f64 {
        let [n, m] = self
            .preferences
            .table
            .get(ctx)
            .and_then(|a| a.get(action))
            .copied()
            .unwrap_or([0.0, 0.0]);
        let [gn, gm] = self
            .preferences
            .totals
            .get(action)
            .copied()
            .unwrap_or([0.0, 0.0]);
        let base = (0.1 * 2.0 + gm * gn.min(3.0)) / (2.0 + gn.min(3.0));
        (n * m + 3.0 * base) / (n + 3.0)
    }
    pub fn choose(
        &self,
        ctx: &str,
        allowed: &[String],
        epsilon: f64,
        rng: &mut Rng,
    ) -> Option<String> {
        if allowed.is_empty() {
            return None;
        }
        if rng.uniform() < epsilon {
            return Some(allowed[rng.index(allowed.len())].clone());
        }
        let best = allowed
            .iter()
            .map(|a| self.estimate(ctx, a))
            .fold(f64::NEG_INFINITY, f64::max);
        let ties: Vec<_> = allowed
            .iter()
            .filter(|a| self.estimate(ctx, a) >= best - 1e-9)
            .collect();
        Some(ties[rng.index(ties.len())].clone())
    }
    pub fn start(
        &mut self,
        ctx: String,
        action: String,
        thing: Option<u64>,
        now: f64,
        wall_time: f64,
    ) -> String {
        self.counter += 1;
        let id = format!("{:.0}-{}", wall_time * 1_000_000.0, self.counter);
        self.last = Some((id.clone(), now, action.clone()));
        self.open.push(OpenEpisode {
            id: id.clone(),
            when: format!("{wall_time:.3}"),
            context: ctx,
            action,
            start: now,
            thing,
            attention: false,
        });
        id
    }
    fn learn(&mut self, ctx: &str, action: &str, v: f64) {
        let local = self
            .preferences
            .table
            .entry(ctx.into())
            .or_default()
            .entry(action.into())
            .or_default();
        local[0] = (local[0] + 1.0).min(40.0);
        local[1] += (v - local[1]) / local[0];
        let global = self.preferences.totals.entry(action.into()).or_default();
        global[0] = (global[0] + 1.0).min(40.0);
        global[1] += (v - global[1]) / global[0];
    }
    fn resolve(&mut self, index: usize, result: &str, v: f64) {
        let ep = self.open.remove(index);
        self.learn(&ep.context, &ep.action, v);
        self.recent.push(Episode {
            id: ep.id,
            when: ep.when,
            context: ep.context,
            action: ep.action,
            result: result.into(),
            valence: (v * 1000.0).round() / 1000.0,
        });
        if self.recent.len() > 500 {
            self.recent.remove(0);
        }
    }
    pub fn interaction(&mut self) {
        for ep in &mut self.open {
            ep.attention = true;
        }
    }
    pub fn tick(&mut self, now: f64, keep: f64, window: f64) {
        for i in (0..self.open.len()).rev() {
            let ep = &self.open[i];
            let age = now - ep.start;
            if ep.thing.is_some() && age >= keep {
                let v = 0.3 + if ep.attention { 0.3 } else { 0.0 };
                self.resolve(i, "ficou na tela", v);
            } else if ep.thing.is_none() && age >= window {
                let v = if ep.attention { 0.5 } else { 0.0 };
                self.resolve(i, "episódio concluído", v);
            }
        }
    }
    pub fn dismiss(&mut self, id: u64, elapsed: f64, patience: f64) {
        if let Some(i) = self.open.iter().position(|e| e.thing == Some(id)) {
            self.resolve(i, "dispensado", dismiss_valence(elapsed, patience));
        }
    }
    pub fn clear_things(&mut self) {
        for i in (0..self.open.len()).rev() {
            if self.open[i].thing.is_some() {
                self.resolve(i, "limpo pelo usuário", -0.1);
            }
        }
    }
    pub fn punish(&mut self, now: f64, window: f64) -> Option<String> {
        let (id, start, action) = self.last.take()?;
        if now - start > window || action == "nada" {
            return None;
        }
        if let Some(i) = self.open.iter().position(|e| e.id == id) {
            self.resolve(i, "castigo logo depois", -0.8);
        } else if let Some(ep) = self.recent.iter_mut().find(|e| e.id == id) {
            let delta = -0.8 - ep.valence;
            ep.result = "castigo logo depois".into();
            ep.valence = -0.8;
            if let Some(s) = self
                .preferences
                .table
                .get_mut(&ep.context)
                .and_then(|m| m.get_mut(&ep.action))
            {
                s[1] += delta / s[0].max(1.0);
            }
            if let Some(s) = self.preferences.totals.get_mut(&ep.action) {
                s[1] += delta / s[0].max(1.0);
            }
        }
        Some(action)
    }
    pub fn consolidate(&mut self) {
        let mut table: BTreeMap<String, Stats> = BTreeMap::new();
        let mut totals: Stats = BTreeMap::new();
        for (i, ep) in self.recent.iter().enumerate() {
            let w = 0.5_f64.powf((self.recent.len() - 1 - i) as f64 / 150.0);
            for s in [
                table
                    .entry(ep.context.clone())
                    .or_default()
                    .entry(ep.action.clone())
                    .or_default(),
                totals.entry(ep.action.clone()).or_default(),
            ] {
                s[0] += w;
                s[1] += w * ep.valence;
            }
        }
        if self.recent.is_empty() {
            return;
        }
        for row in table.values_mut() {
            for s in row.values_mut() {
                s[1] /= s[0];
                s[0] = s[0].min(40.0);
            }
        }
        for s in totals.values_mut() {
            s[1] /= s[0];
            s[0] = s[0].min(40.0);
        }
        self.preferences = Preferences { table, totals };
    }
}
