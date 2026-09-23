//! Concrete background jobs.
//!
//! Each function here is the body of a job submitted to the
//! [`OperationCoordinator`](super::OperationCoordinator). They run on worker
//! threads and report results back to the application through a channel, so the
//! UI thread never blocks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::avd::Discovery;
use crate::doctor::{self, DoctorReport, Environment, ToolObservation};
use crate::paths::{AppDirs, PathContext};
use crate::process::RunOptions;
use crate::providers::ProviderSet;
use crate::runtime::{
    AdbTool, DeviceRuntime, DeviceState, EmulatorTool, LaunchRecord, LaunchVerification,
    RuntimeObservation, Serial, StartOptions, derive_state,
};
use crate::settings::ProviderPreference;

use super::{JobContext, JobOutcome};

/// How long a single probe command may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long the emulator gets to become reachable before it is called failed.
pub const STARTUP_GRACE: Duration = Duration::from_secs(180);

/// Everything a job needs that is not the job itself.
#[derive(Debug, Clone)]
pub struct JobEnvironment {
    /// Resolved SDK and AVD paths.
    pub paths: PathContext,
    /// Avdium's own directories, for logs and launch records.
    pub dirs: AppDirs,
    /// Detected providers.
    pub providers: ProviderSet,
}

/// Results a job sends back to the application.
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// Device definitions and detected providers.
    Discovery {
        /// The discovered devices.
        discovery: Discovery,
        /// The detected providers.
        providers: ProviderSet,
    },
    /// Runtime state for every known device, keyed by AVD name.
    Runtimes(BTreeMap<String, DeviceRuntime>),
    /// A completed doctor report.
    Doctor(Box<DoctorReport>),
    /// A message for the status bar.
    Notice(String),
    /// A failure the user should see.
    Failure(String),
    /// A folder chosen in a native picker. `target` is `sdk` or `avd_home`.
    FolderPicked {
        /// Which path the picker was for.
        target: String,
        /// The chosen folder, or `None` when the dialog was dismissed.
        path: Option<PathBuf>,
    },
}

/// A short probe command configuration.
fn probe_options() -> RunOptions {
    RunOptions {
        timeout: Some(PROBE_TIMEOUT),
        ..RunOptions::default()
    }
}

/// Re-read device definitions and provider detection.
pub fn discover(
    context: &JobContext,
    environment: &JobEnvironment,
    sender: &Sender<AppEvent>,
) -> JobOutcome {
    let sdk_root = environment
        .paths
        .sdk
        .as_ref()
        .map(|resolution| resolution.path.as_path());
    context.info(format!("Scanning {}", environment.paths.avd_home.display()));
    let discovery = crate::avd::AvdRepository::new(environment.paths.avd_home.clone())
        .with_relative_base(environment.paths.emulator_home.clone())
        .discover(sdk_root);
    context.info(format!(
        "Found {} device definition(s)",
        discovery.avds.len()
    ));
    for avd in discovery.broken() {
        if let Some(reason) = avd.primary_problem() {
            context.info(format!("{}: {}", avd.name, reason.summary()));
        }
    }
    let providers = ProviderSet::detect(&environment.paths, environment.providers.preference);
    context.info(providers.summary());

    let _ = sender.send(AppEvent::Discovery {
        discovery,
        providers,
    });
    JobOutcome::Succeeded
}

/// Probe runtime state for every known device.
///
/// This is read-only. It never starts, stops, edits or deletes anything.
pub fn probe_runtimes(
    context: &JobContext,
    environment: &JobEnvironment,
    known: Vec<String>,
    sender: &Sender<AppEvent>,
) -> JobOutcome {
    let mut runtimes: BTreeMap<String, DeviceRuntime> = BTreeMap::new();
    for name in &known {
        runtimes.insert(name.clone(), DeviceRuntime::stopped());
    }

    let launches = reap_dead_launches(context, environment);
    let pending: Vec<String> = launches
        .iter()
        .filter(|record| record.verify() == LaunchVerification::Alive)
        .map(|record| record.avd.clone())
        .collect();

    let adb = environment
        .paths
        .adb_program()
        .map(AdbTool::new)
        .map(|tool| tool.with_server_port(AdbTool::server_port_from_env()));

    let Some(adb) = adb else {
        context.info("No adb executable was found, so runtime state is limited to launch records.");
        for name in &pending {
            runtimes.insert(
                name.clone(),
                DeviceRuntime {
                    state: DeviceState::Starting,
                    owned: true,
                    pid: launches
                        .iter()
                        .find(|record| &record.avd == name)
                        .map(|record| record.pid),
                    ..DeviceRuntime::default()
                },
            );
        }
        let _ = sender.send(AppEvent::Runtimes(runtimes));
        return JobOutcome::Succeeded;
    };

    let output = match context.run(&adb.devices_spec(), &probe_options()) {
        Ok(output) => output,
        Err(error) => {
            let message = format!("adb devices could not be run: {error}");
            context.error(message.clone());
            let _ = sender.send(AppEvent::Failure(message));
            return JobOutcome::Failed("adb could not be run".to_owned());
        }
    };
    if !output.success {
        context.error(output.describe_failure());
    }

    let devices = adb.parse_devices(&output.stdout);
    context.info(format!("adb reports {} device(s)", devices.len()));

    for device in &devices {
        if !device.serial.is_emulator() {
            // Physical devices are shown by ADB but are not AVDs.
            continue;
        }
        let name = match context.run(&adb.avd_name_spec(&device.serial), &probe_options()) {
            Ok(output) if output.success => crate::runtime::adb::parse_avd_name(&output.stdout),
            Ok(output) => {
                context.info(format!(
                    "{}: avd name query failed ({})",
                    device.serial,
                    output.describe_failure()
                ));
                None
            }
            Err(error) => {
                context.info(format!(
                    "{}: avd name query failed ({error})",
                    device.serial
                ));
                None
            }
        };

        let boot_completed = if device.state.is_queryable() {
            match context.run(&adb.boot_completed_spec(&device.serial), &probe_options()) {
                Ok(output) => crate::runtime::adb::parse_boot_completed(&output.stdout),
                Err(_) => false,
            }
        } else {
            false
        };

        // Prefer the guest's own answer about which AVD it is. Fall back to a
        // launch record only when the guest cannot be asked.
        let avd_name = name.or_else(|| {
            launches
                .iter()
                .find(|record| record.serial.as_deref() == Some(device.serial.as_str()))
                .map(|record| record.avd.clone())
        });

        let Some(avd_name) = avd_name else {
            context.info(format!(
                "{}: could not be mapped to an AVD, so it is not attributed to any device",
                device.serial
            ));
            continue;
        };

        let state = derive_state(&RuntimeObservation {
            adb_state: Some(device.state.clone()),
            boot_completed,
            launch_pending: false,
            stop_requested: false,
            last_error: None,
        });

        let record = launches.iter().find(|record| record.avd == avd_name);
        runtimes.insert(
            avd_name.clone(),
            DeviceRuntime {
                state,
                serial: Some(device.serial.clone()),
                pid: record.map(|record| record.pid),
                owned: record.is_some(),
                detail: None,
            },
        );
    }

    for record in &launches {
        if runtimes
            .get(&record.avd)
            .is_some_and(|runtime| runtime.serial.is_none())
        {
            let state = derive_state(&RuntimeObservation {
                launch_pending: true,
                ..RuntimeObservation::default()
            });
            runtimes.insert(
                record.avd.clone(),
                DeviceRuntime {
                    state,
                    serial: record.serial.as_deref().and_then(Serial::parse),
                    pid: Some(record.pid),
                    owned: true,
                    detail: Some("Started by Avdium and not reachable over ADB yet.".to_owned()),
                },
            );
        }
    }

    let _ = sender.send(AppEvent::Runtimes(runtimes));
    JobOutcome::Succeeded
}

/// Reap launch records whose process is gone, and return the live ones.
fn reap_dead_launches(context: &JobContext, environment: &JobEnvironment) -> Vec<LaunchRecord> {
    let directory = environment.dirs.launches_dir();
    let mut live = Vec::new();
    for record in LaunchRecord::load_all(&directory) {
        match record.verify() {
            LaunchVerification::Alive => live.push(record),
            LaunchVerification::Dead => {
                // Reap the zombie so it does not accumulate while the GUI runs.
                // SAFETY: `waitpid` on a specific pid with `WNOHANG` is
                // non-blocking and only affects our own child.
                unsafe {
                    libc::waitpid(
                        record.pid as libc::pid_t,
                        std::ptr::null_mut(),
                        libc::WNOHANG,
                    );
                }
                context.info(format!(
                    "{}: the emulator process exited after {}s",
                    record.avd,
                    record.age().as_secs()
                ));
                let _ = LaunchRecord::remove(&directory, &record.avd);
            }
            LaunchVerification::Reused | LaunchVerification::Unverifiable => {
                context.info(format!(
                    "{}: the recorded process id no longer identifies this emulator, so the \
                     record was dropped rather than trusted",
                    record.avd
                ));
                let _ = LaunchRecord::remove(&directory, &record.avd);
            }
        }
    }
    live
}

/// Run the doctor, optionally executing tool probes.
pub fn run_doctor(
    context: &JobContext,
    environment: &JobEnvironment,
    discovery: Discovery,
    probe: bool,
    sender: &Sender<AppEvent>,
) -> JobOutcome {
    context.info("Collecting environment observations");
    let mut observations =
        Environment::probe(&environment.paths, &discovery, &environment.providers);

    if probe {
        if let Some(emulator) = observations.emulator.clone() {
            let tool = EmulatorTool::new(&emulator.path);
            match context.run(&tool.version_spec(), &probe_options()) {
                Ok(output) if output.success => {
                    if let Some(version) = tool.parse_version(&output.stdout)
                        && let Some(entry) = observations.emulator.as_mut()
                    {
                        entry.version = Some(version.version);
                    }
                }
                Ok(output) => {
                    if let Some(entry) = observations.emulator.as_mut() {
                        entry.error = Some(output.describe_failure());
                    }
                }
                Err(error) => {
                    if let Some(entry) = observations.emulator.as_mut() {
                        entry.error = Some(error.to_string());
                    }
                }
            }
            if let Ok(output) = context.run(&tool.accel_check_spec(), &probe_options()) {
                observations.accel = Some(tool.parse_accel_check(&output.stdout));
            }
        }

        if let Some(cli) = &environment.providers.android_cli {
            let mut tool = cli.clone();
            tool.sdk = environment
                .paths
                .sdk
                .as_ref()
                .map(|resolution| resolution.path.clone());
            match context.run(&tool.info_spec(), &probe_options()) {
                Ok(output) if output.success => {
                    let info = crate::providers::android_cli::parse_info(&output.stdout);
                    observations.android_cli = Some(ToolObservation {
                        path: tool.program.clone(),
                        version: info.version,
                        error: None,
                    });
                }
                Ok(output) => {
                    observations.android_cli = Some(ToolObservation {
                        path: tool.program.clone(),
                        version: None,
                        error: Some(output.describe_failure()),
                    });
                }
                Err(error) => {
                    observations.android_cli = Some(ToolObservation {
                        path: tool.program.clone(),
                        version: None,
                        error: Some(error.to_string()),
                    });
                }
            }
        }

        observations.java = probe_java(context);
        observations.java_probed = true;
    }

    let report = doctor::evaluate(&observations);
    context.info(report.summary());
    let _ = sender.send(AppEvent::Doctor(Box::new(report)));
    JobOutcome::Succeeded
}

/// Look for a Java runtime without requiring one.
fn probe_java(context: &JobContext) -> Option<ToolObservation> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(java_home) = std::env::var_os("JAVA_HOME") {
        candidates.push(PathBuf::from(java_home).join("bin/java"));
    }
    if let Some(path_var) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path_var).map(|dir| dir.join("java")));
    }
    for candidate in candidates {
        if !crate::process::is_executable(&candidate) {
            continue;
        }
        let output = context.run(
            &crate::process::CommandSpec::new(&candidate).arg("-version"),
            &probe_options(),
        );
        let version = output.ok().and_then(|output| {
            // `java -version` prints to stderr.
            let text = format!("{}{}", output.stdout, output.stderr);
            text.lines().next().and_then(parse_java_version_line)
        });
        return Some(ToolObservation {
            path: candidate,
            version,
            error: None,
        });
    }
    None
}

fn parse_java_version_line(line: &str) -> Option<String> {
    let start = line.find('"')? + 1;
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

/// Start a device.
///
/// The emulator is spawned detached with its output redirected to a log file,
/// so it keeps running after the GUI closes and cannot fill an unread pipe. The
/// job returns as soon as the process is spawned: a successful spawn is not
/// readiness, and the UI learns the real state from the runtime probe.
pub fn start_device(
    context: &JobContext,
    environment: &JobEnvironment,
    avd: &str,
    options: &StartOptions,
    sender: &Sender<AppEvent>,
) -> JobOutcome {
    let Some(program) = environment.paths.emulator_program() else {
        let message = "No emulator executable was found in the selected SDK.".to_owned();
        context.error(message.clone());
        let _ = sender.send(AppEvent::Failure(message));
        return JobOutcome::Failed("the emulator executable is missing".to_owned());
    };

    let tool = EmulatorTool::new(&program);
    let spec = tool.start_spec(avd, options);
    context.set_command(spec.preview());

    let logs = environment.dirs.logs_dir();
    if let Err(error) = std::fs::create_dir_all(&logs) {
        let message = format!("Could not create {}: {error}", logs.display());
        context.error(message.clone());
        let _ = sender.send(AppEvent::Failure(message));
        return JobOutcome::Failed("the log directory could not be created".to_owned());
    }
    let log_path = logs.join(format!("{}.emulator.log", sanitize(avd)));
    let log = match std::fs::File::create(&log_path) {
        Ok(file) => file,
        Err(error) => {
            let message = format!("Could not create {}: {error}", log_path.display());
            context.error(message.clone());
            let _ = sender.send(AppEvent::Failure(message));
            return JobOutcome::Failed("the emulator log could not be created".to_owned());
        }
    };
    let log_for_stderr = match log.try_clone() {
        Ok(file) => file,
        Err(error) => {
            let message = format!("Could not prepare the emulator log: {error}");
            context.error(message.clone());
            let _ = sender.send(AppEvent::Failure(message));
            return JobOutcome::Failed("the emulator log could not be prepared".to_owned());
        }
    };

    let mut command = std::process::Command::new(&spec.program);
    command.args(&spec.args);
    for (key, value) in environment.paths.child_env() {
        command.env(key, value);
    }
    for (key, value) in &spec.env {
        command.env(key, value);
    }
    command.stdin(Stdio::null());
    command.stdout(Stdio::from(log));
    command.stderr(Stdio::from(log_for_stderr));

    // Own process group so a later stop can signal the whole emulator tree.
    // SAFETY: `setpgid` only affects the child's own process group.
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let message = format!("The emulator could not be started: {error}");
            context.error(message.clone());
            let _ = sender.send(AppEvent::Failure(message));
            return JobOutcome::Failed("the emulator could not be started".to_owned());
        }
    };
    let pid = child.id();
    // Dropping the handle detaches the process. Its exit is detected later by
    // verifying the launch record, which also reaps the zombie.
    drop(child);

    let arguments: Vec<String> = spec
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let record = LaunchRecord::new(avd, pid, program.clone(), arguments, options.cold_boot);
    if let Err(error) = record.save(&environment.dirs.launches_dir()) {
        context.error(format!("The launch could not be recorded: {error}"));
    }

    context.info(format!("Started {avd} with pid {pid}"));
    context.info(format!(
        "Emulator output is being written to {}",
        log_path.display()
    ));
    let _ = sender.send(AppEvent::Notice(format!(
        "{avd} is starting. Readiness is reported once the guest finishes booting."
    )));
    JobOutcome::Succeeded
}

/// Stop a device.
pub fn stop_device(
    context: &JobContext,
    environment: &JobEnvironment,
    avd: &str,
    serial: Option<Serial>,
    sender: &Sender<AppEvent>,
) -> JobOutcome {
    let mut stopped = false;

    // Preferred path: ask the guest to shut down through its own console.
    if let (Some(serial), Some(program)) = (serial.as_ref(), environment.paths.adb_program()) {
        let adb = AdbTool::new(program).with_server_port(AdbTool::server_port_from_env());
        let spec = adb.emu_kill_spec(serial);
        match context.run(&spec, &probe_options()) {
            Ok(output) if output.success => {
                context.info(format!("Asked {serial} to shut down"));
                stopped = true;
            }
            Ok(output) => {
                context.info(format!("{serial}: {}", output.describe_failure()));
            }
            Err(error) => {
                context.info(format!("{serial}: adb could not be run ({error})"));
            }
        }
    }

    // Fall back to signalling the process group Avdium started.
    let directory = environment.dirs.launches_dir();
    let records = LaunchRecord::load_all(&directory);
    if let Some(record) = records.iter().find(|record| record.avd == avd) {
        match record.verify() {
            LaunchVerification::Alive => {
                // SAFETY: `killpg` signals a process group created by us.
                unsafe {
                    libc::killpg(record.pid as libc::pid_t, libc::SIGTERM);
                }
                context.info(format!(
                    "Signalled the emulator process group ({})",
                    record.pid
                ));
                stopped = true;
            }
            LaunchVerification::Dead => unsafe {
                libc::waitpid(
                    record.pid as libc::pid_t,
                    std::ptr::null_mut(),
                    libc::WNOHANG,
                );
            },
            LaunchVerification::Reused | LaunchVerification::Unverifiable => {
                context.info(
                    "The recorded process id no longer identifies this emulator, so it was \
                     not signalled.",
                );
            }
        }
        let _ = LaunchRecord::remove(&directory, avd);
    }

    if stopped {
        let _ = sender.send(AppEvent::Notice(format!("{avd} is shutting down.")));
        JobOutcome::Succeeded
    } else {
        let message = format!("No running emulator was found for {avd}.");
        context.info(message.clone());
        let _ = sender.send(AppEvent::Notice(message));
        JobOutcome::Succeeded
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

/// The mutation key for a device.
pub fn avd_mutation_key(name: &str) -> String {
    format!("avd:{name}")
}

/// The mutation key for the SDK.
pub fn sdk_mutation_key() -> String {
    "sdk".to_owned()
}

/// Whether a provider preference needs Java.
pub fn provider_needs_java(preference: ProviderPreference, providers: &ProviderSet) -> bool {
    match preference {
        ProviderPreference::CmdlineTools => true,
        ProviderPreference::AndroidCli => false,
        ProviderPreference::Auto => {
            providers.android_cli.is_none() && providers.cmdline_tools.is_some()
        }
    }
}

/// The log file path an emulator writes to.
pub fn emulator_log_path(dirs: &AppDirs, avd: &str) -> PathBuf {
    dirs.logs_dir()
        .join(format!("{}.emulator.log", sanitize(avd)))
}

/// Whether a path is inside the staging directory.
pub fn is_staged(dirs: &AppDirs, path: &Path) -> bool {
    path.starts_with(dirs.staging_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::OperationCoordinator;
    use crate::paths::EnvSnapshot;
    use std::sync::mpsc;

    fn environment() -> (tempfile::TempDir, JobEnvironment) {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = PathContext::resolve(temp.path(), None, None, &EnvSnapshot::default());
        let dirs = AppDirs::from_roots(
            temp.path().join("config"),
            temp.path().join("cache"),
            temp.path().join("data"),
            temp.path().join("state"),
        );
        let providers = ProviderSet::detect(&paths, ProviderPreference::Auto);
        (
            temp,
            JobEnvironment {
                paths,
                dirs,
                providers,
            },
        )
    }

    #[test]
    fn discovery_reports_definitions_without_touching_them() {
        let (temp, environment) = environment();
        let avd_home = temp.path().join(".android").join("avd");
        std::fs::create_dir_all(avd_home.join("Demo.avd")).expect("create");
        std::fs::write(
            avd_home.join("Demo.avd").join("config.ini"),
            "target=android-36\n",
        )
        .expect("write");
        std::fs::write(avd_home.join("Demo.ini"), "path.rel=avd/Demo.avd\n").expect("write");
        let before = std::fs::read_to_string(avd_home.join("Demo.ini")).expect("read");

        let coordinator = OperationCoordinator::new();
        let (sender, receiver) = mpsc::channel();
        let id = coordinator
            .submit("discover", None, move |context| {
                discover(&context, &environment, &sender)
            })
            .expect("submit");
        crate::operations::wait_for(&coordinator, id, Duration::from_secs(10));

        match receiver.try_recv().expect("event") {
            AppEvent::Discovery { discovery, .. } => {
                assert_eq!(discovery.avds.len(), 1);
                assert_eq!(discovery.avds[0].name, "Demo");
            }
            other => panic!("unexpected event: {other:?}"),
        }
        // Discovery must not rewrite the user's files.
        assert_eq!(
            std::fs::read_to_string(avd_home.join("Demo.ini")).expect("read"),
            before
        );
    }

    #[test]
    fn probing_runtimes_without_adb_still_reports_known_devices() {
        let (_temp, environment) = environment();
        let coordinator = OperationCoordinator::new();
        let (sender, receiver) = mpsc::channel();
        let id = coordinator
            .submit("probe", None, move |context| {
                probe_runtimes(&context, &environment, vec!["Demo".to_owned()], &sender)
            })
            .expect("submit");
        crate::operations::wait_for(&coordinator, id, Duration::from_secs(10));

        let mut found = None;
        while let Ok(event) = receiver.try_recv() {
            if let AppEvent::Runtimes(runtimes) = event {
                found = Some(runtimes);
            }
        }
        let runtimes = found.expect("runtimes event");
        assert_eq!(runtimes.len(), 1);
        assert_eq!(runtimes["Demo"].state, DeviceState::Stopped);
    }

    #[test]
    fn starting_without_an_emulator_fails_with_a_reason() {
        let (_temp, environment) = environment();
        let coordinator = OperationCoordinator::new();
        let (sender, receiver) = mpsc::channel();
        let id = coordinator
            .submit("start", None, move |context| {
                start_device(
                    &context,
                    &environment,
                    "Demo",
                    &StartOptions::default(),
                    &sender,
                )
            })
            .expect("submit");
        let job =
            crate::operations::wait_for(&coordinator, id, Duration::from_secs(10)).expect("job");
        assert!(matches!(job.state, crate::operations::JobState::Failed(_)));
        match receiver.try_recv().expect("event") {
            AppEvent::Failure(message) => assert!(message.contains("emulator executable")),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn stopping_a_device_that_is_not_running_is_not_an_error() {
        let (_temp, environment) = environment();
        let coordinator = OperationCoordinator::new();
        let (sender, _receiver) = mpsc::channel();
        let id = coordinator
            .submit("stop", None, move |context| {
                stop_device(&context, &environment, "Demo", None, &sender)
            })
            .expect("submit");
        let job =
            crate::operations::wait_for(&coordinator, id, Duration::from_secs(10)).expect("job");
        assert_eq!(job.state, crate::operations::JobState::Succeeded);
        assert!(job.log.text().contains("No running emulator"));
    }

    #[test]
    fn the_doctor_job_produces_a_report() {
        let (_temp, environment) = environment();
        let coordinator = OperationCoordinator::new();
        let (sender, receiver) = mpsc::channel();
        let id = coordinator
            .submit("doctor", None, move |context| {
                run_doctor(&context, &environment, Discovery::default(), false, &sender)
            })
            .expect("submit");
        crate::operations::wait_for(&coordinator, id, Duration::from_secs(10));

        let mut report = None;
        while let Ok(event) = receiver.try_recv() {
            if let AppEvent::Doctor(value) = event {
                report = Some(value);
            }
        }
        let report = report.expect("doctor report");
        assert!(report.by_id("sdk.path").is_some());
        assert!(report.checks.len() > 10);
    }

    #[test]
    fn java_version_lines_are_parsed_from_the_quoted_form() {
        assert_eq!(
            parse_java_version_line(r#"openjdk version "21.0.11" 2026-04-21"#),
            Some("21.0.11".to_owned())
        );
        assert_eq!(parse_java_version_line("no quotes here"), None);
    }

    #[test]
    fn mutation_keys_name_the_resource() {
        assert_eq!(avd_mutation_key("Pixel_9"), "avd:Pixel_9");
        assert_eq!(sdk_mutation_key(), "sdk");
    }

    #[test]
    fn emulator_log_paths_are_sanitized() {
        let dirs = AppDirs::from_roots("/c", "/ca", "/d", "/s");
        let path = emulator_log_path(&dirs, "weird/name");
        assert_eq!(path, PathBuf::from("/s/logs/weird_name.emulator.log"));
    }

    #[test]
    fn staging_membership_is_detected() {
        let dirs = AppDirs::from_roots("/c", "/ca", "/d", "/s");
        assert!(is_staged(&dirs, &dirs.staging_dir().join("x")));
        assert!(!is_staged(&dirs, Path::new("/tmp/x")));
    }

    #[test]
    fn a_provider_preference_decides_whether_java_is_needed() {
        let providers = ProviderSet::default();
        assert!(!provider_needs_java(
            ProviderPreference::AndroidCli,
            &providers
        ));
        assert!(provider_needs_java(
            ProviderPreference::CmdlineTools,
            &providers
        ));
        assert!(!provider_needs_java(ProviderPreference::Auto, &providers));
    }
}
