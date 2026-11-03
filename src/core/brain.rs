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
    /// Sparse base weights. Rows only store a target when the weight is non-zero.
    base: Vec<Vec<(usize, f64)>>,
    outgoing: Vec<Vec<(usize, f64)>>,
    plastic_masks: Vec<Vec<f64>>,
    neuron_pool: Vec<usize>,
    counts: Vec<f64>,
    bin: Vec<bool>,
    scratch_drive: Vec<f64>,
    scratch_normal: Vec<f64>,
    scratch_uniform: Vec<f64>,
    rng: Rng,
}

fn add_weight(row: &mut Vec<(usize, f64)>, target: usize, weight: f64) {
    if weight == 0.0 {
        return;
    }
    if let Some(slot) = row.iter_mut().find(|(neuron, _)| *neuron == target) {
        slot.1 += weight;
    } else {
        row.push((target, weight));
    }
}

impl Brain {
    pub fn new(seed: u64, noise_seed: u64, temperament: &BTreeMap<String, f64>) -> Self {
        let mut pools = Vec::new();
        let mut cursor = 0;
        for (group, spec) in [(0, SENSORS), (1, INTER), (2, MOTORS)] {
            for &(name, n, _, _, _) in spec {
                let n = grown(n);
                pools.push(Pool {
                    name: name.into(),
                    start: cursor,
                    n,
                    group,
                });
                cursor += n;
            }
        }
        let ns: usize = pools
            .iter()
            .filter(|pool| pool.group == 0)
            .map(|pool| pool.n)
            .sum();
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
        let mut base: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
        let mut connect = |pre: &str, post: &str, weight: f64, density: f64| {
            let a = pools.iter().find(|p| p.name == pre).expect("static pre");
            let b = pools.iter().find(|p| p.name == post).expect("static post");
            let a_start = a.start;
            let a_n = a.n;
            let b_start = b.start;
            let b_n = b.n;
            for row in base.iter_mut().skip(a_start).take(a_n) {
                for j in b_start..b_start + b_n {
                    let mask = rng.uniform() < density;
                    let strength = (0.25 * rng.normal() - 0.25_f64.powi(2) / 2.0).exp();
                    if mask {
                        add_weight(row, j, weight * strength);
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
            scratch_drive: vec![0.0; nl],
            scratch_normal: vec![0.0; nl],
            scratch_uniform: vec![0.0; ns],
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

    #[allow(clippy::needless_range_loop)]
    fn substep(&mut self, normal: &[f64], uniform: &[f64]) {
        self.scratch_drive.fill(0.0);
        for (i, spike) in self.spikes.iter().enumerate() {
            if *spike {
                for &(j, weight) in &self.outgoing[i] {
                    self.scratch_drive[j - self.ns] += self.gain[i] * weight;
                }
            }
        }
        for i in 0..self.v.len() {
            self.v[i] = self.v[i] * self.decay[i] + self.scratch_drive[i] + 0.025 * normal[i];
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
            let mut normal = std::mem::take(&mut self.scratch_normal);
            let mut uniform = std::mem::take(&mut self.scratch_uniform);
            for value in &mut normal {
                *value = self.rng.normal();
            }
            for value in &mut uniform {
                *value = self.rng.uniform();
            }
            self.substep(&normal, &uniform);
            self.scratch_normal = normal;
            self.scratch_uniform = uniform;
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
        let ns = self.ns;
        for (i, row) in self.outgoing.iter_mut().enumerate() {
            row.clear();
            row.extend(
                self.base[i]
                    .iter()
                    .copied()
                    .filter(|(target, weight)| *target >= ns && *weight != 0.0),
            );
        }
        let mut projections = Vec::new();
        for (sensor_index, sensor) in self.pools[..SENSORS.len()].iter().enumerate() {
            for (plastic_index, &(post, _)) in PLASTIC.iter().enumerate() {
                let Some(target) = self.pools.iter().find(|pool| pool.name == post) else {
                    continue;
                };
                projections.push((
                    sensor_index,
                    plastic_index,
                    sensor.start,
                    sensor.n,
                    target.start,
                    target.n,
                    self.learned[sensor_index][plastic_index],
                ));
            }
        }
        for (sensor_index, plastic_index, pre_start, pre_n, post_start, post_n, scale) in
            projections
        {
            if scale == 0.0 {
                continue;
            }
            for x in 0..pre_n {
                let pre = pre_start + x;
                for y in 0..post_n {
                    let factor = self.plastic_masks[sensor_index * PLASTIC.len() + plastic_index]
                        [x * post_n + y];
                    if factor == 0.0 {
                        continue;
                    }
                    let post_neuron = post_start + y;
                    if post_neuron < ns {
                        continue;
                    }
                    add_weight(&mut self.outgoing[pre], post_neuron, scale * factor);
                }
            }
        }
        for row in &mut self.outgoing {
            row.retain(|(_, weight)| *weight != 0.0);
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

    fn scaled_neurons() -> usize {
        SENSORS
            .iter()
            .chain(INTER)
            .chain(MOTORS)
            .map(|spec| grown(spec.1))
            .sum()
    }

    fn scaled_sensors() -> usize {
        SENSORS.iter().map(|spec| grown(spec.1)).sum()
    }

    fn pool_count(name: &str) -> usize {
        SENSORS
            .iter()
            .chain(INTER)
            .chain(MOTORS)
            .find(|spec| spec.0 == name)
            .map(|spec| spec.1)
            .unwrap()
    }

    /// Mean number of synapses if every listed projection keeps its density.
    fn expected_synapses(scale: impl Fn(usize) -> usize) -> f64 {
        let mut total = 0.0;
        for &(pre, post, _, density) in CONNECTIONS {
            total += scale(pool_count(pre)) as f64 * scale(pool_count(post)) as f64 * density;
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
                    total += scale(a.1) as f64 * scale(b.1) as f64 * 0.7;
                }
            }
        }
        total
    }

    #[test]
    fn simulation_uses_the_grown_network_and_synapses_follow() {
        let mut brain = Brain::new(7, 19, &BTreeMap::new());
        let mut again = Brain::new(7, 19, &BTreeMap::new());
        assert_eq!(brain.n, scaled_neurons());
        assert_eq!(brain.ns, scaled_sensors());
        assert!(brain.n > REFERENCE_NEURONS);
        let neuron_ratio = brain.n as f64 / REFERENCE_NEURONS as f64;
        let synapse_ratio = expected_synapses(grown) / expected_synapses(|count| count);
        assert!(
            synapse_ratio > neuron_ratio,
            "expected synapses grew by {synapse_ratio}, neurons by {neuron_ratio}"
        );
        let actual = brain.synapses() as f64;
        let expected = expected_synapses(grown);
        assert!(
            (0.55..1.45).contains(&(actual / expected)),
            "synapses {actual} vs density expectation {expected}"
        );
        let levels = [("POKE".into(), 1.0), ("WORK".into(), 0.5)].into();
        for step in 0..150 {
            brain.stimulate(&levels);
            again.stimulate(&levels);
            let noise: Vec<_> = (0..brain.n - brain.ns)
                .map(|i| (i as f64 * 0.7 + step as f64 * 0.11).sin())
                .collect();
            let uniform: Vec<_> = (0..brain.ns)
                .map(|i| ((i * 17 + step * 13) % 100) as f64 / 100.0)
                .collect();
            brain.substep(&noise, &uniform);
            again.substep(&noise, &uniform);
        }
        assert_eq!(brain.v, again.v);
        assert_eq!(brain.spikes, again.spikes);
        assert!(brain.v.iter().all(|value| value.is_finite()));
        assert!(brain.gain.iter().all(|value| value.is_finite()));
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
        assert_eq!(a.n, scaled_neurons());
        assert_eq!(a.ns, scaled_sensors());
        assert!(a.n > REFERENCE_NEURONS);
        let neuron_ratio = a.n as f64 / REFERENCE_NEURONS as f64;
        let synapse_ratio = expected_synapses(grown) / expected_synapses(|count| count);
        assert!(synapse_ratio > neuron_ratio);
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
