//! The emulator executable.
//!
//! Runtime behavior goes through the emulator binary directly rather than
//! through a wrapper, because a wrapper that returns once the device is ready
//! is not a handle to the long-lived emulator process.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::process::CommandSpec;

/// The graphics mode passed as `-gpu`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuMode {
    /// Let the emulator decide. The default, and the only mode Avdium selects
    /// without evidence that another is needed.
    #[default]
    Auto,
    /// Host GPU acceleration.
    Host,
    /// Software rendering. Supported by the installed build, but slower.
    Software,
    /// Deprecated from emulator 36.4.9. Kept only so a device that already
    /// records it can be displayed and migrated, never as a fallback.
    SwiftshaderIndirect,
}

impl GpuMode {
    /// The value passed to `-gpu`.
    pub fn as_flag_value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Host => "host",
            Self::Software => "software",
            Self::SwiftshaderIndirect => "swiftshader_indirect",
        }
    }

    /// Parse a `hw.gpu.mode` value.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "host" => Some(Self::Host),
            "software" | "swiftshader" => Some(Self::Software),
            "swiftshader_indirect" => Some(Self::SwiftshaderIndirect),
            _ => None,
        }
    }

    /// Whether this mode is deprecated in current emulator releases.
    pub fn is_deprecated(self) -> bool {
        matches!(self, Self::SwiftshaderIndirect)
    }

    /// Label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Automatic",
            Self::Host => "Host GPU",
            Self::Software => "Software",
            Self::SwiftshaderIndirect => "SwiftShader indirect (deprecated)",
        }
    }
}

/// Options for a runtime launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartOptions {
    /// Start without loading a snapshot. This is what "cold boot" means here.
    pub cold_boot: bool,
    /// Do not load and do not save a snapshot.
    pub no_snapshot: bool,
    /// Start without a window.
    pub headless: bool,
    /// Graphics mode.
    pub gpu_mode: GpuMode,
    /// Erase the user data partition at startup. This is the supported wipe.
    pub wipe_data: bool,
    /// Pin the console port, which fixes the ADB serial.
    pub port: Option<u16>,
    /// Additional arguments. Kept explicit rather than passing a generic bag of
    /// flags to every emulator version.
    pub extra_args: Vec<OsString>,
}

impl Default for StartOptions {
    fn default() -> Self {
        Self {
            cold_boot: false,
            no_snapshot: false,
            headless: false,
            gpu_mode: GpuMode::Auto,
            wipe_data: false,
            port: None,
            extra_args: Vec::new(),
        }
    }
}

impl StartOptions {
    /// A cold boot: load no snapshot, but still save one on exit.
    pub fn cold() -> Self {
        Self {
            cold_boot: true,
            ..Self::default()
        }
    }
}

/// Emulator capabilities that Avdium checks before using a flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmulatorCapabilities {
    /// The reported version.
    pub version: Option<EmulatorVersion>,
    /// Whether `-gpu software` is supported. Unknown until probed.
    pub supports_software_gpu: Option<bool>,
    /// Whether `-no-snapshot-load` is supported.
    pub supports_no_snapshot_load: bool,
}

/// The emulator version line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmulatorVersion {
    /// Version string, for example `36.2.12.0`.
    pub version: String,
    /// Build id, when present.
    pub build_id: Option<String>,
}

impl EmulatorVersion {
    /// Version as the major.minor.micro prefix, when it parses.
    pub fn short(&self) -> String {
        let parts: Vec<&str> = self.version.split('.').take(3).collect();
        parts.join(".")
    }
}

/// The emulator executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmulatorTool {
    /// Path to the `emulator` binary.
    pub program: PathBuf,
}

impl EmulatorTool {
    /// Create a tool for a binary path.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    /// `emulator -version`.
    pub fn version_spec(&self) -> CommandSpec {
        CommandSpec::new(&self.program).arg("-version")
    }

    /// `emulator -list-avds`.
    pub fn list_avds_spec(&self) -> CommandSpec {
        CommandSpec::new(&self.program).arg("-list-avds")
    }

    /// `emulator -accel-check`.
    pub fn accel_check_spec(&self) -> CommandSpec {
        CommandSpec::new(&self.program).arg("-accel-check")
    }

    /// `emulator -avd <name> [flags]`.
    pub fn start_spec(&self, avd: &str, options: &StartOptions) -> CommandSpec {
        let mut spec = CommandSpec::new(&self.program).args(["-avd", avd]);
        if options.cold_boot {
            // Cold boot means "do not load a snapshot". It is not the same as
            // disabling snapshot saving as well.
            spec = spec.arg("-no-snapshot-load");
        }
        if options.no_snapshot {
            spec = spec.arg("-no-snapshot");
        }
        if options.headless {
            spec = spec.arg("-no-window");
        }
        if options.wipe_data {
            spec = spec.arg("-wipe-data");
        }
        if options.gpu_mode != GpuMode::Auto {
            spec = spec.args(["-gpu", options.gpu_mode.as_flag_value()]);
        }
        if let Some(port) = options.port {
            spec = spec.args(["-port", &port.to_string()]);
        }
        spec = spec.args(options.extra_args.iter().cloned());
        spec
    }

    /// `emulator -avd <name> -wipe-data`.
    ///
    /// Wiping is done by the emulator, never by deleting a single image file.
    pub fn wipe_spec(&self, avd: &str) -> CommandSpec {
        self.start_spec(
            avd,
            &StartOptions {
                wipe_data: true,
                ..StartOptions::default()
            },
        )
    }

    /// Parse `emulator -version`.
    pub fn parse_version(&self, text: &str) -> Option<EmulatorVersion> {
        parse_version(text)
    }

    /// Parse `emulator -list-avds`.
    pub fn parse_list_avds(&self, text: &str) -> Vec<String> {
        parse_list_avds(text)
    }

    /// Parse `emulator -accel-check`.
    pub fn parse_accel_check(&self, text: &str) -> AccelStatus {
        parse_accel_check(text)
    }
}

/// Parse `emulator -version`.
pub fn parse_version(text: &str) -> Option<EmulatorVersion> {
    let first = text.lines().next()?;
    // `Android emulator version 36.2.12.0 (build_id 14214601) (CL:N/A)`
    let version = first.split_whitespace().nth(3)?.to_owned();
    let build_id = first
        .split("build_id ")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    Some(EmulatorVersion { version, build_id })
}

/// Parse `emulator -list-avds`.
pub fn parse_list_avds(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("INFO") && !line.starts_with("WARNING"))
        .map(str::to_owned)
        .collect()
}

/// The result of `emulator -accel-check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccelStatus {
    /// Exit-like code printed by the tool. Zero means usable.
    pub code: i32,
    /// The human-readable message.
    pub message: String,
}

impl AccelStatus {
    /// Whether hardware acceleration is usable.
    pub fn is_usable(&self) -> bool {
        self.code == 0
    }
}

/// Parse `emulator -accel-check`.
///
/// The tool prints a three-line envelope:
///
/// ```text
/// accel:
/// 0
/// KVM (version 12) is installed and usable.
/// accel
/// ```
pub fn parse_accel_check(text: &str) -> AccelStatus {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let code = lines
        .iter()
        .find_map(|line| line.parse::<i32>().ok())
        .unwrap_or(-1);
    let message = lines
        .iter()
        .find(|line| !line.eq_ignore_ascii_case("accel:") && line.parse::<i32>().is_err())
        .map(|line| (*line).to_owned())
        .unwrap_or_else(|| "no acceleration message was reported".to_owned());
    AccelStatus { code, message }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/emulator")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path:?}: {error}"))
    }

    fn tool() -> EmulatorTool {
        EmulatorTool::new("/sdk/emulator/emulator")
    }

    #[test]
    fn version_parsing_reads_the_captured_output() {
        let version = parse_version(&fixture("version.txt")).expect("version");
        assert_eq!(version.version, "36.2.12.0");
        assert_eq!(version.build_id.as_deref(), Some("14214601"));
        assert_eq!(version.short(), "36.2.12");
    }

    #[test]
    fn version_parsing_survives_unexpected_output() {
        assert!(parse_version("").is_none());
        assert!(parse_version("something else entirely").is_none());
        let odd = parse_version("Android emulator version 1.2.3").expect("version");
        assert_eq!(odd.build_id, None);
    }

    #[test]
    fn list_avds_parsing_reads_the_captured_output() {
        let names = parse_list_avds(&fixture("list-avds.txt"));
        assert_eq!(names, vec!["Pixel_9", "Pixel_9_Pro", "Pixel_Tablet"]);
    }

    #[test]
    fn list_avds_ignores_log_lines() {
        let names = parse_list_avds("INFO | something\nReal_Device\nWARNING | noise\n\n");
        assert_eq!(names, vec!["Real_Device"]);
    }

    #[test]
    fn accel_check_parsing_reads_the_captured_output() {
        let status = parse_accel_check(&fixture("accel-check.txt"));
        assert_eq!(status.code, 0);
        assert!(status.is_usable());
        assert!(status.message.contains("KVM"));
    }

    #[test]
    fn accel_check_parsing_reports_failure_codes() {
        let status = parse_accel_check("accel:\n1\nKVM is not installed\naccel\n");
        assert_eq!(status.code, 1);
        assert!(!status.is_usable());
        assert_eq!(status.message, "KVM is not installed");
    }

    #[test]
    fn accel_check_parsing_on_garbage_returns_unknown_not_success() {
        let status = parse_accel_check("unexpected output");
        assert_eq!(status.code, -1);
        assert!(!status.is_usable());
    }

    #[test]
    fn a_plain_launch_passes_only_the_device() {
        let spec = tool().start_spec("Pixel_9", &StartOptions::default());
        let args: Vec<String> = spec
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["-avd", "Pixel_9"]);
    }

    #[test]
    fn cold_boot_uses_no_snapshot_load_and_not_the_unverified_flag() {
        let spec = tool().start_spec("Pixel_9", &StartOptions::cold());
        let args: Vec<String> = spec
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"-no-snapshot-load".to_owned()));
        assert!(!args.contains(&"-no-snapshot".to_owned()));
        assert!(!args.contains(&"-cold-boot".to_owned()));
    }

    #[test]
    fn disabling_both_snapshot_behaviors_is_a_separate_flag() {
        let options = StartOptions {
            cold_boot: true,
            no_snapshot: true,
            ..StartOptions::default()
        };
        let spec = tool().start_spec("Pixel_9", &options);
        let args: Vec<String> = spec
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"-no-snapshot-load".to_owned()));
        assert!(args.contains(&"-no-snapshot".to_owned()));
    }

    #[test]
    fn auto_graphics_adds_no_flag() {
        let spec = tool().start_spec("Pixel_9", &StartOptions::default());
        assert!(!spec.args.iter().any(|arg| arg == "-gpu"));
    }

    #[test]
    fn an_explicit_graphics_mode_is_passed_through() {
        let options = StartOptions {
            gpu_mode: GpuMode::Software,
            ..StartOptions::default()
        };
        let spec = tool().start_spec("Pixel_9", &options);
        let args: Vec<String> = spec
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|pair| pair == ["-gpu", "software"]));
    }

    #[test]
    fn swiftshader_indirect_is_never_selected_by_default() {
        assert_eq!(StartOptions::default().gpu_mode, GpuMode::Auto);
        assert!(GpuMode::SwiftshaderIndirect.is_deprecated());
        assert!(!GpuMode::Software.is_deprecated());
        assert!(!GpuMode::Auto.is_deprecated());
    }

    #[test]
    fn gpu_mode_parsing_round_trips() {
        for mode in [
            GpuMode::Auto,
            GpuMode::Host,
            GpuMode::Software,
            GpuMode::SwiftshaderIndirect,
        ] {
            assert_eq!(GpuMode::parse(mode.as_flag_value()), Some(mode));
        }
        assert_eq!(GpuMode::parse("SWIFTSHADER"), Some(GpuMode::Software));
        assert_eq!(GpuMode::parse("nonsense"), None);
    }

    #[test]
    fn headless_and_port_are_passed_when_requested() {
        let options = StartOptions {
            headless: true,
            port: Some(5554),
            ..StartOptions::default()
        };
        let spec = tool().start_spec("Pixel_9", &options);
        let args: Vec<String> = spec
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"-no-window".to_owned()));
        assert!(args.windows(2).any(|pair| pair == ["-port", "5554"]));
    }

    #[test]
    fn wiping_uses_the_emulator_operation() {
        let spec = tool().wipe_spec("Pixel_9");
        let args: Vec<String> = spec
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"-wipe-data".to_owned()));
        // Never a raw file deletion.
        assert!(!args.iter().any(|arg| arg.contains("userdata-qemu")));
    }

    #[test]
    fn extra_arguments_are_appended_verbatim() {
        let options = StartOptions {
            extra_args: vec![OsString::from("-no-boot-anim")],
            ..StartOptions::default()
        };
        let spec = tool().start_spec("Pixel_9", &options);
        assert!(spec.args.iter().any(|arg| arg == "-no-boot-anim"));
    }

    #[test]
    fn every_runtime_command_targets_the_emulator_binary() {
        assert_eq!(
            tool().version_spec().program,
            PathBuf::from("/sdk/emulator/emulator")
        );
        assert_eq!(
            tool().list_avds_spec().program,
            PathBuf::from("/sdk/emulator/emulator")
        );
        assert_eq!(
            tool().accel_check_spec().program,
            PathBuf::from("/sdk/emulator/emulator")
        );
    }
}
