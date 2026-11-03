// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{
    config::{PetDefinition, Profile, Settings, valid_pet_id},
    core::{
        brain::{Brain, BrainState},
        conditioning::Conditioning,
        emotion::{Emotion, EmotionState},
        homeostasis::Drives,
        memory::{Memory, Preferences},
        mind::{Corner, Desk, Protest},
        motor::MotorTone,
        physics::{Body, GRAVITY, Platform, Rect, ToyTracker, wrap_angle},
        rng::Rng,
        unit,
    },
    dialogue::Dialogue,
    platform::{self, Snapshot},
    senses::{self, ActivityTracker, Senses},
    storage::{self, InstanceLock},
    timeline::Timeline,
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub enum ThingKind {
    Note(String),
    Image(PathBuf),
    Paper,
}
#[derive(Clone, Debug)]
pub struct Thing {
    pub id: u64,
    pub kind: ThingKind,
    pub body: Body,
    pub born: f64,
    pub carried: bool,
    pub expires: Option<f64>,
}
#[derive(Clone, Debug)]
pub struct Particle {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub life: f64,
    pub color: [u8; 3],
}
#[derive(Serialize)]
pub struct Summary {
    pub project: &'static str,
    pub pet: String,
    pub neurons: usize,
    pub synapses: usize,
    pub simulated_seconds: f64,
    pub emotion: Emotion,
    pub energy: f64,
    pub stimulation: f64,
    pub contact: f64,
    pub protest_stage: u8,
    pub sad_stage: u8,
    pub episodes: usize,
    pub motor_hz: BTreeMap<String, f64>,
    pub warnings: Vec<String>,
}

pub struct Engine {
    pub root: PathBuf,
    pub content_root: PathBuf,
    pub settings: Settings,
    pub pet: PetDefinition,
    pub brain: Brain,
    pub drives: Drives,
    pub memory: Memory,
    pub dialogue: Dialogue,
    pub conditioning: Conditioning,
    pub emotion: EmotionState,
    pub tone: MotorTone,
    pub protest: Protest,
    pub sadness: Protest,
    pub corner: Corner,
    pub desk: Desk,
    pub body: Body,
    pub ball: Body,
    pub ball_stored: bool,
    pub ball_handled_until: f64,
    pub things: Vec<Thing>,
    pub particles: Vec<Particle>,
    pub bounds: Rect,
    pub platforms: Vec<Platform>,
    pub activity: String,
    pub intensities: BTreeMap<String, f64>,
    pub time: f64,
    pub paused: bool,
    pub hidden: bool,
    pub asleep: bool,
    pub in_flight: bool,
    pub gaze: (f64, f64),
    pub scale: f64,
    pub warnings: Vec<String>,
    pub panic_latched: bool,
    pub panic_generation: u64,
    pub sign_until: f64,
    pub sign_text: Option<String>,
    pub sign_hidden_until: f64,
    pub scene_positions: BTreeMap<String, [f64; 2]>,
    pub senses: Senses,
    tracker: ActivityTracker,
    pub timeline: Timeline,
    rng: Rng,
    toy: ToyTracker,
    think_accumulator: f64,
    sense_accumulator: f64,
    next_roam: f64,
    target_x: Option<f64>,
    next_play: f64,
    next_paper: f64,
    caress_until: f64,
    penalty_until: f64,
    dismisses: Vec<f64>,
    poke_at: f64,
    poke_probe: Option<(f64, f64)>,
    mission: Option<(Option<u64>, f64, f64)>,
    escape_target: Option<f64>,
    tipped_since: Option<f64>,
    dizzy_until: f64,
    euphoria: f64,
    next_firework: f64,
    force_mischief_until: f64,
    prev_cursor: (f64, f64),
    cursor_velocity: (f64, f64),
    mouse_moved_at: f64,
    last_push: f64,
    prev_distance: Option<f64>,
    march_dir: f64,
    _lock: InstanceLock,
}

impl Engine {
    pub fn load(root: PathBuf, seed: u64) -> io::Result<Self> {
        Self::load_with_content(root, seed, None)
    }

    pub fn load_with_content(
        root: PathBuf,
        seed: u64,
        content: Option<PathBuf>,
    ) -> io::Result<Self> {
        let lock = InstanceLock::acquire(&root)?;
        let mut warnings = Vec::new();
        let mut settings = match storage::read_json::<Settings>(&root.join("permissions.json")) {
            Ok(mut s) => {
                if !valid_pet_id(&s.pet) {
                    s.pet = "bryan".into();
                    warnings.push("Identificador do pet inválido; usando Bryan.".into());
                }
                s
            }
            Err(e) => {
                if e.kind() != io::ErrorKind::NotFound {
                    warnings.push(format!("Permissões ilegíveis: {e}"));
                }
                Settings::default()
            }
        };
        let content_root = content.unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .filter(|p| p.join("pets").exists())
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
        });
        let pet = match definition(&content_root, &settings.pet) {
            Ok(pet) => pet,
            Err(error) => {
                warnings.push(format!(
                    "Definição do personagem inválida: {error}; usando Bryan."
                ));
                settings.pet = "bryan".into();
                PetDefinition::builtin("bryan").expect("embedded definition")
            }
        };
        let brain = Brain::new(pet.seed, seed, &pet.temperament);
        let bounds = Rect::default();
        let mut out = Self {
            root,
            content_root,
            settings,
            pet,
            brain,
            drives: Drives::default(),
            memory: Memory::default(),
            dialogue: Dialogue::default(),
            conditioning: Conditioning::default(),
            emotion: EmotionState::default(),
            tone: MotorTone::default(),
            protest: Protest {
                since: -1e9,
                ..Default::default()
            },
            sadness: Protest {
                since: -1e9,
                ..Default::default()
            },
            corner: Corner::default(),
            desk: Desk {
                visible: true,
                position: (900.0, 1000.0),
                ..Default::default()
            },
            body: Body::new(bounds.right - 220.0, 120.0, 60.0, 45.0),
            ball: Body::new(400.0, bounds.bottom - 40.0, 18.0, 18.0),
            ball_stored: false,
            ball_handled_until: 0.0,
            things: Vec::new(),
            particles: Vec::new(),
            bounds,
            platforms: Vec::new(),
            activity: "IDLE".into(),
            intensities: BTreeMap::new(),
            time: 0.0,
            paused: false,
            hidden: false,
            asleep: false,
            in_flight: false,
            gaze: (0.0, 0.0),
            scale: 1.0,
            warnings,
            panic_latched: false,
            panic_generation: 0,
            sign_until: 0.0,
            sign_text: None,
            sign_hidden_until: 0.0,
            scene_positions: BTreeMap::new(),
            senses: Senses::default(),
            tracker: ActivityTracker::default(),
            timeline: Timeline::default(),
            rng: Rng(seed.wrapping_add(2)),
            toy: ToyTracker::default(),
            think_accumulator: 0.0,
            sense_accumulator: 0.0,
            next_roam: 0.0,
            target_x: None,
            next_play: 0.0,
            next_paper: 0.0,
            caress_until: 0.0,
            penalty_until: 0.0,
            dismisses: Vec::new(),
            poke_at: -1e9,
            poke_probe: None,
            mission: None,
            escape_target: None,
            tipped_since: None,
            dizzy_until: 0.0,
            euphoria: 0.0,
            next_firework: 0.0,
            force_mischief_until: 0.0,
            prev_cursor: (0.0, 0.0),
            cursor_velocity: (0.0, 0.0),
            mouse_moved_at: -1e9,
            last_push: -1e9,
            prev_distance: None,
            march_dir: 1.0,
            _lock: lock,
        };
        out.corner.position = (10.0, bounds.bottom - 150.0);
        if let Ok(positions) =
            storage::read_json::<BTreeMap<String, [f64; 2]>>(&out.root.join("panels.json"))
        {
            out.scene_positions = positions
                .into_iter()
                .filter(|(_, xy)| xy.iter().all(|v| v.is_finite()))
                .collect();
        }
        if let Some(xy) = out.scene_positions.get("notebook") {
            out.desk.position = (xy[0], xy[1]);
        }
        if let Some(xy) = out.scene_positions.get("castigo") {
            out.corner.position = (xy[0], xy[1]);
        }
        out.restore_pet();
        Ok(out)
    }
    pub fn pet_dir(&self) -> PathBuf {
        self.root.join("pets").join(&self.settings.pet)
    }
    pub fn profile(&self) -> Profile {
        Profile::new(self.settings.test_mode)
    }
    fn warning(&mut self, text: String) {
        storage::log(&self.root, &text);
        if !self.warnings.contains(&text) {
            self.warnings.push(text);
        }
    }

    fn restore_pet(&mut self) {
        let path = self.pet_dir();
        let now = storage::wall_time();
        let p = self.profile();
        macro_rules! load {
            ($file:literal,$ty:ty,$apply:expr) => {
                match storage::read_json::<$ty>(&path.join($file)) {
                    Ok(value) => {
                        if !($apply)(self, value) {
                            self.warning(format!(
                                "Estado inválido em {}; padrão preservado.",
                                $file
                            ));
                        }
                    }
                    Err(e) if e.kind() != io::ErrorKind::NotFound => {
                        self.warning(format!("Falha ao ler {}: {e}", $file))
                    }
                    _ => {}
                }
            };
        }
        load!("nervous.json", BrainState, |s: &mut Self, b: BrainState| s
            .brain
            .restore(&b, now));
        load!("drives.json", Drives, |s: &mut Self, d: Drives| s
            .drives
            .restore(d, &p, now));
        load!(
            "conditioning.json",
            Conditioning,
            |s: &mut Self, mut c: Conditioning| {
                c.validate();
                s.conditioning = c;
                true
            }
        );
        load!("protest.json", Protest, |s: &mut Self, v: Protest| s
            .protest
            .restore(v, now, p.grievance_down));
        load!("sad_protest.json", Protest, |s: &mut Self, v: Protest| s
            .sadness
            .restore(v, now, p.sad_down));
        load!("castigo.json", Corner, |s: &mut Self, c: Corner| {
            if !c.validate() {
                return false;
            }
            s.corner.resentment = c.resentment;
            s.corner.fear = c.fear;
            s.corner.victories = c.victories;
            true
        });
        load!(
            "preferences.json",
            Preferences,
            |s: &mut Self, mut pref: Preferences| {
                pref.validate();
                s.memory.preferences = pref;
                true
            }
        );
        match storage::read_episodes(&path.join("episodes.jsonl")) {
            Ok(e) => self.memory.recent = e,
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                self.warning(format!("Histórico de episódios ilegível: {e}"))
            }
            _ => {}
        }
        match Dialogue::load(&path) {
            Ok(dialogue) => self.dialogue = dialogue,
            Err(error) => self.warning(format!("Conversa ilegível: {error}")),
        }
    }
    pub fn switch_pet(&mut self, id: &str) -> io::Result<()> {
        self.select_pet(id, true)
    }

    pub fn select_pet_for_import(&mut self, id: &str) -> io::Result<()> {
        self.select_pet(id, false)
    }

    fn select_pet(&mut self, id: &str, persist: bool) -> io::Result<()> {
        let def = definition(&self.content_root, id)?;
        if persist {
            self.save()?;
        }
        self.settings.pet = id.into();
        self.pet = def;
        self.brain = Brain::new(self.pet.seed, self.rng.0, &self.pet.temperament);
        self.drives = Drives::default();
        self.memory = Memory::default();
        self.dialogue = Dialogue::default();
        self.conditioning = Conditioning::default();
        self.protest = Protest {
            since: -1e9,
            ..Default::default()
        };
        self.sadness = Protest {
            since: -1e9,
            ..Default::default()
        };
        self.corner = Corner {
            position: self.corner.position,
            ..Default::default()
        };
        self.emotion = EmotionState::default();
        self.tone = MotorTone::default();
        self.asleep = false;
        self.mission = None;
        self.things.clear();
        self.restore_pet();
        if persist {
            self.save()?;
        }
        Ok(())
    }
    pub fn available_pets(&self) -> Vec<String> {
        let mut ids = vec![
            "bryan".into(),
            "jorge".into(),
            "bia".into(),
            "sophia".into(),
        ];
        if let Ok(entries) = fs::read_dir(self.content_root.join("pets")) {
            for e in entries.flatten() {
                if e.path().extension().is_some_and(|s| s == "json")
                    && let Some(id) = e.path().file_stem().and_then(|s| s.to_str())
                    && valid_pet_id(id)
                {
                    ids.push(id.into());
                }
            }
        }
        ids.sort();
        ids.dedup();
        ids
    }
    pub fn pet_name(&self, id: &str) -> String {
        definition(&self.content_root, id)
            .map(|pet| pet.name)
            .unwrap_or_else(|_| id.to_string())
    }
    pub fn save(&mut self) -> io::Result<()> {
        let now = storage::wall_time();
        let path = self.pet_dir();
        self.drives.saved_at = now;
        self.protest.saved_at = now;
        self.sadness.saved_at = now;
        storage::write_json(&self.root.join("permissions.json"), &self.settings)?;
        storage::write_json(&path.join("nervous.json"), &self.brain.state(now))?;
        storage::write_json(&path.join("drives.json"), &self.drives)?;
        storage::write_json(&path.join("conditioning.json"), &self.conditioning)?;
        storage::write_json(&path.join("protest.json"), &self.protest)?;
        storage::write_json(&path.join("sad_protest.json"), &self.sadness)?;
        storage::write_json(&path.join("castigo.json"), &self.corner)?;
        storage::write_json(&path.join("preferences.json"), &self.memory.preferences)?;
        storage::write_episodes(&path.join("episodes.jsonl"), &self.memory.recent)?;
        self.dialogue.save(&path)?;
        self.scene_positions.insert(
            "notebook".into(),
            [self.desk.position.0, self.desk.position.1],
        );
        self.scene_positions.insert(
            "castigo".into(),
            [self.corner.position.0, self.corner.position.1],
        );
        storage::write_json(&self.root.join("panels.json"), &self.scene_positions)?;
        Ok(())
    }
    pub fn panic(&mut self) {
        self.settings.active = false;
        self.panic_latched = true;
        self.panic_generation = self.panic_generation.wrapping_add(1);
        self.corner.release();
        self.corner.escape = None;
        self.things.clear();
        self.particles.clear();
        self.mission = None;
        self.escape_target = None;
        self.body.held = false;
        self.ball.held = false;
        self.desk.seated = false;
        self.senses.dragging = false;
        if let Err(e) = self.save() {
            self.warning(format!("Falha ao persistir pânico: {e}"));
        }
    }
    pub fn visible(&self, input: &Snapshot) -> bool {
        self.settings.active && !self.hidden && (!input.fullscreen || self.settings.show_fullscreen)
    }
    pub fn caressing(&self) -> bool {
        self.settings.caress && self.time < self.caress_until
    }
    pub fn caress(&mut self, seconds: f64) {
        if !self.settings.caress || !self.settings.active {
            return;
        }
        self.caress_until = self.time + seconds;
        self.timeline.interactions += seconds;
        self.memory.interaction();
    }
    pub fn poke(&mut self) {
        if !self.settings.poke || self.time - self.poke_at < 0.25 || !self.settings.active {
            return;
        }
        self.poke_at = self.time;
        self.senses.kick("POKE", 0.8);
        self.drives.stimulate("poke", 1.0);
        self.drives.touched("poke", 1.0);
        self.memory.interaction();
        self.timeline.interactions += 1.0;
        self.conditioning.outcome(self.time, &["PRESS", "APPROACH"]);
        if self.asleep {
            self.wake(true);
        }
        self.poke_probe = Some((self.time + 0.5, self.brain.hz("CHEER")));
    }
    pub fn drag_start(&mut self) {
        if !self.settings.drag {
            return;
        }
        self.body.held = true;
        self.senses.dragging = true;
        self.corner.release();
        self.mission = None;
        self.drives.stimulate("drag", 1.0);
        self.drives.touched("drag", 1.0);
        self.memory.interaction();
        self.conditioning.outcome(self.time, &["PRESS", "APPROACH"]);
        if self.desk.seated {
            self.desk.seated = false;
            self.protest.grievance = (self.protest.grievance + 0.35).min(1.3);
            self.senses.kick("DISMISS", 0.7);
        }
        if self.asleep {
            self.wake(true);
        }
    }
    pub fn drag_release(&mut self, vx: f64, vy: f64) {
        self.body.held = false;
        self.senses.dragging = false;
        let speed = vx.hypot(vy);
        self.body
            .jump(vx.clamp(-4000.0, 4000.0), vy.clamp(-4000.0, 4000.0), 0.0);
        self.body.omega = self.body.omega.clamp(-1000.0, 1000.0);
        if speed > 700.0 {
            self.in_flight = true;
            self.senses.kick("THROW", 0.4 + 0.5 * unit(speed / 2500.0));
            self.drives.stimulate("throw", 1.0);
            self.drives.touched("throw", 1.0);
        }
        if self.corner.enabled {
            self.corner.pending_landing = Some(self.time + 3.0);
        }
    }
    pub fn sleep(&mut self) {
        self.asleep = true;
        self.memory.consolidate();
        self.mission = None;
        self.desk.seated = false;
    }
    pub fn wake(&mut self, startled: bool) {
        self.asleep = false;
        self.emotion.clear();
        if startled {
            self.emotion.force(Emotion::Startled, 1.2, self.time, 40);
        }
    }
    pub fn request_mischief(&mut self) {
        self.force_mischief_until = self.time + 10.0;
    }
    pub fn clear_things(&mut self) {
        self.memory.clear_things();
        self.things.clear();
        self.mission = None;
    }
    pub fn dismiss(&mut self, id: u64) {
        if let Some(index) = self.things.iter().position(|t| t.id == id) {
            let thing = self.things.remove(index);
            let p = self.profile();
            self.memory
                .dismiss(id, self.time - thing.born, p.dismiss_patience);
            self.senses.kick("DISMISS", 0.7);
            self.drives.touched("dismiss", 1.0);
            self.penalty_until = self.time + 1.0;
            self.dismisses.retain(|t| self.time - *t <= 15.0);
            self.dismisses.push(self.time);
            if self.dismisses.len() >= 3
                || (self.time - thing.born < 3.0 && self.dismisses.len() >= 2)
            {
                self.emotion.force(Emotion::Dramatic, 4.0, self.time, 35);
                self.dismisses.clear();
            }
            if self.mission.is_some_and(|m| m.0 == Some(id)) {
                self.mission = None;
            }
        }
    }
    pub fn dismiss_sign(&mut self) {
        self.sign_hidden_until = self.time + 3.0;
        if self.protest.stage > 0 {
            self.protest.repressed();
            self.senses.kick("DISMISS", 0.7);
        }
    }

    pub fn advance(&mut self, dt: f64, input: &Snapshot) {
        if input.panic {
            if !self.panic_latched {
                self.panic();
            }
            return;
        }
        self.panic_latched = false;
        if !dt.is_finite() || dt <= 0.0 || self.paused || !self.visible(input) {
            return;
        }
        let dt = dt.min(0.05);
        self.time += dt;
        self.think_accumulator += dt;
        self.sense_accumulator += dt;
        let cursor = (input.cursor.0 / self.scale, input.cursor.1 / self.scale);
        self.cursor_velocity = (
            (cursor.0 - self.prev_cursor.0) / dt,
            (cursor.1 - self.prev_cursor.1) / dt,
        );
        if (cursor.0 - self.prev_cursor.0).abs() + (cursor.1 - self.prev_cursor.1).abs() > 2.0 {
            self.mouse_moved_at = self.time;
        }
        self.prev_cursor = cursor;
        if self.sense_accumulator >= 0.5 {
            let activity = senses::detect_activity(
                &input.app,
                input.idle_seconds.unwrap_or(0.0),
                self.profile().away_after,
            );
            let events = self.tracker.observe(&input.app, activity, self.time);
            self.senses.events(&events);
            for e in &events {
                if *e == "APP_CHANGED" {
                    self.drives.stimulate("app", 1.0);
                } else if ["ACTIVITY_CHANGED", "USER_RETURNED"].contains(e) {
                    self.drives.stimulate("novelty", 1.0);
                }
            }
            self.activity = activity.into();
            self.platforms = if self.settings.perch {
                platform::platforms(&input.windows, &[], self.bounds, self.scale)
            } else {
                Vec::new()
            };
            let result = self.timeline.observe(
                &self.root.join("timeline.csv"),
                self.settings.routine,
                &self.activity,
                input.idle_seconds,
                self.senses.typing > 0.5,
                &events,
                self.emotion.current.name(),
                self.time - self.tracker.session_start,
                self.sense_accumulator,
            );
            if let Err(e) = result {
                self.warning(format!("Falha ao gravar rotina: {e}"));
            }
            self.sense_accumulator = 0.0;
        }
        if self.think_accumulator >= 0.1 {
            let think_dt = self.think_accumulator;
            self.think_accumulator = 0.0;
            self.think(think_dt, input, cursor);
        }
        self.corner_frame(dt, cursor);
        self.behave(dt, cursor);
        if !self.desk.seated {
            self.body.step(dt, self.bounds, &self.platforms);
        }
        if self.body.grounded {
            self.in_flight = false;
        }
        let impacts = std::mem::take(&mut self.body.impacts);
        for speed in impacts {
            if speed > 1500.0 {
                self.senses.kick("IMPACT", 0.5 * unit(speed / 3000.0));
                if speed > 2000.0 {
                    self.dizzy_until = self.time + 1.6;
                }
            }
        }
        if self.settings.ball && !self.ball_stored {
            self.ball.step(dt, self.bounds, &self.platforms);
            let rects: Vec<Rect> = self
                .things
                .iter()
                .filter(|t| !matches!(t.kind, ThingKind::Paper))
                .map(|t| Rect {
                    left: t.body.x - t.body.half_w,
                    top: t.body.y - t.body.half_h,
                    right: t.body.x + t.body.half_w,
                    bottom: t.body.y + t.body.half_h,
                })
                .collect();
            self.ball.circle_bounce(&rects);
            self.push_ball(cursor);
        }
        if self.mission.is_none() {
            for thing in &mut self.things {
                thing.carried = false;
            }
        }
        for thing in &mut self.things {
            if !thing.carried {
                thing.body.step(dt, self.bounds, &[]);
            }
        }
        self.things
            .retain(|t| !t.expires.is_some_and(|until| self.time > until));
        for p in &mut self.particles {
            p.life -= dt;
            p.vy += 260.0 * dt;
            p.x += p.vx * dt;
            p.y += p.vy * dt;
        }
        self.particles.retain(|p| p.life > 0.0);
        self.maybe_mischief(dt, cursor);
    }

    fn think(&mut self, dt: f64, input: &Snapshot, cursor: (f64, f64)) {
        let p = self.profile();
        let caress = self.caressing();
        let moving = unit(self.body.vx.abs() / 200.0 + if self.body.grounded { 0.0 } else { 1.0 });
        self.drives.update(
            dt,
            &p,
            self.asleep,
            moving,
            self.emotion.arousal,
            self.activity == "AWAY",
            ["PROGRAMMING", "DESIGNING"].contains(&self.activity.as_str()),
            self.corner.inside,
            self.corner.visit > 0.5,
        );
        self.corner.tick(
            dt,
            (cursor.0 - self.body.x).hypot(cursor.1 - self.body.y) < 160.0,
        );
        if caress {
            self.drives.stimulate("caress", dt);
            self.drives.touched("caress", dt);
            self.memory.interaction();
        }
        self.intensities = self.senses.update(
            &self.activity,
            input.idle_seconds,
            self.drives.channels(),
            caress,
            self.time - self.mouse_moved_at < 0.4,
            dt,
        );
        let dist = (cursor.0 - self.body.x).hypot(cursor.1 - self.body.y);
        let approaching = self
            .prev_distance
            .is_some_and(|old| dist < 220.0 && (old - dist) / dt > 500.0);
        self.prev_distance = Some(dist);
        self.conditioning
            .set_cue("PRESS", self.body.held, self.time);
        self.conditioning
            .set_cue("APPROACH", approaching, self.time);
        self.conditioning.set_cue(
            "PLACA",
            self.corner.enabled
                && self.body.held
                && (self.body.x - self.corner.position.0 - 95.0)
                    .hypot(self.body.y - self.corner.position.1 - 75.0)
                    < 260.0,
            self.time,
        );
        self.conditioning.tick(self.time, p.cond_rate);
        self.intensities.insert(
            "ANTICIPATE".into(),
            self.conditioning.strength(&["PRESS", "APPROACH"]),
        );
        self.intensities.insert(
            "ANTECIPA_PLACA".into(),
            self.conditioning.strength(&["PLACA"]),
        );
        let (taken, near) = self.toy.update(
            (self.ball.x, self.ball.y),
            (self.body.x, self.body.y),
            self.time < self.ball_handled_until,
        );
        if taken && self.settings.ball && !self.ball_stored {
            self.senses.kick("TOY_TAKEN", 0.9);
        }
        self.intensities.insert(
            "TOY_NEAR".into(),
            if self.settings.ball && !self.ball_stored {
                near
            } else {
                0.0
            },
        );
        let placa = if self.corner.enabled {
            unit(
                1.0 - (self.body.x - self.corner.position.0 - 95.0)
                    .hypot(self.body.y - self.corner.position.1 - 75.0)
                    / 500.0,
            )
        } else {
            0.0
        };
        for (name, value) in [
            ("PEDIDO", self.desk.signal(self.time)),
            ("PRESO", if self.corner.inside { 1.0 } else { 0.0 }),
            ("LEMBRA_FUGA", self.corner.escape_memory()),
            ("PLACA", if self.corner.inside { 1.0 } else { placa }),
            (
                "VISITA",
                if self.corner.inside {
                    self.corner.visit
                } else {
                    0.0
                },
            ),
            ("RECEIO", self.corner.fear),
            ("RANCOR", self.corner.resentment),
        ] {
            self.intensities.insert(name.into(), value);
        }
        self.brain.stimulate(&self.intensities);
        self.brain.step(dt * 1000.0);
        self.tone.update(|m| self.brain.hz(m), dt);
        if !self.corner.inside && !self.asleep {
            let changed = self.protest.update(
                dt,
                self.tone.get("ANGRY") + 0.5 * self.tone.get("SAD"),
                &p,
                caress,
                self.time,
                false,
            );
            let sad = self.sadness.update(
                dt,
                self.tone.get("SAD") + 0.5 * self.drives.channels()["LONELY"],
                &p,
                caress,
                self.time,
                true,
            );
            if changed || sad {
                self.sign_text = Some(self.pick_sign());
                self.sign_until = if self.protest.stage == 0 && self.sadness.stage == 0 {
                    self.time + 3.5
                } else {
                    f64::INFINITY
                };
                self.mission = None;
            }
        }
        let mut reward = if self.settings.caress && self.protest.stage == 0 {
            (if caress { 1.0 } else { 0.0 })
                + if self.time < self.penalty_until {
                    -0.6
                } else {
                    0.0
                }
        } else {
            0.0
        };
        if self.time < self.corner.punishment_until {
            reward -= 1.0;
        }
        self.brain.learn(reward, dt, p.learn_rate);
        if let Some((due, before)) = self.poke_probe
            && self.time >= due
        {
            let response = unit((self.brain.hz("CHEER") - before) / 6.0);
            if response >= 0.12 && self.body.grounded {
                self.body.jump(
                    self.rng.range(-150.0, 150.0),
                    -self.tone.hop_power() * response,
                    0.0,
                );
            }
            self.poke_probe = None;
        }
        if self.in_flight {
            let reflex = [
                (Emotion::Happy, self.brain.hz("PLAYFUL")),
                (Emotion::Startled, self.brain.hz("ALERT")),
                (Emotion::Angry, self.brain.hz("FRUSTRATION")),
            ]
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .expect("three reflexes");
            self.emotion.force(
                if reflex.1 < 4.0 {
                    Emotion::Happy
                } else {
                    reflex.0
                },
                0.3,
                self.time,
                30,
            );
        }
        if caress {
            self.emotion.force(Emotion::Content, 0.5, self.time, 15);
        }
        if self.asleep {
            if self.drives.rested() {
                self.wake(false);
            } else {
                self.emotion.force(Emotion::Sleeping, 0.3, self.time, 50);
            }
        } else if self.drives.sleepy()
            && self.body.grounded
            && !self.body.held
            && !self.in_flight
            && self.protest.stage < 2
            && self.mission.is_none()
            && !matches!(self.emotion.current, Emotion::Startled | Emotion::Dramatic)
        {
            self.sleep();
        }
        if !self.asleep {
            if self.protest.stage >= 2 {
                self.emotion.force(Emotion::Angry, 0.3, self.time, 20);
            } else if self.sadness.stage >= 2 {
                self.emotion.force(Emotion::Sad, 0.3, self.time, 20);
            }
        }
        self.emotion.update(|m| self.brain.hz(m), dt, &p, self.time);
        let can_sit = !self.asleep
            && !self.body.held
            && !self.corner.inside
            && self.protest.stage < 2
            && self.sadness.stage < 2
            && self.body.grounded
            && self.mission.is_none();
        self.desk.tick(self.time, self.brain.hz("SENTAR"), can_sit);
        if self.desk.refused {
            self.senses.kick("RECUSA", 1.0);
            self.desk.refused = false;
        }
        self.memory.tick(self.time, p.episode_ok, p.episode_window);
        self.euphoria = (self.euphoria
            + dt / 0.1 * (0.1 * (self.brain.hz("CHEER") - 4.0).max(0.0) / 6.0 - 0.008))
            .clamp(0.0, 1.5);
        if self.euphoria >= 1.0
            && self.emotion.current == Emotion::Happy
            && self.time >= self.next_firework
            && self.settings.fireworks
            && !self.asleep
            && !self.body.held
        {
            self.firework();
            self.euphoria = 0.0;
            self.next_firework = self.time + 40.0;
        }
    }

    fn corner_zone(&self) -> Rect {
        let bottom = self.bounds.bottom.min(self.corner.position.1 + 190.0);
        Rect {
            left: self.corner.position.0,
            top: bottom - 240.0,
            right: self.corner.position.0 + 190.0,
            bottom,
        }
    }
    fn corner_frame(&mut self, dt: f64, _cursor: (f64, f64)) {
        if !self.corner.enabled {
            return;
        }
        let zone = self.corner_zone();
        let inside = zone.contains(self.body.x, self.body.y) && self.body.grounded;
        if let Some((start, x0, x1, right)) = self.corner.escape {
            if self.time - start > 30.0 || self.body.held {
                self.corner.escape = None;
                self.escape_target = None;
            } else if if right {
                self.body.x > x1 + 40.0
            } else {
                self.body.x < x0 - 40.0
            } {
                *self.corner.victories.entry("fugir".into()).or_default() += 1.0;
                self.corner.last_success = Some(self.time);
                self.corner.escape = None;
                self.escape_target = None;
            }
        }
        if self.body.held {
            self.corner.pending_landing = None;
            self.corner.release();
            return;
        }
        if let Some(until) = self.corner.pending_landing {
            if self.body.grounded {
                self.corner.pending_landing = None;
                if inside && self.time <= until {
                    self.corner.enter(self.time);
                    self.conditioning.outcome(self.time, &["PLACA"]);
                    self.memory.punish(self.time, 20.0);
                    self.mission = None;
                }
            } else if self.time > until {
                self.corner.pending_landing = None;
            }
        }
        if self.corner.inside && !inside {
            self.corner.release();
        }
        if !self.corner.inside || self.asleep {
            return;
        }
        if self.brain.hz("ACEITAR") >= 12.0 {
            self.corner.resentment *= 0.5;
            self.corner.release();
            return;
        }
        self.corner.urge =
            (self.corner.urge + dt * (self.brain.hz("FUGIR") - 2.0) / 300.0).max(0.0);
        if self.corner.urge >= 1.0 {
            let right =
                (zone.left + zone.right) / 2.0 < (self.bounds.left + self.bounds.right) / 2.0;
            self.corner.escape = Some((self.time, zone.left, zone.right, right));
            self.escape_target = Some(
                if right {
                    zone.right + 300.0
                } else {
                    zone.left - 300.0
                }
                .clamp(self.bounds.left + 60.0, self.bounds.right - 60.0),
            );
            self.corner.release();
        }
    }

    fn slide(&mut self, target: f64, speed: f64) {
        self.body.vx = if target > self.body.x { speed } else { -speed };
    }
    fn behave(&mut self, dt: f64, cursor: (f64, f64)) {
        let p = self.profile();
        let emotion = self.emotion.current;
        let dist = (cursor.0 - self.body.x)
            .hypot(cursor.1 - self.body.y)
            .max(1.0);
        self.gaze = (
            (cursor.0 - self.body.x) / dist,
            (cursor.1 - self.body.y) / dist,
        );
        if self.time < self.dizzy_until {
            self.gaze = ((self.time * 14.0).cos(), (self.time * 14.0).sin());
        }
        if self.body.held {
            return;
        }
        if self.desk.seated {
            self.body.x = self.desk.position.0 + 80.0;
            self.body.y = self.desk.position.1 - 5.0;
            self.body.vx = 0.0;
            self.body.vy = 0.0;
            self.body.grounded = true;
            self.body.angle = 0.0;
            self.gaze = (0.0, 0.9);
            return;
        }
        if !self.body.grounded {
            return;
        }
        if let Some(target) = self.escape_target {
            if (target - self.body.x).abs() < 20.0 {
                self.escape_target = None;
            } else {
                self.slide(target, 160.0);
                return;
            }
        }
        if self.corner.inside {
            self.body.vx = 0.0;
            if self.brain.hz("ANGRY") >= 3.0 {
                self.gaze = (
                    if self.body.x - self.bounds.left < self.bounds.right - self.body.x {
                        -1.0
                    } else {
                        1.0
                    },
                    0.0,
                );
            } else if self.brain.hz("WATCH") < 3.0 {
                self.gaze = (0.0, 0.9);
            }
            return;
        }
        if self.asleep || self.caressing() || self.time < self.dizzy_until {
            self.body.vx = 0.0;
            return;
        }
        if !self.body.upright() {
            let since = *self.tipped_since.get_or_insert(self.time);
            if emotion != Emotion::Sad && self.time - since > p.righting_after {
                self.body
                    .jump(0.0, -650.0, -wrap_angle(self.body.angle) * 1.6);
                self.tipped_since = None;
            }
            return;
        }
        self.tipped_since = None;
        if let Some((thing, target, deadline)) = self.mission {
            if let Some(id) = thing
                && let Some(t) = self.things.iter_mut().find(|t| t.id == id)
            {
                t.carried = true;
                t.body.x = self.body.x
                    - (target - self.body.x).signum() * (self.body.half_w + t.body.half_w + 5.0);
                t.body.y = self.body.y + self.body.half_h - t.body.half_h;
            }
            if (self.body.x - target).abs() < 20.0 || self.time > deadline {
                for t in &mut self.things {
                    t.carried = false;
                }
                self.mission = None;
                self.next_roam = self.time + 2.0;
            } else {
                self.slide(
                    target,
                    if thing.is_some() {
                        190.0 * self.tone.vigor().max(0.5)
                    } else {
                        260.0
                    },
                );
                return;
            }
        }
        let stage = if self.protest.stage > 0 {
            self.protest.stage
        } else if self.sadness.stage >= 2 {
            3
        } else {
            0
        };
        if stage == 1 {
            self.body.vx = 0.0;
            return;
        }
        if stage == 3 || self.brain.hz("CANTO") >= 3.0 {
            let target = if self.body.x - self.bounds.left < self.bounds.right - self.body.x {
                self.bounds.left + 90.0
            } else {
                self.bounds.right - 90.0
            };
            if (self.body.x - target).abs() > 12.0 {
                self.slide(target, 110.0);
            } else {
                self.body.vx = 0.0;
            }
            self.gaze = (0.0, 0.9);
            return;
        }
        if stage == 2 || stage == 4 {
            if self.body.x > self.bounds.right - 120.0 {
                self.march_dir = -1.0;
            }
            if self.body.x < self.bounds.left + 120.0 {
                self.march_dir = 1.0;
            }
            self.body.vx = self.march_dir * if stage == 2 { 150.0 } else { 105.0 };
            if self.rng.uniform() < dt * if stage == 2 { 1.4 } else { 0.6 } {
                self.body.jump(self.body.vx, -330.0, 0.0);
            }
            if stage == 4
                && self.time >= self.next_paper
                && self.pet.play.get("arremesso").copied().unwrap_or(0.0) > 0.0
                && self.rng.uniform() < self.brain.hz("PAPEL") * 0.05 * dt
            {
                self.paper(cursor);
                self.next_paper = self.time + 0.45;
            }
            return;
        }
        self.play_ball(cursor);
        if matches!(
            emotion,
            Emotion::Sad | Emotion::Startled | Emotion::Dramatic
        ) {
            return;
        }
        if self.time >= self.next_roam {
            self.next_roam =
                self.time + self.rng.range(p.roam_min, p.roam_max) * (1.6 - self.tone.drive());
            let reach = 120.0 + 450.0 * self.tone.drive();
            let mut target = (self.body.x + self.rng.range(-reach, reach))
                .clamp(self.bounds.left + 80.0, self.bounds.right - 80.0);
            if self.settings.ball
                && !self.ball_stored
                && matches!(
                    emotion,
                    Emotion::Happy | Emotion::Content | Emotion::Curious
                )
                && self.rng.uniform() < 0.75
            {
                target = self
                    .ball
                    .x
                    .clamp(self.bounds.left + 80.0, self.bounds.right - 80.0);
            }
            self.target_x =
                if emotion == Emotion::Watching && self.rng.uniform() < 1.0 - self.tone.drive() {
                    None
                } else {
                    Some(target)
                };
            if !self.platforms.is_empty()
                && self.rng.uniform()
                    < (0.03 + 0.5 * self.tone.get("WANDER")) * (1.0 - self.tone.get("SAD"))
                && self.jump_to_window()
            {
                return;
            }
            if self.rng.uniform() < self.tone.hop_chance() {
                let spin = if self.rng.uniform() < 0.4 * self.tone.get("CHEER") {
                    if self.rng.uniform() < 0.5 {
                        -720.0
                    } else {
                        720.0
                    }
                } else {
                    0.0
                };
                self.body.jump(
                    self.body.vx,
                    -self.tone.hop_power() * self.rng.range(0.85, 1.15),
                    spin,
                );
                return;
            }
        }
        if let Some(target) = self.target_x {
            if (self.body.x - target).abs() < 12.0
                || self.rng.uniform() < 1.5 * self.tone.get("DROWSE") * self.tone.get("WANDER") * dt
            {
                self.target_x = None;
            } else {
                self.slide(target, self.tone.speed());
            }
        }
    }
    fn jump_to_window(&mut self) -> bool {
        let (ex, ey) = self.body.extents();
        let bottom = self.body.y + ey;
        let options: Vec<_> = self
            .platforms
            .iter()
            .filter(|p| {
                Some(p.id) != self.body.platform
                    && p.right - p.left - 150.0 - 2.0 * ex > 20.0
                    && (bottom - p.top).abs() < 900.0
            })
            .copied()
            .collect();
        if options.is_empty() {
            return false;
        }
        let p = options[self.rng.index(options.len())];
        let tx =
            (self.body.x + self.rng.range(-200.0, 200.0)).clamp(p.left + ex, p.right - 150.0 - ex);
        let apex = bottom.min(p.top) - 70.0;
        let up = (2.0 * (bottom - apex) / GRAVITY).sqrt();
        let down = (2.0 * (p.top - apex) / GRAVITY).sqrt();
        self.body.jump(
            (tx - self.body.x) / (up + down),
            -GRAVITY * up,
            self.rng.range(-90.0, 90.0),
        );
        true
    }
    fn play_ball(&mut self, cursor: (f64, f64)) {
        if !self.settings.ball
            || self.ball_stored
            || self.ball.held
            || self.time < self.next_play
            || (self.ball.x - self.body.x).hypot(self.ball.y - self.body.y) > 240.0
        {
            return;
        }
        let focus = unit(self.brain.hz("FOCUS") / 6.0);
        let distraction = unit(self.brain.hz("PLAYFUL").max(self.brain.hz("CURIOUS")) / 6.0);
        if distraction <= focus {
            return;
        }
        self.next_play = self.time + self.rng.range(2.5, 5.0);
        let e = self.emotion.current;
        if matches!(e, Emotion::Happy | Emotion::Content | Emotion::Curious)
            && self.rng.uniform() < 0.7 * self.pet.play.get("bola").copied().unwrap_or(0.0)
        {
            let side = if self.ball.x >= self.body.x {
                1.0
            } else {
                -1.0
            };
            self.ball.jump(
                side * self.rng.range(380.0, 620.0),
                -self.rng.range(300.0, 520.0),
                0.0,
            );
            self.body.jump(0.0, -180.0, 0.0);
        } else if e == Emotion::Angry
            && self.rng.uniform() < 0.6 * self.pet.play.get("arremesso").copied().unwrap_or(0.0)
        {
            self.ball.jump(
                ((cursor.0 - self.ball.x) / 0.7).clamp(-1400.0, 1400.0),
                (cursor.1 - self.ball.y - 0.5 * GRAVITY * 0.49) / 0.7,
                0.0,
            );
        }
    }
    fn push_ball(&mut self, cursor: (f64, f64)) {
        if self.ball.held || self.time - self.last_push < 0.25 {
            return;
        }
        let dx = self.ball.x - cursor.0;
        let dy = self.ball.y - cursor.1;
        let d = dx.hypot(dy);
        if d >= 24.0 {
            return;
        }
        let (nx, ny) = if d > 1e-6 {
            (dx / d, dy / d)
        } else {
            (0.0, -1.0)
        };
        self.ball.x = cursor.0 + nx * 24.0;
        self.ball.y = cursor.1 + ny * 24.0;
        let speed_in = self.cursor_velocity.0.hypot(self.cursor_velocity.1);
        let (ux, uy) = if speed_in > 50.0 {
            (
                self.cursor_velocity.0 / speed_in,
                self.cursor_velocity.1 / speed_in,
            )
        } else {
            (nx, ny)
        };
        let speed = (speed_in * 0.8).clamp(260.0, 1200.0);
        self.ball.jump(ux * speed, uy * speed - 120.0, 0.0);
        self.last_push = self.time;
    }
    fn paper(&mut self, cursor: (f64, f64)) {
        let id = self.next_id();
        let mut body = Body::new(self.body.x, self.body.y - 50.0, 10.0, 10.0);
        body.jump(
            ((cursor.0 - body.x) / 0.7).clamp(-1400.0, 1400.0),
            (cursor.1 - body.y - 0.5 * GRAVITY * 0.49) / 0.7,
            self.rng.range(-600.0, 600.0),
        );
        self.things.push(Thing {
            id,
            kind: ThingKind::Paper,
            body,
            born: self.time,
            carried: false,
            expires: Some(self.time + 8.0),
        });
    }
    fn next_id(&mut self) -> u64 {
        self.rng.0 = self.rng.0.wrapping_add(1);
        self.rng.0
    }
    fn images(&self, dir: &Path) -> Vec<PathBuf> {
        let Ok(items) = fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut files: Vec<_> = items
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension().and_then(|x| x.to_str()).is_some_and(|x| {
                        ["png", "jpg", "jpeg", "webp", "gif", "bmp"]
                            .contains(&x.to_lowercase().as_str())
                    })
            })
            .collect();
        files.sort();
        files
    }
    fn maybe_mischief(&mut self, dt: f64, _cursor: (f64, f64)) {
        if !self.settings.mischief
            || self.asleep
            || self.body.held
            || !self.body.grounded
            || self.protest.stage == 3
            || self.sadness.stage >= 2
            || self.corner.inside
            || self.mission.is_some()
            || self
                .things
                .iter()
                .filter(|t| !matches!(t.kind, ThingKind::Paper))
                .count()
                >= self.profile().max_objects
        {
            return;
        }
        if self.time >= self.force_mischief_until
            && self.rng.uniform() >= self.brain.hz("TRAVESSURA") * 0.002 * dt
        {
            return;
        }
        let base = self.content_root.join("pets").join(&self.settings.pet);
        let memes = self.images(&base.join("memes").join(self.emotion.current.name()));
        let photos = self.images(&self.content_root.join("assets/comum/fotos"));
        let mut allowed = vec!["nada".into(), "atravessar".into()];
        if self.settings.notes && self.pet.play.get("postit").copied().unwrap_or(0.5) > 0.0 {
            allowed.push("bilhete".into());
        }
        if self.settings.memes && !memes.is_empty() {
            allowed.push("meme".into());
        }
        if self.settings.photos && !photos.is_empty() {
            allowed.push("foto".into());
        }
        let ctx = format!("{}|{}", self.activity, self.emotion.current.name());
        let Some(action) =
            self.memory
                .choose(&ctx, &allowed, self.profile().explore, &mut self.rng)
        else {
            return;
        };
        self.force_mischief_until = 0.0;
        let mut thing_id = None;
        match action.as_str() {
            "bilhete" | "meme" | "foto" => {
                let kind = if action == "bilhete" {
                    ThingKind::Note(self.pick_note())
                } else {
                    let list = if action == "meme" { &memes } else { &photos };
                    ThingKind::Image(list[self.rng.index(list.len())].clone())
                };
                let id = self.next_id();
                thing_id = Some(id);
                let target = self
                    .rng
                    .range(self.bounds.left + 180.0, self.bounds.right - 180.0);
                self.things.push(Thing {
                    id,
                    kind,
                    body: Body::new(self.body.x, self.body.y, 85.0, 60.0),
                    born: self.time,
                    carried: true,
                    expires: None,
                });
                self.mission = Some((Some(id), target, self.time + 12.0));
            }
            "atravessar" => {
                let target = if self.body.x < (self.bounds.left + self.bounds.right) / 2.0 {
                    self.bounds.right - 80.0
                } else {
                    self.bounds.left + 80.0
                };
                self.mission = Some((None, target, self.time + 15.0));
            }
            _ => {}
        }
        self.memory.start(
            ctx,
            action.clone(),
            thing_id,
            self.time,
            storage::wall_time(),
        );
        if action != "nada" {
            self.drives.stimulate("mischief", 1.0);
        }
    }
    fn pick_note(&mut self) -> String {
        let sad = self.emotion.current == Emotion::Sad;
        let file = if sad {
            "recadinhos_tristes.txt"
        } else {
            "recadinhos.txt"
        };
        let path = self
            .content_root
            .join("pets")
            .join(&self.settings.pet)
            .join(file);
        let text = fs::read_to_string(path).unwrap_or_default();
        let lines: Vec<_> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        if !lines.is_empty() {
            return lines[self.rng.index(lines.len())]
                .chars()
                .take(500)
                .collect();
        }
        let lines = if sad {
            vec!["Passando para dizer oi.", "Uma pausa também faz parte."]
        } else if self.settings.pet == "jorge" {
            vec![
                "Minha mentoria: beba água e faça uma pausa.",
                "Retorno garantido: mais um carinho.",
            ]
        } else if self.settings.pet == "bia" {
            vec![
                "Energia alta. Um carinho acalma.",
                "Essa tela está pedindo uma pausa.",
            ]
        } else if self.settings.pet == "sophia" {
            vec![
                "Estou por aqui, no seu ritmo.",
                "Um carinho vale mais que pressa.",
            ]
        } else {
            vec![
                "Seu código já ganhou companhia.",
                "Pausa rápida para um carinho?",
                "Estou de olho nesse cursor.",
            ]
        };
        lines[self.rng.index(lines.len())].into()
    }
    fn pick_sign(&self) -> String {
        let section = if self.protest.stage > 0 {
            match self.protest.stage {
                1 => "reclamacao",
                2 => "manifestacao",
                3 => "greve",
                _ => "revolucao",
            }
        } else if self.sadness.stage > 0 {
            match self.sadness.stage {
                1 => "saudade",
                2 => "chorando",
                _ => "abandonado",
            }
        } else {
            "acordo"
        };
        let path = self
            .content_root
            .join("pets")
            .join(&self.settings.pet)
            .join("cartazes.txt");
        if let Ok(text) = fs::read_to_string(path) {
            let mut active = false;
            for line in text.lines().map(str::trim) {
                if line.starts_with('[') && line.ends_with(']') {
                    active = &line[1..line.len() - 1] == section;
                } else if active && !line.is_empty() && !line.starts_with('#') {
                    return line.chars().take(200).collect();
                }
            }
        }
        match section {
            "reclamacao" => "EI, CURSOR!",
            "manifestacao" => "MAIS CARINHO!",
            "greve" => "EM GREVE",
            "revolucao" => "VIVA A REVOLUÇÃO!",
            "saudade" => "Tem alguém aí?",
            "chorando" => "Me visita?",
            "abandonado" => "Estou no meu cantinho.",
            _ => "ACORDO FECHADO",
        }
        .into()
    }
    fn firework(&mut self) {
        for _ in 0..70 {
            let angle = self.rng.range(0.0, std::f64::consts::TAU);
            let speed = self.rng.range(60.0, 220.0);
            self.particles.push(Particle {
                x: self.body.x,
                y: self.body.y - 150.0,
                vx: angle.cos() * speed,
                vy: angle.sin() * speed,
                life: self.rng.range(1.0, 1.8),
                color: match self.rng.index(4) {
                    0 => [255, 195, 70],
                    1 => [94, 234, 212],
                    2 => [248, 113, 113],
                    _ => [167, 139, 250],
                },
            });
        }
    }
    pub fn summary(&self) -> Summary {
        Summary {
            project: "MyDigitalBuddy",
            pet: self.pet.name.clone(),
            neurons: self.brain.n,
            synapses: self.brain.synapses(),
            simulated_seconds: self.time,
            emotion: self.emotion.current,
            energy: self.drives.energy,
            stimulation: self.drives.stimulation,
            contact: self.drives.contact,
            protest_stage: self.protest.stage,
            sad_stage: self.sadness.stage,
            episodes: self.memory.recent.len(),
            motor_hz: crate::core::topology::MOTORS
                .iter()
                .map(|m| (m.0.into(), self.brain.hz(m.0)))
                .collect(),
            warnings: self.warnings.clone(),
        }
    }

    pub fn import_legacy(&mut self, source: &Path) -> io::Result<String> {
        let root = if source.join("memory").is_dir() {
            source.join("memory")
        } else {
            source.to_path_buf()
        };
        let standard = root.join("pets").join(&self.settings.pet);
        let old = if standard.is_dir() {
            standard
        } else {
            root.clone()
        };
        let dest = self.pet_dir();
        if dest.exists() && fs::read_dir(&dest)?.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "target pet already has memory; choose a fresh --data-dir to import",
            ));
        }
        let names = [
            "nervous.json",
            "drives.json",
            "conditioning.json",
            "protest.json",
            "sad_protest.json",
            "castigo.json",
            "preferences.json",
            "episodes.jsonl",
        ];
        let mut candidate = Vec::new();
        for name in names {
            let legacy = old.join(name);
            let legacy = if name == "nervous.json" && !legacy.exists() {
                root.join(format!("{}_nervous.json", self.settings.pet))
            } else {
                legacy
            };
            if !legacy.exists() {
                continue;
            }
            if name == "episodes.jsonl" {
                let e = storage::read_episodes(&legacy)?;
                let mut bytes = Vec::new();
                for ep in e {
                    serde_json::to_writer(&mut bytes, &ep).map_err(io::Error::other)?;
                    bytes.push(b'\n');
                }
                candidate.push((name, bytes));
                continue;
            }
            let v: serde_json::Value = storage::read_json(&legacy)?;
            let valid = match name {
                "nervous.json" => serde_json::from_value::<BrainState>(v.clone())
                    .ok()
                    .is_some_and(|b| {
                        let mut brain = Brain::new(self.pet.seed, 1, &self.pet.temperament);
                        brain.restore(&b, storage::wall_time())
                    }),
                "drives.json" => serde_json::from_value::<Drives>(v.clone())
                    .ok()
                    .is_some_and(|d| {
                        Drives::default().restore(d, &self.profile(), storage::wall_time())
                    }),
                "castigo.json" => {
                    serde_json::from_value::<Corner>(v.clone()).is_ok_and(|c| c.validate())
                }
                "conditioning.json" => {
                    serde_json::from_value::<Conditioning>(v.clone()).is_ok_and(|c| {
                        c.values
                            .values()
                            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    })
                }
                "protest.json" | "sad_protest.json" => serde_json::from_value::<Protest>(v.clone())
                    .ok()
                    .is_some_and(|p| Protest::default().restore(p, storage::wall_time(), 3600.0)),
                "preferences.json" => serde_json::from_value::<Preferences>(v.clone()).is_ok(),
                _ => false,
            };
            if !valid {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("legacy file failed validation: {name}"),
                ));
            }
            candidate.push((
                name,
                serde_json::to_vec_pretty(&v).map_err(io::Error::other)?,
            ));
        }
        if candidate.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no compatible legacy memory found",
            ));
        }
        let staging = self.root.join(format!("import-{}", std::process::id()));
        fs::create_dir(&staging)?;
        let result = (|| {
            for (name, data) in &candidate {
                storage::atomic_bytes(&staging.join(name), data)?;
            }
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            if dest.exists() {
                fs::remove_dir(&dest)?;
            }
            fs::rename(&staging, &dest)?;
            Ok::<_, io::Error>(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result?;
        self.restore_pet();
        Ok(format!(
            "Imported {} validated files. Source preserved. NumPy and Rust random streams differ; memory values are retained.",
            candidate.len()
        ))
    }
    pub fn wipe_memory(&mut self) -> io::Result<()> {
        self.clear_things();
        self.memory = Memory::default();
        self.dialogue = Dialogue::default();
        self.drives = Drives::default();
        self.conditioning = Conditioning::default();
        self.brain = Brain::new(self.pet.seed, self.rng.0, &self.pet.temperament);
        self.protest = Protest {
            since: -1e9,
            ..Default::default()
        };
        self.sadness = Protest {
            since: -1e9,
            ..Default::default()
        };
        self.corner = Corner {
            position: self.corner.position,
            ..Default::default()
        };
        self.timeline.discard(&self.root.join("timeline.csv"))?;
        self.save()
    }
}

fn definition(content: &Path, id: &str) -> io::Result<PetDefinition> {
    if !valid_pet_id(id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid pet id",
        ));
    }
    let path = content.join("pets").join(format!("{id}.json"));
    let pet = if path.is_file() {
        storage::read_json(&path)?
    } else {
        PetDefinition::builtin(id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "pet definition not found"))?
    };
    if !pet.validate() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid pet definition",
        ));
    }
    Ok(pet)
}
