//! Device runtime: launch tracking, identity and readiness.
//!
//! The emulator and ADB are driven directly. A device's state is derived from
//! observations rather than from a wrapper's exit code, because a wrapper that
//! returns once a device is ready says nothing about whether the emulator
//! process is still alive.

pub mod adb;
pub mod emulator;

pub use adb::{AdbDevice, AdbState, AdbTool, Serial};
pub use emulator::{AccelStatus, EmulatorTool, EmulatorVersion, GpuMode, StartOptions};

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The lifecycle state of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeviceState {
    /// Not running.
    Stopped,
    /// A launch is in progress and the guest is not reachable yet.
    Starting,
    /// ADB reports the device, but the system has not finished booting.
    AdbConnected,
    /// `sys.boot_completed` reports 1.
    BootComplete,
    /// ADB sees the device but it is not responding.
    Offline,
    /// A stop is in progress.
    Stopping,
    /// The last operation failed.
    Failed,
    /// Not enough evidence to decide. This is the default because assuming
    /// "stopped" from missing evidence would be a claim Avdium cannot support.
    #[default]
    Unknown,
}

impl DeviceState {
    /// Whether the device is running in some form.
    pub fn is_running(self) -> bool {
        matches!(
            self,
            Self::Starting | Self::AdbConnected | Self::BootComplete | Self::Offline
        )
    }

    /// Whether the device is fully usable.
    pub fn is_ready(self) -> bool {
        matches!(self, Self::BootComplete)
    }

    /// Whether a mutation must be refused in this state.
    ///
    /// Editing, wiping and deleting are refused whenever activity is uncertain,
    /// not only when the device is demonstrably running.
    pub fn blocks_mutation(self) -> bool {
        !matches!(self, Self::Stopped)
    }

    /// Label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Starting => "Starting",
            Self::AdbConnected => "Booting",
            Self::BootComplete => "Running",
            Self::Offline => "Offline",
            Self::Stopping => "Stopping",
            Self::Failed => "Failed",
            Self::Unknown => "Unknown",
        }
    }
}

/// Everything known about a device's runtime, independent of its configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceRuntime {
    /// The derived state.
    pub state: DeviceState,
    /// The ADB serial, once known.
    pub serial: Option<Serial>,
    /// The process id, when Avdium started the device.
    pub pid: Option<u32>,
    /// Whether Avdium owns the process.
    pub owned: bool,
    /// Extra context for the UI, especially on failure.
    pub detail: Option<String>,
}

impl DeviceRuntime {
    /// A runtime for a device that is not running.
    pub fn stopped() -> Self {
        Self {
            state: DeviceState::Stopped,
            ..Self::default()
        }
    }
}

/// Observations used to derive a state.
#[derive(Debug, Clone, Default)]
pub struct RuntimeObservation {
    /// What ADB reports for this device, if anything.
    pub adb_state: Option<AdbState>,
    /// Whether the guest reported `sys.boot_completed`.
    pub boot_completed: bool,
    /// Whether Avdium has started a launch that has not settled.
    pub launch_pending: bool,
    /// Whether a stop has been requested.
    pub stop_requested: bool,
    /// A failure that should be surfaced.
    pub last_error: Option<String>,
}

/// Derive a device state from observations.
///
/// Ordering matters. A stop request wins over everything, because reporting
/// "Running" while stopping is misleading. A recorded failure is next, then
/// boot completion, then ADB reachability, then an in-flight launch.
pub fn derive_state(observation: &RuntimeObservation) -> DeviceState {
    if observation.stop_requested {
        return DeviceState::Stopping;
    }
    if observation.last_error.is_some() {
        return DeviceState::Failed;
    }
    match &observation.adb_state {
        Some(AdbState::Device) => {
            if observation.boot_completed {
                DeviceState::BootComplete
            } else {
                DeviceState::AdbConnected
            }
        }
        Some(AdbState::Offline) => DeviceState::Offline,
        Some(AdbState::Unauthorized)
        | Some(AdbState::Bootloader)
        | Some(AdbState::Recovery)
        | Some(AdbState::NoPermissions)
        | Some(AdbState::Other(_)) => DeviceState::Failed,
        None => {
            if observation.launch_pending {
                DeviceState::Starting
            } else {
                DeviceState::Stopped
            }
        }
    }
}

/// A record of a launch that survives an application restart.
///
/// The process id is recorded but never trusted on its own: a reused pid would
/// otherwise make Avdium believe an unrelated process is an emulator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchRecord {
    /// The AVD name.
    pub avd: String,
    /// The ADB serial, when it was known.
    pub serial: Option<String>,
    /// The process id.
    pub pid: u32,
    /// When the launch started, as a Unix timestamp.
    pub started_at_unix: u64,
    /// The executable that was launched.
    pub program: PathBuf,
    /// The argument array.
    pub args: Vec<String>,
    /// Whether this was a cold boot.
    pub cold_boot: bool,
}

/// The result of checking whether a recorded launch is still ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchVerification {
    /// The process exists and its command line matches the record.
    Alive,
    /// The process no longer exists.
    Dead,
    /// A process with that id exists but is not this emulator.
    Reused,
    /// The process exists but its command line could not be read.
    Unverifiable,
}

impl LaunchRecord {
    /// Build a record for a launch that just started.
    pub fn new(avd: &str, pid: u32, program: PathBuf, args: Vec<String>, cold_boot: bool) -> Self {
        Self {
            avd: avd.to_owned(),
            serial: None,
            pid,
            started_at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0),
            program,
            args,
            cold_boot,
        }
    }

    /// Record the serial once it is known.
    pub fn with_serial(mut self, serial: Option<Serial>) -> Self {
        self.serial = serial.map(|serial| serial.as_str().to_owned());
        self
    }

    /// How long ago this launch started.
    pub fn age(&self) -> Duration {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        Duration::from_secs(now.saturating_sub(self.started_at_unix))
    }

    /// Verify that the recorded process is still this emulator.
    pub fn verify(&self) -> LaunchVerification {
        let cmdline_path = PathBuf::from(format!("/proc/{}/cmdline", self.pid));
        let Ok(raw) = std::fs::read(&cmdline_path) else {
            // No /proc entry: either it exited, or /proc is not mounted.
            return if Path::new("/proc/self/cmdline").exists() {
                LaunchVerification::Dead
            } else {
                LaunchVerification::Unverifiable
            };
        };
        if raw.is_empty() {
            return LaunchVerification::Dead;
        }
        let args: Vec<String> = raw
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect();

        // The command line must still name this AVD.
        let names_avd = args
            .windows(2)
            .any(|pair| pair[0] == "-avd" && pair[1] == self.avd);
        if !names_avd {
            return LaunchVerification::Reused;
        }
        let program_matches = args
            .first()
            .is_some_and(|first| Path::new(first).file_name() == self.program.file_name());
        if program_matches {
            LaunchVerification::Alive
        } else {
            LaunchVerification::Reused
        }
    }

    /// Write the record into a directory, named after the AVD.
    pub fn save(&self, directory: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(directory)?;
        let document = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let temporary = directory.join(format!("{}.json.tmp", sanitize(&self.avd)));
        let target = directory.join(format!("{}.json", sanitize(&self.avd)));
        std::fs::write(&temporary, document)?;
        std::fs::rename(&temporary, &target)
    }

    /// Read every record in a directory, skipping anything unreadable.
    pub fn load_all(directory: &Path) -> Vec<Self> {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return Vec::new();
        };
        let mut records: Vec<Self> = entries
            .flatten()
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|extension| extension.to_str())
                    == Some("json")
            })
            .filter_map(|entry| {
                let text = std::fs::read_to_string(entry.path()).ok()?;
                serde_json::from_str(&text).ok()
            })
            .collect();
        records.sort_by(|a, b| a.avd.cmp(&b.avd));
        records
    }

    /// Remove the record for an AVD.
    pub fn remove(directory: &Path, avd: &str) -> std::io::Result<()> {
        let target = directory.join(format!("{}.json", sanitize(avd)));
        match std::fs::remove_file(target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// Keep an AVD name usable as a file name.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stop_request_wins_over_every_other_observation() {
        let observation = RuntimeObservation {
            adb_state: Some(AdbState::Device),
            boot_completed: true,
            stop_requested: true,
            ..RuntimeObservation::default()
        };
        assert_eq!(derive_state(&observation), DeviceState::Stopping);
    }

    #[test]
    fn an_adb_device_is_not_boot_complete_until_the_guest_says_so() {
        let connected = RuntimeObservation {
            adb_state: Some(AdbState::Device),
            ..RuntimeObservation::default()
        };
        assert_eq!(derive_state(&connected), DeviceState::AdbConnected);
        assert!(!derive_state(&connected).is_ready());
        assert!(derive_state(&connected).is_running());

        let booted = RuntimeObservation {
            boot_completed: true,
            ..connected.clone()
        };
        assert_eq!(derive_state(&booted), DeviceState::BootComplete);
        assert!(derive_state(&booted).is_ready());
    }

    #[test]
    fn an_offline_device_is_offline_not_stopped() {
        let observation = RuntimeObservation {
            adb_state: Some(AdbState::Offline),
            ..RuntimeObservation::default()
        };
        assert_eq!(derive_state(&observation), DeviceState::Offline);
        assert!(derive_state(&observation).is_running());
    }

    #[test]
    fn an_unauthorized_device_is_a_failure_with_a_reason() {
        let observation = RuntimeObservation {
            adb_state: Some(AdbState::Unauthorized),
            ..RuntimeObservation::default()
        };
        assert_eq!(derive_state(&observation), DeviceState::Failed);
    }

    #[test]
    fn a_pending_launch_with_no_adb_presence_is_starting() {
        let observation = RuntimeObservation {
            launch_pending: true,
            ..RuntimeObservation::default()
        };
        assert_eq!(derive_state(&observation), DeviceState::Starting);
        assert!(derive_state(&observation).is_running());
    }

    #[test]
    fn no_observations_at_all_means_stopped() {
        assert_eq!(
            derive_state(&RuntimeObservation::default()),
            DeviceState::Stopped
        );
    }

    #[test]
    fn a_recorded_error_is_surfaced_even_when_adb_is_happy() {
        let observation = RuntimeObservation {
            adb_state: Some(AdbState::Device),
            boot_completed: true,
            last_error: Some("emulator exited with code 1".to_owned()),
            ..RuntimeObservation::default()
        };
        assert_eq!(derive_state(&observation), DeviceState::Failed);
    }

    #[test]
    fn mutations_are_blocked_whenever_activity_is_uncertain() {
        assert!(!DeviceState::Stopped.blocks_mutation());
        for state in [
            DeviceState::Starting,
            DeviceState::AdbConnected,
            DeviceState::BootComplete,
            DeviceState::Offline,
            DeviceState::Stopping,
            DeviceState::Failed,
            DeviceState::Unknown,
        ] {
            assert!(state.blocks_mutation(), "{state:?} must block mutation");
        }
    }

    #[test]
    fn launch_records_round_trip_through_a_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let record = LaunchRecord::new(
            "Pixel_9",
            4242,
            PathBuf::from("/sdk/emulator/emulator"),
            vec!["-avd".to_owned(), "Pixel_9".to_owned()],
            true,
        )
        .with_serial(Serial::parse("emulator-5554"));
        record.save(temp.path()).expect("save");

        let loaded = LaunchRecord::load_all(temp.path());
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].avd, "Pixel_9");
        assert_eq!(loaded[0].serial.as_deref(), Some("emulator-5554"));
        assert!(loaded[0].cold_boot);

        LaunchRecord::remove(temp.path(), "Pixel_9").expect("remove");
        assert!(LaunchRecord::load_all(temp.path()).is_empty());
        // Removing something that is not there is not an error.
        LaunchRecord::remove(temp.path(), "Pixel_9").expect("remove again");
    }

    #[test]
    fn a_missing_launch_directory_is_not_an_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert!(LaunchRecord::load_all(&temp.path().join("nope")).is_empty());
    }

    #[test]
    fn unreadable_launch_records_are_skipped_rather_than_failing_the_scan() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("broken.json"), "{not json").expect("write");
        assert!(LaunchRecord::load_all(temp.path()).is_empty());
    }

    #[test]
    fn a_dead_pid_is_reported_as_dead() {
        let temp = tempfile::tempdir().expect("tempdir");
        // Pick a pid that is extremely unlikely to exist.
        let mut record = LaunchRecord::new(
            "Ghost",
            0x7FFF_FFFE,
            PathBuf::from("/sdk/emulator/emulator"),
            vec!["-avd".to_owned(), "Ghost".to_owned()],
            false,
        );
        record.pid = 0x7FFF_FFFE;
        assert_eq!(record.verify(), LaunchVerification::Dead);
        let _ = temp;
    }

    #[test]
    fn a_reused_pid_is_detected_because_the_command_line_does_not_match() {
        // The current test process is certainly not an emulator for this AVD.
        let record = LaunchRecord::new(
            "Pixel_9",
            std::process::id(),
            PathBuf::from("/sdk/emulator/emulator"),
            vec!["-avd".to_owned(), "Pixel_9".to_owned()],
            false,
        );
        assert_eq!(record.verify(), LaunchVerification::Reused);
    }

    #[test]
    fn launch_age_is_never_negative() {
        let mut record = LaunchRecord::new("X", 1, PathBuf::from("/e"), Vec::new(), false);
        record.started_at_unix = u64::MAX;
        assert_eq!(record.age(), Duration::ZERO);
    }

    #[test]
    fn avd_names_are_sanitized_for_use_as_file_names() {
        assert_eq!(sanitize("Pixel_9"), "Pixel_9");
        assert_eq!(sanitize("weird/name with spaces"), "weird_name_with_spaces");
        assert_eq!(sanitize("../../etc/passwd"), ".._.._etc_passwd");
    }

    #[test]
    fn state_labels_are_stable() {
        assert_eq!(DeviceState::BootComplete.label(), "Running");
        assert_eq!(DeviceState::AdbConnected.label(), "Booting");
        assert_eq!(DeviceState::Stopped.label(), "Stopped");
    }
}
