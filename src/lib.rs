//! QuickSwitch: a Spotlight-style window, Explorer-tab and folder switcher
//! for Windows 11, built on eframe/egui.
//!
//! Platform-independent parts (`actions::paths`, `search`, `config`) are unit
//! tested; the rest is Windows integration.

pub mod actions;
pub mod app;
pub mod command;
pub mod config;
pub mod os;
pub mod search;
pub mod startup;
pub mod ui;
