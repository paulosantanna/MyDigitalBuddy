// SPDX-License-Identifier: GPL-3.0-or-later
use chrono::{Datelike, Local, Timelike};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
};

pub struct Timeline {
    minute: i64,
    stamp: String,
    weekday: u32,
    minute_of_day: u32,
    seconds: f64,
    activity: BTreeMap<String, f64>,
    emotions: BTreeMap<String, f64>,
    away: f64,
    idle: f64,
    typing: f64,
    switches: u64,
    pub interactions: f64,
    session: f64,
}
impl Default for Timeline {
    fn default() -> Self {
        let now = Local::now();
        Self {
            minute: now.timestamp() / 60,
            stamp: now.format("%Y-%m-%dT%H:%M").to_string(),
            weekday: now.weekday().num_days_from_monday(),
            minute_of_day: now.hour() * 60 + now.minute(),
            seconds: 0.0,
            activity: BTreeMap::new(),
            emotions: BTreeMap::new(),
            away: 0.0,
            idle: 0.0,
            typing: 0.0,
            switches: 0,
            interactions: 0.0,
            session: 0.0,
        }
    }
}
impl Timeline {
    #[allow(clippy::too_many_arguments)]
    pub fn observe(
        &mut self,
        path: &Path,
        enabled: bool,
        activity: &str,
        idle: Option<f64>,
        typing: bool,
        events: &[&str],
        emotion: &str,
        session: f64,
        dt: f64,
    ) -> io::Result<()> {
        if !enabled {
            *self = Self::default();
            return Ok(());
        }
        if Local::now().timestamp() / 60 != self.minute {
            self.flush(path)?;
            *self = Self::default();
        }
        self.seconds += dt;
        *self.activity.entry(activity.into()).or_default() += dt;
        *self.emotions.entry(emotion.into()).or_default() += dt;
        if activity == "AWAY" {
            self.away += dt;
        }
        if idle.is_some_and(|v| v > 5.0) {
            self.idle += dt;
        }
        if typing {
            self.typing += dt;
        }
        self.switches += events.iter().filter(|e| **e == "APP_CHANGED").count() as u64;
        self.session = session;
        Ok(())
    }
    pub fn flush(&mut self, path: &Path) -> io::Result<()> {
        if self.seconds < 30.0 || self.activity.is_empty() {
            return Ok(());
        }
        if let Some(p) = path.parent() {
            fs::create_dir_all(p)?;
        }
        let empty = fs::metadata(path).map_or(true, |m| m.len() == 0);
        let mut f = OpenOptions::new().create(true).append(true).open(path)?;
        if empty {
            writeln!(
                f,
                "inicio,dia_semana,minuto_do_dia,atividade,ausente,ocioso,digitando,trocas_app,sessao_min,interacoes,emocao"
            )?;
        }
        fn dominant(map: &BTreeMap<String, f64>) -> &str {
            map.iter()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(k, _)| k.as_str())
                .unwrap_or("")
        }
        let s = self.seconds;
        writeln!(
            f,
            "{},{},{},{},{:.3},{:.3},{:.3},{},{:.1},{:.1},{}",
            self.stamp,
            self.weekday,
            self.minute_of_day,
            dominant(&self.activity),
            self.away / s,
            self.idle / s,
            self.typing / s,
            self.switches,
            self.session / 60.0,
            self.interactions,
            dominant(&self.emotions)
        )?;
        *self = Self::default();
        Ok(())
    }
    pub fn discard(&mut self, path: &Path) -> io::Result<()> {
        *self = Self::default();
        match fs::remove_file(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }
}
