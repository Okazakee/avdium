//! Deterministic synthetic state for demo mode.
//!
//! **Invariant:** this module performs no filesystem or subprocess I/O. It
//! cannot discover or mutate a real SDK, because it never looks at one. Demo
//! devices are built from parsed INI text with synthetic paths under
//! [`DEMO_ROOT`], and a test asserts that no demo path escapes that prefix.
//!
//! Demo mode exists so the UI can be developed and inspected without an SDK,
//! and so layout checks can cover states that are awkward to reproduce: a
//! broken device, a device with a very long name, and a busy operation.

use std::path::{Path, PathBuf};

use crate::avd::{Avd, BrokenReason, DataDirOrigin, Discovery, IniFile};
use crate::doctor::{
    Check, CheckStatus, DiskObservation, DoctorReport, Environment, KvmObservation, ToolObservation,
};
use crate::paths::{PathContext, PathOrigin};
use crate::runtime::{DeviceRuntime, DeviceState, Serial};
use crate::sdk::packages::PackageId;

/// Every synthetic path lives under this prefix.
pub const DEMO_ROOT: &str = "/demo";

/// The synthetic SDK root.
pub const DEMO_SDK: &str = "/demo/sdk";

/// The synthetic AVD home.
pub const DEMO_AVD_HOME: &str = "/demo/avd";

/// The synthetic home directory.
pub const DEMO_HOME: &str = "/demo/home";

/// A device in the demo dataset, with its synthetic runtime state.
#[derive(Debug, Clone, PartialEq)]
pub struct DemoDevice {
    /// The device definition.
    pub avd: Avd,
    /// Its synthetic runtime state.
    pub runtime: DeviceRuntime,
}

/// A package row in the demo dataset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoPackage {
    /// The identifier.
    pub id: PackageId,
    /// The installed version.
    pub installed: Option<String>,
    /// The offered version, when an update exists.
    pub available: Option<String>,
    /// The description.
    pub description: String,
    /// The declared download size, when the repository publishes one.
    pub download_size: Option<u64>,
    /// The installed size on disk, when known.
    pub installed_size: Option<u64>,
}

impl DemoPackage {
    /// Whether an update is offered.
    pub fn has_update(&self) -> bool {
        match (&self.installed, &self.available) {
            (Some(installed), Some(available)) => installed != available,
            _ => false,
        }
    }
}

/// Build the synthetic path context.
pub fn path_context() -> PathContext {
    PathContext {
        home: PathBuf::from(DEMO_HOME),
        sdk: Some(crate::paths::Resolution {
            path: PathBuf::from(DEMO_SDK),
            origin: PathOrigin::Conventional,
            overridden: Vec::new(),
        }),
        avd_home: PathBuf::from(DEMO_AVD_HOME),
        avd_home_origin: PathOrigin::Conventional,
        android_user_home: PathBuf::from("/demo/home/.android"),
        emulator_home: PathBuf::from("/demo/home/.android"),
        conflicts: Vec::new(),
        warnings: Vec::new(),
    }
}

fn config(entries: &[(&str, &str)]) -> IniFile {
    let mut ini = IniFile::new();
    for (key, value) in entries {
        ini.set(key, value);
    }
    ini
}

/// The configuration of a synthetic device, before a runtime state is attached.
struct DeviceSpec<'a> {
    name: &'a str,
    display: &'a str,
    target: &'a str,
    tag: &'a str,
    abi: &'a str,
    image_dir: &'a str,
    ram: u32,
    cores: u32,
    gpu: &'a str,
}

fn device(spec: DeviceSpec<'_>, runtime: DeviceRuntime) -> DemoDevice {
    let DeviceSpec {
        name,
        display,
        target,
        tag,
        abi,
        image_dir,
        ram,
        cores,
        gpu,
    } = spec;
    let data_dir = PathBuf::from(DEMO_AVD_HOME).join(format!("{name}.avd"));
    let config_ini = config(&[
        ("AvdId", name),
        ("avd.ini.displayname", display),
        ("abi.type", abi),
        ("hw.cpu.arch", abi),
        ("hw.cpu.ncore", &cores.to_string()),
        ("hw.device.name", &name.to_lowercase()),
        ("hw.gpu.enabled", "yes"),
        ("hw.gpu.mode", gpu),
        ("hw.keyboard", "yes"),
        ("hw.lcd.density", "420"),
        ("hw.lcd.height", "2424"),
        ("hw.lcd.width", "1080"),
        ("hw.ramSize", &ram.to_string()),
        ("image.sysdir.1", image_dir),
        ("tag.display", tag),
        ("tag.id", tag.to_lowercase().replace(' ', "_").as_str()),
        ("target", target),
    ]);
    let companion = config(&[
        ("avd.ini.encoding", "UTF-8"),
        ("path", &data_dir.to_string_lossy()),
        ("target", target),
    ]);

    DemoDevice {
        avd: Avd {
            name: name.to_owned(),
            ini_path: PathBuf::from(DEMO_AVD_HOME).join(format!("{name}.ini")),
            data_dir: data_dir.clone(),
            data_dir_origin: DataDirOrigin::AbsolutePathKey,
            config_path: data_dir.join("config.ini"),
            companion,
            config: config_ini,
            broken: Vec::new(),
        },
        runtime,
    }
}

fn running(serial: &str, pid: u32) -> DeviceRuntime {
    DeviceRuntime {
        state: DeviceState::BootComplete,
        serial: Serial::parse(serial),
        pid: Some(pid),
        owned: true,
        detail: None,
    }
}

/// The synthetic device list.
///
/// Covers the states the UI has to render: running, stopped, broken, booting
/// with a failure detail, and a name long enough to break a naive layout.
pub fn devices() -> Vec<DemoDevice> {
    let mut list = Vec::new();

    list.push(device(
        DeviceSpec {
            name: "Pixel_9",
            display: "Pixel 9",
            target: "android-36.1",
            tag: "Google APIs",
            abi: "x86_64",
            image_dir: "system-images/android-36.1/google_apis/x86_64/",
            ram: 8192,
            cores: 6,
            gpu: "auto",
        },
        running("emulator-5554", 42_424),
    ));

    list.push(device(
        DeviceSpec {
            name: "Pixel_Tablet",
            display: "Pixel Tablet",
            target: "android-37.0",
            tag: "Google Play",
            abi: "x86_64",
            image_dir: "system-images/android-37.0/google_apis_playstore_ps16k/x86_64/",
            ram: 4096,
            cores: 4,
            gpu: "host",
        },
        DeviceRuntime::stopped(),
    ));

    // A device whose image is not installed.
    let mut broken = device(
        DeviceSpec {
            name: "Pixel_9_Pro",
            display: "Pixel 9 Pro",
            target: "android-36.1",
            tag: "Google Play",
            abi: "x86_64",
            image_dir: "system-images/android-36.1/google_apis_playstore/x86_64/",
            ram: 8192,
            cores: 6,
            gpu: "auto",
        },
        DeviceRuntime::stopped(),
    );
    broken.avd.broken.push(BrokenReason::MissingImage {
        path: PathBuf::from(DEMO_SDK)
            .join("system-images/android-36.1/google_apis_playstore/x86_64"),
    });
    list.push(broken);

    // A device that is starting and has a failure to explain.
    let mut failing = device(
        DeviceSpec {
            name: "Avdium_Demo_Device_With_An_Extremely_Long_Name_For_Layout_Checks",
            display: "Demo Device With An Extremely Long Display Name For Layout Checks",
            target: "android-34",
            tag: "Google APIs",
            abi: "x86_64",
            image_dir: "system-images/android-34/google_apis/x86_64/",
            ram: 2048,
            cores: 2,
            gpu: "software",
        },
        DeviceRuntime {
            state: DeviceState::Failed,
            serial: None,
            pid: None,
            owned: false,
            detail: Some(
                "The emulator exited with code 1 before the guest became reachable.".to_owned(),
            ),
        },
    );
    failing.avd.data_dir = PathBuf::from(
        "/demo/avd/a-deliberately-very-long-data-directory-name/Avdium_Demo_Device_With_An_Extremely_Long_Name_For_Layout_Checks.avd",
    );
    list.push(failing);

    list
}

/// The synthetic discovery result.
pub fn discovery() -> Discovery {
    Discovery {
        avds: devices().into_iter().map(|device| device.avd).collect(),
        warnings: Vec::new(),
    }
}

/// Synthetic runtime states keyed by device name.
pub fn runtimes() -> Vec<(String, DeviceRuntime)> {
    devices()
        .into_iter()
        .map(|device| (device.avd.name, device.runtime))
        .collect()
}

/// A row of synthetic package data.
struct PackageRow<'a> {
    id: &'a str,
    installed: Option<&'a str>,
    available: Option<&'a str>,
    description: &'a str,
    download: Option<u64>,
    installed_size: Option<u64>,
}

/// Synthetic packages, including an available update and a large image.
pub fn packages() -> Vec<DemoPackage> {
    let rows: Vec<PackageRow<'_>> = vec![
        PackageRow {
            id: "emulator",
            installed: Some("36.2.12"),
            available: Some("37.1.11"),
            description: "Android Emulator",
            download: Some(353_795_619),
            installed_size: Some(1_200_000_000),
        },
        PackageRow {
            id: "platform-tools",
            installed: Some("36.0.0"),
            available: Some("37.0.1"),
            description: "Android SDK Platform-Tools",
            download: Some(9_054_187),
            installed_size: Some(30_000_000),
        },
        PackageRow {
            id: "build-tools/36.1.0",
            installed: Some("36.1.0"),
            available: None,
            description: "Android SDK Build-Tools 36.1",
            download: Some(64_195_204),
            installed_size: Some(210_000_000),
        },
        PackageRow {
            id: "platforms/android-36",
            installed: Some("2.0.0"),
            available: None,
            description: "Android SDK Platform 36",
            download: Some(65_878_410),
            installed_size: Some(180_000_000),
        },
        PackageRow {
            id: "system-images/android-36.1/google_apis/x86_64",
            installed: Some("4.0.0"),
            available: None,
            description: "Google APIs Intel x86_64 Atom System Image",
            download: Some(1_500_000_000),
            installed_size: Some(4_800_000_000),
        },
        PackageRow {
            id: "system-images/android-36.1/google_apis_playstore/x86_64",
            installed: Some("3.0.0"),
            available: Some("4.0.0"),
            description: "Google Play Intel x86_64 Atom System Image",
            download: Some(1_700_000_000),
            installed_size: Some(5_400_000_000),
        },
    ];

    rows.into_iter()
        .filter_map(|row| {
            Some(DemoPackage {
                id: PackageId::parse(row.id).ok()?,
                installed: row.installed.map(str::to_owned),
                available: row.available.map(str::to_owned),
                description: row.description.to_owned(),
                download_size: row.download,
                installed_size: row.installed_size,
            })
        })
        .collect()
}

/// A synthetic environment for the doctor, mixing every status.
pub fn environment() -> Environment {
    Environment {
        sdk_path: Some(PathBuf::from(DEMO_SDK)),
        sdk_exists: true,
        sdk_is_dir: true,
        sdk_has_emulator: true,
        sdk_has_platform_tools: true,
        sdk_has_cmdline_tools: false,
        sdk_writable: true,
        avd_home: PathBuf::from(DEMO_AVD_HOME),
        avd_home_exists: true,
        avd_count: devices().len(),
        broken_avds: vec![(
            "Pixel_9_Pro".to_owned(),
            "The system image /demo/sdk/system-images/android-36.1/google_apis_playstore/x86_64 is not installed."
                .to_owned(),
        )],
        path_problems: vec![
            "SDK root: using /demo/sdk (default location), ignoring /demo/other-sdk (ANDROID_HOME)"
                .to_owned(),
        ],
        emulator: Some(ToolObservation {
            path: PathBuf::from(DEMO_SDK).join("emulator/emulator"),
            version: Some("36.2.12.0".to_owned()),
            error: None,
        }),
        adb: Some(ToolObservation {
            path: PathBuf::from(DEMO_SDK).join("platform-tools/adb"),
            version: None,
            error: None,
        }),
        android_cli: Some(ToolObservation {
            path: PathBuf::from("/demo/home/.android/bin/android-cli"),
            version: Some("1.0.16406183".to_owned()),
            error: None,
        }),
        java: None,
        java_probed: true,
        provider_needs_java: false,
        kvm: KvmObservation::Present {
            readable: true,
            writable: true,
            mode: "0666".to_owned(),
            group: "kvm".to_owned(),
        },
        accel: Some(crate::runtime::AccelStatus {
            code: 0,
            message: "KVM (version 12) is installed and usable.".to_owned(),
        }),
        dri_nodes: vec!["card0".to_owned(), "renderD128".to_owned()],
        gl_libraries: vec!["libEGL.so.1".to_owned(), "libGL.so.1".to_owned()],
        portal_libraries: vec!["libgtk-3.so.0".to_owned()],
        disks: vec![
            DiskObservation {
                label: "SDK root".to_owned(),
                path: PathBuf::from(DEMO_SDK),
                available_bytes: Some(2 * 1024 * 1024 * 1024),
                total_bytes: Some(500 * 1024 * 1024 * 1024),
            },
            DiskObservation {
                label: "AVD home".to_owned(),
                path: PathBuf::from(DEMO_AVD_HOME),
                available_bytes: Some(2 * 1024 * 1024 * 1024),
                total_bytes: Some(500 * 1024 * 1024 * 1024),
            },
        ],
        memory_mb: Some(32_768),
        host_arch: "x86_64".to_owned(),
    }
}

/// A synthetic doctor report, including an explicit unknown.
pub fn doctor_report() -> DoctorReport {
    let mut report = crate::doctor::evaluate(&environment());
    report.checks.push(Check {
        id: "demo.unknown",
        title: "Network reachability".to_owned(),
        status: CheckStatus::Unknown,
        evidence: vec!["Not run. Demo mode does not touch the network.".to_owned()],
        hint: Some(
            "Reachability is only checked when setup needs it or you ask for it.".to_owned(),
        ),
    });
    report
}

/// Whether a path belongs to the synthetic demo dataset.
///
/// Used by tests to prove demo mode cannot reach a real SDK.
pub fn is_demo_path(path: &Path) -> bool {
    path.starts_with(DEMO_ROOT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_paths_never_escape_the_synthetic_root() {
        let paths = path_context();
        assert!(is_demo_path(&paths.avd_home));
        assert!(is_demo_path(&paths.android_user_home));
        assert!(is_demo_path(&paths.emulator_home));
        assert!(is_demo_path(&paths.sdk.expect("sdk").path));

        for device in devices() {
            assert!(
                is_demo_path(&device.avd.ini_path),
                "{}",
                device.avd.ini_path.display()
            );
            assert!(
                is_demo_path(&device.avd.data_dir),
                "{}",
                device.avd.data_dir.display()
            );
            assert!(is_demo_path(&device.avd.config_path));
        }
        for package in packages() {
            let id = package.id.as_str();
            assert!(!id.starts_with('/'), "{id}");
            assert!(!id.contains(".."), "{id}");
            assert!(!id.contains('\\'), "{id}");
        }
    }

    #[test]
    fn no_demo_path_points_at_a_real_home_directory() {
        let home = crate::paths::home_dir();
        for device in devices() {
            if let Some(home) = &home {
                assert!(!device.avd.data_dir.starts_with(home));
            }
            assert!(!device.avd.data_dir.starts_with("/home"));
            assert!(!device.avd.data_dir.starts_with("/root"));
        }
    }

    #[test]
    fn the_demo_dataset_is_deterministic() {
        assert_eq!(devices(), devices());
        assert_eq!(packages(), packages());
        assert_eq!(doctor_report(), doctor_report());
    }

    #[test]
    fn the_demo_dataset_covers_the_states_the_ui_must_render() {
        let devices = devices();
        assert!(
            devices
                .iter()
                .any(|device| device.runtime.state == DeviceState::BootComplete)
        );
        assert!(
            devices
                .iter()
                .any(|device| device.runtime.state == DeviceState::Stopped)
        );
        assert!(
            devices
                .iter()
                .any(|device| device.runtime.state == DeviceState::Failed)
        );
        assert!(devices.iter().any(|device| device.avd.is_broken()));
        assert!(
            devices.iter().any(|device| device.avd.name.len() > 40),
            "the demo set must include a long name for layout checks"
        );
        assert!(
            devices.iter().any(|device| device.runtime.serial.is_some()),
            "the demo set must include a device with a serial"
        );
        assert!(
            devices
                .iter()
                .any(|device| device.avd.data_dir.to_string_lossy().len() > 60)
        );
    }

    #[test]
    fn the_demo_packages_include_an_update_and_a_large_image() {
        let packages = packages();
        assert!(packages.iter().any(|package| package.has_update()));
        assert!(
            packages.iter().any(|package| package
                .download_size
                .is_some_and(|size| size > 1_000_000_000)),
            "download sizes must be visible, not just the launcher size"
        );
        assert!(
            packages
                .iter()
                .all(|package| package.installed_size.is_some())
        );
    }

    #[test]
    fn the_demo_doctor_report_includes_every_status() {
        let report = doctor_report();
        let (pass, warn, fail, unknown) = report.counts();
        assert!(pass > 0, "expected passes");
        assert!(warn > 0, "expected warnings");
        assert_eq!(fail, 0, "the demo report should not claim a broken host");
        assert!(unknown > 0, "expected at least one explicit unknown");
    }

    #[test]
    fn demo_discovery_is_broken_device_aware() {
        let discovery = discovery();
        assert_eq!(discovery.avds.len(), devices().len());
        assert!(discovery.broken().next().is_some());
        assert!(discovery.warnings.is_empty());
    }

    #[test]
    fn demo_runtime_states_match_the_demo_devices() {
        let runtimes = runtimes();
        assert_eq!(runtimes.len(), devices().len());
        let running = runtimes
            .iter()
            .find(|(name, _)| name == "Pixel_9")
            .expect("Pixel_9");
        assert_eq!(running.1.state, DeviceState::BootComplete);
        assert_eq!(
            running.1.serial.as_ref().expect("serial").as_str(),
            "emulator-5554"
        );
    }
}
