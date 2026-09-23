//! The application: state, event handling and the egui update loop.
//!
//! The UI thread only renders and submits work. Results arrive through a
//! channel from worker threads, and every subprocess, scan or download happens
//! on a worker.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use crate::avd::Discovery;
use crate::doctor::DoctorReport;
use crate::identity;
use crate::operations::jobs::{self, AppEvent, JobEnvironment};
use crate::operations::{OperationCoordinator, SubmitError};
use crate::paths::{AppDirs, PathContext};
use crate::providers::{Capabilities, ProviderSet};
use crate::runtime::{DeviceRuntime, EmulatorTool, Serial, StartOptions};
use crate::settings::Settings;
use crate::ui::{Screen, apply_theme};

/// How much of an emulator log to read for display.
const LOG_TAIL_BYTES: u64 = 32 * 1024;

/// Which path a folder picker is choosing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathTarget {
    /// The SDK root.
    Sdk,
    /// The AVD home.
    AvdHome,
}

impl PathTarget {
    /// The identifier sent back with the picked path.
    pub fn key(self) -> &'static str {
        match self {
            Self::Sdk => "sdk",
            Self::AvdHome => "avd_home",
        }
    }

    /// Parse the identifier.
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "sdk" => Some(Self::Sdk),
            "avd_home" => Some(Self::AvdHome),
            _ => None,
        }
    }
}

/// What the process was launched with.
#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    /// Start in demo mode.
    pub demo: bool,
    /// An SDK root chosen on the command line.
    pub sdk: Option<PathBuf>,
    /// An AVD home chosen on the command line.
    pub avd_home: Option<PathBuf>,
    /// A log level chosen on the command line.
    pub log: Option<String>,
    /// A directory to write screenshots into, then exit.
    pub screenshot: Option<PathBuf>,
}

/// The application.
pub struct AvdiumApp {
    /// Whether demo mode is active.
    pub demo: bool,
    /// Avdium's own directories.
    pub dirs: AppDirs,
    /// Persisted settings.
    pub settings: Settings,
    /// Problems recovered from while loading settings.
    pub settings_warnings: Vec<String>,
    /// Resolved paths.
    pub paths: PathContext,
    /// Detected providers.
    pub providers: ProviderSet,
    /// Discovered devices.
    pub discovery: Discovery,
    /// Runtime state per device name.
    pub runtimes: BTreeMap<String, DeviceRuntime>,
    /// The most recent doctor report.
    pub doctor: Option<DoctorReport>,
    /// Background work.
    pub coordinator: OperationCoordinator,
    /// The visible screen.
    pub screen: Screen,
    /// The selected device name.
    pub selected: Option<String>,
    /// A short status message.
    pub status: String,
    /// Editable SDK root.
    pub sdk_input: String,
    /// Editable AVD home.
    pub avd_home_input: String,
    /// Immutable job inputs.
    pub job_environment: JobEnvironment,
    /// The screenshot driver, when screenshot mode is active.
    pub screenshot: Option<crate::ui::screenshot::ScreenshotState>,
    /// Whether the process should exit once screenshots are done.
    pub quit_when_screenshot_done: bool,
    /// Whether the last doctor run included tool probes.
    pub last_doctor_probe: bool,

    events: Receiver<AppEvent>,
    event_sender: Sender<AppEvent>,
}

impl AvdiumApp {
    /// Build the application from launch options.
    pub fn new(creation: &eframe::CreationContext<'_>, options: LaunchOptions) -> Self {
        let dirs = AppDirs::from_process().unwrap_or_else(|| {
            let fallback = std::env::temp_dir().join("avdium");
            AppDirs::from_roots(
                fallback.join("config"),
                fallback.join("cache"),
                fallback.join("data"),
                fallback.join("state"),
            )
        });
        let _ = dirs.ensure();

        let loaded = Settings::load(&dirs.settings_file());
        let mut settings = loaded.settings;
        // Command line choices are a one-run override, not a silent settings edit.
        let sdk_override = options.sdk.clone();
        let avd_override = options.avd_home.clone();
        if let Some(log) = &options.log {
            settings.log_level = parse_log_level(log).unwrap_or(settings.log_level);
        }

        let effective_sdk = sdk_override.clone().or_else(|| settings.sdk_path.clone());
        let effective_avd = avd_override.clone().or_else(|| settings.avd_home.clone());
        let paths = PathContext::from_process(effective_sdk.as_deref(), effective_avd.as_deref());

        let providers = if options.demo {
            ProviderSet::default()
        } else {
            ProviderSet::detect(&paths, settings.provider)
        };

        let (event_sender, events) = channel();
        let job_environment = JobEnvironment {
            paths: paths.clone(),
            dirs: dirs.clone(),
            providers: providers.clone(),
        };

        let mut app = Self {
            demo: options.demo,
            dirs,
            settings,
            settings_warnings: loaded.warnings,
            paths,
            providers,
            discovery: Discovery::default(),
            runtimes: BTreeMap::new(),
            doctor: None,
            coordinator: OperationCoordinator::new(),
            screen: Screen::Devices,
            selected: None,
            status: String::new(),
            sdk_input: effective_sdk
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            avd_home_input: effective_avd
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            job_environment,
            screenshot: None,
            quit_when_screenshot_done: false,
            last_doctor_probe: false,
            events,
            event_sender,
        };

        if options.demo {
            app.load_demo_state();
            app.status = "Demo mode: synthetic devices, no access to a real SDK.".to_owned();
        } else {
            app.refresh_all();
        }

        if let Some(directory) = options.screenshot.clone() {
            app.screenshot = Some(crate::ui::screenshot::ScreenshotState::new(directory));
            app.quit_when_screenshot_done = true;
        }

        apply_theme(&creation.egui_ctx, app.settings.theme);
        app
    }

    /// Load the synthetic demo dataset.
    fn load_demo_state(&mut self) {
        self.paths = crate::demo::path_context();
        self.discovery = crate::demo::discovery();
        self.runtimes = crate::demo::runtimes().into_iter().collect();
        self.doctor = Some(crate::demo::doctor_report());
        self.selected = self.discovery.avds.first().map(|avd| avd.name.clone());
        self.job_environment.paths = self.paths.clone();
    }

    /// Runtime state for a device, defaulting to stopped.
    pub fn runtime_for(&self, name: &str) -> DeviceRuntime {
        self.runtimes
            .get(name)
            .cloned()
            .unwrap_or_else(DeviceRuntime::stopped)
    }

    /// Whether a mutation key is currently held by a running job.
    pub fn is_busy(&self, key: &str) -> bool {
        self.coordinator
            .active_mutations()
            .iter()
            .any(|active| active == key)
    }

    /// Whether a doctor run is in flight.
    pub fn doctor_running(&self) -> bool {
        self.coordinator
            .running()
            .iter()
            .any(|job| job.title == "doctor")
    }

    /// Refresh device definitions and runtime state.
    pub fn refresh_all(&mut self) {
        if self.demo {
            self.load_demo_state();
            return;
        }
        self.refresh_discovery();
        self.refresh_runtimes();
    }

    /// Re-read device definitions.
    pub fn refresh_discovery(&mut self) {
        if self.demo {
            return;
        }
        let environment = self.job_environment.clone();
        let sender = self.event_sender.clone();
        if let Err(error) = self.coordinator.submit("discovery", None, move |context| {
            jobs::discover(&context, &environment, &sender)
        }) {
            self.status = error.to_string();
        }
    }

    /// Probe runtime state.
    pub fn refresh_runtimes(&mut self) {
        if self.demo {
            return;
        }
        let environment = self.job_environment.clone();
        let sender = self.event_sender.clone();
        let known: Vec<String> = self
            .discovery
            .avds
            .iter()
            .map(|avd| avd.name.clone())
            .collect();
        if let Err(error) = self.coordinator.submit("runtimes", None, move |context| {
            jobs::probe_runtimes(&context, &environment, known, &sender)
        }) {
            self.status = error.to_string();
        }
    }

    /// Run the doctor.
    pub fn run_doctor(&mut self, probe: bool) {
        if self.demo {
            self.doctor = Some(crate::demo::doctor_report());
            return;
        }
        let environment = self.job_environment.clone();
        let sender = self.event_sender.clone();
        let discovery = self.discovery.clone();
        self.last_doctor_probe = probe;
        if let Err(error) = self.coordinator.submit("doctor", None, move |context| {
            jobs::run_doctor(&context, &environment, discovery, probe, &sender)
        }) {
            self.status = error.to_string();
        }
    }

    /// Start a device.
    pub fn start_device(&mut self, name: &str, options: StartOptions) {
        if self.demo {
            self.status = format!(
                "Demo mode: {name} was not started, because demo mode never touches a real SDK."
            );
            return;
        }
        let environment = self.job_environment.clone();
        let sender = self.event_sender.clone();
        let name_owned = name.to_owned();
        let title = if options.cold_boot {
            "cold boot"
        } else {
            "start"
        };
        if let Err(error) =
            self.coordinator
                .submit(title, Some(jobs::avd_mutation_key(name)), move |context| {
                    jobs::start_device(&context, &environment, &name_owned, &options, &sender)
                })
        {
            self.report_submit_error(error);
        }
    }

    /// Stop a device.
    pub fn stop_device(&mut self, name: &str) {
        if self.demo {
            self.status = format!("Demo mode: {name} was not stopped.");
            return;
        }
        let environment = self.job_environment.clone();
        let sender = self.event_sender.clone();
        let serial = self.runtime_for(name).serial;
        let name_owned = name.to_owned();
        if let Err(error) =
            self.coordinator
                .submit("stop", Some(jobs::avd_mutation_key(name)), move |context| {
                    jobs::stop_device(&context, &environment, &name_owned, serial, &sender)
                })
        {
            self.report_submit_error(error);
        }
    }

    fn report_submit_error(&mut self, error: SubmitError) {
        self.status = error.to_string();
    }

    /// The command that would start a device.
    pub fn start_preview(&self, name: &str, options: &StartOptions) -> Option<String> {
        let program = self.paths.emulator_program()?;
        Some(
            EmulatorTool::new(program)
                .start_spec(name, options)
                .preview(),
        )
    }

    /// The command that would stop a device.
    pub fn stop_preview(&self, serial: &Serial) -> Option<String> {
        let program = self.paths.adb_program()?;
        let tool = crate::runtime::AdbTool::new(program)
            .with_server_port(crate::runtime::AdbTool::server_port_from_env());
        Some(tool.emu_kill_spec(serial).preview())
    }

    /// The tail of an emulator log, bounded.
    pub fn emulator_log(&self, name: &str) -> Option<String> {
        read_log_tail(&jobs::emulator_log_path(&self.dirs, name), LOG_TAIL_BYTES)
    }

    /// Versions reported by the doctor, for the About screen.
    pub fn tool_versions(&self) -> Vec<(&'static str, String)> {
        let Some(report) = &self.doctor else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for (id, label) in [
            ("tool.emulator", "Emulator"),
            ("tool.adb", "ADB"),
            ("tool.android_cli", "Android CLI"),
            ("tool.java", "Java"),
        ] {
            if let Some(check) = report.by_id(id) {
                let value = check
                    .evidence
                    .iter()
                    .find(|line| line.starts_with("Version "))
                    .map(|line| line.trim_start_matches("Version ").to_owned())
                    .unwrap_or_else(|| check.evidence.first().cloned().unwrap_or_default());
                rows.push((label, value));
            }
        }
        rows
    }

    /// The capability matrix for every detected provider.
    pub fn capability_matrix(&self) -> Vec<(&'static str, Capabilities)> {
        let mut rows: Vec<(&'static str, Capabilities)> = Vec::new();
        if let Some(cli) = &self.providers.android_cli {
            rows.push(("Android CLI", cli.capabilities()));
        }
        if let Some(tools) = &self.providers.cmdline_tools {
            rows.push((
                "Classic tools (sdkmanager, avdmanager)",
                tools.capabilities(),
            ));
        }
        rows
    }

    /// A diagnostic report with local paths intact.
    pub fn diagnostic_text(&self) -> String {
        let mut text = String::new();
        text.push_str(&format!("{} {}\n", identity::NAME, identity::VERSION));
        text.push_str(&format!("application id: {}\n", identity::APP_ID));
        text.push_str(&format!(
            "host: {} {}\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
        text.push_str(&format!("sdk: {}\n", self.paths.sdk_summary()));
        text.push_str(&format!(
            "avd home: {} (from {})\n",
            self.paths.avd_home.display(),
            self.paths.avd_home_origin.label()
        ));
        text.push_str(&format!("providers: {}\n", self.providers.summary()));
        text.push_str(&format!("devices: {}\n", self.discovery.avds.len()));
        for avd in &self.discovery.avds {
            text.push_str(&format!(
                "  {} target={} abi={} state={}\n",
                avd.name,
                avd.target().unwrap_or("?"),
                avd.abi().unwrap_or("?"),
                self.runtime_for(&avd.name).state.label()
            ));
        }
        if let Some(report) = &self.doctor {
            text.push_str(&format!("doctor: {}\n", report.summary()));
            for check in &report.checks {
                text.push_str(&format!(
                    "  [{}] {}: {}\n",
                    check.status.label(),
                    check.id,
                    check.evidence.join("; ")
                ));
            }
        }
        text.push_str("license text is deliberately excluded from this report\n");
        text
    }

    /// A diagnostic report with the home directory replaced.
    pub fn redacted_diagnostic(&self) -> String {
        let raw = self.diagnostic_text();
        let home = self.paths.home.display().to_string();
        let mut redacted = raw.replace(&home, "$HOME");
        // Belt and braces: any remaining absolute home-like prefix.
        for prefix in ["/home/", "/Users/"] {
            while let Some(index) = redacted.find(prefix) {
                let rest = &redacted[index + prefix.len()..];
                let end = rest
                    .find(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != '.')
                    .unwrap_or(rest.len());
                let user = rest[..end].to_owned();
                redacted = redacted.replace(&format!("{prefix}{user}"), "$HOME");
            }
        }
        redacted
    }

    /// Persist settings and re-resolve paths.
    pub fn save_settings(&mut self) {
        self.settings
            .set_sdk_path(non_empty(&self.sdk_input).map(PathBuf::from));
        self.settings
            .set_avd_home(non_empty(&self.avd_home_input).map(PathBuf::from));
        match self.settings.save(&self.dirs.settings_file()) {
            Ok(()) => {
                self.settings_warnings.clear();
                self.status = format!("Settings saved to {}", self.dirs.settings_file().display());
                self.rebuild_environment();
            }
            Err(error) => {
                self.status = format!("Settings could not be saved: {error}");
            }
        }
    }

    /// Discard unsaved edits.
    pub fn reload_settings(&mut self) {
        let loaded = Settings::load(&self.dirs.settings_file());
        self.settings = loaded.settings;
        self.settings_warnings = loaded.warnings;
        self.sdk_input = self
            .settings
            .sdk_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        self.avd_home_input = self
            .settings
            .avd_home
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        self.status = "Settings reloaded from disk.".to_owned();
        self.rebuild_environment();
    }

    /// Re-resolve paths and providers after a settings change.
    fn rebuild_environment(&mut self) {
        self.paths = PathContext::from_process(
            self.settings.sdk_path.as_deref(),
            self.settings.avd_home.as_deref(),
        );
        self.providers = ProviderSet::detect(&self.paths, self.settings.provider);
        self.job_environment.paths = self.paths.clone();
        self.job_environment.providers = self.providers.clone();
        self.selected = None;
        self.refresh_all();
    }

    /// Open a folder picker on a worker thread.
    ///
    /// The dialog is user-initiated, but it is still not run on the UI thread:
    /// a portal that is not responding must not freeze the window.
    pub fn pick_folder(&mut self, target: PathTarget) {
        let sender = self.event_sender.clone();
        let start = match target {
            PathTarget::Sdk => self
                .paths
                .sdk
                .as_ref()
                .map(|resolution| resolution.path.clone())
                .unwrap_or_else(|| self.paths.home.clone()),
            PathTarget::AvdHome => self.paths.avd_home.clone(),
        };
        let key = target.key();
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new().set_directory(start).pick_folder();
            let _ = sender.send(AppEvent::FolderPicked {
                target: key.to_owned(),
                path: picked,
            });
        });
    }

    /// Apply events from workers. Never blocks.
    fn drain_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                AppEvent::Discovery {
                    discovery,
                    providers,
                } => {
                    self.discovery = discovery;
                    self.providers = providers;
                    self.job_environment.providers = self.providers.clone();
                    if self
                        .selected
                        .as_ref()
                        .is_some_and(|name| self.discovery.get(name).is_none())
                    {
                        self.selected = None;
                    }
                    if self.selected.is_none() {
                        self.selected = self.discovery.avds.first().map(|avd| avd.name.clone());
                    }
                }
                AppEvent::Runtimes(runtimes) => {
                    self.runtimes = runtimes;
                }
                AppEvent::Doctor(report) => {
                    self.doctor = Some(*report);
                }
                AppEvent::Notice(message) => {
                    self.status = message;
                }
                AppEvent::Failure(message) => {
                    self.status = message;
                }
                AppEvent::FolderPicked { target, path } => {
                    let Some(path) = path else { continue };
                    let text = path.display().to_string();
                    match PathTarget::from_key(&target) {
                        Some(PathTarget::Sdk) => self.sdk_input = text,
                        Some(PathTarget::AvdHome) => self.avd_home_input = text,
                        None => {}
                    }
                }
            }
        }
    }

    fn render_navigation(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(identity::NAME)
                    .size(crate::ui::tokens::TEXT_HEADING)
                    .strong(),
            );
            ui.add_space(crate::ui::tokens::SPACING_S);
            for screen in Screen::ALL {
                let selected = self.screen == screen;
                if ui
                    .selectable_label(selected, screen.label())
                    .on_hover_text(screen.description())
                    .clicked()
                {
                    self.screen = screen;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.demo {
                    crate::ui::badge(ui, "DEMO", crate::ui::accent(ui.visuals()));
                }
                let active = self.coordinator.running().len();
                if active > 0 {
                    ui.spinner();
                    ui.label(
                        egui::RichText::new(format!("{active} running"))
                            .size(crate::ui::tokens::TEXT_SMALL)
                            .weak(),
                    );
                }
            });
        });
    }

    fn render_status(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let runtime = self.paths.sdk_summary();
            ui.label(
                egui::RichText::new(format!("SDK: {runtime}"))
                    .size(crate::ui::tokens::TEXT_SMALL)
                    .weak(),
            );
            ui.separator();
            if self.status.is_empty() {
                ui.label(
                    egui::RichText::new(if self.demo {
                        "Demo mode. Synthetic devices only."
                    } else {
                        "Ready. Closing the window leaves running emulators running."
                    })
                    .size(crate::ui::tokens::TEXT_SMALL)
                    .weak(),
                );
            } else {
                ui.label(egui::RichText::new(&self.status).size(crate::ui::tokens::TEXT_SMALL));
            }
        });
    }
}

impl eframe::App for AvdiumApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        self.drain_events();
        apply_theme(&context, self.settings.theme);

        if let Some(screenshot) = &mut self.screenshot {
            self.screen = screenshot.current_screen();
        }

        let navigation_frame =
            egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(10, 6));
        egui::Panel::top("navigation")
            .frame(navigation_frame)
            .show(ui, |ui| self.render_navigation(ui));

        let status_frame =
            egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(10, 4));
        egui::Panel::bottom("status")
            .frame(status_frame)
            .show(ui, |ui| self.render_status(ui));

        egui::CentralPanel::default_margins().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(format!("screen-{:?}", self.screen))
                .show(ui, |ui| match self.screen {
                    Screen::Devices => crate::ui::devices::show(ui, self),
                    Screen::Doctor => crate::ui::doctor::show(ui, self),
                    Screen::Activity => crate::ui::activity::show(ui, self),
                    Screen::Settings => crate::ui::settings::show(ui, self),
                    Screen::About => crate::ui::about::show(ui, self),
                });
        });

        if self.coordinator.take_repaint() {
            context.request_repaint();
        }
        if !self.coordinator.running().is_empty() {
            // Keep progress and cancellation responsive without spinning.
            context.request_repaint_after(Duration::from_millis(250));
        }

        if let Some(mut screenshot) = self.screenshot.take() {
            screenshot.drive(&context);
            if screenshot.is_finished() {
                // Reported once: the sequence is finished, so the driver is
                // dropped rather than replayed on the next frame.
                if let Some(error) = screenshot.error() {
                    log::error!("screenshot mode failed: {error}");
                    eprintln!("screenshot mode failed: {error}");
                } else {
                    for path in screenshot.written() {
                        println!("wrote {}", path.display());
                    }
                }
                if self.quit_when_screenshot_done {
                    context.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            } else {
                self.screenshot = Some(screenshot);
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Running emulators are deliberately left alone. Closing Avdium does not
        // stop a device the user started; that is an explicit action.
        log::info!("Avdium closed; running emulators were left running.");
    }
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn parse_log_level(value: &str) -> Option<crate::settings::LogLevel> {
    match value.trim().to_ascii_lowercase().as_str() {
        "error" => Some(crate::settings::LogLevel::Error),
        "warn" | "warning" => Some(crate::settings::LogLevel::Warn),
        "info" => Some(crate::settings::LogLevel::Info),
        "debug" => Some(crate::settings::LogLevel::Debug),
        "trace" => Some(crate::settings::LogLevel::Trace),
        _ => None,
    }
}

/// Read at most `max_bytes` from the end of a file.
fn read_log_tail(path: &std::path::Path, max_bytes: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    if length == 0 {
        return None;
    }
    let start = length.saturating_sub(max_bytes);
    if start > 0 {
        file.seek(SeekFrom::Start(start)).ok()?;
    }
    let mut buffer = Vec::with_capacity(max_bytes as usize);
    file.take(max_bytes).read_to_end(&mut buffer).ok()?;
    let mut text = String::from_utf8_lossy(&buffer).into_owned();
    if start > 0 {
        // Drop the partial first line.
        if let Some(index) = text.find('\n') {
            text = text[index + 1..].to_owned();
        }
        text = format!("[...earlier output not shown...]\n{text}");
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_targets_round_trip_through_their_keys() {
        for target in [PathTarget::Sdk, PathTarget::AvdHome] {
            assert_eq!(PathTarget::from_key(target.key()), Some(target));
        }
        assert_eq!(PathTarget::from_key("nonsense"), None);
    }

    #[test]
    fn log_levels_parse_leniently() {
        assert_eq!(
            parse_log_level("DEBUG"),
            Some(crate::settings::LogLevel::Debug)
        );
        assert_eq!(
            parse_log_level("warning"),
            Some(crate::settings::LogLevel::Warn)
        );
        assert_eq!(parse_log_level("chatty"), None);
    }

    #[test]
    fn empty_input_means_no_override() {
        assert_eq!(non_empty("   "), None);
        assert_eq!(non_empty(" /opt/sdk "), Some("/opt/sdk"));
    }

    #[test]
    fn log_tail_reading_is_bounded_and_drops_a_partial_first_line() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("emulator.log");
        let mut content = String::new();
        for index in 0..2000 {
            content.push_str(&format!(
                "line {index} with some padding text to make it longer\n"
            ));
        }
        std::fs::write(&path, &content).expect("write");

        let tail = read_log_tail(&path, 512).expect("tail");
        assert!(tail.len() <= 512 + 64, "retained {} bytes", tail.len());
        assert!(tail.contains("earlier output not shown"));
        // The last line must be intact.
        assert!(tail.trim_end().ends_with("padding text to make it longer"));
    }

    #[test]
    fn a_small_log_is_returned_whole() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("small.log");
        std::fs::write(&path, "one\ntwo\n").expect("write");
        assert_eq!(read_log_tail(&path, 4096).expect("tail"), "one\ntwo\n");
    }

    #[test]
    fn a_missing_or_empty_log_is_not_an_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert!(read_log_tail(&temp.path().join("missing.log"), 4096).is_none());
        let empty = temp.path().join("empty.log");
        std::fs::write(&empty, "").expect("write");
        assert!(read_log_tail(&empty, 4096).is_none());
    }
}
