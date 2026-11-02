// SPDX-License-Identifier: GPL-3.0-or-later
use super::{rng::Rng, topology::*, unit};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

const PLASTIC: &[(&str, f64)] = &[
    ("PLAYFUL", 1.0),
    ("CALM", 1.0),
    ("FRUSTRATION", -1.0),
    ("GLOOM", -1.0),
];
pub const LEARN_MIN: f64 = -0.05;
pub const LEARN_MAX: f64 = 0.08;

#[derive(Clone, Debug, Serialize)]
pub struct Pool {
    pub name: String,
    pub start: usize,
    pub n: usize,
    pub group: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrainState {
    pub version: u32,
    pub seed: u64,
    pub saved_at: f64,
    #[serde(default = "legacy_rng")]
    pub rng_algorithm: String,
    pub efficacy: BTreeMap<String, Vec<f64>>,
    #[serde(default)]
    pub learned_version: u32,
    #[serde(default)]
    pub learned: BTreeMap<String, BTreeMap<String, f64>>,
}

fn legacy_rng() -> String {
    "numpy-pcg64".into()
}

pub struct Brain {
    pub pools: Vec<Pool>,
    pub n: usize,
    pub ns: usize,
    pub seed: u64,
    pub rate: Vec<f64>,
    pub trace: Vec<f64>,
    pub raster: VecDeque<Vec<bool>>,
    pub recording: bool,
    pub time_ms: u64,
    pub learned: Vec<[f64; 4]>,
    pub eligibility: Vec<f64>,
    decay: Vec<f64>,
    adapt_b: Vec<f64>,
    adapt_decay: Vec<f64>,
    v: Vec<f64>,
    a: Vec<f64>,
    refractory: Vec<u8>,
    spikes: Vec<bool>,
    gain: Vec<f64>,
    sensor_prob: Vec<f64>,
    s_depression: Vec<f64>,
    s_recovery: Vec<f64>,
    levels: Vec<f64>,
    base: Vec<f64>,
    outgoing: Vec<Vec<(usize, f64)>>,
    plastic_masks: Vec<Vec<f64>>,
    neuron_pool: Vec<usize>,
    counts: Vec<f64>,
    bin: Vec<bool>,
    rng: Rng,
}

impl Brain {
    pub fn new(seed: u64, noise_seed: u64, temperament: &BTreeMap<String, f64>) -> Self {
        let mut pools = Vec::new();
        let mut cursor = 0;
        for (group, spec) in [(0, SENSORS), (1, INTER), (2, MOTORS)] {
            for &(name, n, _, _, _) in spec {
                pools.push(Pool {
                    name: name.into(),
                    start: cursor,
                    n,
                    group,
                });
                cursor += n;
            }
        }
        let ns: usize = SENSORS.iter().map(|x| x.1).sum();
        let n = cursor;
        let nl = n - ns;
        let mut rng = Rng(seed);
        // The legacy consumes one heterogeneity draw for every neuron, including sensors.
        let heterogeneity: Vec<f64> = (0..n).map(|_| rng.range(0.9, 1.1)).collect();
        let mut decay = vec![0.0; nl];
        let mut adapt_b = vec![0.0; nl];
        let mut adapt_decay = vec![0.0; nl];
        let mut neuron_pool = vec![0; n];
        for (pi, p) in pools.iter().enumerate() {
            neuron_pool[p.start..p.start + p.n].fill(pi);
            if p.group == 0 {
                continue;
            }
            let spec = if p.group == 1 { INTER } else { MOTORS };
            let &(_, _, tau, b, bt) = spec.iter().find(|x| x.0 == p.name).expect("static pool");
            for i in p.start..p.start + p.n {
                decay[i - ns] = (-1.0 / (tau * heterogeneity[i])).exp();
                adapt_b[i - ns] = b;
                adapt_decay[i - ns] = (-1.0 / (bt * 1000.0)).exp();
            }
        }
        let mut base = vec![0.0; n * n];
        let mut connect = |pre: &str, post: &str, weight: f64, density: f64| {
            let a = pools.iter().find(|p| p.name == pre).expect("static pre");
            let b = pools.iter().find(|p| p.name == post).expect("static post");
            for i in a.start..a.start + a.n {
                for j in b.start..b.start + b.n {
                    let mask = rng.uniform() < density;
                    let strength = (0.25 * rng.normal() - 0.25_f64.powi(2) / 2.0).exp();
                    if mask {
                        base[i * n + j] += weight * strength;
                    }
                }
            }
        };
        for &(pre, post, w, density) in CONNECTIONS {
            let gain = if SENSORS.iter().any(|s| s.0 == pre) {
                temperament
                    .get(&format!("{pre}->{post}"))
                    .or_else(|| temperament.get(post))
                    .copied()
                    .unwrap_or(1.0)
            } else {
                1.0
            };
            connect(pre, post, w * gain, density);
        }
        let decisions = [
            "SENTAR",
            "FUGIR",
            "CARTAZ",
            "CANTO",
            "TRAVESSURA",
            "PAPEL",
            "ACEITAR",
        ];
        for a in MOTORS {
            for b in MOTORS {
                if a.0 != b.0 && decisions.contains(&a.0) == decisions.contains(&b.0) {
                    connect(a.0, b.0, if a.0 == "STARTLE" { -0.1 } else { -0.05 }, 0.7);
                }
            }
        }
        let mut plastic_rng = Rng(seed.wrapping_add(1));
        let mut plastic_masks = Vec::new();
        for c in &pools[..SENSORS.len()] {
            for &(post, _) in PLASTIC {
                let b = pools
                    .iter()
                    .find(|p| p.name == post)
                    .expect("static plastic");
                let mask = (0..c.n * b.n)
                    .map(|_| {
                        let on = plastic_rng.uniform() < 0.5;
                        let w = (0.25 * plastic_rng.normal() - 0.03125).exp();
                        if on { w } else { 0.0 }
                    })
                    .collect();
                plastic_masks.push(mask);
            }
        }
        let np = pools.len();
        let mut brain = Self {
            pools,
            n,
            ns,
            seed,
            rate: vec![0.0; np],
            trace: vec![0.0; n],
            raster: VecDeque::new(),
            recording: false,
            time_ms: 0,
            learned: vec![[0.0; 4]; SENSORS.len()],
            eligibility: vec![0.0; SENSORS.len()],
            decay,
            adapt_b,
            adapt_decay,
            v: vec![0.0; nl],
            a: vec![0.0; nl],
            refractory: vec![0; nl],
            spikes: vec![false; n],
            gain: vec![1.0; n],
            sensor_prob: vec![0.0; ns],
            s_depression: vec![0.0; ns],
            s_recovery: vec![0.0; ns],
            levels: vec![0.0; SENSORS.len()],
            base,
            outgoing: vec![Vec::new(); n],
            plastic_masks,
            neuron_pool,
            counts: vec![0.0; np],
            bin: vec![false; n],
            rng: Rng(noise_seed),
        };
        for (i, &(_, _, _, d, tau)) in SENSORS.iter().enumerate() {
            let p = &brain.pools[i];
            for k in p.start..p.start + p.n {
                brain.s_depression[k] = d;
                brain.s_recovery[k] = 0.001 / tau;
            }
        }
        brain.apply_learning();
        brain
    }

    pub fn pool(&self, name: &str) -> Option<&Pool> {
        self.pools.iter().find(|p| p.name == name)
    }
    pub fn hz(&self, name: &str) -> f64 {
        self.pools
            .iter()
            .position(|p| p.name == name)
            .map(|i| self.rate[i])
            .unwrap_or(0.0)
    }
    pub fn synapses(&self) -> usize {
        self.outgoing.iter().map(Vec::len).sum()
    }

    pub fn stimulate(&mut self, levels: &BTreeMap<String, f64>) {
        for (i, &(name, _, hz, _, _)) in SENSORS.iter().enumerate() {
            let level = unit(levels.get(name).copied().unwrap_or(0.0));
            self.levels[i] = level;
            let p = &self.pools[i];
            self.sensor_prob[p.start..p.start + p.n].fill(hz * level * 0.001);
        }
    }

    fn substep(&mut self, normal: &[f64], uniform: &[f64]) {
        let mut drive = vec![0.0; self.n - self.ns];
        for (i, spike) in self.spikes.iter().enumerate() {
            if *spike {
                for &(j, weight) in &self.outgoing[i] {
                    drive[j - self.ns] += self.gain[i] * weight;
                }
            }
        }
        for i in 0..self.v.len() {
            self.v[i] = self.v[i] * self.decay[i] + drive[i] + 0.025 * normal[i];
            if self.refractory[i] > 0 {
                self.v[i] = 0.0;
                self.refractory[i] -= 1;
            }
            self.v[i] = self.v[i].max(-0.5);
            let fired = self.v[i] >= 1.0 + self.a[i];
            self.a[i] *= self.adapt_decay[i];
            if fired {
                self.v[i] = 0.0;
                self.refractory[i] = 3;
                self.a[i] += self.adapt_b[i];
            }
            self.spikes[self.ns + i] = fired;
        }
        for (i, &u) in uniform.iter().enumerate() {
            let fired = u < self.sensor_prob[i];
            self.spikes[i] = fired;
            if fired {
                self.gain[i] *= 1.0 - self.s_depression[i];
            }
            self.gain[i] += (1.0 - self.gain[i]) * self.s_recovery[i];
        }
        for (i, &fired) in self.spikes.iter().enumerate() {
            self.trace[i] *= (-1.0_f64 / 120.0).exp();
            if fired {
                self.trace[i] = 1.0;
                self.counts[self.neuron_pool[i]] += 1.0;
                self.bin[i] = true;
            }
        }
    }

    pub fn step(&mut self, dt_ms: f64) {
        let n = if dt_ms.is_finite() {
            dt_ms.round_ties_even().clamp(1.0, 400.0) as usize
        } else {
            1
        };
        for _ in 0..n {
            let normal: Vec<_> = (0..self.n - self.ns).map(|_| self.rng.normal()).collect();
            let uniform: Vec<_> = (0..self.ns).map(|_| self.rng.uniform()).collect();
            self.substep(&normal, &uniform);
            self.time_ms += 1;
            if self.recording && self.time_ms.is_multiple_of(10) {
                self.raster.push_back(self.bin.clone());
                self.bin.fill(false);
                if self.raster.len() > 300 {
                    self.raster.pop_front();
                }
            }
        }
        let dt = n as f64 * 0.001;
        let k = 1.0 - (-dt / 0.5).exp();
        for (i, p) in self.pools.iter().enumerate() {
            self.rate[i] += k * (self.counts[i] / (dt * p.n as f64) - self.rate[i]);
        }
        self.counts.fill(0.0);
    }

    pub fn learn(&mut self, reward: f64, dt: f64, rate: f64) {
        let dt = dt.clamp(0.0, 1.0);
        let k = 1.0 - (-dt / 4.0).exp();
        for i in 0..SENSORS.len() {
            self.eligibility[i] += (self.levels[i] - self.eligibility[i]) * k;
            if reward.is_finite() && rate.is_finite() && reward != 0.0 {
                for (j, &(_, sign)) in PLASTIC.iter().enumerate() {
                    self.learned[i][j] = (self.learned[i][j]
                        + rate * reward * dt * self.eligibility[i] * sign)
                        .clamp(LEARN_MIN, LEARN_MAX);
                }
            }
        }
        if reward.is_finite() && reward != 0.0 {
            self.apply_learning();
        }
    }

    fn apply_learning(&mut self) {
        let mut w = self.base.clone();
        for (i, a) in self.pools[..SENSORS.len()].iter().enumerate() {
            for (j, &(post, _)) in PLASTIC.iter().enumerate() {
                let b = self.pool(post).expect("static plastic pool");
                for x in 0..a.n {
                    for y in 0..b.n {
                        w[(a.start + x) * self.n + b.start + y] +=
                            self.learned[i][j] * self.plastic_masks[i * 4 + j][x * b.n + y];
                    }
                }
            }
        }
        for (i, row) in self.outgoing.iter_mut().enumerate() {
            row.clear();
            for j in self.ns..self.n {
                let weight = w[i * self.n + j];
                if weight != 0.0 {
                    row.push((j, weight));
                }
            }
        }
    }

    pub fn efficacy(&self, channel: &str) -> f64 {
        self.pool(channel)
            .map(|p| self.gain[p.start..p.start + p.n].iter().sum::<f64>() / p.n as f64)
            .unwrap_or(1.0)
    }
    pub fn reset_habituation(&mut self) {
        self.gain[..self.ns].fill(1.0);
    }

    pub fn state(&self, now: f64) -> BrainState {
        BrainState {
            version: 1,
            seed: self.seed,
            saved_at: now,
            rng_algorithm: "splitmix64-boxmuller-v1".into(),
            learned_version: 2,
            efficacy: SENSORS
                .iter()
                .map(|s| {
                    let p = self.pool(s.0).expect("static sensor");
                    (s.0.into(), self.gain[p.start..p.start + p.n].to_vec())
                })
                .collect(),
            learned: SENSORS
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    (
                        s.0.into(),
                        PLASTIC
                            .iter()
                            .enumerate()
                            .map(|(j, p)| (p.0.into(), self.learned[i][j]))
                            .collect(),
                    )
                })
                .collect(),
        }
    }

    /// Validate the complete candidate before publishing any restored state.
    pub fn restore(&mut self, state: &BrainState, now: f64) -> bool {
        if state.version != 1
            || state.seed != self.seed
            || !state.saved_at.is_finite()
            || state.saved_at < 0.0
        {
            return false;
        }
        let elapsed = (now - state.saved_at).max(0.0);
        let mut learned = self.learned.clone();
        let mut gain = self.gain.clone();
        for (i, &(name, _, _, _, tau)) in SENSORS.iter().enumerate() {
            for (j, &(post, _)) in PLASTIC.iter().enumerate() {
                let w = state
                    .learned
                    .get(name)
                    .and_then(|p| p.get(post))
                    .copied()
                    .unwrap_or(0.0);
                if !w.is_finite() || !(LEARN_MIN..=LEARN_MAX).contains(&w) {
                    return false;
                }
                learned[i][j] = if name == "TYPING" && state.learned_version < 2 {
                    0.0
                } else {
                    w
                };
            }
            if let Some(saved) = state.efficacy.get(name) {
                let p = &self.pools[i];
                if saved.len() != p.n
                    || saved
                        .iter()
                        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                {
                    return false;
                }
                for (j, &v) in saved.iter().enumerate() {
                    gain[p.start + j] = unit(1.0 - (1.0 - v) * (-elapsed / tau).exp());
                }
            }
        }
        self.learned = learned;
        self.gain = gain;
        self.apply_learning();
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn neural_equations_match_executed_python_oracle() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/neural.json")).unwrap();
        let mut brain = Brain::new(7, 19, &BTreeMap::new());
        brain.base.fill(0.0);
        for row in fixture["edges"].as_array().unwrap() {
            let i = row[0].as_u64().unwrap() as usize;
            let j = row[1].as_u64().unwrap() as usize;
            brain.base[i * brain.n + j] = row[2].as_f64().unwrap();
        }
        let floats = |key: &str| -> Vec<f64> {
            fixture[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect()
        };
        brain.decay = floats("decay");
        brain.adapt_b = floats("adapt_b");
        brain.adapt_decay = floats("adapt_decay");
        brain.apply_learning();
        brain.stimulate(&serde_json::from_value(fixture["levels"].clone()).unwrap());
        for step in 0..fixture["steps"].as_u64().unwrap() as usize {
            let noise: Vec<_> = (0..brain.n - brain.ns)
                .map(|i| (i as f64 * 0.7 + step as f64 * 0.11).sin())
                .collect();
            let uniform: Vec<_> = (0..brain.ns)
                .map(|i| ((i * 17 + step * 13) % 100) as f64 / 100.0)
                .collect();
            brain.substep(&noise, &uniform);
        }
        let expected = &fixture["expected"];
        for (name, actual) in [
            ("v", &brain.v),
            ("a", &brain.a),
            ("gain", &brain.gain),
            ("trace", &brain.trace),
            ("counts", &brain.counts),
        ] {
            let values = expected[name].as_array().unwrap();
            assert_eq!(actual.len(), values.len());
            for (i, (a, b)) in actual.iter().zip(values).enumerate() {
                assert!(
                    (a - b.as_f64().unwrap()).abs() < 1e-10,
                    "{name}[{i}]: {a} != {b}"
                );
            }
        }
        for (a, b) in brain
            .refractory
            .iter()
            .zip(expected["ref"].as_array().unwrap())
        {
            assert_eq!(*a as u64, b.as_u64().unwrap());
        }
        for (a, b) in brain
            .spikes
            .iter()
            .zip(expected["spikes"].as_array().unwrap())
        {
            assert_eq!(*a, b.as_f64().unwrap() != 0.0);
        }
    }

    #[test]
    fn restore_is_atomic_for_corrupt_channel_or_wrong_individual() {
        let mut brain = Brain::new(7, 1, &BTreeMap::new());
        let before = serde_json::to_value(brain.state(10.0)).unwrap();
        let mut bad = brain.state(10.0);
        bad.efficacy.insert("MOTION".into(), vec![0.5; 6]);
        bad.efficacy.insert("RANCOR".into(), vec![0.5; 3]);
        assert!(!brain.restore(&bad, 10.0));
        assert_eq!(before, serde_json::to_value(brain.state(10.0)).unwrap());
        bad = brain.state(10.0);
        bad.seed = 1729;
        assert!(!brain.restore(&bad, 10.0));
    }

    #[test]
    fn neural_reproducibility_plastic_bounds_and_habituation() {
        let mut a = Brain::new(7, 42, &BTreeMap::new());
        let mut b = Brain::new(7, 42, &BTreeMap::new());
        assert_eq!(a.n, 470);
        assert_eq!(a.ns, 180);
        let levels = [("POKE".into(), 1.0), ("WORK".into(), 1.0)].into();
        for _ in 0..20 {
            a.stimulate(&levels);
            b.stimulate(&levels);
            a.step(100.0);
            b.step(100.0);
        }
        assert_eq!(a.v, b.v);
        assert_eq!(a.rate, b.rate);
        assert!(a.efficacy("POKE") < 1.0);
        for _ in 0..20 {
            a.learn(100.0, 1.0, 1.0);
        }
        assert!(
            a.learned
                .iter()
                .flatten()
                .all(|w| (LEARN_MIN..=LEARN_MAX).contains(w))
        );
        a.reset_habituation();
        assert_eq!(a.efficacy("POKE"), 1.0);
    }
}
