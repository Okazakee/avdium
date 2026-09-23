//! Avdium: a native Android virtual device manager.
//!
//! The library holds everything except process startup. `src/main.rs` only
//! parses the command line and calls into here, so every behavior worth testing
//! is reachable from `cargo test`.
//!
//! Layering:
//!
//! - [`paths`] and [`settings`] resolve where things live. No I/O beyond reads.
//! - [`avd`] models devices and their INI files. Pure parsing and planning.
//! - [`process`] runs subprocesses with cancellation, bounded output and no
//!   shell involvement.
//! - [`providers`] describe what each official tool can actually do.
//! - [`sdk`] parses package identifiers and the official repository catalog.
//! - [`runtime`] drives the emulator and ADB directly.
//! - [`doctor`] evaluates pre-collected observations into actionable checks.
//! - [`operations`] schedules background jobs for the UI.
//! - [`ui`] and [`app`] are the egui front end.
//!
//! Blocking work never runs on the UI thread. The UI emits intents; the
//! operation coordinator runs them on worker threads and reports progress back
//! through a channel.

pub mod app;
pub mod avd;
pub mod cli;
pub mod demo;
pub mod doctor;
pub mod download;
pub mod operations;
pub mod paths;
pub mod process;
pub mod providers;
pub mod provision;
pub mod runtime;
pub mod sdk;
pub mod settings;
pub mod ui;

/// Product identity. Kept in one place so the window title, CLI output and
/// diagnostics cannot drift apart.
pub mod identity {
    /// Human-readable product name.
    pub const NAME: &str = "Avdium";
    /// Cargo package and binary name.
    pub const PACKAGE: &str = "avdium";
    /// Desktop entry basename and installed icon name.
    pub const APP_ID: &str = "dev.okazakee.Avdium";
    /// Short description used by `--help` and packaging.
    pub const DESCRIPTION: &str = "A native Android virtual device manager.";
    /// Tagline. Avoid em dashes in user-facing text.
    pub const TAGLINE: &str = "Android emulators, without the IDE.";
    /// Version reported by `--version` and the About screen.
    pub const VERSION: &str = env!("CARGO_PKG_VERSION");
}
