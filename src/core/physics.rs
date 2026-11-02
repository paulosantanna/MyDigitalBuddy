// SPDX-License-Identifier: GPL-3.0-or-later
use serde::{Deserialize, Serialize};
pub const GRAVITY: f64 = 2200.0;
pub const RESTITUTION: f64 = 0.35;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}
impl Default for Rect {
    fn default() -> Self {
        Self {
            left: 0.0,
            top: 0.0,
            right: 1920.0,
            bottom: 1040.0,
        }
    }
}
impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }
    pub fn width(self) -> f64 {
        self.right - self.left
    }
    pub fn height(self) -> f64 {
        self.bottom - self.top
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Platform {
    pub id: u64,
    pub left: f64,
    pub right: f64,
    pub top: f64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Body {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub angle: f64,
    pub omega: f64,
    pub half_w: f64,
    pub half_h: f64,
    pub grounded: bool,
    pub platform: Option<u64>,
    pub surface: f64,
    pub held: bool,
    #[serde(skip)]
    pub impacts: Vec<f64>,
}
pub fn wrap_angle(v: f64) -> f64 {
    (v + 180.0).rem_euclid(360.0) - 180.0
}
impl Body {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            angle: 0.0,
            omega: 0.0,
            half_w: w,
            half_h: h,
            grounded: false,
            platform: None,
            surface: 0.0,
            held: false,
            impacts: Vec::new(),
        }
    }
    pub fn extents(&self) -> (f64, f64) {
        let a = self.angle.to_radians();
        let (c, s) = (a.cos().abs(), a.sin().abs());
        (
            self.half_w * c + self.half_h * s,
            self.half_w * s + self.half_h * c,
        )
    }
    pub fn upright(&self) -> bool {
        wrap_angle(self.angle).abs() < 30.0
    }
    pub fn jump(&mut self, vx: f64, vy: f64, spin: f64) {
        self.grounded = false;
        self.platform = None;
        self.vx = vx;
        self.vy = vy;
        self.omega += spin;
    }
    pub fn step(&mut self, dt: f64, bounds: Rect, platforms: &[Platform]) {
        if self.held || dt <= 0.0 || !dt.is_finite() {
            return;
        }
        if !self.grounded {
            self.vy += GRAVITY * dt;
            self.vx -= self.vx * 0.15 * dt;
            self.omega -= self.omega * 0.8 * dt;
        } else {
            self.vx -= self.vx * 6.0 * dt;
            let target = (self.angle / 90.0).round_ties_even() * 90.0;
            self.omega += ((target - self.angle) * 60.0 - self.omega * 9.0) * dt;
        }
        self.omega = self.omega.clamp(-1000.0, 1000.0);
        self.angle += self.omega * dt;
        if self.angle.abs() > 3600.0 {
            self.angle = wrap_angle(self.angle);
        }
        let (ex, ey) = self.extents();
        let prev_bottom = self.y + ey;
        self.x += self.vx * dt;
        self.y += self.vy * dt;
        if self.x - ex < bounds.left {
            self.x = bounds.left + ex;
            self.impacts.push(self.vx.abs());
            self.vx = self.vx.abs() * RESTITUTION;
            self.omega += self.vy * 0.15;
        } else if self.x + ex > bounds.right {
            self.x = bounds.right - ex;
            self.impacts.push(self.vx.abs());
            self.vx = -self.vx.abs() * RESTITUTION;
            self.omega -= self.vy * 0.15;
        }
        if self.y - ey < bounds.top {
            self.y = bounds.top + ey;
            self.impacts.push(self.vy.abs());
            self.vy = self.vy.abs() * RESTITUTION;
        }
        if self.grounded {
            let mut surface = bounds.bottom;
            if let Some(id) = self.platform {
                if let Some(p) = platforms.iter().find(|p| {
                    p.id == id
                        && p.left <= self.x
                        && self.x <= p.right - 150.0
                        && (p.top - self.surface).abs() <= 4.0
                }) {
                    surface = p.top;
                } else {
                    self.grounded = false;
                    self.platform = None;
                }
            }
            if self.grounded {
                self.surface = surface;
                self.y = surface - ey;
                self.vy = 0.0;
                return;
            }
        }
        let new_bottom = self.y + ey;
        if self.vy > 0.0
            && let Some(p) = platforms
                .iter()
                .filter(|p| {
                    p.left <= self.x
                        && self.x <= p.right - 150.0
                        && prev_bottom <= p.top + 2.0
                        && p.top + 2.0 <= new_bottom + 2.0
                        && new_bottom >= p.top
                })
                .min_by(|a, b| a.top.total_cmp(&b.top))
        {
            self.land(p.top, ey, Some(p.id));
            return;
        }
        if self.y + ey >= bounds.bottom {
            self.land(bounds.bottom, ey, None);
        }
    }
    fn land(&mut self, surface: f64, ey: f64, id: Option<u64>) {
        self.y = surface - ey;
        self.surface = surface;
        self.impacts.push(self.vy.abs());
        if self.vy > 260.0 {
            self.vy = -self.vy * RESTITUTION;
            self.omega += self.vx * 0.4;
        } else {
            self.vy = 0.0;
            self.grounded = true;
            self.platform = id;
        }
    }
    pub fn circle_bounce(&mut self, rects: &[Rect]) {
        let radius = self.half_w;
        for r in rects {
            let dx = self.x - self.x.clamp(r.left, r.right);
            let dy = self.y - self.y.clamp(r.top, r.bottom);
            let d = dx.hypot(dy);
            if d >= radius {
                continue;
            }
            let (nx, ny, push) = if d < 1e-6 {
                [
                    (self.x - r.left, -1.0, 0.0),
                    (r.right - self.x, 1.0, 0.0),
                    (self.y - r.top, 0.0, -1.0),
                    (r.bottom - self.y, 0.0, 1.0),
                ]
                .into_iter()
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(v, x, y)| (x, y, v + radius))
                .expect("four faces")
            } else {
                (dx / d, dy / d, radius - d)
            };
            self.x += nx * push;
            self.y += ny * push;
            let vn = self.vx * nx + self.vy * ny;
            if vn < 0.0 {
                self.vx -= 1.45 * vn * nx;
                self.vy -= 1.45 * vn * ny;
                if self.vy.abs() < 40.0 && ny < -0.5 {
                    self.vy = 0.0;
                }
            }
        }
    }
}

#[derive(Default)]
pub struct ToyTracker {
    away: bool,
}
impl ToyTracker {
    pub fn update(&mut self, ball: (f64, f64), pet: (f64, f64), handled: bool) -> (bool, f64) {
        let d = (ball.0 - pet.0).hypot(ball.1 - pet.1);
        let taken = d > 420.0 && handled && !self.away;
        if taken {
            self.away = true;
        }
        if d < 170.0 {
            self.away = false;
        }
        (taken, super::unit((255.0 - d) / 204.0))
    }
}
