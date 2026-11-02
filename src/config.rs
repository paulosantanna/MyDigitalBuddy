// SPDX-License-Identifier: GPL-3.0-or-later
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    #[serde(alias = "ativo")]
    pub active: bool,
    pub pet: String,
    #[serde(alias = "travessuras")]
    pub mischief: bool,
    pub memes: bool,
    #[serde(alias = "fotos")]
    pub photos: bool,
    #[serde(alias = "bilhetes")]
    pub notes: bool,
    #[serde(alias = "cutucar")]
    pub poke: bool,
    #[serde(alias = "carinho")]
    pub caress: bool,
    #[serde(alias = "arrastar")]
    pub drag: bool,
    #[serde(alias = "pousar_janelas")]
    pub perch: bool,
    #[serde(alias = "tela_cheia")]
    pub show_fullscreen: bool,
    #[serde(alias = "rotina")]
    pub routine: bool,
    #[serde(alias = "modo_teste")]
    pub test_mode: bool,
    #[serde(alias = "mostrar_emocao")]
    pub show_emotion: bool,
    #[serde(alias = "bolinha")]
    pub ball: bool,
    #[serde(alias = "fogos")]
    pub fireworks: bool,
    pub onboarding_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            active: true,
            pet: "bryan".into(),
            mischief: false,
            memes: true,
            photos: false,
            notes: true,
            poke: true,
            caress: true,
            drag: true,
            perch: true,
            show_fullscreen: false,
            routine: true,
            test_mode: false,
            show_emotion: false,
            ball: true,
            fireworks: true,
            onboarding_done: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PetDefinition {
    #[serde(rename = "nome")]
    pub name: String,
    pub seed: u64,
    #[serde(default, rename = "temperamento")]
    pub temperament: BTreeMap<String, f64>,
    #[serde(default, rename = "brincadeira")]
    pub play: BTreeMap<String, f64>,
    #[serde(default = "angry", rename = "reacao_notebook")]
    pub notebook_reaction: String,
    #[serde(default, rename = "aparencia")]
    pub appearance: BTreeMap<String, serde_json::Value>,
}

fn angry() -> String {
    "bravo".into()
}

pub fn valid_pet_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

impl PetDefinition {
    pub fn builtin(id: &str) -> Option<Self> {
        let data = match id {
            "bryan" => include_str!("../pets/bryan.json"),
            "jorge" => include_str!("../pets/jorge.json"),
            "bia" => include_str!("../pets/bia.json"),
            "sophia" => include_str!("../pets/sophia.json"),
            _ => return None,
        };
        serde_json::from_str(data).ok()
    }

    pub fn validate(&self) -> bool {
        !self.name.trim().is_empty()
            && self.name.len() <= 100
            && self
                .temperament
                .values()
                .all(|v| v.is_finite() && (0.0..=10.0).contains(v))
            && self
                .play
                .values()
                .all(|v| v.is_finite() && (0.0..=10.0).contains(v))
            && ["bravo", "triste"].contains(&self.notebook_reaction.as_str())
    }
}

#[derive(Clone, Debug)]
pub struct Profile {
    pub emotion_tau: f64,
    pub emotion_dwell: f64,
    pub learn_rate: f64,
    pub cond_rate: f64,
    pub energy_span: f64,
    pub sleep_recover: f64,
    pub stim_decay: f64,
    pub contact_span: f64,
    pub episode_ok: f64,
    pub episode_window: f64,
    pub dismiss_patience: f64,
    pub explore: f64,
    pub grievance_up: f64,
    pub grievance_down: f64,
    pub stage_up: [f64; 4],
    pub stage_dwell: f64,
    pub sad_up: f64,
    pub sad_down: f64,
    pub sad_stage_up: [f64; 4],
    pub stage_hyst: f64,
    pub away_after: f64,
    pub roam_min: f64,
    pub roam_max: f64,
    pub max_objects: usize,
    pub righting_after: f64,
}

impl Profile {
    pub fn new(test: bool) -> Self {
        if test {
            Self {
                emotion_tau: 1.5,
                emotion_dwell: 0.6,
                learn_rate: 0.004,
                cond_rate: 0.12,
                energy_span: 300.0,
                sleep_recover: 60.0,
                stim_decay: 40.0,
                contact_span: 120.0,
                episode_ok: 45.0,
                episode_window: 15.0,
                dismiss_patience: 30.0,
                explore: 0.25,
                grievance_up: 45.0,
                grievance_down: 420.0,
                stage_up: [0.12, 0.4, 0.68, 1.0],
                stage_dwell: 20.0,
                sad_up: 30.0,
                sad_down: 300.0,
                sad_stage_up: [0.2, 0.5, 0.8, 1.0],
                stage_hyst: 0.06,
                away_after: 10.0,
                roam_min: 2.0,
                roam_max: 5.0,
                max_objects: 3,
                righting_after: 3.0,
            }
        } else {
            Self {
                emotion_tau: 5.0,
                emotion_dwell: 1.5,
                learn_rate: 0.001,
                cond_rate: 0.03,
                energy_span: 10800.0,
                sleep_recover: 1200.0,
                stim_decay: 240.0,
                contact_span: 2400.0,
                episode_ok: 300.0,
                episode_window: 30.0,
                dismiss_patience: 60.0,
                explore: 0.1,
                grievance_up: 1800.0,
                grievance_down: 3600.0,
                stage_up: [0.25, 0.5, 0.75, 1.0],
                stage_dwell: 120.0,
                sad_up: 2400.0,
                sad_down: 5400.0,
                sad_stage_up: [0.25, 0.5, 0.75, 1.0],
                stage_hyst: 0.12,
                away_after: 60.0,
                roam_min: 6.0,
                roam_max: 15.0,
                max_objects: 3,
                righting_after: 8.0,
            }
        }
    }
}
