//! The doctor.
//!
//! Every result carries a status, the evidence it was based on, and a next
//! action where one exists. `Unknown` means unknown: it is never reported as a
//! pass or a failure.
//!
//! Evaluation is a pure function over pre-collected observations, so it is
//! fully testable and cannot itself run a privileged change.

use std::path::{Path, PathBuf};

use crate::avd::Discovery;
use crate::paths::PathContext;
use crate::providers::ProviderSet;
use crate::runtime::AccelStatus;

/// The outcome of one check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckStatus {
    /// Everything is fine.
    Pass,
    /// Usable, but something is worth knowing.
    Warn,
    /// Broken, with a known next step.
    Fail,
    /// Not enough evidence to decide.
    Unknown,
}

impl CheckStatus {
    /// Label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pass => "Pass",
            Self::Warn => "Warning",
            Self::Fail => "Fail",
            Self::Unknown => "Unknown",
        }
    }
}

/// One doctor result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Stable identifier, for tests and for "fix this" links.
    pub id: &'static str,
    /// Human-readable title.
    pub title: String,
    /// The outcome.
    pub status: CheckStatus,
    /// What was actually observed.
    pub evidence: Vec<String>,
    /// What the user can do next, when there is something to do.
    pub hint: Option<String>,
}

impl Check {
    fn new(id: &'static str, title: impl Into<String>, status: CheckStatus) -> Self {
        Self {
            id,
            title: title.into(),
            status,
            evidence: Vec::new(),
            hint: None,
        }
    }

    fn evidence(mut self, line: impl Into<String>) -> Self {
        self.evidence.push(line.into());
        self
    }

    fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// A collection of checks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DoctorReport {
    /// Checks in display order.
    pub checks: Vec<Check>,
}

impl DoctorReport {
    /// Look up a check by identifier.
    pub fn by_id(&self, id: &str) -> Option<&Check> {
        self.checks.iter().find(|check| check.id == id)
    }

    /// Counts of pass, warn, fail and unknown.
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        let mut counts = (0, 0, 0, 0);
        for check in &self.checks {
            match check.status {
                CheckStatus::Pass => counts.0 += 1,
                CheckStatus::Warn => counts.1 += 1,
                CheckStatus::Fail => counts.2 += 1,
                CheckStatus::Unknown => counts.3 += 1,
            }
        }
        counts
    }

    /// The most severe status present.
    pub fn worst(&self) -> CheckStatus {
        self.checks
            .iter()
            .map(|check| check.status)
            .max()
            .unwrap_or(CheckStatus::Unknown)
    }

    /// A one-line summary for the status bar.
    pub fn summary(&self) -> String {
        let (pass, warn, fail, unknown) = self.counts();
        format!("{pass} passed, {warn} warnings, {fail} failed, {unknown} unknown")
    }
}

/// How `/dev/kvm` looked.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum KvmObservation {
    /// The device node exists.
    Present {
        /// Whether the current user can open it for reading.
        readable: bool,
        /// Whether the current user can open it for writing.
        writable: bool,
        /// The permission bits as reported by the filesystem.
        mode: String,
        /// The owning group, resolved to a name when possible.
        group: String,
    },
    /// The device node does not exist.
    Missing,
    /// The check could not be performed.
    #[default]
    Unknown,
}

/// One executable the doctor observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolObservation {
    /// Where the executable is.
    pub path: PathBuf,
    /// The version it reported, when it was asked.
    pub version: Option<String>,
    /// Why it could not be queried, when it could not.
    pub error: Option<String>,
}

/// Free space at a location that matters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskObservation {
    /// What this location is for, for example `SDK root`.
    pub label: String,
    /// The path that was measured.
    pub path: PathBuf,
    /// Available bytes.
    pub available_bytes: Option<u64>,
    /// Total bytes.
    pub total_bytes: Option<u64>,
}

impl DiskObservation {
    /// Available space in GiB, rounded down.
    pub fn available_gib(&self) -> Option<u64> {
        self.available_bytes
            .map(|bytes| bytes / (1024 * 1024 * 1024))
    }
}

/// Everything the doctor reasons about.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    /// The resolved SDK root, when one was found.
    pub sdk_path: Option<PathBuf>,
    /// Whether that path exists.
    pub sdk_exists: bool,
    /// Whether it is a directory.
    pub sdk_is_dir: bool,
    /// Whether `emulator/` is present.
    pub sdk_has_emulator: bool,
    /// Whether `platform-tools/` is present.
    pub sdk_has_platform_tools: bool,
    /// Whether `cmdline-tools/` is present.
    pub sdk_has_cmdline_tools: bool,
    /// Whether the SDK root is writable by this user.
    pub sdk_writable: bool,
    /// The resolved AVD home.
    pub avd_home: PathBuf,
    /// Whether the AVD home exists.
    pub avd_home_exists: bool,
    /// How many devices were discovered.
    pub avd_count: usize,
    /// Devices that cannot be used, with a one-line reason.
    pub broken_avds: Vec<(String, String)>,
    /// Environment conflicts and warnings from path resolution.
    pub path_problems: Vec<String>,
    /// The emulator executable.
    pub emulator: Option<ToolObservation>,
    /// The ADB executable.
    pub adb: Option<ToolObservation>,
    /// The Android CLI launcher.
    pub android_cli: Option<ToolObservation>,
    /// A Java runtime, which is only needed for classic tools.
    pub java: Option<ToolObservation>,
    /// Whether a Java runtime was actually looked for.
    ///
    /// Without this, an absent `java` field would be reported as "no Java was
    /// found" even when the check never ran, which would be a claim Avdium
    /// cannot support.
    pub java_probed: bool,
    /// Whether the selected provider needs Java at all.
    pub provider_needs_java: bool,
    /// How `/dev/kvm` looked.
    pub kvm: KvmObservation,
    /// The result of `emulator -accel-check`, when it was run.
    pub accel: Option<AccelStatus>,
    /// `/dev/dri` render nodes that exist.
    pub dri_nodes: Vec<String>,
    /// Graphics libraries that were found.
    pub gl_libraries: Vec<String>,
    /// Desktop portal libraries that were found.
    pub portal_libraries: Vec<String>,
    /// Free space at the locations that matter.
    pub disks: Vec<DiskObservation>,
    /// Total system memory in MiB.
    pub memory_mb: Option<u64>,
    /// The host architecture, for example `x86_64`.
    pub host_arch: String,
}

impl Environment {
    /// Collect everything that can be observed without running a subprocess.
    ///
    /// Tool versions and `-accel-check` are filled in by the operation worker,
    /// because running a tool can write caches and must stay off the UI thread.
    pub fn probe(paths: &PathContext, discovery: &Discovery, providers: &ProviderSet) -> Self {
        let sdk_path = paths.sdk.as_ref().map(|resolution| resolution.path.clone());
        let sdk_exists = sdk_path.as_ref().is_some_and(|path| path.exists());
        let sdk_is_dir = sdk_path.as_ref().is_some_and(|path| path.is_dir());
        let sdk_has_emulator = sdk_path
            .as_ref()
            .is_some_and(|path| path.join("emulator").is_dir());
        let sdk_has_platform_tools = sdk_path
            .as_ref()
            .is_some_and(|path| path.join("platform-tools").is_dir());
        let sdk_has_cmdline_tools = sdk_path
            .as_ref()
            .is_some_and(|path| path.join("cmdline-tools").is_dir());
        let sdk_writable = sdk_path.as_ref().is_some_and(|path| can_write(path));

        let mut path_problems: Vec<String> = paths
            .conflicts
            .iter()
            .map(|conflict| conflict.summary())
            .collect();
        path_problems.extend(paths.warnings.iter().cloned());

        let provider_needs_java = providers
            .selected()
            .is_some_and(|provider| provider.as_cmdline_tools().is_some());

        let mut disks = Vec::new();
        if let Some(path) = &sdk_path {
            disks.push(measure("SDK root", path));
        }
        disks.push(measure("AVD home", &paths.avd_home));

        Self {
            sdk_path,
            sdk_exists,
            sdk_is_dir,
            sdk_has_emulator,
            sdk_has_platform_tools,
            sdk_has_cmdline_tools,
            sdk_writable,
            avd_home: paths.avd_home.clone(),
            avd_home_exists: paths.avd_home.is_dir(),
            avd_count: discovery.avds.len(),
            broken_avds: discovery
                .broken()
                .map(|avd| {
                    (
                        avd.name.clone(),
                        avd.primary_problem()
                            .map(|reason| reason.summary())
                            .unwrap_or_default(),
                    )
                })
                .collect(),
            path_problems,
            emulator: paths.emulator_program().map(|path| ToolObservation {
                path,
                version: None,
                error: None,
            }),
            adb: paths.adb_program().map(|path| ToolObservation {
                path,
                version: None,
                error: None,
            }),
            android_cli: providers.android_cli.as_ref().map(|cli| ToolObservation {
                path: cli.program.clone(),
                version: None,
                error: None,
            }),
            java: None,
            java_probed: false,
            provider_needs_java,
            kvm: probe_kvm(),
            accel: None,
            dri_nodes: probe_dri_nodes(),
            gl_libraries: probe_libraries(&["libEGL.so.1", "libGL.so.1"]),
            portal_libraries: probe_libraries(&[
                "libgtk-3.so.0",
                "libgio-2.0.so.0",
                "libwayland-client.so.0",
            ]),
            disks,
            memory_mb: probe_memory_mb(),
            host_arch: std::env::consts::ARCH.to_owned(),
        }
    }
}

/// Evaluate every check.
pub fn evaluate(environment: &Environment) -> DoctorReport {
    DoctorReport {
        checks: vec![
            check_host_arch(environment),
            check_sdk_path(environment),
            check_sdk_layout(environment),
            check_sdk_writable(environment),
            check_path_conflicts(environment),
            check_avd_home(environment),
            check_broken_devices(environment),
            check_emulator(environment),
            check_adb(environment),
            check_android_cli(environment),
            check_java(environment),
            check_kvm(environment),
            check_acceleration(environment),
            check_graphics(environment),
            check_portals(environment),
            check_disk(environment),
            check_memory(environment),
        ],
    }
}

fn check_host_arch(environment: &Environment) -> Check {
    let mut check = Check::new("host.arch", "Host architecture", CheckStatus::Pass)
        .evidence(format!("Detected {}", environment.host_arch));
    if environment.host_arch != "x86_64" {
        check.status = CheckStatus::Fail;
        check = check
            .evidence("Avdium 0.1 targets Linux x86_64 only.")
            .hint(
                "Other architectures are future platforms. The official emulator \
                 publishes no Linux ARM64 archive.",
            );
    }
    check
}

fn check_sdk_path(environment: &Environment) -> Check {
    let mut check = Check::new("sdk.path", "SDK root", CheckStatus::Unknown);
    match (
        &environment.sdk_path,
        environment.sdk_exists,
        environment.sdk_is_dir,
    ) {
        (Some(path), true, true) => {
            check.status = CheckStatus::Pass;
            check = check.evidence(format!("{} exists", path.display()));
        }
        (Some(path), true, false) => {
            check.status = CheckStatus::Fail;
            check = check
                .evidence(format!("{} is not a directory", path.display()))
                .hint("Choose an SDK directory in Settings.");
        }
        (Some(path), false, _) => {
            check.status = CheckStatus::Warn;
            check = check
                .evidence(format!("{} does not exist yet", path.display()))
                .hint(
                    "Existing installations are used as-is. To create a new SDK, use \
                     setup, or choose a different location in Settings.",
                );
        }
        (None, _, _) => {
            check.status = CheckStatus::Unknown;
            check = check
                .evidence("No SDK root could be resolved.")
                .hint("Set ANDROID_HOME or choose an SDK directory in Settings.");
        }
    }
    check
}

fn check_sdk_layout(environment: &Environment) -> Check {
    let mut check = Check::new("sdk.layout", "SDK components", CheckStatus::Unknown);
    if !environment.sdk_is_dir {
        check = check.evidence("Not checked, because the SDK root is not a directory.");
        return check;
    }
    let mut present: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for (name, exists) in [
        ("emulator", environment.sdk_has_emulator),
        ("platform-tools", environment.sdk_has_platform_tools),
        ("cmdline-tools", environment.sdk_has_cmdline_tools),
    ] {
        if exists {
            present.push(name.to_owned());
        } else {
            missing.push(name.to_owned());
        }
    }
    check = check.evidence(format!("Present: {}", join_or_none(&present)));
    if missing.is_empty() {
        check.status = CheckStatus::Pass;
    } else {
        check.status = CheckStatus::Warn;
        check = check
            .evidence(format!("Missing: {}", missing.join(", ")))
            .hint("Missing components are installed through package management, not by hand.");
    }
    check
}

fn check_sdk_writable(environment: &Environment) -> Check {
    let mut check = Check::new("sdk.writable", "SDK is writable", CheckStatus::Unknown);
    if !environment.sdk_is_dir {
        check = check.evidence("Not checked, because the SDK root is not a directory.");
        return check;
    }
    if environment.sdk_writable {
        check.status = CheckStatus::Pass;
        check = check.evidence("The SDK root is writable by this user.");
    } else {
        check.status = CheckStatus::Warn;
        check = check
            .evidence("The SDK root is not writable by this user.")
            .hint(
                "Avdium never uses sudo. Package installation will fail until the SDK \
                 is in a location you own.",
            );
    }
    check
}

fn check_path_conflicts(environment: &Environment) -> Check {
    let mut check = Check::new("sdk.env", "Path configuration", CheckStatus::Pass);
    if environment.path_problems.is_empty() {
        check = check.evidence("No conflicting environment configuration was found.");
    } else {
        check.status = CheckStatus::Warn;
        for problem in &environment.path_problems {
            check = check.evidence(problem.clone());
        }
        check = check.hint(
            "Conflicting values are resolved by preference and reported, never hidden. \
             Avdium does not edit shell startup files.",
        );
    }
    check
}

fn check_avd_home(environment: &Environment) -> Check {
    let mut check = Check::new("avd.home", "AVD home", CheckStatus::Unknown)
        .evidence(format!("{}", environment.avd_home.display()));
    if !environment.avd_home_exists {
        check.status = CheckStatus::Warn;
        check = check
            .evidence("The AVD home does not exist, so there are no devices yet.")
            .hint("Devices are created by the emulator tooling, not by hand.");
    } else if environment.avd_count == 0 {
        check.status = CheckStatus::Warn;
        check = check.evidence("No devices were found in the AVD home.");
    } else {
        check.status = CheckStatus::Pass;
        check = check.evidence(format!("{} device(s) found", environment.avd_count));
    }
    check
}

fn check_broken_devices(environment: &Environment) -> Check {
    let mut check = Check::new("avd.broken", "Device definitions", CheckStatus::Pass);
    if environment.broken_avds.is_empty() {
        check = check.evidence("Every discovered device definition is usable.");
        return check;
    }
    check.status = CheckStatus::Warn;
    for (name, reason) in &environment.broken_avds {
        check = check.evidence(format!("{name}: {reason}"));
    }
    check = check.hint(
        "Broken devices are still listed so they can be inspected. Nothing is repaired \
         or deleted automatically.",
    );
    check
}

fn check_emulator(environment: &Environment) -> Check {
    let mut check = Check::new("tool.emulator", "Emulator", CheckStatus::Unknown);
    match &environment.emulator {
        Some(tool) => {
            check = check.evidence(format!("{}", tool.path.display()));
            match (&tool.version, &tool.error) {
                (Some(version), _) => {
                    check.status = CheckStatus::Pass;
                    check = check.evidence(format!("Version {version}"));
                }
                (None, Some(error)) => {
                    check.status = CheckStatus::Warn;
                    check = check.evidence(error.clone());
                }
                (None, None) => {
                    check.status = CheckStatus::Warn;
                    check = check.evidence("The emulator was not queried for a version.");
                }
            }
        }
        None => {
            check.status = CheckStatus::Warn;
            check = check
                .evidence("No emulator executable was found in the SDK.")
                .hint("Install the emulator package, or point Avdium at the right SDK.");
        }
    }
    check
}

fn check_adb(environment: &Environment) -> Check {
    let mut check = Check::new("tool.adb", "ADB", CheckStatus::Unknown);
    match &environment.adb {
        Some(tool) => {
            check.status = CheckStatus::Pass;
            check = check.evidence(format!("{}", tool.path.display()));
            if let Some(version) = &tool.version {
                check = check.evidence(format!("Version {version}"));
            }
        }
        None => {
            check.status = CheckStatus::Warn;
            check = check
                .evidence("No adb executable was found in the SDK.")
                .hint("ADB is required for device identity and readiness.");
        }
    }
    check
}

fn check_android_cli(environment: &Environment) -> Check {
    let mut check = Check::new("tool.android_cli", "Android CLI", CheckStatus::Unknown);
    match &environment.android_cli {
        Some(tool) => {
            check = check.evidence(format!("{}", tool.path.display()));
            match (&tool.version, &tool.error) {
                (Some(version), _) => {
                    check.status = CheckStatus::Pass;
                    check = check.evidence(format!("Version {version}"));
                }
                (None, Some(error)) => {
                    check.status = CheckStatus::Warn;
                    check = check.evidence(error.clone());
                }
                (None, None) => {
                    check.status = CheckStatus::Warn;
                    check = check.evidence("Android CLI was not queried for a version.");
                }
            }
        }
        None => {
            check.status = CheckStatus::Warn;
            check = check.evidence("Android CLI was not found.").hint(
                "Existing SDKs work without it. It is the preferred provider for \
                     package operations.",
            );
        }
    }
    check
}

fn check_java(environment: &Environment) -> Check {
    let mut check = Check::new("tool.java", "Java runtime", CheckStatus::Unknown);
    match &environment.java {
        Some(tool) => {
            let version = tool
                .version
                .clone()
                .unwrap_or_else(|| "unknown version".to_owned());
            check = check.evidence(format!("{} ({version})", tool.path.display()));
            check.status = CheckStatus::Pass;
            if environment.provider_needs_java {
                check = check.evidence("The selected provider needs Java, and one was found.");
            } else {
                check = check.evidence(
                    "The selected provider does not need Java. A system Java install is \
                     not required to use Avdium.",
                );
            }
        }
        // Not checked is not the same as not found.
        None if !environment.java_probed => {
            check = check
                .evidence("Not checked. Java is only needed by the classic tools.")
                .hint("Run the checks with tool probes to look for a Java runtime.");
        }
        None => {
            if environment.provider_needs_java {
                check.status = CheckStatus::Fail;
                check = check
                    .evidence("The selected provider needs Java, and none was found.")
                    .hint(
                        "A private runtime can be provisioned instead of installing one \
                         system-wide.",
                    );
            } else {
                check.status = CheckStatus::Pass;
                check = check.evidence(
                    "No Java was found, which is fine: the selected provider does not need it.",
                );
            }
        }
    }
    check
}

fn check_kvm(environment: &Environment) -> Check {
    let mut check = Check::new("accel.kvm", "KVM", CheckStatus::Unknown);
    match &environment.kvm {
        KvmObservation::Present {
            readable,
            writable,
            mode,
            group,
        } => {
            check = check.evidence(format!("/dev/kvm exists with mode {mode}, group {group}"));
            if *readable && *writable {
                check.status = CheckStatus::Pass;
                check =
                    check.evidence("The current user can open /dev/kvm for reading and writing.");
            } else {
                check.status = CheckStatus::Warn;
                check = check
                    .evidence("The current user cannot open /dev/kvm for reading and writing.")
                    .hint(
                        "Group membership is one possible fix, not the diagnosis. Confirm \
                         the group and the device permissions first, then ask for the \
                         change: Avdium never runs privileged commands.",
                    );
            }
        }
        KvmObservation::Missing => {
            check.status = CheckStatus::Fail;
            check = check.evidence("/dev/kvm does not exist.").hint(
                "Hardware acceleration needs virtualization enabled in firmware and the \
                 matching kernel modules loaded.",
            );
        }
        KvmObservation::Unknown => {
            check = check.evidence("The /dev/kvm check could not be performed.");
        }
    }
    check
}

fn check_acceleration(environment: &Environment) -> Check {
    let mut check = Check::new("accel.check", "Emulator acceleration", CheckStatus::Unknown);
    match &environment.accel {
        Some(status) => {
            check = check.evidence(format!(
                "emulator -accel-check reported code {}",
                status.code
            ));
            check = check.evidence(status.message.clone());
            if status.is_usable() {
                check.status = CheckStatus::Pass;
            } else {
                check.status = CheckStatus::Warn;
                check = check.hint("The emulator can still run without acceleration, but slowly.");
            }
        }
        None => {
            check = check
                .evidence("Not run. It requires executing the emulator, which is done on request.");
        }
    }
    check
}

fn check_graphics(environment: &Environment) -> Check {
    let mut check = Check::new("graphics", "Graphics", CheckStatus::Unknown);
    if !environment.dri_nodes.is_empty() {
        check = check.evidence(format!(
            "Render nodes: {}",
            environment.dri_nodes.join(", ")
        ));
    } else {
        check = check.evidence("No /dev/dri render nodes were found.");
    }
    check = check.evidence(format!(
        "Libraries found: {}",
        join_or_none(&environment.gl_libraries)
    ));
    if environment.gl_libraries.len() >= 2 {
        check.status = CheckStatus::Pass;
    } else {
        check.status = CheckStatus::Warn;
        check = check.hint(
            "The emulator defaults to automatic graphics. Only suggest a fallback that \
             the installed emulator build supports.",
        );
    }
    check
}

fn check_portals(environment: &Environment) -> Check {
    let mut check = Check::new(
        "desktop.portals",
        "Desktop integration",
        CheckStatus::Unknown,
    )
    .evidence(format!(
        "Libraries found: {}",
        join_or_none(&environment.portal_libraries)
    ));
    if environment.portal_libraries.is_empty() {
        check.status = CheckStatus::Warn;
        check = check.hint(
            "Without a desktop portal, the native folder picker is unavailable. Manual \
             path entry still works.",
        );
    } else {
        check.status = CheckStatus::Pass;
    }
    check
}

fn check_disk(environment: &Environment) -> Check {
    let mut check = Check::new("disk", "Free space", CheckStatus::Unknown);
    if environment.disks.is_empty() {
        check = check.evidence("No location could be measured.");
        return check;
    }
    let mut tight = false;
    for disk in &environment.disks {
        match disk.available_gib() {
            Some(gib) => {
                check = check.evidence(format!("{}: {gib} GiB available", disk.label));
                if gib < 10 {
                    tight = true;
                }
            }
            None => {
                check = check.evidence(format!("{}: could not be measured", disk.label));
            }
        }
    }
    if tight {
        check.status = CheckStatus::Warn;
        check = check.hint(
            "A system image is multiple gigabytes. The download and the installed copy \
             both need space.",
        );
    } else {
        check.status = CheckStatus::Pass;
    }
    check
}

fn check_memory(environment: &Environment) -> Check {
    let mut check = Check::new("memory", "System memory", CheckStatus::Unknown);
    match environment.memory_mb {
        Some(mib) => {
            check = check.evidence(format!("{} MiB total", mib));
            // A device's RAM is allocated from this, and the guest plus the
            // emulator overhead is roughly twice the configured RAM.
            if mib < 4096 {
                check.status = CheckStatus::Warn;
                check = check.hint("Choose a device RAM size below the available memory.");
            } else {
                check.status = CheckStatus::Pass;
            }
        }
        None => {
            check = check.evidence("Total memory could not be read.");
        }
    }
    check
}

fn join_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_owned()
    } else {
        items.join(", ")
    }
}

fn can_write(path: &Path) -> bool {
    // `access` avoids creating a probe file in the user's SDK.
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    // SAFETY: `access` only inspects permissions for the given path.
    unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 }
}

/// Read `/dev/kvm` permissions.
fn probe_kvm() -> KvmObservation {
    use std::os::unix::fs::MetadataExt;
    let path = Path::new("/dev/kvm");
    let Ok(metadata) = std::fs::metadata(path) else {
        return if path.exists() {
            KvmObservation::Unknown
        } else {
            KvmObservation::Missing
        };
    };
    let mode = format!("{:04o}", metadata.mode() & 0o7777);
    let group = group_name(metadata.gid()).unwrap_or_else(|| metadata.gid().to_string());
    let readable = can_read(path);
    let writable = can_write(path);
    KvmObservation::Present {
        readable,
        writable,
        mode,
        group,
    }
}

fn can_read(path: &Path) -> bool {
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    // SAFETY: `access` only inspects permissions for the given path.
    unsafe { libc::access(c_path.as_ptr(), libc::R_OK) == 0 }
}

fn group_name(gid: u32) -> Option<String> {
    let text = std::fs::read_to_string("/etc/group").ok()?;
    for line in text.lines() {
        let mut fields = line.split(':');
        let _name = fields.next()?;
        let _password = fields.next()?;
        let id = fields.next()?;
        if id.parse::<u32>().ok() == Some(gid) {
            return Some(_name.to_owned());
        }
    }
    None
}

fn probe_dri_nodes() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir("/dev/dri") else {
        return Vec::new();
    };
    let mut nodes: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    nodes.sort();
    nodes
}

fn probe_libraries(names: &[&str]) -> Vec<String> {
    const DIRECTORIES: [&str; 5] = [
        "/usr/lib",
        "/usr/lib64",
        "/usr/lib/x86_64-linux-gnu",
        "/lib",
        "/lib64",
    ];
    let mut found = Vec::new();
    for name in names {
        let present = DIRECTORIES
            .iter()
            .any(|directory| Path::new(directory).join(name).exists());
        if present {
            found.push((*name).to_owned());
        }
    }
    found
}

fn probe_memory_mb() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib / 1024)
}

fn measure(label: &str, path: &Path) -> DiskObservation {
    let (available_bytes, total_bytes) = free_space(path);
    DiskObservation {
        label: label.to_owned(),
        path: path.to_path_buf(),
        available_bytes,
        total_bytes,
    }
}

/// Free space for the filesystem holding a path.
///
/// Walks up to the nearest existing ancestor so a not-yet-created SDK root can
/// still be measured.
fn free_space(path: &Path) -> (Option<u64>, Option<u64>) {
    let mut candidate = path;
    loop {
        if candidate.exists() {
            break;
        }
        match candidate.parent() {
            Some(parent) => candidate = parent,
            None => return (None, None),
        }
    }
    let Ok(c_path) = std::ffi::CString::new(candidate.as_os_str().as_encoded_bytes()) else {
        return (None, None);
    };
    // SAFETY: `statvfs` fills a caller-provided struct and does not allocate.
    unsafe {
        let mut stats: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c_path.as_ptr(), &mut stats) != 0 {
            return (None, None);
        }
        let block = stats.f_frsize as u64;
        (
            Some(stats.f_bavail as u64 * block),
            Some(stats.f_blocks as u64 * block),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> Environment {
        Environment {
            sdk_path: Some(PathBuf::from("/sdk")),
            sdk_exists: true,
            sdk_is_dir: true,
            sdk_has_emulator: true,
            sdk_has_platform_tools: true,
            sdk_has_cmdline_tools: true,
            sdk_writable: true,
            avd_home: PathBuf::from("/avd"),
            avd_home_exists: true,
            avd_count: 2,
            broken_avds: Vec::new(),
            path_problems: Vec::new(),
            emulator: Some(ToolObservation {
                path: PathBuf::from("/sdk/emulator/emulator"),
                version: Some("36.2.12.0".to_owned()),
                error: None,
            }),
            adb: Some(ToolObservation {
                path: PathBuf::from("/sdk/platform-tools/adb"),
                version: None,
                error: None,
            }),
            android_cli: Some(ToolObservation {
                path: PathBuf::from("/home/u/.android/bin/android-cli"),
                version: Some("1.0.16406183".to_owned()),
                error: None,
            }),
            java: Some(ToolObservation {
                path: PathBuf::from("/usr/bin/java"),
                version: Some("21.0.11".to_owned()),
                error: None,
            }),
            java_probed: true,
            provider_needs_java: false,
            kvm: KvmObservation::Present {
                readable: true,
                writable: true,
                mode: "0666".to_owned(),
                group: "kvm".to_owned(),
            },
            accel: Some(AccelStatus {
                code: 0,
                message: "KVM (version 12) is installed and usable.".to_owned(),
            }),
            dri_nodes: vec!["card0".to_owned(), "renderD128".to_owned()],
            gl_libraries: vec!["libEGL.so.1".to_owned(), "libGL.so.1".to_owned()],
            portal_libraries: vec!["libgtk-3.so.0".to_owned()],
            disks: vec![DiskObservation {
                label: "SDK root".to_owned(),
                path: PathBuf::from("/sdk"),
                available_bytes: Some(120 * 1024 * 1024 * 1024),
                total_bytes: Some(500 * 1024 * 1024 * 1024),
            }],
            memory_mb: Some(32_768),
            host_arch: "x86_64".to_owned(),
        }
    }

    #[test]
    fn a_healthy_host_has_no_failures() {
        let report = evaluate(&healthy());
        let (_, warnings, failures, unknown) = report.counts();
        assert_eq!(failures, 0, "unexpected failures: {:?}", report.checks);
        assert_eq!(unknown, 0, "unexpected unknowns: {:?}", report.checks);
        assert_eq!(warnings, 0, "unexpected warnings: {:?}", report.checks);
        assert_eq!(report.worst(), CheckStatus::Pass);
        assert!(report.summary().contains("passed"));
    }

    #[test]
    fn an_unsupported_architecture_is_a_failure_with_an_explanation() {
        let environment = Environment {
            host_arch: "aarch64".to_owned(),
            ..healthy()
        };
        let report = evaluate(&environment);
        let check = report.by_id("host.arch").expect("check");
        assert_eq!(check.status, CheckStatus::Fail);
        assert!(
            check
                .hint
                .as_deref()
                .expect("hint")
                .contains("future platforms")
        );
    }

    #[test]
    fn a_missing_sdk_is_a_warning_not_a_failure() {
        let environment = Environment {
            sdk_exists: false,
            sdk_is_dir: false,
            sdk_has_emulator: false,
            sdk_has_platform_tools: false,
            sdk_has_cmdline_tools: false,
            ..healthy()
        };
        let report = evaluate(&environment);
        assert_eq!(
            report.by_id("sdk.path").expect("check").status,
            CheckStatus::Warn
        );
        assert_eq!(
            report.by_id("sdk.layout").expect("check").status,
            CheckStatus::Unknown
        );
        assert_eq!(
            report.by_id("sdk.writable").expect("check").status,
            CheckStatus::Unknown
        );
    }

    #[test]
    fn a_partial_sdk_lists_exactly_what_is_missing() {
        let environment = Environment {
            sdk_has_cmdline_tools: false,
            ..healthy()
        };
        let report = evaluate(&environment);
        let check = report.by_id("sdk.layout").expect("check");
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(
            check
                .evidence
                .iter()
                .any(|line| line.contains("Missing: cmdline-tools"))
        );
        assert!(
            !check
                .evidence
                .iter()
                .any(|line| line.contains("Missing: emulator"))
        );
    }

    #[test]
    fn a_read_only_sdk_explains_that_sudo_is_never_used() {
        let environment = Environment {
            sdk_writable: false,
            ..healthy()
        };
        let report = evaluate(&environment);
        let check = report.by_id("sdk.writable").expect("check");
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(
            check
                .hint
                .as_deref()
                .expect("hint")
                .contains("never uses sudo")
        );
    }

    #[test]
    fn environment_conflicts_are_surfaced_verbatim() {
        let environment = Environment {
            path_problems: vec![
                "SDK root: using /a (Avdium settings), ignoring /b (ANDROID_HOME)".to_owned(),
            ],
            ..healthy()
        };
        let report = evaluate(&environment);
        let check = report.by_id("sdk.env").expect("check");
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.evidence[0].contains("ignoring /b"));
    }

    #[test]
    fn broken_devices_are_listed_with_their_reasons() {
        let environment = Environment {
            broken_avds: vec![(
                "Ghost".to_owned(),
                "The AVD data directory does not exist.".to_owned(),
            )],
            ..healthy()
        };
        let report = evaluate(&environment);
        let check = report.by_id("avd.broken").expect("check");
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.evidence.iter().any(|line| line.contains("Ghost")));
        assert!(
            check
                .hint
                .as_deref()
                .expect("hint")
                .contains("Nothing is repaired")
        );
    }

    #[test]
    fn a_missing_java_is_only_a_failure_when_the_provider_needs_it() {
        let environment = Environment {
            java: None,
            java_probed: true,
            provider_needs_java: false,
            ..healthy()
        };
        let report = evaluate(&environment);
        let check = report.by_id("tool.java").expect("check");
        assert_eq!(check.status, CheckStatus::Pass);
        assert!(
            check
                .evidence
                .iter()
                .any(|line| line.contains("does not need it"))
        );

        let needed = Environment {
            java: None,
            java_probed: true,
            provider_needs_java: true,
            ..healthy()
        };
        let report = evaluate(&needed);
        let check = report.by_id("tool.java").expect("check");
        assert_eq!(check.status, CheckStatus::Fail);
        assert!(
            check
                .hint
                .as_deref()
                .expect("hint")
                .contains("private runtime")
        );
    }

    #[test]
    fn java_that_was_not_looked_for_is_unknown_not_absent() {
        let environment = Environment {
            java: None,
            java_probed: false,
            provider_needs_java: false,
            ..healthy()
        };
        let report = evaluate(&environment);
        let check = report.by_id("tool.java").expect("check");
        assert_eq!(check.status, CheckStatus::Unknown);
        assert!(
            check
                .evidence
                .iter()
                .any(|line| line.contains("Not checked"))
        );
    }

    #[test]
    fn kvm_permission_problems_are_reported_as_a_possible_cause_not_a_diagnosis() {
        let environment = Environment {
            kvm: KvmObservation::Present {
                readable: true,
                writable: false,
                mode: "0660".to_owned(),
                group: "kvm".to_owned(),
            },
            ..healthy()
        };
        let report = evaluate(&environment);
        let check = report.by_id("accel.kvm").expect("check");
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.evidence.iter().any(|line| line.contains("0660")));
        assert!(check.evidence.iter().any(|line| line.contains("kvm")));
        let hint = check.hint.as_deref().expect("hint");
        assert!(hint.contains("one possible fix, not the diagnosis"));
    }

    #[test]
    fn a_missing_kvm_device_is_a_failure() {
        let environment = Environment {
            kvm: KvmObservation::Missing,
            ..healthy()
        };
        let report = evaluate(&environment);
        let check = report.by_id("accel.kvm").expect("check");
        assert_eq!(check.status, CheckStatus::Fail);
        assert!(check.hint.as_deref().expect("hint").contains("firmware"));
    }

    #[test]
    fn acceleration_that_was_not_run_is_unknown_not_failed() {
        let environment = Environment {
            accel: None,
            ..healthy()
        };
        let report = evaluate(&environment);
        let check = report.by_id("accel.check").expect("check");
        assert_eq!(check.status, CheckStatus::Unknown);
        assert!(
            check
                .evidence
                .iter()
                .any(|line| line.contains("on request"))
        );
    }

    #[test]
    fn a_low_disk_warning_mentions_download_and_install_space() {
        let environment = Environment {
            disks: vec![DiskObservation {
                label: "SDK root".to_owned(),
                path: PathBuf::from("/sdk"),
                available_bytes: Some(2 * 1024 * 1024 * 1024),
                total_bytes: Some(500 * 1024 * 1024 * 1024),
            }],
            ..healthy()
        };
        let report = evaluate(&environment);
        let check = report.by_id("disk").expect("check");
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(
            check
                .hint
                .as_deref()
                .expect("hint")
                .contains("multiple gigabytes")
        );
    }

    #[test]
    fn unknown_is_never_reported_as_a_pass_or_a_failure() {
        let report = evaluate(&Environment::default());
        for check in &report.checks {
            if check.status == CheckStatus::Unknown {
                assert!(
                    check.evidence.iter().any(|line| {
                        line.contains("Not checked")
                            || line.contains("not be performed")
                            || line.contains("could not")
                            || line.contains("Not run")
                            || line.contains("No SDK root")
                            || line.contains("No location")
                            || line.contains("not be read")
                            || line.contains("Not checked")
                    }),
                    "unknown check {} has no explanation: {:?}",
                    check.id,
                    check.evidence
                );
            }
        }
    }

    #[test]
    fn every_check_has_a_stable_identifier() {
        let report = evaluate(&healthy());
        let ids: Vec<&str> = report.checks.iter().map(|check| check.id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len(), "duplicate check id");
        for expected in [
            "host.arch",
            "sdk.path",
            "sdk.layout",
            "sdk.writable",
            "sdk.env",
            "avd.home",
            "avd.broken",
            "tool.emulator",
            "tool.adb",
            "tool.android_cli",
            "tool.java",
            "accel.kvm",
            "accel.check",
            "graphics",
            "desktop.portals",
            "disk",
            "memory",
        ] {
            assert!(report.by_id(expected).is_some(), "missing check {expected}");
        }
    }

    #[test]
    fn low_memory_produces_a_concrete_recommendation() {
        let environment = Environment {
            memory_mb: Some(2048),
            ..healthy()
        };
        let report = evaluate(&environment);
        let check = report.by_id("memory").expect("check");
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(
            check
                .hint
                .as_deref()
                .expect("hint")
                .contains("below the available memory")
        );
    }

    #[test]
    fn probing_a_real_environment_does_not_panic() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = PathContext::resolve(
            temp.path(),
            None,
            None,
            &crate::paths::EnvSnapshot::default(),
        );
        let discovery = Discovery::default();
        let providers = ProviderSet::default();
        let environment = Environment::probe(&paths, &discovery, &providers);
        assert!(!environment.host_arch.is_empty());
        let report = evaluate(&environment);
        assert!(!report.checks.is_empty());
    }

    #[test]
    fn free_space_measurement_walks_up_to_an_existing_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let nested = temp.path().join("does/not/exist");
        let (available, total) = free_space(&nested);
        assert!(available.is_some());
        assert!(total.is_some());
    }
}
