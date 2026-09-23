//! Integration tests: the built binary as a black box.
//!
//! These cover what unit tests cannot: argument parsing, environment-driven
//! path resolution, provider detection, subprocess construction and printed
//! output, all together, with fake tools and temporary directories so nothing
//! can reach a real SDK or the network.
//!
mod common;

use common::{
    ACCEL_SCENARIO, ANDROID_CLI_INFO_SCENARIO, DEVICE_CONFIG, Sandbox, sandbox_with_tools,
};

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn version_prints_without_touching_the_environment() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["--version"]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("avdium"));
    // Nothing was executed, and no state directory was created.
    assert!(!sandbox.was_called("emulator"));
    assert!(!sandbox.home.join(".config/avdium").exists());
}

#[test]
fn paths_resolves_from_the_environment() {
    let sandbox = sandbox_with_tools();
    let output = sandbox.run(&["paths"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains(&sandbox.sdk.display().to_string()), "{text}");
    assert!(text.contains("from ANDROID_HOME"), "{text}");
    assert!(
        text.contains(&sandbox.avd_home.display().to_string()),
        "{text}"
    );
}

#[test]
fn paths_reports_a_conflict_between_the_environment_and_saved_settings() {
    let sandbox = sandbox_with_tools();
    // A saved settings file pointing somewhere else.
    let config = sandbox.home.join(".config/avdium");
    std::fs::create_dir_all(&config).expect("create config");
    std::fs::write(
        config.join("settings.json"),
        r#"{"schema":1,"sdk_path":"/somewhere/else"}"#,
    )
    .expect("write settings");

    let output = sandbox.run(&["paths"]);
    assert!(output.status.success());
    let text = stdout(&output);
    // The saved choice wins and the disagreement is stated, not hidden.
    assert!(text.contains("/somewhere/else"), "{text}");
    assert!(text.contains("conflict"), "{text}");
    assert!(text.contains("ANDROID_HOME"), "{text}");
}

#[test]
fn devices_lists_a_device_with_its_image() {
    let sandbox = sandbox_with_tools();
    sandbox.add_device("Test_Device", DEVICE_CONFIG);
    // The image must exist, or the device is correctly reported as broken.
    std::fs::create_dir_all(
        sandbox
            .sdk
            .join("system-images/android-36.1/google_apis/x86_64"),
    )
    .expect("create image");

    let output = sandbox.run(&["devices"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("Test_Device"), "{text}");
    assert!(text.contains("android-36.1 Google APIs x86_64"), "{text}");
    assert!(text.contains("ok"), "{text}");
    assert!(
        text.contains(&sandbox.avd_home.display().to_string()),
        "{text}"
    );
}

#[test]
fn devices_reports_a_missing_image_instead_of_hiding_the_device() {
    let sandbox = sandbox_with_tools();
    sandbox.add_device("Broken_Device", DEVICE_CONFIG);

    let output = sandbox.run(&["devices"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(
        text.contains("Broken_Device"),
        "the device must still be listed: {text}"
    );
    assert!(text.contains("broken"), "{text}");
    assert!(text.contains("not installed"), "{text}");
}

#[test]
fn devices_json_is_valid_and_complete() {
    let sandbox = sandbox_with_tools();
    sandbox.add_device("Test_Device", DEVICE_CONFIG);
    let output = sandbox.run(&["devices", "--json"]);
    assert!(output.status.success());
    let text = stdout(&output);
    let value: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    let devices = value["devices"].as_array().expect("devices array");
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0]["name"], "Test_Device");
    assert_eq!(devices[0]["target"], "android-36.1");
    assert_eq!(devices[0]["abi"], "x86_64");
    assert_eq!(devices[0]["broken"], true);
}

#[test]
fn tools_detects_the_fake_providers_and_runs_them() {
    let sandbox = sandbox_with_tools();
    let output = sandbox.run(&["tools"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("emulator: 36.2.12.0"), "{text}");
    assert!(text.contains("android_cli: 1.0.16406183"), "{text}");
    assert!(text.contains("cmdline_tools: not found"), "{text}");
    // The fake tools really were executed.
    assert!(sandbox.was_called("emulator"));
    assert!(sandbox.was_called("android-cli"));
}

#[test]
fn capabilities_state_the_selected_image_gap_for_android_cli() {
    let sandbox = sandbox_with_tools();
    let output = sandbox.run(&["capabilities"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("provider: Android CLI"), "{text}");
    assert!(text.contains("Create from a chosen image"), "{text}");
    // The reason must name the missing capability rather than being blank.
    assert!(text.contains("no image selector"), "{text}");
    assert!(text.contains("license consent: implicit"), "{text}");
}

#[test]
fn doctor_without_probe_does_not_execute_any_tool() {
    let sandbox = sandbox_with_tools();
    sandbox.add_device("Test_Device", DEVICE_CONFIG);
    let output = sandbox.run(&["doctor"]);
    let text = stdout(&output);
    assert!(text.contains("Not checked"), "{text}");
    // Nothing was run, so nothing recorded an invocation.
    assert!(
        !sandbox.was_called("emulator"),
        "the emulator must not run without --probe"
    );
    assert!(!sandbox.was_called("android-cli"));
}

#[test]
fn doctor_with_probe_executes_the_tools_and_reports_versions() {
    let sandbox = sandbox_with_tools();
    sandbox.add_device("Test_Device", DEVICE_CONFIG);
    let output = sandbox.run(&["doctor", "--probe"]);
    let text = stdout(&output);
    assert!(text.contains("Version 36.2.12.0"), "{text}");
    assert!(text.contains("1.0.16406183"), "{text}");
    assert!(sandbox.was_called("emulator"));
    assert!(sandbox.was_called("android-cli"));
}

#[test]
fn doctor_json_parses_and_carries_every_check() {
    let sandbox = sandbox_with_tools();
    let output = sandbox.run(&["doctor", "--json"]);
    let text = stdout(&output);
    let value: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    let checks = value["checks"].as_array().expect("checks");
    assert!(checks.len() > 10);
    let ids: Vec<&str> = checks
        .iter()
        .filter_map(|check| check["id"].as_str())
        .collect();
    assert!(ids.contains(&"sdk.path"));
    assert!(ids.contains(&"tool.emulator"));
    assert!(ids.contains(&"accel.kvm"));
}

#[test]
fn provision_pins_shows_the_provenance_of_every_digest() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["provision", "pins"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("android-cli-launcher"), "{text}");
    assert!(text.contains("cmdline-tools"), "{text}");
    assert!(text.contains("reproduced by this project"), "{text}");
    assert!(
        text.contains("from the publisher's repository metadata"),
        "{text}"
    );
    assert!(text.contains("installs as: cmdline-tools/23.0"), "{text}");
}

#[test]
fn provision_plan_reuses_what_exists_and_names_what_is_missing() {
    let sandbox = sandbox_with_tools();
    sandbox.install_fake_java();
    let output = sandbox.run(&["provision", "plan", "--create-devices"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    // Existing tools are reused, never scheduled for upgrade.
    assert!(text.contains("Android CLI"), "{text}");
    assert!(text.contains("[already present]"), "{text}");
    // The one genuinely missing piece is named.
    assert!(
        text.contains("Install classic command line tools"),
        "{text}"
    );
    assert!(text.contains("172.7 MiB"), "{text}");
    // An existing Java is reused, and nothing is downloaded for it.
    assert!(
        text.contains("Reusing the Java already on this machine"),
        "{text}"
    );
}

#[test]
fn provision_plan_blocks_and_exits_nonzero_when_java_is_needed_and_absent() {
    // Creating devices needs Java, and the sandbox deliberately has none.
    let sandbox = sandbox_with_tools();
    let output = sandbox.run(&["provision", "plan", "--create-devices"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "a blocked plan must not report success"
    );
    let text = stdout(&output);
    assert!(text.contains("[blocked]"), "{text}");
    assert!(text.contains("not pinned"), "{text}");
    // It must not pretend something else will do the job.
    assert!(!text.contains("Reusing the Java"), "{text}");
}

#[test]
fn provision_plan_json_parses() {
    let sandbox = sandbox_with_tools();
    let output = sandbox.run(&["provision", "plan", "--json"]);
    let text = stdout(&output);
    let value: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert!(value["steps"].as_array().expect("steps").len() > 3);
    assert!(value["download_bytes"].is_number());
}

#[test]
fn provision_install_refuses_an_unknown_pin_without_downloading() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["provision", "install", "--pin", "not-a-pin"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("unknown pin"));
}

#[test]
fn provision_install_dry_run_touches_nothing() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&[
        "provision",
        "install",
        "--pin",
        "cmdline-tools",
        "--dry-run",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("dry run"), "{text}");
    assert!(
        text.contains("from the publisher's repository metadata"),
        "{text}"
    );
    assert!(!sandbox.sdk.join("cmdline-tools").exists());
    assert!(!sandbox.sdk.join(".avdium-staging").exists());
}

#[test]
fn create_without_classic_tools_explains_the_gap_and_substitutes_nothing() {
    // Android CLI alone is present, which cannot create from a chosen image.
    let sandbox = sandbox_with_tools();
    let output = sandbox.run(&[
        "create",
        "--name",
        "Test_Device",
        "--image",
        "system-images;android-36.1;google_apis;x86_64",
    ]);
    assert!(!output.status.success());
    let text = stderr(&output);
    assert!(text.contains("avdmanager was not found"), "{text}");
    assert!(text.contains("chosen image"), "{text}");
    // It must point at the fix rather than silently using a different tool.
    assert!(text.contains("provision install"), "{text}");
    assert!(!sandbox.avd_home.join("Test_Device.ini").exists());
}

#[test]
fn create_dry_run_prints_the_exact_argument_array() {
    let sandbox = sandbox_with_tools();
    sandbox.install_fake_classic_tools("latest");
    let output = sandbox.run(&[
        "create",
        "--name",
        "Test_Device",
        "--image",
        "system-images;android-36.1;google_apis;x86_64",
        "--dry-run",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("avdmanager create avd -n Test_Device"),
        "{text}"
    );
    // Classic tools take the semicolon form.
    assert!(
        text.contains("-k 'system-images;android-36.1;google_apis;x86_64'"),
        "{text}"
    );
    assert!(text.contains("dry run"), "{text}");
    assert!(
        !sandbox.was_called("avdmanager"),
        "a dry run must not execute anything"
    );
}

#[test]
fn create_rejects_a_package_that_is_not_a_system_image() {
    let sandbox = sandbox_with_tools();
    sandbox.install_fake_classic_tools("latest");
    let output = sandbox.run(&[
        "create",
        "--name",
        "Test_Device",
        "--image",
        "platforms;android-36",
    ]);
    assert!(!output.status.success());
    let text = stderr(&output);
    assert!(text.contains("is not a system image"), "{text}");
    // It must refuse rather than picking an image that happens to be installed.
    assert!(!sandbox.was_called("avdmanager"));
}

#[test]
fn create_rejects_a_malformed_identifier() {
    let sandbox = sandbox_with_tools();
    sandbox.install_fake_classic_tools("latest");
    let output = sandbox.run(&["create", "--name", "X", "--image", "../../etc"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("not a valid system image package identifier"));
    assert!(!sandbox.was_called("avdmanager"));
}

#[test]
fn create_passes_the_avd_home_and_the_environment_to_avdmanager() {
    let sandbox = sandbox_with_tools();
    sandbox.install_fake_classic_tools("latest");
    let output = sandbox.run(&[
        "create",
        "--name",
        "Test_Device",
        "--image",
        "system-images;android-36.1;google_apis/x86_64",
    ]);
    // The fake avdmanager succeeds and records how it was invoked.
    assert!(output.status.success(), "{}", stderr(&output));
    let record = sandbox.record("avdmanager");
    assert!(record.contains("arg1=create"), "{record}");
    assert!(record.contains("arg2=avd"), "{record}");
    assert!(record.contains("arg3=-n"), "{record}");
    assert!(record.contains("arg4=Test_Device"), "{record}");
    // The AVD home is passed explicitly so the device lands where Avdium looks.
    assert!(
        record.contains(&format!("ANDROID_AVD_HOME={}", sandbox.avd_home.display())),
        "{record}"
    );
}

#[test]
fn a_device_created_by_the_tool_is_then_discovered() {
    let sandbox = sandbox_with_tools();
    sandbox.install_fake_classic_tools("latest");
    sandbox.add_device("Test_Device", DEVICE_CONFIG);

    let output = sandbox.run(&["devices"]);
    assert!(stdout(&output).contains("Test_Device"));
}

#[test]
fn settings_are_written_into_the_isolated_config_directory() {
    let sandbox = sandbox_with_tools();
    let output = sandbox.run(&["paths"]);
    assert!(output.status.success());
    // Avdium's own directories are derived from the sandbox home, so the real
    // user's configuration is never touched by these tests.
    let config = sandbox.home.join(".config/avdium");
    assert!(
        !config.exists() || std::fs::read_dir(&config).expect("read").count() == 0,
        "a reporting command must not write settings"
    );
}

#[test]
fn every_reporting_subcommand_exits_zero_on_a_bare_sandbox() {
    let sandbox = Sandbox::new();
    for args in [
        vec!["paths"],
        vec!["tools"],
        vec!["devices"],
        vec!["capabilities"],
        vec!["provision", "pins"],
        vec!["provision", "plan"],
        vec!["doctor"],
    ] {
        let output = sandbox.run(&args);
        assert!(
            output.status.success() || output.status.code() == Some(1),
            "{args:?} exited with {:?}: {}",
            output.status.code(),
            stderr(&output)
        );
        // Nothing may panic.
        assert!(!stderr(&output).contains("panicked"), "{args:?} panicked");
    }
}

#[test]
fn a_missing_sdk_is_reported_rather_than_failing() {
    let sandbox = Sandbox::new();
    // Point at a directory that does not exist.
    let mut command = std::process::Command::new(common::binary());
    command.args(["devices"]);
    sandbox.apply_environment(&mut command);
    command.env("ANDROID_HOME", sandbox.temp.path().join("no-such-sdk"));
    command.env("ANDROID_SDK_ROOT", sandbox.temp.path().join("no-such-sdk"));
    let output = command.output().expect("run");
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stderr(&output).contains("panicked"));
}

#[test]
fn the_emulator_accel_check_is_parsed_when_the_tool_reports_it() {
    // The acceleration envelope on its own, so the parser sees exactly what a
    // real `-accel-check` prints.
    let sandbox = Sandbox::new();
    sandbox.install_fake_tool("emulator/emulator", "emulator", ACCEL_SCENARIO);
    let output = sandbox.run(&["doctor", "--probe"]);
    let text = stdout(&output);
    assert!(text.contains("accel-check reported code 0"), "{text}");
    assert!(
        text.contains("KVM (version 12) is installed and usable"),
        "{text}"
    );
    assert!(sandbox.was_called("emulator"));
}

#[test]
fn the_fake_android_cli_scenario_is_used_for_info() {
    let sandbox = Sandbox::new();
    sandbox.install_fake_android_cli(ANDROID_CLI_INFO_SCENARIO);
    let output = sandbox.run(&["tools"]);
    assert!(
        stdout(&output).contains("1.0.16406183"),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&output),
        stderr(&output)
    );
    // The telemetry opt-out is present in every invocation.
    let record = sandbox.record("android-cli");
    assert!(record.contains("--no-metrics"), "{record}");
}
