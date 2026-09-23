//! Shared helpers for the integration tests.
//!
//! Integration tests treat the built binary as a black box: they spawn it with
//! an isolated environment and read what it prints. That exercises argument
//! parsing, path resolution, provider detection, subprocess construction and
//! output formatting together, which unit tests deliberately do not.
//!
//! Everything here is offline. Tools are fake executables that record their
//! arguments, and every directory is temporary, so a test can never reach a
//! real SDK, a real AVD or the network.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The binary under test.
pub fn binary() -> PathBuf {
    // Provided by Cargo for integration tests.
    PathBuf::from(env!("CARGO_BIN_EXE_avdium"))
}

/// The fixture directory.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The recording fake tool.
pub fn fake_tool() -> PathBuf {
    fixtures().join("bin/fake-tool")
}

/// A disposable environment: an SDK, an AVD home, a user home, and Avdium's own
/// XDG directories, all under one temporary tree.
pub struct Sandbox {
    /// The temporary root. Dropped at the end of the test.
    pub temp: tempfile::TempDir,
    /// The fake SDK root.
    pub sdk: PathBuf,
    /// The AVD home.
    pub avd_home: PathBuf,
    /// The user home.
    pub home: PathBuf,
    /// Where fake tools write their invocation records.
    pub records: PathBuf,
    /// Where per-tool scenario files live, dispatched by tool name.
    pub scenarios: PathBuf,
}

impl Sandbox {
    /// Create an empty sandbox with a usable SDK layout.
    pub fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().to_path_buf();
        let sdk = root.join("sdk");
        let avd_home = root.join("home/.android/avd");
        let home = root.join("home");
        let records = root.join("records");
        let scenarios = root.join("scenarios");
        for directory in [&sdk, &avd_home, &home, &records, &scenarios] {
            std::fs::create_dir_all(directory).expect("create sandbox directory");
        }
        Self {
            temp,
            sdk,
            avd_home,
            home,
            records,
            scenarios,
        }
    }

    /// Install the fake tool itself at a path, dispatching on its own name.
    ///
    /// A symlink to the shared fake tool, not a copy. The shared file is
    /// written once when the repository is prepared and never opened for
    /// writing afterwards, so executing it cannot race with a writer. That
    /// matters: a newly written executable can intermittently fail to exec,
    /// which made these tests produce empty output under load.
    fn install_dispatch_tool(&self, path: &Path, tool_name: &str, scenario: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create tool directory");
        }
        std::fs::write(
            self.scenarios.join(format!("{tool_name}.scenario")),
            scenario,
        )
        .expect("write scenario");
        std::os::unix::fs::symlink(fake_tool(), path).expect("link the fake tool");
    }

    /// Install a fake executable at a path inside the sandbox.
    pub fn install_fake_tool(&self, relative: &str, tool_name: &str, scenario: &str) -> PathBuf {
        let path = self.sdk.join(relative);
        self.install_dispatch_tool(&path, tool_name, scenario);
        path
    }

    /// Install a fake Android CLI in `$HOME/.android/bin`, which is where the
    /// official installer puts it.
    pub fn install_fake_android_cli(&self, scenario: &str) -> PathBuf {
        let path = self.home.join(".android/bin/android-cli");
        self.install_dispatch_tool(&path, "android-cli", scenario);
        path
    }

    /// Install a fake classic-tools layout.
    pub fn install_fake_classic_tools(&self, version: &str) -> PathBuf {
        let bin = self.sdk.join("cmdline-tools").join(version).join("bin");
        for tool in ["sdkmanager", "avdmanager"] {
            self.install_fake_tool(
                &format!("cmdline-tools/{version}/bin/{tool}"),
                tool,
                "out usage\nexit 0\n",
            );
        }
        bin
    }

    /// Install a fake Java runtime. `JAVA_HOME` is set to it by
    /// [`Sandbox::apply_environment`] when this directory exists.
    pub fn install_fake_java(&self) -> PathBuf {
        let path = self.home.join("jdk/bin/java");
        self.install_dispatch_tool(
            &path,
            "java",
            "out openjdk version \"21.0.11\" 2026-04-21\nexit 0\n",
        );
        path
    }

    /// Create a device definition in the AVD home.
    pub fn add_device(&self, name: &str, config: &str) -> PathBuf {
        let data = self.avd_home.join(format!("{name}.avd"));
        std::fs::create_dir_all(&data).expect("create data directory");
        std::fs::write(data.join("config.ini"), config).expect("write config");
        let ini = self.avd_home.join(format!("{name}.ini"));
        std::fs::write(
            &ini,
            format!(
                "avd.ini.encoding=UTF-8\npath={}\ntarget=android-36.1\n",
                data.display()
            ),
        )
        .expect("write companion");
        ini
    }

    /// Run the binary with the sandbox environment.
    pub fn run(&self, args: &[&str]) -> Output {
        let mut command = Command::new(binary());
        command.args(args);
        self.apply_environment(&mut command);
        command.output().expect("the binary should run")
    }

    /// Apply the isolated environment to a command.
    pub fn apply_environment(&self, command: &mut Command) {
        command
            .env("HOME", &self.home)
            .env("ANDROID_HOME", &self.sdk)
            .env("ANDROID_SDK_ROOT", &self.sdk)
            .env("ANDROID_USER_HOME", self.home.join(".android"))
            .env("ANDROID_EMULATOR_HOME", self.home.join(".android"))
            .env("ANDROID_AVD_HOME", &self.avd_home)
            // Avdium's own state must not touch the real user's directories.
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("FAKE_RECORD_DIR", &self.records)
            .env("FAKE_SCENARIO_DIR", &self.scenarios)
            // A PATH with nothing usable on it, so no test can accidentally
            // find a real android, sdkmanager, avdmanager, emulator, adb or java.
            .env("PATH", self.temp.path().join("empty-path"))
            // Neutralize anything that could confuse provider detection.
            .env_remove("JAVA_HOME")
            .env_remove("RUST_LOG");

        // A fake runtime is opted into by installing one, never inherited from
        // the machine running the tests.
        let jdk = self.home.join("jdk");
        if jdk.is_dir() {
            command.env("JAVA_HOME", jdk);
        }
    }

    /// The invocation record for a fake tool, or an empty string if it was
    /// never called.
    pub fn record(&self, tool: &str) -> String {
        std::fs::read_to_string(self.records.join(format!("{tool}.log"))).unwrap_or_default()
    }

    /// Whether a fake tool was called at all.
    pub fn was_called(&self, tool: &str) -> bool {
        self.records.join(format!("{tool}.log")).exists()
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("chmod");
}

/// The standard device configuration used by the integration tests.
pub const DEVICE_CONFIG: &str = "\
AvdId=Test_Device
abi.type=x86_64
avd.ini.displayname=Test Device
hw.cpu.arch=x86_64
hw.cpu.ncore=4
hw.gpu.mode=auto
hw.keyboard=yes
hw.ramSize=4096
image.sysdir.1=system-images/android-36.1/google_apis/x86_64/
tag.display=Google APIs
tag.id=google_apis
target=android-36.1
";

/// A fake emulator that reports a version and usable acceleration.
pub const EMULATOR_SCENARIO: &str = "\
out Android emulator version 36.2.12.0 (build_id 14214601) (CL:N/A)
exit 0
";

/// A fake `emulator -accel-check` response.
pub const ACCEL_SCENARIO: &str = "\
out accel:
out 0
out KVM (version 12) is installed and usable.
out accel
exit 0
";

/// A fake `android info` response.
pub const ANDROID_CLI_INFO_SCENARIO: &str = "\
out sdk: /sandbox/sdk
out version: 1.0.16406183
exit 0
";

/// Create a sandbox with the emulator, ADB and Android CLI installed.
pub fn sandbox_with_tools() -> Sandbox {
    let sandbox = Sandbox::new();
    sandbox.install_fake_tool("emulator/emulator", "emulator", EMULATOR_SCENARIO);
    sandbox.install_fake_tool(
        "platform-tools/adb",
        "adb",
        "out List of devices attached\nexit 0\n",
    );
    sandbox.install_fake_android_cli(ANDROID_CLI_INFO_SCENARIO);
    sandbox
}
