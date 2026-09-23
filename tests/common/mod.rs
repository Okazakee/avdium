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

    /// Generate a self-contained fake tool whose scenario is inlined.
    ///
    /// The scenario is translated into literal shell statements when the
    /// fixture is created, so the tool reads nothing at run time. An earlier
    /// version read a scenario file written moments before, and under parallel
    /// load that read intermittently returned only the first line. Inlining
    /// removes the whole class of problem rather than masking it.
    ///
    /// Only the directives the integration tests need are supported, and an
    /// unsupported one is a hard error so a fixture cannot silently misbehave.
    fn install_dispatch_tool(&self, path: &Path, tool_name: &str, scenario: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create tool directory");
        }
        let script = render_scenario_script(tool_name, scenario);
        std::fs::write(path, script).expect("write the generated fake tool");
        set_executable(path);
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

/// Turn scenario directives into a self-contained shell script.
///
/// Supported: `out`, `err`, `exit`, and blank or `#` comment lines. Anything
/// else panics, because an integration fixture that silently does nothing is
/// exactly the failure mode this replaced.
fn render_scenario_script(tool_name: &str, scenario: &str) -> String {
    let mut script = String::from("#!/bin/sh\n# Generated by tests/common/mod.rs. Do not edit.\n");
    script.push_str(&format!(
        "if [ -n \"${{FAKE_RECORD_DIR:-}}\" ]; then\n  {{\n    \
         printf 'argv0=%s\\n' \"$0\"\n    printf 'name=%s\\n' '{tool_name}'\n    \
         i=0\n    for a in \"$@\"; do i=$((i+1)); printf 'arg%d=%s\\n' \"$i\" \"$a\"; done\n    \
         printf 'cwd=%s\\n' \"$(pwd)\"\n    \
         printf 'ANDROID_AVD_HOME=%s\\n' \"${{ANDROID_AVD_HOME:-}}\"\n    \
         printf 'ANDROID_HOME=%s\\n' \"${{ANDROID_HOME:-}}\"\n  \
         }} >>\"$FAKE_RECORD_DIR/{tool_name}.log\" 2>/dev/null || true\nfi\n"
    ));

    for (index, line) in scenario.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (directive, rest) = match trimmed.split_once(' ') {
            Some((directive, rest)) => (directive, rest),
            None => (trimmed, ""),
        };
        match directive {
            "out" => script.push_str(&format!("printf '%s\\n' {}\n", shell_literal(rest))),
            "err" => script.push_str(&format!("printf '%s\\n' {} >&2\n", shell_literal(rest))),
            "exit" => script.push_str(&format!("exit {}\n", rest.trim())),
            other => panic!(
                "scenario line {} uses the `{other}` directive, which the integration                  fixture generator does not support. Add it to render_scenario_script, or                  use a unit test with a scenario file instead.",
                index + 1
            ),
        }
    }
    script.push_str("exit 0\n");
    script
}

/// Quote a value for a single-quoted shell literal.
fn shell_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
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
