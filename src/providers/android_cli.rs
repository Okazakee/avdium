//! Android CLI adapter.
//!
//! Every invocation passes `--no-metrics`, the supported telemetry opt-out.
//! The flag is accepted by the installed binary even though `--help` does not
//! list it.
//!
//! Verified against the installed binary `1.0.16406183`; the parsers below are
//! tested against captures in `tests/fixtures/android-cli`.

use std::path::PathBuf;

use super::{Capabilities, LicenseBoundary};
use crate::process::CommandSpec;
use crate::sdk::packages::{PackageId, SeparatorStyle};

/// The supported telemetry opt-out.
pub const NO_METRICS: &str = "--no-metrics";

/// Android CLI adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidCli {
    /// Path to the launcher.
    pub program: PathBuf,
    /// SDK root pinned with `--sdk` so an ambient configuration cannot change
    /// which SDK the tool acts on.
    pub sdk: Option<PathBuf>,
}

impl AndroidCli {
    /// Create an adapter for a launcher path.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            sdk: None,
        }
    }

    /// Pin the SDK root passed as `--sdk`.
    pub fn with_sdk(mut self, sdk: Option<PathBuf>) -> Self {
        self.sdk = sdk;
        self
    }

    /// Build a command for a subcommand path, with the SDK pin applied.
    ///
    /// The telemetry opt-out is appended by [`AndroidCli::with_opt_out`] after
    /// the subcommand's own flags, so every invocation carries it while still
    /// leaving positional arguments last.
    fn base(&self, path: &[&str]) -> CommandSpec {
        let mut spec = CommandSpec::new(&self.program).args(path.iter().copied());
        if let Some(sdk) = &self.sdk {
            spec = spec.arg(format!("--sdk={}", sdk.display()));
        }
        spec
    }

    /// Append the supported telemetry opt-out.
    fn with_opt_out(spec: CommandSpec) -> CommandSpec {
        spec.arg(NO_METRICS)
    }

    /// `android info`.
    pub fn info_spec(&self) -> CommandSpec {
        Self::with_opt_out(self.base(&["info"]))
    }

    /// `android emulator list`.
    pub fn list_devices_spec(&self) -> CommandSpec {
        Self::with_opt_out(self.base(&["emulator", "list"]))
    }

    /// `android emulator list --long`.
    pub fn list_devices_long_spec(&self) -> CommandSpec {
        Self::with_opt_out(self.base(&["emulator", "list"]).arg("--long"))
    }

    /// `android emulator create --list-profiles`.
    pub fn list_profiles_spec(&self) -> CommandSpec {
        Self::with_opt_out(self.base(&["emulator", "create"]).arg("--list-profiles"))
    }

    /// `android sdk list`.
    pub fn list_packages_spec(&self) -> CommandSpec {
        Self::with_opt_out(self.base(&["sdk", "list"]))
    }

    /// `android sdk install <id>[@<version>]`.
    pub fn install_spec(
        &self,
        id: &PackageId,
        version: Option<&str>,
        channel: InstallChannel,
    ) -> CommandSpec {
        let mut spec = self.base(&["sdk", "install"]);
        if let Some(flag) = channel.flag() {
            spec = spec.arg(flag);
        }
        let argument = match version {
            Some(version) => id.with_version(version),
            None => id.as_str(),
        };
        Self::with_opt_out(spec).arg(argument)
    }

    /// `android sdk remove <id>`.
    pub fn remove_spec(&self, id: &PackageId) -> CommandSpec {
        Self::with_opt_out(self.base(&["sdk", "remove"])).arg(id.as_str())
    }

    /// `android sdk update <id>`.
    pub fn update_spec(&self, id: &PackageId) -> CommandSpec {
        Self::with_opt_out(self.base(&["sdk", "update"])).arg(id.as_str())
    }

    /// `android emulator start [--cold] [--headless] <device>`.
    pub fn start_spec(&self, device: &str, options: &StartOptions) -> CommandSpec {
        let mut spec = self.base(&["emulator", "start"]);
        if options.cold_boot {
            spec = spec.arg("--cold");
        }
        if options.headless {
            spec = spec.arg("--headless");
        }
        Self::with_opt_out(spec).arg(device)
    }

    /// `android emulator stop [<device>]`.
    ///
    /// The positional accepts an emulator name or a serial number.
    pub fn stop_spec(&self, device: Option<&str>) -> CommandSpec {
        let spec = Self::with_opt_out(self.base(&["emulator", "stop"]));
        match device {
            Some(device) => spec.arg(device),
            None => spec,
        }
    }

    /// `android emulator remove [--force] <device>`.
    pub fn remove_device_spec(&self, device: &str, force: bool) -> CommandSpec {
        let mut spec = self.base(&["emulator", "remove"]);
        if force {
            spec = spec.arg("--force");
        }
        Self::with_opt_out(spec).arg(device)
    }

    /// What this provider can do.
    ///
    /// `create_with_image` is `false`: the installed binary's
    /// `emulator create --help` accepts only a positional profile and has no
    /// image or API selector.
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            list_packages: true,
            install_packages: true,
            update_packages: true,
            remove_packages: true,
            list_devices: true,
            create_with_profile: true,
            create_with_image: false,
            list_profiles: true,
            remove_device: true,
            rename_device: false,
            start_device: true,
            stop_device: true,
            license_boundary: LicenseBoundary::Implicit,
        }
    }

    /// The separator style this provider expects.
    pub fn separator_style(&self) -> SeparatorStyle {
        SeparatorStyle::Slash
    }
}

/// Release channel for an install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InstallChannel {
    /// Stable release.
    #[default]
    Stable,
    /// Beta channel.
    Beta,
    /// Canary channel.
    Canary,
}

impl InstallChannel {
    /// The flag this channel adds, if any.
    pub fn flag(self) -> Option<&'static str> {
        match self {
            Self::Stable => None,
            Self::Beta => Some("--beta"),
            Self::Canary => Some("--canary"),
        }
    }

    /// Label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Beta => "Beta",
            Self::Canary => "Canary",
        }
    }

    /// Every channel, for the UI.
    pub const ALL: [Self; 3] = [Self::Stable, Self::Beta, Self::Canary];
}

/// Options for `android emulator start`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StartOptions {
    /// Start without loading a snapshot.
    pub cold_boot: bool,
    /// Start without a window.
    pub headless: bool,
}

/// What `android info` reports.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AndroidCliInfo {
    /// The SDK the CLI resolved.
    pub sdk: Option<PathBuf>,
    /// The application version.
    pub version: Option<String>,
    /// The launcher version, when reported.
    ///
    /// The installed binary `1.0.16406183` does not print this line, so it
    /// stays optional rather than being required.
    pub launcher_version: Option<String>,
}

/// Device status as reported by `android emulator list --long`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceStatus {
    /// The emulator is running.
    Running,
    /// The emulator is not running.
    Offline,
    /// Some other value the provider printed.
    Other(String),
}

impl DeviceStatus {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "running" | "online" | "booted" => Self::Running,
            "offline" | "stopped" => Self::Offline,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Label for the UI.
    pub fn label(&self) -> &str {
        match self {
            Self::Running => "Running",
            Self::Offline => "Offline",
            Self::Other(value) => value,
        }
    }
}

/// One device row from `android emulator list --long`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDevice {
    /// The AVD id.
    pub id: String,
    /// The display name.
    pub name: String,
    /// The API token, for example `android-36.1`, kept as a string.
    pub api_level: Option<String>,
    /// The reported status.
    pub status: DeviceStatus,
    /// The serial, when the provider reports one.
    pub serial: Option<String>,
}

/// One package row from `android sdk list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkListEntry {
    /// The parsed identifier.
    pub id: PackageId,
    /// The installed or offered version.
    pub version: Option<String>,
    /// The newer version the provider offers, when there is one.
    pub available: Option<String>,
    /// The human-readable description.
    pub description: String,
}

impl SdkListEntry {
    /// Whether an update is offered.
    pub fn has_update(&self) -> bool {
        match (&self.version, &self.available) {
            (Some(installed), Some(available)) => installed != available,
            _ => false,
        }
    }
}

/// A titled group of package rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkListSection {
    /// The section title as printed, without the trailing colon.
    pub name: String,
    /// The rows in the section.
    pub entries: Vec<SdkListEntry>,
}

impl SdkListSection {
    /// Whether this is the installed section.
    pub fn is_installed(&self) -> bool {
        self.name.to_ascii_lowercase().contains("installed")
    }
}

/// The parsed result of `android sdk list`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SdkList {
    /// Sections in the order printed.
    pub sections: Vec<SdkListSection>,
    /// Lines that looked like rows but could not be parsed.
    pub unparsed: Vec<String>,
}

impl SdkList {
    /// Every installed package.
    pub fn installed(&self) -> impl Iterator<Item = &SdkListEntry> {
        self.sections
            .iter()
            .filter(|section| section.is_installed())
            .flat_map(|s| s.entries.iter())
    }

    /// Installed packages with an offered update.
    pub fn updates(&self) -> impl Iterator<Item = &SdkListEntry> {
        self.installed().filter(|entry| entry.has_update())
    }

    /// Look up an installed package by identifier.
    pub fn installed_package(&self, id: &PackageId) -> Option<&SdkListEntry> {
        self.installed().find(|entry| &entry.id == id)
    }

    /// Whether any section was parsed at all.
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

/// Parse `android info`.
pub fn parse_info(text: &str) -> AndroidCliInfo {
    let mut info = AndroidCliInfo::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "sdk" => info.sdk = Some(PathBuf::from(value)),
            "version" => info.version = Some(value.to_owned()),
            "launcher_version" => info.launcher_version = Some(value.to_owned()),
            _ => {}
        }
    }
    info
}

/// Parse the plain `android emulator list` output: one name per line.
pub fn parse_device_list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Parse `android emulator list --long`.
pub fn parse_device_table(text: &str) -> Vec<ProviderDevice> {
    let mut devices = Vec::new();
    for line in text.lines() {
        let columns = split_columns(line);
        if columns.is_empty() {
            continue;
        }
        // Skip the header row.
        if columns[0] == "AVD ID" {
            continue;
        }
        if columns.len() < 4 {
            continue;
        }
        devices.push(ProviderDevice {
            id: columns[0].to_owned(),
            name: columns[1].to_owned(),
            api_level: Some(columns[2].to_owned()).filter(|value| !value.is_empty()),
            status: DeviceStatus::parse(columns[3]),
            serial: columns
                .get(4)
                .map(|value| (*value).to_owned())
                .filter(|v| !v.is_empty()),
        });
    }
    devices
}

/// Parse `android emulator create --list-profiles`.
pub fn parse_profiles(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.contains(' '))
        .map(str::to_owned)
        .collect()
}

/// Parse `android sdk list`.
pub fn parse_sdk_list(text: &str) -> SdkList {
    let mut result = SdkList::default();
    let mut current: Option<usize> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // A section header has no leading whitespace and ends with a colon.
        if !line.starts_with([' ', '\t']) && trimmed.ends_with(':') {
            let name = trimmed.trim_end_matches(':').trim().to_owned();
            result.sections.push(SdkListSection {
                name,
                entries: Vec::new(),
            });
            current = Some(result.sections.len() - 1);
            continue;
        }

        let columns = split_columns(line);
        if columns.is_empty() {
            continue;
        }
        let raw_id = columns[0];
        let Ok(id) = PackageId::parse(raw_id) else {
            result.unparsed.push(trimmed.to_owned());
            continue;
        };

        let version = columns
            .get(1)
            .map(|value| (*value).to_owned())
            .filter(|v| !v.is_empty());
        let mut available = None;
        let mut description_parts: Vec<&str> = Vec::new();
        let rest = &columns[2..];
        let mut index = 0;
        while index < rest.len() {
            if rest[index] == "->" {
                available = rest.get(index + 1).map(|value| (*value).to_owned());
                index += 2;
                continue;
            }
            description_parts.push(rest[index]);
            index += 1;
        }

        let entry = SdkListEntry {
            id,
            version,
            available,
            description: description_parts.join("  "),
        };

        match current {
            Some(index) => result.sections[index].entries.push(entry),
            None => {
                // Rows before any header still belong somewhere.
                result.sections.push(SdkListSection {
                    name: "Packages".to_owned(),
                    entries: vec![entry],
                });
                current = Some(result.sections.len() - 1);
            }
        }
    }

    result
}

/// Split a fixed-width column line on runs of two or more spaces.
fn split_columns(line: &str) -> Vec<&str> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let bytes = trimmed.as_bytes();
    let mut fields = Vec::new();
    let mut start = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b' ' && bytes.get(index + 1) == Some(&b' ') {
            if start < index {
                fields.push(&trimmed[start..index]);
            }
            while index < bytes.len() && bytes[index] == b' ' {
                index += 1;
            }
            start = index;
            continue;
        }
        index += 1;
    }
    if start < trimmed.len() {
        fields.push(&trimmed[start..]);
    }
    fields.retain(|field| !field.is_empty());
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(path: &str) -> String {
        let full = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
        std::fs::read_to_string(&full).unwrap_or_else(|error| panic!("read {full:?}: {error}"))
    }

    #[test]
    fn every_invocation_passes_the_telemetry_opt_out() {
        let cli = AndroidCli::new("/fake/android-cli");
        let specs = [
            cli.info_spec(),
            cli.list_devices_spec(),
            cli.list_devices_long_spec(),
            cli.list_profiles_spec(),
            cli.list_packages_spec(),
            cli.install_spec(
                &PackageId::parse("emulator").expect("parse"),
                None,
                InstallChannel::Stable,
            ),
            cli.remove_spec(&PackageId::parse("emulator").expect("parse")),
            cli.update_spec(&PackageId::parse("emulator").expect("parse")),
            cli.start_spec("Pixel_9", &StartOptions::default()),
            cli.stop_spec(None),
            cli.remove_device_spec("Pixel_9", true),
        ];
        for spec in specs {
            assert!(
                spec.args.iter().any(|arg| arg == NO_METRICS),
                "missing {NO_METRICS} in {}",
                spec.preview()
            );
        }
    }

    #[test]
    fn the_sdk_pin_is_passed_when_configured() {
        let cli = AndroidCli::new("/fake/android-cli").with_sdk(Some(PathBuf::from("/opt/sdk")));
        let spec = cli.list_devices_spec();
        assert!(
            spec.args.iter().any(|arg| arg == "--sdk=/opt/sdk"),
            "{}",
            spec.preview()
        );
    }

    #[test]
    fn no_sdk_pin_is_added_when_none_is_configured() {
        let cli = AndroidCli::new("/fake/android-cli");
        let spec = cli.list_devices_spec();
        assert!(!spec.preview().contains("--sdk="));
    }

    #[test]
    fn install_arguments_use_the_slash_form_and_a_version_suffix() {
        let cli = AndroidCli::new("/fake/android-cli");
        let id = PackageId::parse("system-images;android-36.1;google_apis;x86_64").expect("parse");
        let spec = cli.install_spec(&id, Some("4.0.0"), InstallChannel::Stable);
        assert!(
            spec.args
                .iter()
                .any(|arg| arg == "system-images/android-36.1/google_apis/x86_64@4.0.0"),
            "{}",
            spec.preview()
        );
        let beta = cli.install_spec(&id, None, InstallChannel::Beta);
        assert!(
            beta.args.iter().any(|arg| arg == "--beta"),
            "{}",
            beta.preview()
        );
        let canary = cli.install_spec(&id, None, InstallChannel::Canary);
        assert!(canary.args.iter().any(|arg| arg == "--canary"));
    }

    #[test]
    fn start_flags_precede_the_device_positional() {
        let cli = AndroidCli::new("/fake/android-cli");
        let spec = cli.start_spec(
            "Pixel_9",
            &StartOptions {
                cold_boot: true,
                headless: false,
            },
        );
        let args: Vec<String> = spec
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec!["emulator", "start", "--cold", NO_METRICS, "Pixel_9"]
        );
    }

    #[test]
    fn stop_accepts_a_device_or_nothing() {
        let cli = AndroidCli::new("/fake/android-cli");
        assert_eq!(
            cli.stop_spec(Some("emulator-5554")).preview(),
            "/fake/android-cli emulator stop --no-metrics emulator-5554"
        );
        assert_eq!(
            cli.stop_spec(None).preview(),
            "/fake/android-cli emulator stop --no-metrics"
        );
    }

    #[test]
    fn info_parsing_reads_the_captured_output() {
        let info = parse_info(&fixture("tests/fixtures/android-cli/info.txt"));
        assert_eq!(info.sdk, Some(PathBuf::from("$HOME/Android/Sdk")));
        assert_eq!(info.version.as_deref(), Some("1.0.16406183"));
        // The installed build does not print a launcher version.
        assert_eq!(info.launcher_version, None);
    }

    #[test]
    fn info_parsing_accepts_a_launcher_version_when_present() {
        let info = parse_info("sdk: /sdk\nversion: 1.0.0\nlauncher_version: 1.0.1\n");
        assert_eq!(info.launcher_version.as_deref(), Some("1.0.1"));
    }

    #[test]
    fn device_list_parsing_reads_the_captured_output() {
        let names = parse_device_list(&fixture("tests/fixtures/android-cli/emulator-list.txt"));
        assert_eq!(names, vec!["Pixel_9", "Pixel_Tablet", "Pixel_9_Pro"]);
    }

    #[test]
    fn device_table_parsing_reads_the_captured_output() {
        let devices = parse_device_table(&fixture(
            "tests/fixtures/android-cli/emulator-list-long.txt",
        ));
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].id, "Pixel_9");
        assert_eq!(devices[0].name, "Pixel 9");
        assert_eq!(devices[0].api_level.as_deref(), Some("android-36.1"));
        assert_eq!(devices[0].status, DeviceStatus::Offline);
        assert_eq!(devices[0].serial, None);
        assert_eq!(devices[2].name, "Pixel 9 Pro");
        assert_eq!(devices[2].api_level.as_deref(), Some("android-37.0"));
    }

    #[test]
    fn device_table_parsing_handles_a_serial_column() {
        let text = "AVD ID        AVD Name    API Level    Status    Serial\n\
                    Pixel_9       Pixel 9     android-36   Running   5554\n";
        let devices = parse_device_table(text);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].status, DeviceStatus::Running);
        assert_eq!(devices[0].serial.as_deref(), Some("5554"));
    }

    #[test]
    fn device_table_parsing_ignores_malformed_rows() {
        let devices = parse_device_table("AVD ID  AVD Name\nbroken row\n");
        assert!(devices.is_empty());
    }

    #[test]
    fn profile_parsing_reads_the_captured_output() {
        let profiles = parse_profiles(&fixture("tests/fixtures/android-cli/profiles.txt"));
        assert_eq!(
            profiles,
            vec![
                "large_desktop",
                "medium_desktop",
                "medium_phone",
                "medium_tablet",
                "small_desktop",
                "small_phone"
            ]
        );
    }

    #[test]
    fn sdk_list_parsing_reads_the_captured_output() {
        let list = parse_sdk_list(&fixture("tests/fixtures/android-cli/sdk-list.txt"));
        assert!(list.unparsed.is_empty(), "unparsed: {:?}", list.unparsed);
        assert_eq!(list.sections.len(), 1);
        assert_eq!(list.sections[0].name, "Installed packages");
        assert!(list.sections[0].is_installed());

        let installed: Vec<String> = list.installed().map(|entry| entry.id.as_str()).collect();
        assert!(installed.contains(&"build-tools/36.1.0".to_owned()));
        assert!(installed.contains(&"system-images/android-36.1/google_apis/x86_64".to_owned()));
        // Identifiers with dots and underscores survive intact.
        assert!(
            installed.contains(
                &"system-images/android-37.0/google_apis_playstore_ps16k/x86_64".to_owned()
            )
        );

        let emulator = list
            .installed_package(&PackageId::parse("emulator").expect("parse"))
            .expect("emulator installed");
        assert_eq!(emulator.version.as_deref(), Some("36.2.12"));
        assert_eq!(emulator.available.as_deref(), Some("37.1.11"));
        assert!(emulator.has_update());
        assert_eq!(emulator.description, "Android Emulator");

        let build_tools = list
            .installed_package(&PackageId::parse("build-tools/36.1.0").expect("parse"))
            .expect("build tools installed");
        assert!(!build_tools.has_update());
        assert_eq!(build_tools.available, None);

        let updates: Vec<String> = list.updates().map(|entry| entry.id.as_str()).collect();
        assert_eq!(
            updates,
            vec![
                "emulator",
                "platform-tools",
                "system-images/android-36.1/google_apis_playstore/x86_64",
                "system-images/android-37.0/google_apis_playstore_ps16k/x86_64"
            ]
        );
    }

    #[test]
    fn sdk_list_parsing_handles_multiple_sections() {
        let text = "\
Installed packages:
  emulator    36.2.12    Android Emulator
Available packages:
  build-tools/36.1.0    36.1.0    Android SDK Build-Tools 36.1
";
        let list = parse_sdk_list(text);
        assert_eq!(list.sections.len(), 2);
        assert!(list.sections[0].is_installed());
        assert!(!list.sections[1].is_installed());
        assert_eq!(list.installed().count(), 1);
    }

    #[test]
    fn sdk_list_parsing_records_rows_it_cannot_understand() {
        let text = "\
Installed packages:
  not a package id at all     1.0.0    Something
";
        let list = parse_sdk_list(text);
        assert_eq!(list.unparsed.len(), 1);
        assert!(list.installed().count() == 0);
    }

    #[test]
    fn sdk_list_parsing_on_garbage_does_not_panic() {
        for text in [
            "",
            "\n\n",
            "no sections here",
            "Installed packages:\n",
            ":::\n",
        ] {
            let list = parse_sdk_list(text);
            assert!(
                list.installed().count() <= 1,
                "unexpected entries for {text:?}"
            );
        }
    }

    #[test]
    fn column_splitting_keeps_single_spaces_inside_a_field() {
        let columns = split_columns("Pixel_9   Pixel 9 Pro   android-36.1   Offline");
        assert_eq!(
            columns,
            vec!["Pixel_9", "Pixel 9 Pro", "android-36.1", "Offline"]
        );
    }

    #[test]
    fn column_splitting_handles_trailing_and_leading_whitespace() {
        assert_eq!(split_columns("   a     b   "), vec!["a", "b"]);
        assert!(split_columns("   ").is_empty());
    }

    #[test]
    fn channels_expose_their_flags_and_labels() {
        assert_eq!(InstallChannel::Stable.flag(), None);
        assert_eq!(InstallChannel::Beta.flag(), Some("--beta"));
        assert_eq!(InstallChannel::Canary.flag(), Some("--canary"));
        assert_eq!(InstallChannel::Beta.label(), "Beta");
    }
}
