// SPDX-License-Identifier: GPL-3.0-or-later
pub mod config;
pub mod core;
pub mod engine;
pub mod platform;
pub mod senses;
mod sophia_icon;
pub mod storage;
pub mod timeline;

#[cfg(feature = "desktop")]
pub mod desktop;
