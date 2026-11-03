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

#[test]
fn conversation_replies_are_learned_and_stay_on_that_pet() {
    let dir = Temp::new();
    let mut sophia = Engine::load(dir.0.clone(), 11).unwrap();
    sophia.switch_pet("sophia").unwrap();
    sophia.settings.assistant = true;
    sophia.settings.cloud_deepseek = false;
    sophia
        .dialogue
        .import_text(
            "bolo.txt",
            "A receita de bolo de laranja leva calda. O forno fica quente.",
        )
        .unwrap();

    assert!(sophia.dialogue.push_user("Olá, tudo bem?"));
    let stimulation = sophia.drives.stimulation;
    let social = mydigitalbuddy::core::topology::SENSORS
        .iter()
        .position(|sensor| sensor.0 == "SOCIAL")
        .unwrap();
    let weights_before = sophia.brain.learned[social];
    sophia.notice_conversation("Olá, tudo bem?");
    assert!(sophia.drives.stimulation > stimulation);
    assert!(
        sophia
            .memory
            .recent
            .iter()
            .any(|episode| episode.context == "conversa"
                && episode.action == "ouvir"
                && episode.result.contains("Olá"))
    );
    let lived = sophia.lived_context();
    assert!(lived.contains("conversa → ouvir"));
    let prompt = sophia
        .dialogue
        .messages("sophia", "Sophia", "curioso", "", &lived)
        .into_iter()
        .map(|message| message.content)
        .collect::<String>();
    assert!(prompt.contains("Olá, tudo bem?"));
    assert!(prompt.contains("conversa → ouvir"));

    for _ in 0..50 {
        sophia.advance(0.05, &Snapshot::default());
    }
    assert!(sophia.time >= 2.0);
    let weights_after = sophia.brain.learned[social];
    assert_ne!(
        weights_before, weights_after,
        "a conversa não moveu os pesos sociais"
    );
    for _ in 0..20 {
        sophia.advance(0.05, &Snapshot::default());
    }
    assert_eq!(sophia.brain.learned[social], weights_after);

    let missing_model = "não encontrei o GGUF. O recomendado nesta placa é Qwen2.5-7B-Instruct Q4_K_M, em models/qwen.gguf ao lado do programa";
    assert!(mydigitalbuddy::dialogue::should_answer_from_memory(
        missing_model,
        true,
        false
    ));
    assert!(mydigitalbuddy::dialogue::should_answer_from_memory(
        missing_model,
        true,
        true
    ));
    assert!(mydigitalbuddy::dialogue::should_answer_from_memory(
        "a NVIDIA recusou a autenticação",
        true,
        true
    ));
    assert!(!mydigitalbuddy::dialogue::should_answer_from_memory(
        "interrompido",
        true,
        true
    ));
    let greeting = mydigitalbuddy::dialogue::reply_from_memory(
        &sophia.dialogue,
        "sophia",
        "Sophia",
        &sophia.lived_context(),
    );
    assert!(
        greeting.to_lowercase().contains("olá") || greeting.to_lowercase().contains("tudo bem"),
        "{greeting}"
    );
    assert!(sophia.dialogue.push_assistant(&greeting));
    assert!(sophia.dialogue.reinforce_last());

    let nvidia = serde_json::json!({
        "choices": [{
            "message": {
                "content": "",
                "reasoning_content": "<think>hmm</think>\no bolo de laranja leva calda<｜end▁of▁sentence｜>"
            }
        }]
    });
    let from_nvidia = mydigitalbuddy::dialogue::extract_reply(&nvidia).unwrap();
    assert_eq!(from_nvidia, "o bolo de laranja leva calda");
    assert!(sophia.dialogue.push_user("me fala do bolo de laranja"));
    sophia.notice_conversation("me fala do bolo de laranja");
    assert!(sophia.dialogue.push_assistant(&from_nvidia));
    assert!(sophia.dialogue.reinforce_last());
    sophia.save().unwrap();
    let saved_weights = sophia.brain.learned[social];
    drop(sophia);

    let chat = fs::read_to_string(dir.0.join("pets/sophia/chat.jsonl")).unwrap();
    assert!(chat.contains("\"role\":\"assistant\""));
    assert!(chat.contains("bolo de laranja leva calda"));
    let episodes = fs::read_to_string(dir.0.join("pets/sophia/episodes.jsonl")).unwrap();
    assert!(episodes.contains("conversa"));

    let mut sophia = Engine::load(dir.0.clone(), 12).unwrap();
    assert_eq!(sophia.settings.pet, "sophia");
    assert!(
        !sophia
            .warnings
            .iter()
            .any(|warning| warning.contains("nervous.json"))
    );
    assert_eq!(sophia.brain.learned[social], saved_weights);
    assert!(sophia.dialogue.push_user("quero de novo o bolo de laranja"));
    let remembered = mydigitalbuddy::dialogue::reply_from_memory(
        &sophia.dialogue,
        "sophia",
        "Sophia",
        &sophia.lived_context(),
    );
    assert!(
        remembered.contains("calda"),
        "a resposta seguinte não usou o que foi aprendido: {remembered}"
    );
    let again = sophia
        .dialogue
        .messages("sophia", "Sophia", "curioso", "", &sophia.lived_context())
        .into_iter()
        .map(|message| message.content)
        .collect::<String>();
    assert!(again.contains("bolo de laranja leva calda"));
    assert!(again.contains("conversa → ouvir"));

    sophia.switch_pet("bia").unwrap();
    assert!(sophia.dialogue.turns.is_empty());
    assert!(sophia.dialogue.reinforced.is_empty());
    assert!(!sophia.lived_context().contains("bolo"));
    assert!(sophia.dialogue.push_user("quero o bolo de laranja"));
    let bia = mydigitalbuddy::dialogue::reply_from_memory(
        &sophia.dialogue,
        "bia",
        "Bia",
        &sophia.lived_context(),
    );
    assert!(
        !bia.contains("calda"),
        "a memória da Sophia vazou para a Bia: {bia}"
    );
}

#[cfg(feature = "desktop")]
#[test]
fn local_worker_reports_the_missing_model_without_calling_nvidia() {
    use mydigitalbuddy::{
        assistant::{Runtime, TurnRequest},
        config::Settings,
        dialogue::PromptMessage,
    };
    use std::{
        thread,
        time::{Duration, Instant},
    };

    let dir = Temp::new();
    let mut runtime = Runtime::new();
    runtime.start_reply(TurnRequest {
        pet: "sophia".into(),
        messages: vec![PromptMessage {
            role: "user",
            content: "Olá, tudo bem?".into(),
        }],
        settings: Settings {
            assistant: true,
            cloud_deepseek: false,
            model_path: String::new(),
            llama_server: String::new(),
            nvidia_key_file: dir.0.join("chave-ausente").display().to_string(),
            ..Settings::default()
        },
        content_root: dir.0.clone(),
        data_root: dir.0.clone(),
    });
    assert!(runtime.busy());
    let started = Instant::now();
    let event = loop {
        if let Some(event) = runtime.poll() {
            break event;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "o pedido local não terminou"
        );
        thread::sleep(Duration::from_millis(20));
    };
    let mydigitalbuddy::assistant::Event::Reply { pet, text } = event else {
        panic!("o worker local devolveu outro tipo de evento");
    };
    assert_eq!(pet, "sophia");
    let error = text.expect_err("sem GGUF o modelo local não responde");
    assert!(
        mydigitalbuddy::dialogue::should_answer_from_memory(&error, true, false),
        "{error}"
    );
    assert!(
        mydigitalbuddy::dialogue::should_answer_from_memory(&error, true, true),
        "com as duas permissões, a falha do modelo ainda precisa de uma fala: {error}"
    );
}

#[test]
fn both_permissions_store_a_visible_reply_and_keep_a_model_answer() {
    use mydigitalbuddy::dialogue::{Spoken, speak_after};
    let dir = Temp::new();
    let mut sophia = Engine::load(dir.0.clone(), 21).unwrap();
    sophia.switch_pet("sophia").unwrap();
    sophia.settings.assistant = true;
    sophia.settings.cloud_deepseek = true;
    assert!(sophia.dialogue.push_user("Oi, como vai?"));
    sophia.notice_conversation("Oi, como vai?");
    let lived = sophia.lived_context();
    let spoken = speak_after(
        Err("a NVIDIA respondeu 429: ocupada".into()),
        true,
        true,
        &sophia.dialogue,
        "sophia",
        "Sophia",
        &lived,
    );
    let Spoken::Memory(answer) = spoken else {
        panic!("as duas permissões ficaram sem fala: {spoken:?}");
    };
    assert!(!answer.trim().is_empty(), "{answer}");
    assert!(sophia.dialogue.push_assistant(&answer));
    assert!(sophia.dialogue.reinforce_last());
    sophia.save().unwrap();
    drop(sophia);

    let chat = fs::read_to_string(dir.0.join("pets/sophia/chat.jsonl")).unwrap();
    assert!(chat.contains("\"role\":\"assistant\""), "{chat}");
    assert!(chat.contains("\"role\":\"user\""), "{chat}");

    let mut sophia = Engine::load(dir.0.clone(), 22).unwrap();
    assert!(
        sophia
            .dialogue
            .turns
            .iter()
            .any(|turn| turn.role == mydigitalbuddy::dialogue::Role::Assistant)
    );
    assert!(sophia.dialogue.push_user("e agora"));
    let model = speak_after(
        Ok("Oi! Tudo bem, sim.".into()),
        sophia.settings.assistant,
        sophia.settings.cloud_deepseek,
        &sophia.dialogue,
        "sophia",
        "Sophia",
        &sophia.lived_context(),
    );
    assert_eq!(model, Spoken::Model("Oi! Tudo bem, sim.".into()));
}
