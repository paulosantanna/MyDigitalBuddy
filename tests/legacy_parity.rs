// SPDX-License-Identifier: GPL-3.0-or-later
#![allow(clippy::unwrap_used)]
use mydigitalbuddy::{
    config::Profile,
    core::{
        conditioning::Conditioning,
        emotion::{Emotion, EmotionState, classify},
        homeostasis::Drives,
        mind::Protest,
        motor::MotorTone,
        physics::{Body, Platform, Rect},
    },
};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/behavior.json")).unwrap()
}
fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}

#[test]
fn classification_boundaries_match_python() {
    for c in fixture()["classify"].as_array().unwrap() {
        assert_eq!(
            classify(f(&c["v"]), f(&c["a"])).name(),
            c["expected"].as_str().unwrap()
        );
    }
}
#[test]
fn homeostasis_matches_python_across_contexts_and_profiles() {
    for c in fixture()["drives"].as_array().unwrap() {
        let a = &c["initial"];
        let mut d = Drives {
            energy: f(&a[0]),
            stimulation: f(&a[1]),
            contact: f(&a[2]),
            saved_at: 0.0,
        };
        let p = Profile::new(c["test"].as_bool().unwrap());
        let a = &c["params"];
        for _ in 0..c["steps"].as_u64().unwrap() {
            d.update(
                f(&c["dt"]),
                &p,
                a[0].as_bool().unwrap(),
                f(&a[1]),
                f(&a[2]),
                a[3].as_bool().unwrap(),
                a[4].as_bool().unwrap(),
                a[5].as_bool().unwrap(),
                a[6].as_bool().unwrap(),
            );
        }
        for (actual, expected) in [d.energy, d.stimulation, d.contact]
            .into_iter()
            .zip(c["expected"].as_array().unwrap())
        {
            close(actual, f(expected));
        }
    }
}
#[test]
fn physical_trajectories_match_python_with_rotation_bounces_and_platforms() {
    for c in fixture()["physics"].as_array().unwrap() {
        let a = &c["initial"];
        let mut b = Body::new(f(&a[0]), f(&a[1]), 60.0, 45.0);
        b.vx = f(&a[2]);
        b.vy = f(&a[3]);
        b.angle = f(&a[4]);
        b.omega = f(&a[5]);
        let platforms: Vec<_> = c["platforms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| Platform {
                id: a[0].as_u64().unwrap(),
                left: f(&a[1]),
                right: f(&a[2]),
                top: f(&a[3]),
            })
            .collect();
        for _ in 0..c["steps"].as_u64().unwrap() {
            b.step(f(&c["dt"]), Rect::default(), &platforms);
        }
        for (actual, expected) in [b.x, b.y, b.vx, b.vy, b.angle, b.omega]
            .into_iter()
            .zip(c["expected"].as_array().unwrap())
        {
            close(actual, f(expected));
        }
        assert_eq!(b.grounded, c["grounded"].as_bool().unwrap());
        assert_eq!(b.platform, c["platform"].as_u64());
    }
}
#[test]
fn emotional_smoothing_dwell_and_override_priority_match_python() {
    let mut e = EmotionState::default();
    let p = Profile::new(false);
    for c in fixture()["emotion"].as_array().unwrap() {
        let step = c["step"].as_u64().unwrap();
        let now = step as f64 * 0.1;
        if step == 20 {
            e.force(Emotion::Sleeping, 0.8, now, 50);
        }
        if step == 21 {
            e.force(Emotion::Content, 0.5, now, 15);
        }
        e.update(
            |name| c["rates"][name].as_f64().unwrap_or(0.0),
            0.1,
            &p,
            now,
        );
        close(e.valence, f(&c["expected"][0]));
        close(e.arousal, f(&c["expected"][1]));
        assert_eq!(e.current.name(), c["current"].as_str().unwrap());
    }
}
#[test]
fn motor_tone_matches_python() {
    let mut m = MotorTone::default();
    for c in fixture()["motor"].as_array().unwrap() {
        m.update(|name| c["rates"][name].as_f64().unwrap_or(0.0), 0.1);
        for (k, v) in c["r"].as_object().unwrap() {
            close(m.get(k), f(v));
        }
        close(m.speed(), f(&c["speed"]));
        close(m.drive(), f(&c["drive"]));
        close(m.vigor(), f(&c["vigor"]));
    }
}
#[test]
fn conditioning_acquisition_and_extinction_match_python() {
    let mut c = Conditioning::default();
    for (step, want) in fixture()["conditioning"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let now = step as f64 * 3.0;
        c.set_cue("PRESS", true, now);
        if step < 8 {
            c.outcome(now + 0.1, &["PRESS", "APPROACH"]);
        }
        c.set_cue("PRESS", false, now + 0.2);
        c.tick(now + 2.0, 0.12);
        for (k, v) in want.as_object().unwrap() {
            close(c.values[k], f(v));
        }
    }
}
#[test]
fn protest_hysteresis_and_caress_relief_match_python() {
    let mut protest = Protest {
        since: -1e9,
        ..Default::default()
    };
    let p = Profile::new(true);
    for (step, want) in fixture()["protest"].as_array().unwrap().iter().enumerate() {
        protest.update(10.0, 0.9, &p, step >= 18, step as f64 * 10.0, false);
        close(protest.grievance, f(&want["grievance"]));
        assert_eq!(protest.stage as u64, want["stage"].as_u64().unwrap());
        assert_eq!(protest.revolutions, want["revolutions"].as_u64().unwrap());
    }
}
