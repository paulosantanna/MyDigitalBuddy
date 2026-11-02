pub mod brain;
pub mod conditioning;
pub mod emotion;
pub mod homeostasis;
pub mod memory;
pub mod mind;
pub mod motor;
pub mod physics;
pub mod rng;
pub mod topology;

pub fn unit(x: f64) -> f64 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}
