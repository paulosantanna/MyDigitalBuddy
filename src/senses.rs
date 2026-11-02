// SPDX-License-Identifier: GPL-3.0-or-later
use crate::core::unit;
use std::collections::BTreeMap;

pub fn detect_activity(app: &str, idle: f64, away_after: f64) -> &'static str {
    if idle >= away_after {
        return "AWAY";
    }
    let name = app.to_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    match name {
        "code" | "visual studio code" | "devenv" | "pycharm64" | "pycharm" | "xcode" | "claude"
        | "codex" => "PROGRAMMING",
        "fusion360" | "fusion" => "DESIGNING",
        "valorant" | "cs2" | "csgo" => "GAMING",
        "chrome" | "google chrome" | "chromium" | "firefox" | "msedge" | "microsoft edge"
        | "opera" | "brave" | "brave browser" | "safari" => "BROWSING",
        "discord" | "teams" | "microsoft teams" | "whatsapp" => "SOCIAL",
        "spotify" => "MUSIC",
        _ => "IDLE",
    }
}

#[derive(Default)]
pub struct ActivityTracker {
    app: Option<String>,
    activity: String,
    pub session_start: f64,
}
impl ActivityTracker {
    pub fn observe(&mut self, app: &str, activity: &str, now: f64) -> Vec<&'static str> {
        let mut events = Vec::new();
        if let Some(old) = &self.app {
            if activity != self.activity {
                events.push(if activity == "AWAY" {
                    "USER_LEFT"
                } else if self.activity == "AWAY" {
                    self.session_start = now;
                    "USER_RETURNED"
                } else {
                    "ACTIVITY_CHANGED"
                });
            }
            if app != old && activity != "AWAY" {
                events.push("APP_CHANGED");
            }
        }
        self.app = Some(app.into());
        self.activity = activity.into();
        events
    }
}

pub struct Senses {
    pub pulse: BTreeMap<String, f64>,
    levels: BTreeMap<String, f64>,
    pub typing: f64,
    drag: f64,
    caress: f64,
    pub dragging: bool,
}
impl Default for Senses {
    fn default() -> Self {
        Self {
            pulse: BTreeMap::new(),
            levels: ["WORK", "PLAY", "SOUND", "SOCIAL", "WEB"]
                .into_iter()
                .map(|k| (k.into(), 0.0))
                .collect(),
            typing: 0.0,
            drag: 0.0,
            caress: 0.0,
            dragging: false,
        }
    }
}
impl Senses {
    pub fn kick(&mut self, name: &str, amount: f64) {
        let v = self.pulse.entry(name.into()).or_default();
        *v = unit(*v + amount);
    }
    pub fn events(&mut self, events: &[&str]) {
        for e in events {
            match *e {
                "APP_CHANGED" => self.kick("MOTION", 0.75),
                "ACTIVITY_CHANGED" | "USER_RETURNED" => self.kick("NOVELTY", 1.0),
                _ => {}
            }
        }
    }
    pub fn update(
        &mut self,
        activity: &str,
        idle: Option<f64>,
        drives: BTreeMap<String, f64>,
        caressing: bool,
        mouse_moving: bool,
        dt: f64,
    ) -> BTreeMap<String, f64> {
        for (name, tau) in [
            ("MOTION", 2.0),
            ("NOVELTY", 3.0),
            ("POKE", 1.5),
            ("DISMISS", 6.0),
            ("THROW", 2.0),
            ("IMPACT", 4.0),
            ("TOY_TAKEN", 4.0),
            ("RECUSA", 1.0),
        ] {
            if let Some(v) = self.pulse.get_mut(name) {
                *v *= (-dt / tau).exp();
            }
        }
        let active = match activity {
            "PROGRAMMING" | "DESIGNING" => "WORK",
            "GAMING" => "PLAY",
            "MUSIC" => "SOUND",
            "SOCIAL" => "SOCIAL",
            "BROWSING" => "WEB",
            _ => "",
        };
        for (name, v) in &mut self.levels {
            *v += (if name == active { 1.0 } else { 0.0 } - *v) * (1.0 - (-dt / 3.0).exp());
        }
        let typing = idle.is_some_and(|s| s < 0.3) && !mouse_moving && !caressing;
        self.typing += (if typing { 1.0 } else { 0.0 } - self.typing) * (1.0 - (-dt / 2.0).exp());
        self.drag +=
            (if self.dragging { 1.0 } else { 0.0 } - self.drag) * (1.0 - (-dt / 0.7).exp());
        self.caress +=
            (if caressing { 1.0 } else { 0.0 } - self.caress) * (1.0 - (-dt / 0.8).exp());
        let mut out = self.levels.clone();
        out.extend(self.pulse.clone());
        out.insert("TYPING".into(), self.typing);
        out.insert("DRAG".into(), self.drag);
        out.insert("CARESS".into(), self.caress);
        out.insert("SILENCE".into(), unit((idle.unwrap_or(0.0) - 5.0) / 60.0));
        out.extend(drives);
        out
    }
}
