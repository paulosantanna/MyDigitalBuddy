// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{core::emotion::Emotion, engine::Engine};
use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2};

fn color(engine: &Engine, key: &str, default: Color32) -> Color32 {
    engine
        .pet
        .appearance
        .get(key)
        .and_then(|v| v.as_str())
        .and_then(|s| Color32::from_hex(s).ok())
        .unwrap_or(default)
}
fn transform(center: Pos2, x: f32, y: f32, angle: f32, scale: f32) -> Pos2 {
    let (c, s) = (angle.cos(), angle.sin());
    center + Vec2::new(x * c - y * s, x * s + y * c) * scale
}
fn rounded(center: Pos2, rect: Rect, r: f32, angle: f32, scale: f32) -> Vec<Pos2> {
    let mut points = Vec::new();
    for (cx, cy, start) in [
        (rect.right() - r, rect.bottom() - r, 0.0),
        (rect.left() + r, rect.bottom() - r, 90.0),
        (rect.left() + r, rect.top() + r, 180.0),
        (rect.right() - r, rect.top() + r, 270.0),
    ] {
        for k in 0..=8 {
            let a = (start + k as f32 * 90.0 / 8.0).to_radians();
            points.push(transform(
                center,
                cx + r * a.cos(),
                cy + r * a.sin(),
                angle,
                scale,
            ));
        }
    }
    points
}
fn polygon(p: &Painter, points: Vec<Pos2>, fill: Color32, stroke: Stroke) {
    p.add(Shape::convex_polygon(points, fill, stroke));
}

pub fn buddy(p: &Painter, center: Pos2, angle_deg: f32, scale: f32, engine: &Engine) {
    let angle = angle_deg.to_radians();
    let frame = color(engine, "moldura", Color32::from_rgb(79, 95, 113));
    let shell = color(engine, "casca", Color32::from_rgb(22, 30, 42));
    let screen = color(engine, "tela", Color32::from_rgb(9, 19, 25));
    let eye = color(engine, "olho", Color32::from_rgb(102, 245, 215));
    let shake = (engine.time * 50.0).sin() as f32 * engine.tone.get("ANGRY") as f32 * 2.5;
    let center = center + Vec2::new(shake, 0.0);
    p.add(Shape::ellipse_filled(
        center + Vec2::new(0.0, 54.0 * scale),
        Vec2::new(57.0, 6.0) * scale,
        Color32::from_black_alpha(45),
    ));
    polygon(
        p,
        rounded(
            center,
            Rect::from_min_max(Pos2::new(-62.0, -47.0), Pos2::new(62.0, 47.0)),
            16.0,
            angle,
            scale,
        ),
        frame,
        Stroke::NONE,
    );
    polygon(
        p,
        rounded(
            center,
            Rect::from_min_max(Pos2::new(-58.0, -43.0), Pos2::new(58.0, 43.0)),
            13.0,
            angle,
            scale,
        ),
        shell,
        Stroke::NONE,
    );
    polygon(
        p,
        rounded(
            center,
            Rect::from_min_max(Pos2::new(-51.0, -36.0), Pos2::new(51.0, 33.0)),
            11.0,
            angle,
            scale,
        ),
        screen,
        Stroke::NONE,
    );
    p.circle_filled(transform(center, 0.0, 39.0, angle, scale), 1.8 * scale, eye);
    let emotion = if engine.caressing() {
        Emotion::Content
    } else if engine.asleep {
        Emotion::Sleeping
    } else {
        engine.emotion.current
    };
    let (w, h, inner, outer, smile, dy) = match emotion {
        Emotion::Watching => (30.0, 40.0, 0.14, 0.0, 0.0, 0.0),
        Emotion::Content => (32.0, 34.0, 0.0, 0.0, 0.36, 1.0),
        Emotion::Happy => (34.0, 36.0, 0.0, 0.0, 0.55, -2.0),
        Emotion::Curious => (30.0, 46.0, 0.0, 0.08, 0.0, -2.0),
        Emotion::Angry => (34.0, 36.0, 0.55, 0.0, 0.0, 2.0),
        Emotion::Sad => (28.0, 34.0, 0.0, 0.5, 0.0, 7.0),
        Emotion::Startled => (26.0, 50.0, 0.0, 0.0, 0.0, -3.0),
        Emotion::Dramatic => (38.0, 52.0, 0.0, 0.22, 0.0, 0.0),
        Emotion::Sleeping => (32.0, 4.0, 0.0, 0.0, 0.0, 8.0),
    };
    let blink = if !engine.asleep && engine.time > 0.2 && engine.time.rem_euclid(4.3) < 0.13 {
        0.12
    } else {
        1.0
    };
    let droop = engine.tone.get("DROWSE") as f32 * 0.45;
    for side in [-1.0_f32, 1.0] {
        let cx = side * 24.0 + engine.gaze.0 as f32 * 4.0;
        let cy = dy + engine.gaze.1 as f32 * 3.0;
        let eh = (h * blink * (1.0 - droop)).max(3.0);
        polygon(
            p,
            rounded(
                center,
                Rect::from_center_size(Pos2::new(cx, cy), Vec2::new(w, eh)),
                (w / 2.0).min(eh / 2.0),
                angle,
                scale,
            ),
            eye,
            Stroke::NONE,
        );
        if inner > 0.0 || outer > 0.0 {
            let left = if side < 0.0 { outer } else { inner };
            let right = if side < 0.0 { inner } else { outer };
            polygon(
                p,
                vec![
                    transform(
                        center,
                        cx - w / 2.0 - 1.0,
                        cy - eh / 2.0 - 1.0,
                        angle,
                        scale,
                    ),
                    transform(
                        center,
                        cx + w / 2.0 + 1.0,
                        cy - eh / 2.0 - 1.0,
                        angle,
                        scale,
                    ),
                    transform(
                        center,
                        cx + w / 2.0 + 1.0,
                        cy - eh / 2.0 + right * eh,
                        angle,
                        scale,
                    ),
                    transform(
                        center,
                        cx - w / 2.0 - 1.0,
                        cy - eh / 2.0 + left * eh,
                        angle,
                        scale,
                    ),
                ],
                screen,
                Stroke::NONE,
            );
        }
        if smile > 0.0 {
            let mut points = vec![transform(
                center,
                cx - w / 2.0 - 1.0,
                cy + eh / 2.0 + 1.0,
                angle,
                scale,
            )];
            for k in 0..=20 {
                let t = k as f32 / 20.0;
                points.push(transform(
                    center,
                    cx - w / 2.0 + t * w,
                    cy + eh / 2.0 - smile * eh * (std::f32::consts::PI * t).sin(),
                    angle,
                    scale,
                ));
            }
            points.push(transform(
                center,
                cx + w / 2.0 + 1.0,
                cy + eh / 2.0 + 1.0,
                angle,
                scale,
            ));
            p.add(Shape::Path(egui::epaint::PathShape {
                points,
                closed: true,
                fill: screen,
                stroke: Stroke::NONE.into(),
            }));
        }
    }
    if engine
        .pet
        .appearance
        .get("acessorios")
        .and_then(|v| v.get("gravata"))
        .is_some()
    {
        polygon(
            p,
            vec![
                transform(center, -6.0, 44.0, angle, scale),
                transform(center, 0.0, 40.0, angle, scale),
                transform(center, 6.0, 44.0, angle, scale),
                transform(center, 8.0, 61.0, angle, scale),
                transform(center, 0.0, 67.0, angle, scale),
                transform(center, -8.0, 61.0, angle, scale),
            ],
            Color32::from_rgb(179, 38, 30),
            Stroke::new(1.0, Color32::BLACK),
        );
    }
    if engine.asleep {
        for k in 0..3 {
            let phase = ((engine.time / 2.4 + k as f64 / 3.0).fract()) as f32;
            let pos = center + Vec2::new(50.0 + phase * 20.0, -45.0 - phase * 45.0) * scale;
            p.text(
                pos,
                Align2::CENTER_CENTER,
                "z",
                FontId::proportional((12.0 + phase * 10.0) * scale),
                Color32::from_rgba_unmultiplied(160, 224, 235, ((1.0 - phase) * 220.0) as u8),
            );
        }
    }
}
pub fn ball(p: &Painter, c: Pos2, angle: f32) {
    p.circle_filled(c, 18.0, Color32::from_rgb(184, 215, 48));
    p.circle_stroke(c, 17.0, Stroke::new(2.0, Color32::from_rgb(84, 115, 30)));
    let a = angle.to_radians();
    for side in [-1.0, 1.0] {
        let points = (0..=20)
            .map(|k| {
                let t = k as f32 / 20.0;
                transform(
                    c,
                    side * (4.0 + 9.0 * (std::f32::consts::PI * t).sin()),
                    -15.0 + 30.0 * t,
                    a,
                    1.0,
                )
            })
            .collect();
        p.add(Shape::line(
            points,
            Stroke::new(2.0, Color32::from_rgb(240, 243, 202)),
        ));
    }
}
pub fn notebook(p: &Painter, working: bool, time: f32) {
    let screen = Rect::from_min_max(Pos2::new(15.0, 12.0), Pos2::new(145.0, 83.0));
    p.rect_filled(screen, 6.0, Color32::from_rgb(61, 71, 86));
    p.rect_filled(screen.shrink(5.0), 3.0, Color32::from_rgb(10, 23, 35));
    for k in 0..5 {
        let y = 27.0 + k as f32 * 9.0;
        let w = if working {
            30.0 + ((time * 2.0 + k as f32).sin() + 1.0) * 30.0
        } else {
            45.0
        };
        p.line_segment(
            [Pos2::new(28.0, y), Pos2::new(28.0 + w, y)],
            Stroke::new(
                2.0,
                if k % 2 == 0 {
                    Color32::from_rgb(94, 234, 212)
                } else {
                    Color32::from_rgb(167, 139, 250)
                },
            ),
        );
    }
    polygon(
        p,
        vec![
            Pos2::new(15.0, 83.0),
            Pos2::new(145.0, 83.0),
            Pos2::new(157.0, 100.0),
            Pos2::new(3.0, 100.0),
        ],
        Color32::from_rgb(116, 130, 148),
        Stroke::new(2.0, Color32::from_rgb(41, 49, 63)),
    );
    p.rect_filled(
        Rect::from_min_max(Pos2::new(57.0, 88.0), Pos2::new(103.0, 96.0)),
        2.0,
        Color32::from_rgb(73, 85, 102),
    );
}
pub fn corner(p: &Painter, inside: bool) {
    for (x, sign) in [(44.0, -1.0), (146.0, 1.0)] {
        p.line_segment(
            [Pos2::new(x, 104.0), Pos2::new(x + sign * 11.0, 146.0)],
            Stroke::new(5.0, Color32::from_rgb(63, 49, 37)),
        );
    }
    let r = Rect::from_min_max(Pos2::new(8.0, 8.0), Pos2::new(182.0, 108.0));
    p.rect_filled(
        r.translate(Vec2::new(4.0, 5.0)),
        12.0,
        Color32::from_black_alpha(60),
    );
    p.rect_filled(r, 12.0, Color32::from_rgb(255, 216, 77));
    p.rect_stroke(
        r,
        12.0,
        Stroke::new(4.0, Color32::from_rgb(63, 49, 37)),
        egui::StrokeKind::Inside,
    );
    p.text(
        r.center(),
        Align2::CENTER_CENTER,
        if inside {
            "CANTINHO\nOCUPADO"
        } else {
            "CANTINHO\nDO CASTIGO"
        },
        FontId::proportional(20.0),
        Color32::from_rgb(159, 46, 29),
    );
}
