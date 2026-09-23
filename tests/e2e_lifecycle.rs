//! End-to-end tests against real tools, a real network and a real emulator.
//!
//! **These are opt-in and ignored by default.** They are the only tests in the
//! repository that download anything, need a network, or boot a device.
//!
//! Run them with:
//!
//! ```sh
//! AVDIUM_E2E=1 cargo test --test e2e_lifecycle -- --ignored --test-threads=1
//! ```
//!
//! They operate on a **disposable SDK** (`AVDIUM_E2E_ROOT`, default
//! `/tmp/avdium-e2e`). The read-only inputs, the emulator, platform-tools and
//! system images, are symlinked from the developer's real SDK, so nothing in
//! the real SDK or the real AVD home is written to. The disposable tree is
//! created by the tests and is safe to delete at any time.
//!
//! Requirements, all reported as a skip rather than a failure when missing:
//!
//! - `AVDIUM_E2E=1`
//! - A real SDK at `ANDROID_HOME` that has the emulator, platform-tools and at
//!   least one system image
//! - `/dev/kvm` usable, for the boot steps
//! - Network access, for the download step

mod common;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Why a test cannot run here.
struct Skipped(String);

fn skip(reason: impl Into<String>) -> Skipped {
    Skipped(reason.into())
}

/// The disposable root, or an explanation of what is missing.
fn e2e_root() -> Result<PathBuf, Skipped> {
    if std::env::var("AVDIUM_E2E").ok().as_deref() != Some("1") {
        return Err(skip(
            "set AVDIUM_E2E=1 to run the end-to-end tests; they download and boot a device",
        ));
    }
    if !Path::new("/dev/kvm").exists() {
        return Err(skip("/dev/kvm is not present"));
    }
    let root = std::env::var("AVDIUM_E2E_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/avdium-e2e"));
    Ok(root)
}

/// The developer's real SDK, used read-only.
fn real_sdk() -> Result<PathBuf, Skipped> {
    let sdk = std::env::var("ANDROID_HOME")
        .map(PathBuf::from)
        .map_err(|_| skip("ANDROID_HOME is not set"))?;
    for required in ["emulator", "platform-tools", "system-images"] {
        if !sdk.join(required).exists() {
            return Err(skip(format!("{} has no {required}", sdk.display())));
        }
    }
    Ok(sdk)
}

/// Build the disposable SDK: a real, writable SDK root whose heavy read-only
/// inputs are symlinks to the developer's own copies.
fn prepare_disposable_sdk(root: &Path, real: &Path) -> PathBuf {
    let sdk = root.join("sdk");
    std::fs::create_dir_all(root.join("avd")).expect("create disposable avd home");
    std::fs::create_dir_all(&sdk).expect("create disposable sdk");
    for link in ["emulator", "platform-tools", "system-images"] {
        let target = sdk.join(link);
        if !target.exists() {
            std::os::unix::fs::symlink(real.join(link), &target).expect("symlink read-only input");
        }
    }
    sdk
}

fn first_system_image(sdk: &Path) -> Option<String> {
    let root = sdk.join("system-images");
    for api in std::fs::read_dir(&root).ok()?.flatten() {
        let api_name = api.file_name().to_string_lossy().into_owned();
        for tag in std::fs::read_dir(api.path()).ok()?.flatten() {
            let tag_name = tag.file_name().to_string_lossy().into_owned();
            for abi in std::fs::read_dir(tag.path()).ok()?.flatten() {
                let abi_name = abi.file_name().to_string_lossy().into_owned();
                if abi.path().is_dir() {
                    return Some(format!("system-images;{api_name};{tag_name};{abi_name}"));
                }
            }
        }
    }
    None
}

struct Disposable {
    sdk: PathBuf,
    avd_home: PathBuf,
    home: PathBuf,
}

impl Disposable {
    /// Run the Avdium binary against the disposable SDK.
    fn run(&self, args: &[&str]) -> std::process::Output {
        let mut command = Command::new(common::binary());
        command
            .args(args)
            .env("HOME", &self.home)
            .env("ANDROID_HOME", &self.sdk)
            .env("ANDROID_SDK_ROOT", &self.sdk)
            .env("ANDROID_USER_HOME", self.home.join(".android"))
            .env("ANDROID_EMULATOR_HOME", self.home.join(".android"))
            .env("ANDROID_AVD_HOME", &self.avd_home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"));
        command.output().expect("the binary should run")
    }

    fn adb(&self) -> PathBuf {
        // The real adb, reached through the symlink.
        self.sdk.join("platform-tools/adb")
    }

    fn emulator(&self) -> PathBuf {
        self.sdk.join("emulator/emulator")
    }
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Run `adb` and return its standard output.
fn adb(disposable: &Disposable, args: &[&str]) -> String {
    let output = Command::new(disposable.adb())
        .args(args)
        .env("ANDROID_HOME", &disposable.sdk)
        .env("ANDROID_ADB_SERVER_PORT", "5039")
        .output()
        .expect("adb should run");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Wait for a device to appear, then for the guest to finish booting.
fn wait_for_boot(disposable: &Disposable, timeout: Duration) -> Option<String> {
    let deadline = Instant::now() + timeout;
    let mut serial: Option<String> = None;
    while Instant::now() < deadline {
        if serial.is_none() {
            let devices = adb(disposable, &["devices"]);
            serial = devices
                .lines()
                .find_map(|line| line.split_whitespace().next())
                .filter(|value| value.starts_with("emulator-"))
                .map(str::to_owned);
        }
        if let Some(found) = &serial {
            let booted = adb(
                disposable,
                &["-s", found, "shell", "getprop", "sys.boot_completed"],
            );
            if booted.trim() == "1" {
                return serial;
            }
        }
        std::thread::sleep(Duration::from_secs(3));
    }
    serial
}

fn stop_emulator(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Provision the classic tools with a real download, then create a device from
/// a chosen image and exercise the whole lifecycle.
#[test]
#[ignore = "downloads 172 MiB, needs a network, a real SDK and KVM"]
fn full_lifecycle_from_provisioning_to_delete() {
    let root = match e2e_root() {
        Ok(root) => root,
        Err(Skipped(reason)) => {
            eprintln!("skipping: {reason}");
            return;
        }
    };
    let real = match real_sdk() {
        Ok(real) => real,
        Err(Skipped(reason)) => {
            eprintln!("skipping: {reason}");
            return;
        }
    };
    let sdk = prepare_disposable_sdk(&root, &real);
    let disposable = Disposable {
        sdk: sdk.clone(),
        avd_home: root.join("avd"),
        home: root.join("home"),
    };
    std::fs::create_dir_all(&disposable.home).expect("create home");

    // 1. The plan must name the missing classic tools and reuse the rest.
    let plan = disposable.run(&["provision", "plan", "--create-devices"]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let plan_text = stdout(&plan);
    assert!(
        plan_text.contains("Install classic command line tools"),
        "{plan_text}"
    );

    // 2. Install them for real, with verification.
    let install = disposable.run(&["provision", "install", "--pin", "cmdline-tools"]);
    assert!(install.status.success(), "{}", stderr(&install));
    let install_text = stdout(&install);
    assert!(install_text.contains("(verified)"), "{install_text}");
    let versioned = sdk.join("cmdline-tools/23.0/bin/avdmanager");
    assert!(
        versioned.is_file(),
        "the tools were not installed where expected"
    );

    // The installer must not have created a `latest` directory.
    assert!(!sdk.join("cmdline-tools/latest").exists());
    // Staging must be empty.
    let staging = sdk.join(".avdium-staging");
    assert_eq!(
        std::fs::read_dir(&staging)
            .map(|dir| dir.count())
            .unwrap_or(0),
        0,
        "staging was not cleaned up"
    );

    // Installing twice must refuse rather than overwrite.
    let again = disposable.run(&["provision", "install", "--pin", "cmdline-tools"]);
    assert!(!again.status.success(), "a second install must refuse");
    assert!(
        stderr(&again).contains("already exists"),
        "{}",
        stderr(&again)
    );

    // 3. Create a device from a specific, chosen image.
    let Some(image) = first_system_image(&sdk) else {
        eprintln!("skipping the lifecycle: no system image is installed");
        return;
    };
    let name = format!("Avdium_E2E_{}", std::process::id());

    let dry = disposable.run(&["create", "--name", &name, "--image", &image, "--dry-run"]);
    assert!(dry.status.success(), "{}", stderr(&dry));
    assert!(
        stdout(&dry).contains("avdmanager create avd"),
        "{}",
        stdout(&dry)
    );

    let create = disposable.run(&["create", "--name", &name, "--image", &image]);
    assert!(create.status.success(), "{}", stderr(&create));
    assert!(disposable.avd_home.join(format!("{name}.ini")).exists());

    // 4. Avdium must discover it, with the image that was asked for.
    let listed = disposable.run(&["devices"]);
    assert!(stdout(&listed).contains(&name), "{}", stdout(&listed));

    // 5. Boot it and wait for the guest to finish starting.
    let mut emulator = Command::new(disposable.emulator())
        .args([
            "-avd",
            &name,
            "-no-snapshot-load",
            "-no-window",
            "-gpu",
            "auto",
        ])
        .env("ANDROID_HOME", &sdk)
        .env("ANDROID_SDK_ROOT", &sdk)
        .env("ANDROID_AVD_HOME", &disposable.avd_home)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the emulator should start");

    let serial = wait_for_boot(&disposable, Duration::from_secs(300));
    let Some(serial) = serial else {
        stop_emulator(&mut emulator);
        panic!("the device never reported boot completion");
    };

    // The serial must map back to the device that was launched.
    let identity = adb(&disposable, &["-s", &serial, "emu", "avd", "name"]);
    assert!(
        identity.contains(&name),
        "serial {serial} identified as {identity:?}, expected {name}"
    );

    // 6. Stop it, and confirm it is gone.
    adb(&disposable, &["-s", &serial, "emu", "kill"]);
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if adb(&disposable, &["devices"]).lines().count() <= 1 {
            break;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    let remaining = adb(&disposable, &["devices"]);
    assert!(
        !remaining.contains(&serial),
        "the device is still listed after stopping: {remaining}"
    );
    stop_emulator(&mut emulator);

    // 7. Wipe through the emulator's own operation, then delete.
    let mut wipe = Command::new(disposable.emulator())
        .args([
            "-avd",
            &name,
            "-no-snapshot-load",
            "-no-window",
            "-wipe-data",
        ])
        .env("ANDROID_HOME", &sdk)
        .env("ANDROID_AVD_HOME", &disposable.avd_home)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the emulator should start for the wipe");
    if let Some(serial) = wait_for_boot(&disposable, Duration::from_secs(300)) {
        adb(&disposable, &["-s", &serial, "emu", "kill"]);
    }
    stop_emulator(&mut wipe);

    let delete = Command::new(sdk.join("cmdline-tools/23.0/bin/avdmanager"))
        .args(["delete", "avd", "-n", &name])
        .env("ANDROID_HOME", &sdk)
        .env("ANDROID_AVD_HOME", &disposable.avd_home)
        .output()
        .expect("avdmanager should run");
    assert!(
        delete.status.success(),
        "delete failed: {}",
        String::from_utf8_lossy(&delete.stderr)
    );
    assert!(!disposable.avd_home.join(format!("{name}.ini")).exists());

    // 8. It must no longer be discovered.
    let after = disposable.run(&["devices"]);
    assert!(!stdout(&after).contains(&name), "{}", stdout(&after));

    // 9. The developer's real SDK must not have gained anything.
    assert!(
        !real.join("cmdline-tools/23.0").exists(),
        "the real SDK was modified: {} appeared",
        real.join("cmdline-tools/23.0").display()
    );
}

/// The provisioning flow alone, for when a device cannot be booted.
#[test]
#[ignore = "downloads 172 MiB and needs a network"]
fn provisioning_installs_and_verifies_the_classic_tools() {
    let root = match e2e_root() {
        Ok(root) => root,
        Err(Skipped(reason)) => {
            eprintln!("skipping: {reason}");
            return;
        }
    };
    let sdk = root.join("sdk-provision-only");
    std::fs::create_dir_all(&sdk).expect("create sdk");
    let home = root.join("home-provision-only");
    std::fs::create_dir_all(&home).expect("create home");

    let output = Command::new(common::binary())
        .args(["provision", "install", "--pin", "cmdline-tools", "--sdk"])
        .arg(&sdk)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .output()
        .expect("the binary should run");

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(sdk.join("cmdline-tools/23.0/bin/sdkmanager").is_file());
    assert!(sdk.join("cmdline-tools/23.0/bin/avdmanager").is_file());

    // The record must name both the provenance and the actual digest.
    let records = home.join(".local/share/avdium/tools");
    let record_file = records.join("cmdline-tools.json");
    let record = std::fs::read_to_string(&record_file).expect("a tool record should exist");
    assert!(
        record.contains("e025545c62a8e64c7559119566a569fb1dec5f60"),
        "{record}"
    );
    assert!(record.contains("publisher"), "{record}");
    assert!(record.contains("\"verified\": true"), "{record}");
}
