// SPDX-License-Identifier: GPL-3.0-or-later
#![allow(clippy::unwrap_used)]
use mydigitalbuddy::{
    config::valid_pet_id,
    core::{
        memory::Memory,
        physics::{Body, Platform, Rect, ToyTracker},
        rng::Rng,
    },
    engine::Engine,
    platform::Snapshot,
    storage,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static ID: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "mydigitalbuddy-test-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn panic_remains_disabled_after_restart_and_pauses_clock() {
    let dir = Temp::new();
    let mut e = Engine::load(dir.0.clone(), 3).unwrap();
    e.advance(0.05, &Snapshot::default());
    let t = e.time;
    e.panic();
    e.advance(0.05, &Snapshot::default());
    assert_eq!(e.time, t);
    drop(e);
    let e = Engine::load(dir.0.clone(), 4).unwrap();
    assert!(!e.settings.active);
}
#[test]
fn pause_and_fullscreen_freeze_simulation() {
    let dir = Temp::new();
    let mut e = Engine::load(dir.0.clone(), 7).unwrap();
    e.paused = true;
    e.advance(0.05, &Snapshot::default());
    assert_eq!(e.time, 0.0);
    e.paused = false;
    e.advance(
        0.05,
        &Snapshot {
            fullscreen: true,
            ..Default::default()
        },
    );
    assert_eq!(e.time, 0.0);
    e.settings.show_fullscreen = true;
    e.advance(
        0.05,
        &Snapshot {
            fullscreen: true,
            ..Default::default()
        },
    );
    assert_eq!(e.time, 0.05);
}
#[test]
fn only_one_process_may_write_a_data_directory() {
    let d = Temp::new();
    let first = Engine::load(d.0.clone(), 1).unwrap();
    assert!(Engine::load(d.0.clone(), 2).is_err());
    drop(first);
    assert!(Engine::load(d.0.clone(), 3).is_ok());
}
#[test]
fn bia_and_sophia_are_builtin_characters() {
    let d = Temp::new();
    let mut e = Engine::load(d.0.clone(), 1).unwrap();
    let pets = e.available_pets();
    assert!(pets.contains(&"bia".to_string()));
    assert!(pets.contains(&"sophia".to_string()));
    e.switch_pet("bia").unwrap();
    assert_eq!(e.pet.name, "Bia");
    assert_eq!(
        e.pet.appearance.get("olho").and_then(|v| v.as_str()),
        Some("#ff4d9a")
    );
    assert_eq!(
        e.pet.appearance.get("moldura").and_then(|v| v.as_str()),
        Some("#b4236e")
    );
    e.switch_pet("sophia").unwrap();
    assert_eq!(e.pet.name, "Sophia");
    assert_eq!(
        e.pet.appearance.get("olho").and_then(|v| v.as_str()),
        Some("#c084fc")
    );
    assert_eq!(
        e.pet.appearance.get("moldura").and_then(|v| v.as_str()),
        Some("#7c3aed")
    );
}
#[test]
fn pet_memories_are_isolated_and_survive_switches() {
    let d = Temp::new();
    let mut e = Engine::load(d.0.clone(), 1).unwrap();
    e.drives.contact = 0.8;
    e.save().unwrap();
    e.switch_pet("jorge").unwrap();
    assert_eq!(e.pet.name, "Jorge");
    assert!(e.drives.contact < 0.01);
    e.drives.contact = 0.2;
    e.switch_pet("bryan").unwrap();
    assert!(e.drives.contact >= 0.8);
    assert!(d.0.join("pets/jorge/nervous.json").exists());
}
#[test]
fn invalid_ids_cannot_escape_content_or_memory_root() {
    for id in ["../abc", "a/b", "a\\b", "..", "", "a.b", "/tmp", "C:"] {
        assert!(!valid_pet_id(id));
    }
    assert!(valid_pet_id("my_pet-42"));
}
#[test]
fn malformed_state_does_not_crash_or_partially_restore() {
    let d = Temp::new();
    fs::create_dir_all(d.0.join("pets/bryan")).unwrap();
    fs::write(d.0.join("permissions.json"), "{broken").unwrap();
    fs::write(
        d.0.join("pets/bryan/drives.json"),
        r#"{"energy":0.4,"stimulation":-9,"contact":0.7}"#,
    )
    .unwrap();
    let e = Engine::load(d.0.clone(), 2).unwrap();
    assert_eq!(e.drives.energy, 1.0);
    assert!(e.warnings.len() >= 2);
}
#[test]
fn punishment_revises_one_episode_and_sleep_does_not_double_count() {
    let mut m = Memory::default();
    m.start(
        "PROGRAMMING|curioso".into(),
        "bilhete".into(),
        Some(8),
        0.0,
        10.0,
    );
    m.tick(5.0, 4.0, 3.0);
    assert_eq!(m.recent.len(), 1);
    assert!(m.recent[0].valence > 0.0);
    assert_eq!(m.punish(6.0, 20.0).as_deref(), Some("bilhete"));
    assert_eq!(m.recent.len(), 1);
    assert_eq!(m.recent[0].valence, -0.8);
    assert!(m.punish(7.0, 20.0).is_none());
    m.consolidate();
    assert_eq!(m.preferences.totals["bilhete"], [1.0, -0.8]);
}
#[test]
fn action_selection_never_escapes_permission_filtered_options() {
    let mut m = Memory::default();
    let mut rng = Rng(4);
    for i in 0..100 {
        let id = m.start("A|B".into(), "meme".into(), None, i as f64, 1.0);
        assert!(!id.is_empty());
        m.tick(i as f64 + 2.0, 1.0, 1.0);
    }
    for _ in 0..100 {
        assert_eq!(
            m.choose("A|B", &["nada".into()], 0.8, &mut rng),
            Some("nada".into())
        );
    }
    assert!(m.choose("A|B", &[], 0.0, &mut rng).is_none());
}
#[test]
fn import_preserves_originals_and_refuses_existing_destination() {
    let source = Temp::new();
    let target = Temp::new();
    let mut old = Engine::load(source.0.clone(), 1).unwrap();
    old.drives.contact = 0.75;
    old.save().unwrap();
    drop(old);
    let bytes = fs::read(source.0.join("pets/bryan/nervous.json")).unwrap();
    let mut new = Engine::load(target.0.clone(), 2).unwrap();
    assert!(
        new.import_legacy(&source.0)
            .unwrap()
            .contains("8 validated")
    );
    assert!(new.drives.contact >= 0.75);
    assert_eq!(
        bytes,
        fs::read(source.0.join("pets/bryan/nervous.json")).unwrap()
    );
    assert!(new.import_legacy(&source.0).is_err());
}
#[test]
fn invalid_import_is_rejected_before_any_destination_file_is_written() {
    let source = Temp::new();
    let target = Temp::new();
    fs::write(
        source.0.join("drives.json"),
        r#"{"energy":1,"stimulation":0.5,"contact":0.5,"saved_at":1}"#,
    )
    .unwrap();
    fs::write(source.0.join("castigo.json"), r#"{"rancor":7,"receio":0}"#).unwrap();
    let mut e = Engine::load(target.0.clone(), 3).unwrap();
    assert!(e.import_legacy(&source.0).is_err());
    assert!(!target.0.join("pets/bryan").exists());
}
#[test]
fn landing_surface_disappears_and_close_buttons_are_avoided() {
    let p = Platform {
        id: 42,
        left: 200.0,
        right: 800.0,
        top: 400.0,
    };
    let mut b = Body::new(500.0, 350.0, 60.0, 45.0);
    b.vy = 100.0;
    for _ in 0..60 {
        b.step(1.0 / 60.0, Rect::default(), &[p]);
    }
    assert_eq!(b.platform, Some(42));
    b.step(1.0 / 60.0, Rect::default(), &[]);
    assert!(!b.grounded);
    let mut close = Body::new(750.0, 350.0, 60.0, 45.0);
    close.vy = 100.0;
    for _ in 0..30 {
        close.step(1.0 / 60.0, Rect::default(), &[p]);
    }
    assert_ne!(close.platform, Some(42));
}
#[test]
fn taking_ball_fires_once_until_it_returns() {
    let mut t = ToyTracker::default();
    assert!(t.update((1000.0, 0.0), (0.0, 0.0), true).0);
    assert!(!t.update((1000.0, 0.0), (0.0, 0.0), true).0);
    t.update((0.0, 0.0), (0.0, 0.0), false);
    assert!(t.update((1000.0, 0.0), (0.0, 0.0), true).0);
}
#[test]
fn random_interactions_preserve_physical_and_neural_invariants() {
    let d = Temp::new();
    let mut e = Engine::load(d.0.clone(), 17).unwrap();
    e.settings.test_mode = true;
    e.settings.mischief = true;
    let mut rng = Rng(96);
    for frame in 0..1200 {
        match frame % 120 {
            0 => e.poke(),
            30 => e.caress(1.0),
            60 => {
                e.drag_start();
                e.body.x = rng.range(100.0, 1800.0);
                e.body.y = rng.range(100.0, 900.0);
                e.drag_release(rng.range(-1500.0, 1500.0), rng.range(-1500.0, 1500.0));
            }
            90 => e.request_mischief(),
            _ => {}
        }
        let snapshot = Snapshot {
            app: "code".into(),
            idle_seconds: Some(0.1),
            cursor: (rng.range(100.0, 1800.0), rng.range(100.0, 900.0)),
            ..Default::default()
        };
        e.advance(1.0 / 60.0, &snapshot);
        assert!(
            [
                e.body.x,
                e.body.y,
                e.body.vx,
                e.body.vy,
                e.body.angle,
                e.emotion.valence,
                e.emotion.arousal
            ]
            .iter()
            .all(|v| v.is_finite())
        );
        assert!((0.0..=1.0).contains(&e.drives.energy));
        assert!(e.brain.rate.iter().all(|v| v.is_finite() && *v >= 0.0));
    }
    e.save().unwrap();
}
#[test]
fn atomic_write_leaves_valid_complete_json_and_no_temporary_files() {
    let d = Temp::new();
    let path = d.0.join("state.json");
    for i in 0..10 {
        storage::write_json(&path, &vec![i; 1000]).unwrap();
        let v: Vec<i32> = storage::read_json(&path).unwrap();
        assert_eq!(v, vec![i; 1000]);
    }
    assert_eq!(fs::read_dir(&d.0).unwrap().count(), 1);
}

#[test]
fn custom_pet_definition_is_loaded_again_after_restart() {
    let d = Temp::new();
    let content = Temp::new();
    fs::create_dir(content.0.join("pets")).unwrap();
    fs::write(content.0.join("pets/custom.json"),r#"{"nome":"Custom","seed":239,"temperamento":{},"brincadeira":{},"reacao_notebook":"triste"}"#).unwrap();
    let mut e = Engine::load_with_content(d.0.clone(), 4, Some(content.0.clone())).unwrap();
    e.switch_pet("custom").unwrap();
    assert_eq!(e.brain.seed, 239);
    drop(e);
    let e = Engine::load_with_content(d.0.clone(), 9, Some(content.0.clone())).unwrap();
    assert_eq!(e.settings.pet, "custom");
    assert_eq!(e.pet.name, "Custom");
    assert_eq!(e.brain.seed, 239);
}

#[test]
fn selecting_a_pet_for_import_does_not_create_destination_memory() {
    let d = Temp::new();
    let mut e = Engine::load(d.0.clone(), 1).unwrap();
    e.select_pet_for_import("jorge").unwrap();
    assert_eq!(e.pet.name, "Jorge");
    assert!(!d.0.join("pets").exists());
}

#[test]
fn a_seated_pet_stays_at_the_notebook_instead_of_falling_to_the_floor() {
    let d = Temp::new();
    let mut e = Engine::load(d.0.clone(), 1).unwrap();
    e.desk.seated = true;
    e.desk.position = (500.0, 700.0);
    e.advance(0.016, &Snapshot::default());
    assert_eq!(e.body.x, 580.0);
    assert_eq!(e.body.y, 695.0);
}

#[test]
fn each_pet_keeps_its_own_chat() {
    let d = Temp::new();
    let mut e = Engine::load(d.0.clone(), 1).unwrap();
    assert!(e.dialogue.push_user("segredo do bryan"));
    assert!(e.dialogue.push_assistant("lembrei do bryan"));
    assert!(e.dialogue.reinforce_last());
    e.save().unwrap();
    e.switch_pet("bia").unwrap();
    assert!(e.dialogue.turns.is_empty());
    assert!(e.dialogue.push_user("segredo da bia"));
    e.save().unwrap();
    e.switch_pet("bryan").unwrap();
    assert!(
        e.dialogue
            .turns
            .iter()
            .any(|turn| turn.text.contains("bryan"))
    );
    assert!(
        !e.dialogue
            .turns
            .iter()
            .any(|turn| turn.text.contains("bia"))
    );
    assert_eq!(e.dialogue.reinforced.len(), 1);
    e.switch_pet("bia").unwrap();
    assert!(
        e.dialogue
            .turns
            .iter()
            .any(|turn| turn.text.contains("bia"))
    );
    assert!(e.dialogue.reinforced.is_empty());
}
