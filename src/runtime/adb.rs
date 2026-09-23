//! ADB, used for identity, readiness and stopping.
//!
//! Avdium uses ADB narrowly on purpose. It is not a general ADB frontend.
//!
//! Two behaviors matter:
//!
//! - `adb devices` reporting `device` is **not** a boot-complete signal. The
//!   guest is reachable before the system finishes starting, so readiness polls
//!   `getprop sys.boot_completed`.
//! - The ADB server is shared with other tools and physical devices, so
//!   `adb kill-server` is never routine recovery.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::process::CommandSpec;

/// The conventional ADB server port.
pub const DEFAULT_SERVER_PORT: u16 = 5037;

/// An ADB serial number.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Serial(String);

impl Serial {
    /// Parse a serial, rejecting anything that could be mistaken for an option
    /// or a path.
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('-') || trimmed.contains('/') {
            return None;
        }
        if !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
        {
            return None;
        }
        Some(Self(trimmed.to_owned()))
    }

    /// The serial as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this is an emulator serial such as `emulator-5554`.
    pub fn is_emulator(&self) -> bool {
        self.0.starts_with("emulator-")
    }

    /// The emulator console port, when this is an emulator serial.
    pub fn console_port(&self) -> Option<u16> {
        self.0.strip_prefix("emulator-")?.parse().ok()
    }

    /// The ADB port, which is the console port plus one.
    pub fn adb_port(&self) -> Option<u16> {
        self.console_port().map(|port| port + 1)
    }
}

impl std::fmt::Display for Serial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The state ADB reports for a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdbState {
    /// Online and reachable.
    Device,
    /// Present but not responding.
    Offline,
    /// Waiting for the user to accept the host key.
    Unauthorized,
    /// In the bootloader.
    Bootloader,
    /// In recovery.
    Recovery,
    /// The host lacks permission, typically a udev rule problem.
    NoPermissions,
    /// Some other state ADB printed.
    Other(String),
}

impl AdbState {
    fn parse(raw: &str) -> Self {
        match raw.trim() {
            "device" => Self::Device,
            "offline" => Self::Offline,
            "unauthorized" => Self::Unauthorized,
            "bootloader" => Self::Bootloader,
            "recovery" => Self::Recovery,
            "no permissions" | "no_permissions" => Self::NoPermissions,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Whether property queries can be issued against this device.
    pub fn is_queryable(&self) -> bool {
        matches!(self, Self::Device)
    }

    /// Label for the UI.
    pub fn label(&self) -> &str {
        match self {
            Self::Device => "device",
            Self::Offline => "offline",
            Self::Unauthorized => "unauthorized",
            Self::Bootloader => "bootloader",
            Self::Recovery => "recovery",
            Self::NoPermissions => "no permissions",
            Self::Other(value) => value,
        }
    }
}

/// One row of `adb devices -l`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbDevice {
    /// The serial.
    pub serial: Serial,
    /// The reported state.
    pub state: AdbState,
    /// Extra `key:value` properties ADB printed.
    pub properties: BTreeMap<String, String>,
}

impl AdbDevice {
    /// A named property, for example `model`.
    pub fn property(&self, key: &str) -> Option<&str> {
        self.properties.get(key).map(String::as_str)
    }
}

/// The ADB executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbTool {
    /// Path to the `adb` binary.
    pub program: PathBuf,
    /// ADB server port. `None` means the default.
    pub server_port: Option<u16>,
}

impl AdbTool {
    /// Create a tool for a binary path.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            server_port: None,
        }
    }

    /// Use a specific server port.
    pub fn with_server_port(mut self, port: Option<u16>) -> Self {
        self.server_port = port;
        self
    }

    /// Read the configured server port from the environment.
    pub fn server_port_from_env() -> Option<u16> {
        std::env::var("ANDROID_ADB_SERVER_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
    }

    fn spec(&self) -> CommandSpec {
        let mut spec = CommandSpec::new(&self.program);
        if let Some(port) = self.server_port {
            // `-P` must precede the command.
            spec = spec.args(["-P", &port.to_string()]);
        }
        spec
    }

    /// `adb devices -l`.
    pub fn devices_spec(&self) -> CommandSpec {
        self.spec().args(["devices", "-l"])
    }

    /// `adb -s <serial> shell getprop sys.boot_completed`.
    pub fn boot_completed_spec(&self, serial: &Serial) -> CommandSpec {
        self.spec().args([
            "-s",
            serial.as_str(),
            "shell",
            "getprop",
            "sys.boot_completed",
        ])
    }

    /// `adb -s <serial> emu avd name`.
    pub fn avd_name_spec(&self, serial: &Serial) -> CommandSpec {
        self.spec()
            .args(["-s", serial.as_str(), "emu", "avd", "name"])
    }

    /// `adb -s <serial> emu kill`.
    pub fn emu_kill_spec(&self, serial: &Serial) -> CommandSpec {
        self.spec().args(["-s", serial.as_str(), "emu", "kill"])
    }

    /// Parse `adb devices -l`.
    pub fn parse_devices(&self, text: &str) -> Vec<AdbDevice> {
        parse_devices(text)
    }
}

/// Parse `adb devices -l`.
///
/// ADB prints the serial, then the state, then optional `key:value`
/// properties. One state, `no permissions`, contains a space, so the state is
/// matched by prefix rather than by taking a single whitespace-delimited field.
pub fn parse_devices(text: &str) -> Vec<AdbDevice> {
    /// Longest-first so `no permissions` is not shadowed by a shorter prefix.
    const MULTI_WORD_STATES: [&str; 1] = ["no permissions"];

    let mut devices = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Header and daemon chatter are not device rows.
        if trimmed.starts_with("List of devices")
            || trimmed.starts_with('*')
            || trimmed.starts_with("adb server")
            || trimmed.contains("daemon")
        {
            continue;
        }

        let mut fields = trimmed.split_whitespace();
        let Some(serial) = fields.next() else {
            continue;
        };
        let Some(serial) = Serial::parse(serial) else {
            continue;
        };
        let remainder = fields.collect::<Vec<_>>().join(" ");

        let (state, properties_text) = match MULTI_WORD_STATES
            .iter()
            .find(|state| remainder.starts_with(**state))
        {
            Some(state) => (
                (*state).to_owned(),
                remainder[state.len()..].trim().to_owned(),
            ),
            None => {
                let mut parts = remainder.splitn(2, ' ');
                let state = parts.next().unwrap_or_default().to_owned();
                let properties = parts.next().unwrap_or_default().to_owned();
                (state, properties)
            }
        };

        if state.is_empty() {
            continue;
        }

        let mut properties = BTreeMap::new();
        for field in properties_text.split_whitespace() {
            if let Some((key, value)) = field.split_once(':') {
                properties.insert(key.to_owned(), value.to_owned());
            }
        }

        devices.push(AdbDevice {
            serial,
            state: AdbState::parse(&state),
            properties,
        });
    }
    devices
}

/// Parse the output of `getprop sys.boot_completed`.
///
/// Anything other than an explicit `1` is not ready. An empty result means the
/// property is unset, which happens while the system is still starting.
pub fn parse_boot_completed(text: &str) -> bool {
    text.lines().any(|line| line.trim() == "1")
}

/// Parse the output of `emu avd name`.
///
/// The console prints the name followed by an `OK` line.
pub fn parse_avd_name(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("OK") {
            continue;
        }
        if trimmed.contains(' ') {
            continue;
        }
        return Some(trimmed.to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/adb")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path:?}: {error}"))
    }

    fn tool() -> AdbTool {
        AdbTool::new("/sdk/platform-tools/adb")
    }

    #[test]
    fn no_devices_parses_to_an_empty_list() {
        assert!(parse_devices(&fixture("devices-l.txt")).is_empty());
    }

    #[test]
    fn a_running_device_is_parsed_with_its_properties() {
        let text = "\
List of devices attached
emulator-5554          device product:sdk_gphone64_x86_64 model:sdk_gphone64_x86_64 device:emu64xa transport_id:1
";
        let devices = parse_devices(text);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].serial.as_str(), "emulator-5554");
        assert_eq!(devices[0].state, AdbState::Device);
        assert!(devices[0].state.is_queryable());
        assert_eq!(devices[0].property("model"), Some("sdk_gphone64_x86_64"));
        assert_eq!(devices[0].property("transport_id"), Some("1"));
    }

    #[test]
    fn offline_and_unauthorized_devices_are_distinguished() {
        let text = "\
List of devices attached
emulator-5556          offline
ABC123                 unauthorized
XYZ789                 bootloader
QRS456                 no permissions (user in plugdev group; are your udev rules wrong?)
";
        let devices = parse_devices(text);
        assert_eq!(devices.len(), 4);
        assert_eq!(devices[0].state, AdbState::Offline);
        assert_eq!(devices[1].state, AdbState::Unauthorized);
        assert_eq!(devices[2].state, AdbState::Bootloader);
        assert_eq!(devices[3].state, AdbState::NoPermissions);
        for device in &devices {
            assert!(!device.state.is_queryable());
        }
    }

    #[test]
    fn daemon_startup_chatter_is_not_mistaken_for_a_device() {
        let text = "\
* daemon not running; starting now at tcp:5037
* daemon started successfully
List of devices attached
emulator-5554\tdevice
";
        let devices = parse_devices(text);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].serial.as_str(), "emulator-5554");
    }

    #[test]
    fn serials_reject_option_like_and_path_like_values() {
        assert!(Serial::parse("emulator-5554").is_some());
        assert!(Serial::parse("0123456789ABCDEF").is_some());
        assert!(Serial::parse("-s").is_none());
        assert!(Serial::parse("a/b").is_none());
        assert!(Serial::parse("a;b").is_none());
        assert!(Serial::parse("").is_none());
        assert!(Serial::parse("   ").is_none());
    }

    #[test]
    fn emulator_serials_expose_their_ports() {
        let serial = Serial::parse("emulator-5554").expect("serial");
        assert!(serial.is_emulator());
        assert_eq!(serial.console_port(), Some(5554));
        assert_eq!(serial.adb_port(), Some(5555));
        let physical = Serial::parse("ABC123").expect("serial");
        assert!(!physical.is_emulator());
        assert_eq!(physical.console_port(), None);
    }

    #[test]
    fn boot_completion_requires_an_explicit_one() {
        assert!(parse_boot_completed("1\n"));
        assert!(parse_boot_completed("1"));
        assert!(!parse_boot_completed(""));
        assert!(!parse_boot_completed("0\n"));
        assert!(!parse_boot_completed("\n\n"));
        assert!(!parse_boot_completed("error: device offline\n"));
    }

    #[test]
    fn avd_name_parsing_skips_the_console_ok_line() {
        assert_eq!(parse_avd_name("Pixel_9\nOK\n"), Some("Pixel_9".to_owned()));
        assert_eq!(parse_avd_name("Pixel_9"), Some("Pixel_9".to_owned()));
        assert_eq!(parse_avd_name("OK\n"), None);
        assert_eq!(parse_avd_name(""), None);
        assert_eq!(parse_avd_name("error: device 'x' not found\n"), None);
    }

    #[test]
    fn commands_target_the_adb_binary_and_name_the_serial_explicitly() {
        let serial = Serial::parse("emulator-5554").expect("serial");
        assert_eq!(
            tool().boot_completed_spec(&serial).preview(),
            "/sdk/platform-tools/adb -s emulator-5554 shell getprop sys.boot_completed"
        );
        assert_eq!(
            tool().emu_kill_spec(&serial).preview(),
            "/sdk/platform-tools/adb -s emulator-5554 emu kill"
        );
        assert_eq!(
            tool().devices_spec().preview(),
            "/sdk/platform-tools/adb devices -l"
        );
    }

    #[test]
    fn a_configured_server_port_precedes_the_command() {
        let tool = tool().with_server_port(Some(5038));
        let args: Vec<String> = tool
            .devices_spec()
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["-P", "5038", "devices", "-l"]);
    }

    #[test]
    fn the_default_server_port_is_not_passed_explicitly() {
        assert!(!tool().devices_spec().preview().contains("-P"));
        assert_eq!(DEFAULT_SERVER_PORT, 5037);
    }

    #[test]
    fn kill_server_is_never_used_for_routine_recovery() {
        // The tool exposes no kill-server helper at all.
        let previews = [
            tool().devices_spec().preview(),
            tool()
                .boot_completed_spec(&Serial::parse("emulator-5554").expect("s"))
                .preview(),
            tool()
                .emu_kill_spec(&Serial::parse("emulator-5554").expect("s"))
                .preview(),
        ];
        for preview in previews {
            assert!(!preview.contains("kill-server"), "{preview}");
        }
    }
}
