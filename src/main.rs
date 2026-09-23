//! Avdium's entry point.
//!
//! This file deliberately does very little. It parses arguments, handles the
//! reporting subcommands, and otherwise starts the window. Keeping the logic in
//! the library is what makes it testable.
//!
//! `--version` and `--help` are resolved by the argument parser before any of
//! this runs, so they never initialize the GUI or touch the network.

use std::process::ExitCode;

use clap::Parser;

use avdium::app::{AvdiumApp, LaunchOptions};
use avdium::cli::{Cli, Command, ProvisionCommand};
use avdium::identity;

fn main() -> ExitCode {
    let mut cli = Cli::parse();

    // Reporting subcommands do not open a window. `take` keeps the rest of the
    // parsed arguments available for them.
    if let Some(command) = cli.command.take() {
        return run_report(command, &cli);
    }

    if let Err(error) = init_logging(cli.log.as_deref()) {
        eprintln!("avdium: logging could not be initialized: {error}");
    }

    let options = LaunchOptions {
        demo: cli.demo,
        sdk: cli.sdk.clone(),
        avd_home: cli.avd_home.clone(),
        log: cli.log.clone(),
        screenshot: screenshot_option(&cli),
    };

    let title = format!("{} {}", identity::NAME, identity::VERSION);
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            // The Wayland app id and the X11 WM_CLASS both come from here, and
            // both match the desktop entry's StartupWMClass.
            .with_app_id(identity::APP_ID)
            .with_inner_size([1120.0, 760.0])
            .with_min_inner_size([780.0, 520.0]),
        ..Default::default()
    };

    match eframe::run_native(
        identity::NAME,
        native_options,
        Box::new(move |creation| {
            Ok(Box::new(AvdiumApp::new(creation, options)) as Box<dyn eframe::App>)
        }),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("avdium: the window could not be opened: {error}");
            eprintln!(
                "avdium: this build needs a display server. On Wayland or X11 run it in your \
                 session; on a headless machine use a virtual display such as Xvfb."
            );
            eprintln!(
                "avdium: the reporting subcommands (paths, tools, doctor, devices, \
                 capabilities) work without a display."
            );
            ExitCode::FAILURE
        }
    }
}

/// Reporting subcommands that never open a window.
fn run_report(command: Command, cli: &Cli) -> ExitCode {
    let loaded = avdium::settings::Settings::load(
        &avdium::paths::AppDirs::from_process()
            .map(|dirs| dirs.settings_file())
            .unwrap_or_else(|| std::path::PathBuf::from("settings.json")),
    );
    let mut settings = loaded.settings;
    if let Some(log) = &cli.log
        && let Some(level) = parse_level(log)
    {
        settings.log_level = level;
    }

    let sdk = cli.sdk.clone().or_else(|| settings.sdk_path.clone());
    let avd_home = cli.avd_home.clone().or_else(|| settings.avd_home.clone());
    let paths = avdium::paths::PathContext::from_process(sdk.as_deref(), avd_home.as_deref());
    let providers = avdium::providers::ProviderSet::detect(&paths, settings.provider);

    match command {
        Command::Paths { json } => {
            if json {
                println!("{}", paths_json(&paths));
            } else {
                println!("sdk: {}", paths.sdk_summary());
                println!(
                    "avd_home: {} ({})",
                    paths.avd_home.display(),
                    paths.avd_home_origin.label()
                );
                println!("android_user_home: {}", paths.android_user_home.display());
                println!("emulator_home: {}", paths.emulator_home.display());
                println!("cmdline_tools: {}", paths.cmdline_tools_installs().len());
                for conflict in &paths.conflicts {
                    println!("conflict: {}", conflict.summary());
                }
                for warning in &paths.warnings {
                    println!("warning: {warning}");
                }
            }
            ExitCode::SUCCESS
        }
        Command::Tools { json } => {
            let report = probe_tools(&paths, &providers);
            if json {
                println!("{}", tools_json(&paths, &providers, &report));
            } else {
                println!("providers: {}", providers.summary());
                for (label, value) in &report {
                    println!("{label}: {value}");
                }
                for warning in &providers.warnings {
                    println!("warning: {warning}");
                }
            }
            ExitCode::SUCCESS
        }
        Command::Devices { json } => {
            let sdk_root = paths
                .sdk
                .as_ref()
                .map(|resolution| resolution.path.as_path());
            let discovery = avdium::avd::AvdRepository::new(paths.avd_home.clone())
                .with_relative_base(paths.emulator_home.clone())
                .discover(sdk_root);
            if json {
                println!("{}", devices_json(&discovery));
            } else {
                for avd in &discovery.avds {
                    let status = if avd.is_broken() { "broken" } else { "ok" };
                    println!(
                        "{}\t{}\t{}\t{}",
                        avd.name,
                        avd.image_label(),
                        status,
                        avd.data_dir.display()
                    );
                    if let Some(reason) = avd.primary_problem() {
                        println!("  note: {}", reason.summary());
                    }
                }
                for warning in &discovery.warnings {
                    println!("warning: {warning}");
                }
            }
            ExitCode::SUCCESS
        }
        Command::Capabilities { json } => {
            let matrix = providers
                .selected()
                .map(|provider| (provider.name(), provider.capabilities()));
            match (json, matrix) {
                (true, Some((name, capabilities))) => {
                    println!("{}", capabilities_json(name, &capabilities));
                }
                (true, None) => println!("{{\"provider\":null}}"),
                (false, Some((name, capabilities))) => {
                    println!("provider: {name}");
                    for (capability, supported) in capabilities.matrix() {
                        let reason = capabilities
                            .unsupported_reason(capability)
                            .unwrap_or_else(|| "supported".to_owned());
                        println!(
                            "{}\t{}\t{}",
                            capability,
                            if supported { "yes" } else { "no" },
                            reason
                        );
                    }
                    println!("license consent: {}", capabilities.license_boundary.label());
                }
                (false, None) => println!("provider: none detected"),
            }
            ExitCode::SUCCESS
        }
        Command::Provision(command) => run_provision(command, &paths, &providers),
        Command::Create {
            name,
            image,
            profile,
            force,
            dry_run,
        } => run_create(
            &paths,
            &providers,
            &name,
            &image,
            profile.as_deref(),
            force,
            dry_run,
        ),
        Command::Doctor { json, probe } => {
            let sdk_root = paths
                .sdk
                .as_ref()
                .map(|resolution| resolution.path.as_path());
            let discovery = avdium::avd::AvdRepository::new(paths.avd_home.clone())
                .with_relative_base(paths.emulator_home.clone())
                .discover(sdk_root);
            let mut environment =
                avdium::doctor::Environment::probe(&paths, &discovery, &providers);
            if probe {
                if let Some(emulator) = &environment.emulator {
                    let tool = avdium::runtime::EmulatorTool::new(&emulator.path);
                    if let Ok(output) = avdium::process::probe(
                        &tool.program,
                        &["-version"],
                        std::time::Duration::from_secs(10),
                    ) && let Some(version) = tool.parse_version(&output.stdout)
                        && let Some(entry) = environment.emulator.as_mut()
                    {
                        entry.version = Some(version.version);
                    }
                    if let Ok(output) = avdium::process::probe(
                        &tool.program,
                        &["-accel-check"],
                        std::time::Duration::from_secs(10),
                    ) {
                        environment.accel = Some(tool.parse_accel_check(&output.stdout));
                    }
                }
                if let Some(cli) = &providers.android_cli {
                    let mut tool = cli.clone();
                    tool.sdk = paths.sdk.as_ref().map(|resolution| resolution.path.clone());
                    if let Ok(output) = avdium::process::probe(
                        &tool.program,
                        &["info", "--no-metrics"],
                        std::time::Duration::from_secs(15),
                    ) {
                        let info = avdium::providers::android_cli::parse_info(&output.stdout);
                        environment.android_cli = Some(avdium::doctor::ToolObservation {
                            path: tool.program.clone(),
                            version: info.version,
                            error: None,
                        });
                    }
                }

                // Java is only needed by the classic tools, but it is still
                // looked for so the report can say whether one exists rather
                // than leaving it unknown.
                environment.java = probe_java();
                environment.java_probed = true;
            }
            let report = avdium::doctor::evaluate(&environment);
            if json {
                println!("{}", doctor_json(&report));
            } else {
                for check in &report.checks {
                    println!(
                        "[{}] {}: {}",
                        check.status.label(),
                        check.title,
                        check.evidence.join("; ")
                    );
                    if let Some(hint) = &check.hint {
                        println!("      next: {hint}");
                    }
                }
                println!("{}", report.summary());
            }
            if report.worst() == avdium::doctor::CheckStatus::Fail {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
    }
}

/// Look for a Java runtime without requiring one.
///
/// `java -version` prints to standard error, so both streams are read.
fn probe_java() -> Option<avdium::doctor::ToolObservation> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Some(java_home) = std::env::var_os("JAVA_HOME") {
        candidates.push(std::path::PathBuf::from(java_home).join("bin/java"));
    }
    if let Some(path_var) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path_var).map(|dir| dir.join("java")));
    }
    for candidate in candidates {
        if !avdium::process::is_executable(&candidate) {
            continue;
        }
        let version = avdium::process::probe(
            &candidate,
            &["-version"],
            std::time::Duration::from_secs(10),
        )
        .ok()
        .and_then(|output| {
            let text = format!("{}{}", output.stdout, output.stderr);
            text.lines().next().and_then(java_version_from_line)
        });
        return Some(avdium::doctor::ToolObservation {
            path: candidate,
            version,
            error: None,
        });
    }
    None
}

/// Extract the version from a `java -version` banner line.
fn java_version_from_line(line: &str) -> Option<String> {
    let start = line.find('"')? + 1;
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn probe_tools(
    paths: &avdium::paths::PathContext,
    providers: &avdium::providers::ProviderSet,
) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    if let Some(program) = paths.emulator_program() {
        let tool = avdium::runtime::EmulatorTool::new(&program);
        let value =
            avdium::process::probe(&program, &["-version"], std::time::Duration::from_secs(10))
                .ok()
                .and_then(|output| tool.parse_version(&output.stdout))
                .map(|version| version.version)
                .unwrap_or_else(|| "not available".to_owned());
        rows.push((
            "emulator".to_owned(),
            format!("{value} ({})", program.display()),
        ));
    } else {
        rows.push(("emulator".to_owned(), "not found".to_owned()));
    }
    if let Some(program) = paths.adb_program() {
        rows.push(("adb".to_owned(), format!("{}", program.display())));
    } else {
        rows.push(("adb".to_owned(), "not found".to_owned()));
    }
    if let Some(cli) = &providers.android_cli {
        let mut tool = cli.clone();
        tool.sdk = paths.sdk.as_ref().map(|resolution| resolution.path.clone());
        let value = avdium::process::probe(
            &tool.program,
            &["info", "--no-metrics"],
            std::time::Duration::from_secs(15),
        )
        .map(|output| {
            avdium::providers::android_cli::parse_info(&output.stdout)
                .version
                .unwrap_or_else(|| {
                    format!(
                        "output could not be read (exit {:?}, {} bytes)",
                        output.exit_code,
                        output.stdout.len()
                    )
                })
        })
        .unwrap_or_else(|error| format!("could not be run: {error}"));
        rows.push((
            "android_cli".to_owned(),
            format!("{value} ({})", tool.program.display()),
        ));
    } else {
        rows.push(("android_cli".to_owned(), "not found".to_owned()));
    }
    if let Some(tools) = &providers.cmdline_tools {
        rows.push((
            "cmdline_tools".to_owned(),
            format!("{} ({})", tools.version, tools.bin.display()),
        ));
    } else {
        rows.push(("cmdline_tools".to_owned(), "not found".to_owned()));
    }
    rows
}

fn parse_level(value: &str) -> Option<avdium::settings::LogLevel> {
    match value.trim().to_ascii_lowercase().as_str() {
        "error" => Some(avdium::settings::LogLevel::Error),
        "warn" | "warning" => Some(avdium::settings::LogLevel::Warn),
        "info" => Some(avdium::settings::LogLevel::Info),
        "debug" => Some(avdium::settings::LogLevel::Debug),
        "trace" => Some(avdium::settings::LogLevel::Trace),
        _ => None,
    }
}

fn init_logging(level: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    // Avdium's own logs are at the requested level; third-party crates are held
    // at warnings so a GUI session is not drowned in dependency chatter.
    let avdium_level = match level.and_then(parse_level) {
        Some(level) => level.as_filter(),
        None => "info",
    };
    let default_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| {
        format!("warn,avdium={avdium_level},avdium::app={avdium_level},avdium::operations={avdium_level}")
    });

    let mut builder = env_logger::Builder::new();
    builder.parse_filters(&default_filter);
    builder.format_timestamp_secs();
    builder
        .try_init()
        .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
}

fn screenshot_option(cli: &Cli) -> Option<std::path::PathBuf> {
    #[cfg(feature = "screenshot")]
    {
        cli.screenshot.clone()
    }
    #[cfg(not(feature = "screenshot"))]
    {
        let _ = cli;
        None
    }
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn paths_json(paths: &avdium::paths::PathContext) -> String {
    let sdk = match &paths.sdk {
        Some(resolution) => format!(
            "{{\"path\":\"{}\",\"origin\":\"{}\"}}",
            escape(&resolution.path.display().to_string()),
            escape(resolution.origin.label())
        ),
        None => "null".to_owned(),
    };
    format!(
        "{{\"sdk\":{sdk},\"avd_home\":\"{}\",\"avd_home_origin\":\"{}\",\
         \"android_user_home\":\"{}\",\"emulator_home\":\"{}\",\
         \"conflicts\":{},\"warnings\":{}}}",
        escape(&paths.avd_home.display().to_string()),
        escape(paths.avd_home_origin.label()),
        escape(&paths.android_user_home.display().to_string()),
        escape(&paths.emulator_home.display().to_string()),
        json_array(paths.conflicts.iter().map(|conflict| conflict.summary())),
        json_array(paths.warnings.iter().cloned())
    )
}

fn tools_json(
    paths: &avdium::paths::PathContext,
    providers: &avdium::providers::ProviderSet,
    report: &[(String, String)],
) -> String {
    let mut rows = String::new();
    for (index, (label, value)) in report.iter().enumerate() {
        if index > 0 {
            rows.push(',');
        }
        rows.push_str(&format!("\"{}\":\"{}\"", escape(label), escape(value)));
    }
    format!(
        "{{\"summary\":\"{}\",\"sdk\":\"{}\",\"tools\":{{{rows}}},\"warnings\":{}}}",
        escape(&providers.summary()),
        escape(&paths.sdk_summary()),
        json_array(providers.warnings.iter().cloned())
    )
}

fn devices_json(discovery: &avdium::avd::Discovery) -> String {
    let rows: Vec<String> = discovery
        .avds
        .iter()
        .map(|avd| {
            format!(
                "{{\"name\":\"{}\",\"display_name\":\"{}\",\"target\":{},\"abi\":{},\
                 \"tag\":{},\"data_dir\":\"{}\",\"ini\":\"{}\",\"broken\":{},\"problems\":{}}}",
                escape(&avd.name),
                escape(avd.display_name()),
                json_option(avd.target()),
                json_option(avd.abi()),
                json_option(avd.tag_id()),
                escape(&avd.data_dir.display().to_string()),
                escape(&avd.ini_path.display().to_string()),
                avd.is_broken(),
                json_array(avd.broken.iter().map(|reason| reason.summary()))
            )
        })
        .collect();
    format!(
        "{{\"devices\":[{}],\"warnings\":{}}}",
        rows.join(","),
        json_array(discovery.warnings.iter().cloned())
    )
}

fn capabilities_json(name: &str, capabilities: &avdium::providers::Capabilities) -> String {
    let mut rows = String::new();
    for (index, (capability, supported)) in capabilities.matrix().into_iter().enumerate() {
        if index > 0 {
            rows.push(',');
        }
        rows.push_str(&format!(
            "\"{}\":{{\"supported\":{},\"reason\":{}}}",
            escape(capability),
            supported,
            json_option(capabilities.unsupported_reason(capability).as_deref())
        ));
    }
    format!(
        "{{\"provider\":\"{}\",\"license_boundary\":\"{}\",\"capabilities\":{{{rows}}}}}",
        escape(name),
        escape(capabilities.license_boundary.label())
    )
}

fn doctor_json(report: &avdium::doctor::DoctorReport) -> String {
    let rows: Vec<String> = report
        .checks
        .iter()
        .map(|check| {
            format!(
                "{{\"id\":\"{}\",\"title\":\"{}\",\"status\":\"{}\",\"evidence\":{},\"hint\":{}}}",
                escape(check.id),
                escape(&check.title),
                check.status.label(),
                json_array(check.evidence.iter().cloned()),
                json_option(check.hint.as_deref())
            )
        })
        .collect();
    let (pass, warn, fail, unknown) = report.counts();
    format!(
        "{{\"checks\":[{}],\"summary\":{{\"pass\":{pass},\"warn\":{warn},\"fail\":{fail},\
         \"unknown\":{unknown}}}}}",
        rows.join(",")
    )
}

fn json_array<I: IntoIterator<Item = String>>(items: I) -> String {
    let rows: Vec<String> = items
        .into_iter()
        .map(|item| format!("\"{}\"", escape(&item)))
        .collect();
    format!("[{}]", rows.join(","))
}

fn json_option(value: Option<&str>) -> String {
    match value {
        Some(value) => format!("\"{}\"", escape(value)),
        None => "null".to_owned(),
    }
}

/// `avdium provision ...`
fn run_provision(
    command: ProvisionCommand,
    paths: &avdium::paths::PathContext,
    providers: &avdium::providers::ProviderSet,
) -> ExitCode {
    match command {
        ProvisionCommand::Pins => {
            for pin in avdium::provision::PINS {
                println!("{}", pin.id);
                println!("  name: {}", pin.name);
                println!("  url: {}", pin.url);
                println!(
                    "  size: {}",
                    avdium::provision::install::pin_download_label(pin)
                );
                match pin.expected_digest() {
                    Ok(Some(digest)) => {
                        println!("  digest: {} ({})", digest.raw(), pin.provenance.label())
                    }
                    Ok(None) => println!("  digest: none"),
                    Err(error) => println!("  digest: unreadable ({error})"),
                }
                println!("  archive: {:?}", pin.archive);
                if let Some(revision) = pin.revision {
                    println!("  installs as: cmdline-tools/{revision}");
                }
                if let Some(note) = pin.note {
                    println!("  note: {note}");
                }
                println!();
            }
            ExitCode::SUCCESS
        }
        ProvisionCommand::Plan {
            create_devices,
            manage_packages,
            json,
        } => {
            let observations = avdium::provision::observe(paths, providers);
            let goals = avdium::provision::Goals {
                manage_devices: true,
                create_devices,
                manage_packages,
            };
            // License steps need the repository catalog, which is fetched by
            // the setup surface rather than by this reporting command.
            let plan = avdium::provision::plan::plan(&observations, &goals, &[]);
            if json {
                println!("{}", provision_plan_json(&plan));
            } else {
                for step in &plan.steps {
                    println!("[{}] {}: {}", step.status.label(), step.title, step.detail);
                    if let Some(reason) = &step.blocked_reason {
                        println!("      blocked: {reason}");
                    }
                }
                println!();
                println!("{}", plan.summary());
                if plan.unsized_download_steps() > 0 {
                    println!(
                        "{} step(s) do not declare a size yet, so the total is a lower bound.",
                        plan.unsized_download_steps()
                    );
                }
            }
            if plan.blocked().next().is_some() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        ProvisionCommand::Install { pin, sdk, dry_run } => {
            let Some(pin) = avdium::provision::manifest::pin(&pin) else {
                eprintln!("avdium: unknown pin `{pin}`. Run `avdium provision pins`.");
                return ExitCode::FAILURE;
            };
            let observations = avdium::provision::observe(paths, providers);
            let Some(sdk_root) = sdk.or(observations.sdk_root) else {
                eprintln!("avdium: no SDK root. Pass --sdk or set ANDROID_HOME.");
                return ExitCode::FAILURE;
            };

            println!("Installing {} into {}", pin.id, sdk_root.display());
            println!("  from: {}", pin.url);
            println!(
                "  size: {}",
                avdium::provision::install::pin_download_label(pin)
            );
            println!("  digest: expected ({})", pin.provenance.label());
            if pin.digest_is_strong() {
                println!("  note: the expected digest is weak. Google publishes SHA-1 only.");
            }
            if dry_run {
                println!("dry run: nothing was downloaded.");
                return ExitCode::SUCCESS;
            }

            let dirs = avdium::paths::AppDirs::from_process().unwrap_or_else(|| {
                avdium::paths::AppDirs::from_roots("/tmp", "/tmp", "/tmp", "/tmp")
            });
            let records = dirs.data.join("tools");
            let cancel = avdium::process::CancellationToken::new();

            println!("Downloading and verifying. This can take a while and is not interactive.");
            let result = avdium::provision::install_pinned(
                pin,
                &sdk_root,
                &records,
                &avdium::provision::HttpFetcher,
                &cancel,
                |stage| println!("  {}", stage.label()),
            );

            match result {
                Ok(installed) => {
                    println!();
                    println!("Installed to {}", installed.path.display());
                    println!("  digest: {} (verified)", installed.record.actual_digest);
                    println!("  provenance: {}", installed.record.digest_provenance);
                    if let Some(note) = &installed.note {
                        println!("  note: {note}");
                    }
                    println!();
                    println!("Run `avdium tools` to confirm the tools are now detected.");
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("avdium: {error}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

/// `avdium create ...`
fn run_create(
    paths: &avdium::paths::PathContext,
    _providers: &avdium::providers::ProviderSet,
    name: &str,
    image: &str,
    profile: Option<&str>,
    force: bool,
    dry_run: bool,
) -> ExitCode {
    let installs = paths.cmdline_tools_installs();
    let Some(avdmanager) = installs.iter().find_map(|install| install.avdmanager()) else {
        eprintln!(
            "avdium: avdmanager was not found. Android CLI cannot create a device from a \
             chosen image, so the classic tools are required."
        );
        eprintln!("avdium: run `avdium provision install --pin cmdline-tools` first.");
        return ExitCode::FAILURE;
    };

    let Ok(image_id) = avdium::sdk::PackageId::parse(image) else {
        eprintln!("avdium: `{image}` is not a valid system image package identifier.");
        return ExitCode::FAILURE;
    };
    if !matches!(image_id.kind(), avdium::sdk::PackageKind::SystemImage) {
        eprintln!(
            "avdium: `{image}` is not a system image. Avdium will not substitute a different \
             image or API level."
        );
        return ExitCode::FAILURE;
    }

    let tools = avdium::providers::CmdlineTools {
        version: installs
            .iter()
            .find(|install| install.avdmanager().is_some())
            .map(|install| install.version.clone())
            .unwrap_or_else(|| "unknown".to_owned()),
        bin: avdmanager
            .parent()
            .map(|path| path.to_path_buf())
            .unwrap_or_default(),
        sdkmanager: None,
        avdmanager: Some(avdmanager),
    };

    let mut request =
        avdium::providers::cmdline_tools::CreateRequest::new(name, image_id).with_force(force);
    if let Some(profile) = profile {
        request = request.with_profile(profile);
    }
    let Some(spec) = tools.create_spec(&request) else {
        eprintln!("avdium: the avdmanager adapter could not build the command.");
        return ExitCode::FAILURE;
    };

    println!("{}", spec.preview());
    if dry_run {
        println!("dry run: nothing was created.");
        return ExitCode::SUCCESS;
    }

    // The AVD home is passed explicitly so the device lands where Avdium will
    // look for it, and never in a tool's own default.
    let spec = spec
        .envs(paths.child_env())
        .env("ANDROID_AVD_HOME", paths.avd_home.clone());

    let cancel = avdium::process::CancellationToken::new();
    match avdium::process::run_collect(
        &spec,
        &avdium::process::RunOptions {
            timeout: Some(std::time::Duration::from_secs(120)),
            ..avdium::process::RunOptions::default()
        },
        &cancel,
    ) {
        Ok(output) if output.success => {
            println!("Created {name}.");
            println!("  avd home: {}", paths.avd_home.display());
            println!("  image: {image}");
            for line in output
                .combined()
                .lines()
                .filter(|line| !line.trim().is_empty())
            {
                println!("  {line}");
            }
            ExitCode::SUCCESS
        }
        Ok(output) => {
            eprintln!("avdium: {}", output.describe_failure());
            for line in output
                .combined()
                .lines()
                .filter(|line| !line.trim().is_empty())
            {
                eprintln!("  {line}");
            }
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("avdium: the create command could not be run: {error}");
            ExitCode::FAILURE
        }
    }
}

fn provision_plan_json(plan: &avdium::provision::ProvisionPlan) -> String {
    let rows: Vec<String> = plan
        .steps
        .iter()
        .map(|step| {
            format!(
                "{{\"id\":\"{}\",\"title\":\"{}\",\"status\":\"{}\",\"detail\":\"{}\",\
                 \"blocked_reason\":{}}}",
                escape(&step.id),
                escape(&step.title),
                step.status.label(),
                escape(&step.detail),
                json_option(step.blocked_reason.as_deref())
            )
        })
        .collect();
    format!(
        "{{\"steps\":[{}],\"download_bytes\":{},\"unsized_download_steps\":{}}}",
        rows.join(","),
        plan.download_bytes(),
        plan.unsized_download_steps()
    )
}
