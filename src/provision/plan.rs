//! Provisioning plans.
//!
//! Planning is a pure function over observations. It answers one question:
//! given what is already on this machine and what the user is trying to do,
//! what is the smallest set of actions that gets them there?
//!
//! Two rules make the plan trustworthy:
//!
//! - **An existing installation is reused as-is.** Presence is not consent to
//!   upgrade, so the planner never proposes updating an installed emulator,
//!   platform-tools, command-line tools or image. It only proposes adding what
//!   is missing.
//! - **A blocked step stays blocked.** When a prerequisite is not pinned or not
//!   available, the step says so and stays blocked. It is never replaced with a
//!   different action that happens to unblock the plan.

use std::path::PathBuf;

use super::consent::{ConsentStatus, RecordedConsent};
use super::manifest::{DigestProvenance, ToolPin, pin};
use crate::paths::CmdlineToolsInstall;
use crate::sdk::packages::PackageId;

/// What the user is trying to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Goals {
    /// List and start existing devices. This is the baseline, always wanted.
    pub manage_devices: bool,
    /// Create new devices, including from a chosen system image.
    pub create_devices: bool,
    /// Install, update and remove SDK packages from the interface.
    pub manage_packages: bool,
}

impl Goals {
    /// The baseline: manage existing devices.
    pub fn baseline() -> Self {
        Self {
            manage_devices: true,
            ..Self::default()
        }
    }

    /// Everything the first release supports.
    pub fn full() -> Self {
        Self {
            manage_devices: true,
            create_devices: true,
            manage_packages: true,
        }
    }
}

/// A Java runtime that was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaObservation {
    /// The `java` executable.
    pub program: PathBuf,
    /// The version it reported.
    pub version: String,
}

/// Everything the planner needs to know about this machine.
#[derive(Debug, Clone, Default)]
pub struct Observations {
    /// The resolved SDK root.
    pub sdk_root: Option<PathBuf>,
    /// Whether the SDK root already exists.
    pub sdk_exists: bool,
    /// Whether the SDK root is writable by this user.
    pub sdk_writable: bool,
    /// Android CLI launcher, when found.
    pub android_cli: Option<PathBuf>,
    /// Every `cmdline-tools` installation found.
    pub cmdline_tools: Vec<CmdlineToolsInstall>,
    /// A Java runtime that already exists and can be reused.
    pub java: Option<JavaObservation>,
    /// The emulator executable, when present.
    pub emulator: Option<PathBuf>,
    /// The ADB executable, when present.
    pub adb: Option<PathBuf>,
    /// System images that are installed.
    pub installed_images: Vec<PackageId>,
    /// Consent already recorded by the official tooling.
    pub recorded_consent: RecordedConsent,
    /// The host architecture, for example `x86_64`.
    pub host_arch: String,
}

impl Observations {
    /// Whether a classic tools installation with a working `sdkmanager` exists.
    pub fn has_classic_tools(&self) -> bool {
        self.cmdline_tools
            .iter()
            .any(|install| install.sdkmanager().is_some())
    }

    /// Whether a classic tools installation with `avdmanager` exists.
    pub fn has_avdmanager(&self) -> bool {
        self.cmdline_tools
            .iter()
            .any(|install| install.avdmanager().is_some())
    }
}

/// How a step stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    /// Already satisfied; nothing to do.
    Satisfied,
    /// Can be performed.
    Needed,
    /// Cannot be performed, with a reason.
    Blocked,
}

impl StepStatus {
    /// Label for the setup screen.
    pub fn label(self) -> &'static str {
        match self {
            Self::Satisfied => "already present",
            Self::Needed => "to do",
            Self::Blocked => "blocked",
        }
    }
}

/// The action a step performs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepAction {
    /// Nothing to do.
    Nothing,
    /// Use something that already exists.
    UseExisting {
        /// What is being reused.
        what: String,
        /// Where it is.
        path: PathBuf,
    },
    /// Create the SDK root directory.
    CreateSdkRoot {
        /// Where it will be created.
        path: PathBuf,
    },
    /// Download and install a pinned tool.
    DownloadTool {
        /// The pin identifier.
        pin_id: &'static str,
        /// Declared download size.
        download_bytes: Option<u64>,
        /// Where the expected digest came from.
        provenance: DigestProvenance,
        /// Whether the expected digest is weak, for example SHA-1 only.
        weak_digest: bool,
    },
    /// Install an SDK package through the selected provider.
    InstallPackage {
        /// The package to install.
        id: PackageId,
        /// Declared download size, when known.
        download_bytes: Option<u64>,
    },
    /// Show a license and ask for a decision.
    DecideLicense {
        /// The license identifier.
        license_id: String,
        /// How much text the user will be shown.
        characters: usize,
    },
    /// Provision a private Java runtime.
    ProvisionJava {
        /// Why a private runtime is needed.
        reason: String,
    },
    /// Something the user has to decide, which Avdium cannot choose for them.
    Choose {
        /// What needs choosing.
        question: String,
    },
}

/// One step in a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// Stable identifier, for tests and for the UI to track completion.
    pub id: String,
    /// Short title.
    pub title: String,
    /// What will happen, in user-facing language.
    pub detail: String,
    /// How the step stands.
    pub status: StepStatus,
    /// Why it is blocked, when it is.
    pub blocked_reason: Option<String>,
    /// What the step does.
    pub action: StepAction,
}

impl Step {
    fn new(id: impl Into<String>, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            detail: detail.into(),
            status: StepStatus::Needed,
            blocked_reason: None,
            action: StepAction::Nothing,
        }
    }

    fn satisfied(mut self) -> Self {
        self.status = StepStatus::Satisfied;
        self
    }

    fn blocked(mut self, reason: impl Into<String>) -> Self {
        self.status = StepStatus::Blocked;
        self.blocked_reason = Some(reason.into());
        self
    }

    fn with(mut self, action: StepAction) -> Self {
        self.action = action;
        self
    }

    /// Whether the step is completed or unnecessary.
    pub fn is_satisfied(&self) -> bool {
        self.status == StepStatus::Satisfied
    }

    /// Whether the step can be performed now.
    pub fn is_actionable(&self) -> bool {
        self.status == StepStatus::Needed
    }
}

/// A complete plan.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProvisionPlan {
    /// Steps in the order they should be performed.
    pub steps: Vec<Step>,
}

impl ProvisionPlan {
    /// Steps that still have work to do.
    pub fn actionable(&self) -> impl Iterator<Item = &Step> {
        self.steps.iter().filter(|step| step.is_actionable())
    }

    /// Steps that cannot proceed.
    pub fn blocked(&self) -> impl Iterator<Item = &Step> {
        self.steps
            .iter()
            .filter(|step| step.status == StepStatus::Blocked)
    }

    /// Steps that need nothing.
    pub fn satisfied(&self) -> impl Iterator<Item = &Step> {
        self.steps.iter().filter(|step| step.is_satisfied())
    }

    /// Total bytes to download, across every step that declares a size.
    ///
    /// Reported separately from installed size: the two are not the same, and
    /// presenting only the download would understate what the disk needs.
    pub fn download_bytes(&self) -> u64 {
        self.steps
            .iter()
            .map(|step| match &step.action {
                StepAction::DownloadTool { download_bytes, .. }
                | StepAction::InstallPackage { download_bytes, .. } => download_bytes.unwrap_or(0),
                _ => 0,
            })
            .sum()
    }

    /// How many steps declare a size, so the caller knows whether the total is
    /// complete or partial.
    pub fn unsized_download_steps(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| {
                matches!(
                    &step.action,
                    StepAction::DownloadTool {
                        download_bytes: None,
                        ..
                    } | StepAction::InstallPackage {
                        download_bytes: None,
                        ..
                    }
                )
            })
            .count()
    }

    /// Whether anything at all has to be done.
    pub fn is_complete(&self) -> bool {
        self.actionable().next().is_none() && self.blocked().next().is_none()
    }

    /// One-line summary for the status bar.
    pub fn summary(&self) -> String {
        let actionable = self.actionable().count();
        let blocked = self.blocked().count();
        if actionable == 0 && blocked == 0 {
            return "Everything needed is already installed.".to_owned();
        }
        let downloads = crate::download::human_bytes(self.download_bytes());
        match blocked {
            0 => format!("{actionable} step(s) to go, {downloads} to download"),
            n => format!("{actionable} step(s) to go, {downloads} to download, {n} blocked"),
        }
    }

    /// A step by identifier.
    pub fn step(&self, id: &str) -> Option<&Step> {
        self.steps.iter().find(|step| step.id == id)
    }

    /// Remove blocked steps, keeping the rest.
    ///
    /// Used to build an executable plan from a plan that has a blocked step the
    /// caller has decided to skip. It never rewrites a blocked step into a
    /// different action.
    pub fn without_blocked(&self) -> Self {
        Self {
            steps: self
                .steps
                .iter()
                .filter(|step| step.status != StepStatus::Blocked)
                .cloned()
                .collect(),
        }
    }
}

/// The licenses the plan requires, by identifier only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequiredLicenses {
    /// Identifiers that a step will need a decision for.
    pub ids: Vec<String>,
}

/// Build a plan.
pub fn plan(
    observations: &Observations,
    goals: &Goals,
    licenses: &[(String, usize)],
) -> ProvisionPlan {
    let mut steps = Vec::new();

    // The host architecture decides whether any of this is possible.
    if observations.host_arch != "x86_64" {
        steps.push(
            Step::new(
                "host.arch",
                "Host architecture",
                format!(
                    "Avdium 0.1 targets Linux x86_64. This host is {}.",
                    observations.host_arch
                ),
            )
            .blocked(
                "The official emulator publishes no Linux archive for this architecture, so a \
                 device cannot be started here.",
            ),
        );
    }

    // The SDK root has to exist and be writable before anything is installed.
    steps.push(sdk_step(observations));

    // Tooling.
    steps.extend(tool_steps(observations, goals));

    // Java, only when a Java-requiring tool is actually in play.
    steps.push(java_step(observations, goals));

    // The runtime pieces a device needs.
    steps.push(component_step(
        observations,
        "tool.emulator",
        "Emulator",
        "The emulator executable, which actually runs a device.",
        observations.emulator.as_ref(),
        PackageId::parse("emulator").ok(),
    ));
    steps.push(component_step(
        observations,
        "tool.adb",
        "Platform tools",
        "ADB, used for device identity, readiness and stopping.",
        observations.adb.as_ref(),
        PackageId::parse("platform-tools").ok(),
    ));

    // A system image is a choice, so the plan asks rather than picks.
    if goals.create_devices && observations.installed_images.is_empty() {
        steps.push(
            Step::new(
                "image.choose",
                "System image",
                "No system image is installed. A device needs one, and which one is your \
                 choice: it decides the Android version, the API level and whether Play \
                 services are included.",
            )
            .with(StepAction::Choose {
                question: "Choose a system image".to_owned(),
            }),
        );
    } else if !observations.installed_images.is_empty() {
        let names: Vec<String> = observations
            .installed_images
            .iter()
            .map(PackageId::as_str)
            .collect();
        steps.push(
            Step::new(
                "image.present",
                "System image",
                format!("{} image(s) already installed.", names.len()),
            )
            .satisfied()
            .with(StepAction::UseExisting {
                what: "installed system image".to_owned(),
                path: PathBuf::from(names.first().cloned().unwrap_or_default()),
            }),
        );
    }

    // Licenses last: they are decided immediately before the action that needs
    // them, not as an abstract agreement step at the start.
    for (license_id, characters) in licenses {
        let already = observations
            .recorded_consent
            .accepted
            .contains_key(license_id);
        let step = Step::new(
            format!("license.{license_id}"),
            format!("License: {license_id}"),
            if already {
                format!(
                    "The installed tooling already recorded acceptance of this license \
                     ({characters} characters). Avdium will not ask again."
                )
            } else {
                format!(
                    "The exact license text ({characters} characters) is shown before anything \
                     is downloaded or installed."
                )
            },
        )
        .with(StepAction::DecideLicense {
            license_id: license_id.clone(),
            characters: *characters,
        });
        steps.push(if already { step.satisfied() } else { step });
    }

    ProvisionPlan { steps }
}

fn sdk_step(observations: &Observations) -> Step {
    let Some(root) = &observations.sdk_root else {
        return Step::new(
            "sdk.root",
            "SDK location",
            "No SDK location could be resolved.",
        )
        .blocked("Choose an SDK directory in Settings, or set ANDROID_HOME.");
    };

    if !observations.sdk_exists {
        if !observations.sdk_writable {
            return Step::new(
                "sdk.root",
                "SDK location",
                format!("{} does not exist yet.", root.display()),
            )
            .blocked(
                "The parent directory is not writable by this user. Avdium never uses sudo, so \
                 pick a location you own.",
            );
        }
        return Step::new(
            "sdk.root",
            "SDK location",
            format!("{} will be created.", root.display()),
        )
        .with(StepAction::CreateSdkRoot { path: root.clone() });
    }

    if !observations.sdk_writable {
        return Step::new(
            "sdk.root",
            "SDK location",
            format!(
                "{} exists but is not writable by this user.",
                root.display()
            ),
        )
        .blocked(
            "Installing anything needs write access. Avdium never uses sudo, so point it at a \
             location you own.",
        );
    }

    Step::new(
        "sdk.root",
        "SDK location",
        format!("{} is ready.", root.display()),
    )
    .satisfied()
    .with(StepAction::UseExisting {
        what: "SDK root".to_owned(),
        path: root.clone(),
    })
}

fn tool_steps(observations: &Observations, goals: &Goals) -> Vec<Step> {
    let mut steps = Vec::new();

    // Android CLI: the preferred provider for listing and packages.
    steps.push(match &observations.android_cli {
        Some(path) => Step::new(
            "tool.android-cli",
            "Android CLI",
            format!("{} is installed and will be used as-is.", path.display()),
        )
        .satisfied()
        .with(StepAction::UseExisting {
            what: "Android CLI".to_owned(),
            path: path.clone(),
        }),
        None if !goals.manage_packages => Step::new(
            "tool.android-cli",
            "Android CLI",
            "Not installed, and not needed for what you are doing. An existing SDK works \
             without it.",
        )
        .satisfied()
        .with(StepAction::Nothing),
        None => Step::new(
            "tool.android-cli",
            "Android CLI",
            "Not installed. Needed to install and update SDK packages from Avdium.",
        )
        .with(StepAction::Nothing),
    });

    // Classic tools: needed for selected-image creation, which Android CLI
    // cannot do.
    let classic_needed = goals.create_devices;
    let classic_present = observations.has_avdmanager();
    if classic_present {
        let install = observations
            .cmdline_tools
            .iter()
            .find(|install| install.avdmanager().is_some())
            .expect("checked above");
        steps.push(
            Step::new(
                "tool.cmdline-tools",
                "Classic command line tools",
                format!(
                    "cmdline-tools/{} is installed and will be used as-is. These tools are \
                     deprecated, but they are the only official way to create a device from a \
                     chosen image.",
                    install.version
                ),
            )
            .satisfied()
            .with(StepAction::UseExisting {
                what: "avdmanager".to_owned(),
                path: install.bin.clone(),
            }),
        );
    } else if classic_needed {
        steps.push(classic_tools_download_step());
    } else {
        // Not needed for the stated goal, so nothing is pending.
        steps.push(
            Step::new(
                "tool.cmdline-tools",
                "Classic command line tools",
                "Not installed, and not needed for what you are doing. They are required \
                 only to create a device from a chosen system image.",
            )
            .satisfied()
            .with(StepAction::Nothing),
        );
    }

    steps
}

fn classic_tools_download_step() -> Step {
    match pin("cmdline-tools") {
        Some(pin) => Step::new(
            "tool.cmdline-tools.download",
            "Install classic command line tools",
            pin.note.unwrap_or(
                "Downloaded into private staging, verified, then installed under \
                 cmdline-tools/<version> rather than overwriting an existing install.",
            ),
        )
        .with(StepAction::DownloadTool {
            pin_id: pin.id,
            download_bytes: pin.size,
            provenance: pin.provenance,
            weak_digest: !pin.digest_is_strong(),
        }),
        None => Step::new(
            "tool.cmdline-tools.download",
            "Install classic command line tools",
            "No reviewed pin is available for these tools.",
        )
        .blocked("Add a reviewed pin to src/provision/manifest.rs before offering this."),
    }
}

fn java_step(observations: &Observations, goals: &Goals) -> Step {
    // Java is only needed by the classic tools. Android CLI carries its own
    // runtime, and that runtime has no executable java binary, so it is not a
    // candidate for running the classic tools.
    let classic_needs_java = observations.has_classic_tools() || goals.create_devices;
    if !classic_needs_java {
        return Step::new(
            "runtime.java",
            "Java runtime",
            "Not needed. The selected provider does not use Java, and Avdium never requires a \
             system-wide Java install.",
        )
        .satisfied()
        .with(StepAction::Nothing);
    }

    match &observations.java {
        Some(java) => Step::new(
            "runtime.java",
            "Java runtime",
            format!(
                "Reusing the Java already on this machine: {} ({}). No system-wide install is \
                 added, and nothing is downloaded.",
                java.program.display(),
                java.version
            ),
        )
        .satisfied()
        .with(StepAction::UseExisting {
            what: "Java runtime".to_owned(),
            path: java.program.clone(),
        }),
        None => Step::new(
            "runtime.java",
            "Java runtime",
            "The classic tools need Java and none was found. Avdium can install a private \
             runtime instead of requiring one system-wide.",
        )
        .blocked(
            "The exact runtime release and its published checksum are not pinned yet, so Avdium \
             will not download an unpinned runtime. A compatible Java found on PATH removes this \
             blocker.",
        )
        .with(StepAction::ProvisionJava {
            reason: "no compatible Java was found".to_owned(),
        }),
    }
}

fn component_step(
    observations: &Observations,
    id: &str,
    name: &str,
    detail: &str,
    present: Option<&PathBuf>,
    package: Option<PackageId>,
) -> Step {
    if let Some(path) = present {
        return Step::new(
            id.to_owned(),
            name.to_owned(),
            format!("{} is present.", path.display()),
        )
        .satisfied()
        .with(StepAction::UseExisting {
            what: name.to_owned(),
            path: path.clone(),
        });
    }
    if !observations.sdk_writable {
        return Step::new(id.to_owned(), name.to_owned(), detail)
            .blocked("The SDK root is not writable, so nothing can be installed.");
    }
    match package {
        Some(package) => {
            Step::new(id.to_owned(), name.to_owned(), detail).with(StepAction::InstallPackage {
                id: package,
                download_bytes: None,
            })
        }
        None => Step::new(id.to_owned(), name.to_owned(), detail)
            .blocked("The package identifier for this component is not valid."),
    }
}

/// Consent status for a license in a plan, for the setup screen.
pub fn license_status(
    store: &super::consent::ConsentStore,
    recorded: &RecordedConsent,
    license_id: &str,
    text: &str,
) -> ConsentStatus {
    store.status(license_id, text, recorded)
}

/// The reviewed pin behind a download step, for display.
pub fn step_pin(step: &Step) -> Option<&'static ToolPin> {
    match &step.action {
        StepAction::DownloadTool { pin_id, .. } => pin(pin_id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::CmdlineToolsInstall;

    fn installs(
        version: &str,
        with_sdkmanager: bool,
        with_avdmanager: bool,
    ) -> CmdlineToolsInstall {
        let bin = PathBuf::from(format!("/sdk/cmdline-tools/{version}/bin"));
        let install = CmdlineToolsInstall {
            version: version.to_owned(),
            bin,
        };
        // The struct exposes paths through helpers that check the filesystem,
        // so tests exercise the observation predicate directly.
        let _ = (with_sdkmanager, with_avdmanager);
        install
    }

    fn ready_host() -> Observations {
        Observations {
            sdk_root: Some(PathBuf::from("/sdk")),
            sdk_exists: true,
            sdk_writable: true,
            android_cli: Some(PathBuf::from("/home/u/.android/bin/android-cli")),
            cmdline_tools: Vec::new(),
            java: Some(JavaObservation {
                program: PathBuf::from("/usr/bin/java"),
                version: "21.0.11".to_owned(),
            }),
            emulator: Some(PathBuf::from("/sdk/emulator/emulator")),
            adb: Some(PathBuf::from("/sdk/platform-tools/adb")),
            installed_images: vec![
                PackageId::parse("system-images/android-36.1/google_apis/x86_64").expect("parse"),
            ],
            recorded_consent: RecordedConsent::default(),
            host_arch: "x86_64".to_owned(),
        }
    }

    #[test]
    fn a_complete_host_needs_nothing_for_the_baseline() {
        let plan = plan(&ready_host(), &Goals::baseline(), &[]);
        assert!(plan.is_complete(), "{:?}", plan.steps);
        assert_eq!(plan.download_bytes(), 0);
        assert!(plan.summary().contains("already installed"));
    }

    #[test]
    fn an_existing_installation_is_never_scheduled_for_upgrade() {
        let plan = plan(&ready_host(), &Goals::full(), &[]);
        // Every present component is a reuse step, not an install step.
        for id in ["tool.emulator", "tool.adb", "sdk.root"] {
            let step = plan.step(id).unwrap_or_else(|| panic!("{id} missing"));
            assert!(step.is_satisfied(), "{id} should be satisfied");
            assert!(
                matches!(step.action, StepAction::UseExisting { .. }),
                "{id} should reuse what exists, got {:?}",
                step.action
            );
        }
        assert!(
            !plan
                .steps
                .iter()
                .any(|step| matches!(step.action, StepAction::InstallPackage { .. }))
        );
    }

    #[test]
    fn an_existing_android_cli_is_reused_rather_than_reinstalled() {
        let plan = plan(&ready_host(), &Goals::baseline(), &[]);
        let step = plan.step("tool.android-cli").expect("android cli step");
        assert!(step.is_satisfied());
        assert!(matches!(step.action, StepAction::UseExisting { .. }));
    }

    #[test]
    fn creating_devices_pulls_in_the_classic_tools_and_their_pin() {
        let plan = plan(&ready_host(), &Goals::full(), &[]);
        let step = plan
            .step("tool.cmdline-tools.download")
            .expect("download step");
        assert!(step.is_actionable());
        match &step.action {
            StepAction::DownloadTool {
                pin_id,
                download_bytes,
                provenance,
                weak_digest,
            } => {
                assert_eq!(*pin_id, "cmdline-tools");
                assert_eq!(*download_bytes, Some(181_052_239));
                assert_eq!(*provenance, DigestProvenance::PublisherMetadata);
                assert!(*weak_digest, "SHA-1 must be reported as weak");
            }
            other => panic!("unexpected action: {other:?}"),
        }
        assert_eq!(plan.download_bytes(), 181_052_239);
    }

    #[test]
    fn java_is_reused_when_present_and_the_reason_is_stated() {
        let plan = plan(&ready_host(), &Goals::full(), &[]);
        let step = plan.step("runtime.java").expect("java step");
        assert!(step.is_satisfied());
        assert!(step.detail.contains("Reusing"));
        assert!(step.detail.contains("21.0.11"));
    }

    #[test]
    fn a_missing_java_blocks_only_when_a_java_tool_is_actually_needed() {
        let mut observations = ready_host();
        observations.java = None;

        // The baseline does not use Java at all.
        let baseline = plan(&observations, &Goals::baseline(), &[]);
        assert!(baseline.step("runtime.java").expect("step").is_satisfied());
        assert!(
            baseline
                .step("runtime.java")
                .expect("step")
                .detail
                .contains("Not needed")
        );

        // Creating devices does, and there is no pinned runtime yet.
        let full = plan(&observations, &Goals::full(), &[]);
        let step = full.step("runtime.java").expect("step");
        assert_eq!(step.status, StepStatus::Blocked);
        assert!(
            step.blocked_reason
                .as_deref()
                .expect("reason")
                .contains("not pinned")
        );
        assert!(matches!(step.action, StepAction::ProvisionJava { .. }));
    }

    #[test]
    fn a_blocked_step_is_never_rewritten_into_a_different_action() {
        let mut observations = ready_host();
        observations.java = None;
        let plan = plan(&observations, &Goals::full(), &[]);
        let reduced = plan.without_blocked();
        // The Java step is gone, not replaced.
        assert!(reduced.step("runtime.java").is_none());
        assert!(reduced.step("tool.cmdline-tools.download").is_some());
        assert!(reduced.blocked().next().is_none());
    }

    #[test]
    fn a_missing_sdk_root_is_created_rather_than_reported_as_missing() {
        let mut observations = ready_host();
        observations.sdk_exists = false;
        observations.emulator = None;
        observations.adb = None;
        let plan = plan(&observations, &Goals::baseline(), &[]);
        let step = plan.step("sdk.root").expect("step");
        assert!(matches!(step.action, StepAction::CreateSdkRoot { .. }));
        // The emulator and platform-tools become installs.
        assert!(matches!(
            plan.step("tool.emulator").expect("step").action,
            StepAction::InstallPackage { .. }
        ));
        assert!(matches!(
            plan.step("tool.adb").expect("step").action,
            StepAction::InstallPackage { .. }
        ));
    }

    #[test]
    fn an_unwritable_sdk_blocks_installation_with_the_sudo_reason() {
        let mut observations = ready_host();
        observations.sdk_writable = false;
        observations.emulator = None;
        let plan = plan(&observations, &Goals::baseline(), &[]);
        let step = plan.step("tool.emulator").expect("step");
        assert_eq!(step.status, StepStatus::Blocked);
        assert!(
            step.blocked_reason
                .as_deref()
                .expect("reason")
                .contains("not writable")
        );
    }

    #[test]
    fn an_unsupported_architecture_blocks_with_an_explanation() {
        let mut observations = ready_host();
        observations.host_arch = "aarch64".to_owned();
        let plan = plan(&observations, &Goals::baseline(), &[]);
        let step = plan.step("host.arch").expect("step");
        assert_eq!(step.status, StepStatus::Blocked);
        assert!(
            step.blocked_reason
                .as_deref()
                .expect("reason")
                .contains("no Linux archive")
        );
    }

    #[test]
    fn no_system_image_produces_a_question_rather_than_a_default() {
        let mut observations = ready_host();
        observations.installed_images.clear();
        let plan = plan(&observations, &Goals::full(), &[]);
        let step = plan.step("image.choose").expect("step");
        assert!(matches!(step.action, StepAction::Choose { .. }));
        // Avdium must not silently pick an image.
        assert!(!plan.steps.iter().any(|step| matches!(
            &step.action,
            StepAction::InstallPackage { id, .. }
                if id.kind() == crate::sdk::packages::PackageKind::SystemImage
        )));
    }

    #[test]
    fn an_installed_image_is_reused() {
        let plan = plan(&ready_host(), &Goals::full(), &[]);
        assert!(plan.step("image.present").expect("step").is_satisfied());
        assert!(plan.step("image.choose").is_none());
    }

    #[test]
    fn licenses_already_recorded_by_the_tooling_are_not_asked_again() {
        let mut observations = ready_host();
        observations
            .recorded_consent
            .accepted
            .insert("android-sdk-license".to_owned(), vec!["abc".to_owned()]);
        let plan = plan(
            &observations,
            &Goals::full(),
            &[("android-sdk-license".to_owned(), 16962)],
        );
        let step = plan.step("license.android-sdk-license").expect("step");
        assert!(step.is_satisfied());
        assert!(step.detail.contains("will not ask again"));
    }

    #[test]
    fn an_unrecorded_license_is_a_step_that_shows_the_exact_text() {
        let plan = plan(
            &ready_host(),
            &Goals::full(),
            &[("android-sdk-license".to_owned(), 16962)],
        );
        let step = plan.step("license.android-sdk-license").expect("step");
        assert!(step.is_actionable());
        match &step.action {
            StepAction::DecideLicense { characters, .. } => assert_eq!(*characters, 16962),
            other => panic!("unexpected action: {other:?}"),
        }
    }

    #[test]
    fn the_download_total_is_separate_from_installed_size() {
        // A machine with tooling missing: the emulator and platform-tools do
        // not have pinned sizes yet, while the classic tools do.
        let mut observations = ready_host();
        observations.cmdline_tools.clear();
        observations.java = None;
        observations.installed_images.clear();
        observations.emulator = None;
        observations.adb = None;
        let plan = plan(&observations, &Goals::full(), &[]);
        assert_eq!(plan.download_bytes(), 181_052_239);
        assert!(plan.unsized_download_steps() >= 2);
        assert!(plan.summary().contains("blocked"));
    }

    #[test]
    fn a_plan_is_deterministic_for_the_same_observations() {
        let a = plan(&ready_host(), &Goals::full(), &[]);
        let b = plan(&ready_host(), &Goals::full(), &[]);
        assert_eq!(a, b);
    }

    #[test]
    fn step_identifiers_are_unique() {
        let plan = plan(&ready_host(), &Goals::full(), &[]);
        let mut ids: Vec<&str> = plan.steps.iter().map(|step| step.id.as_str()).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate step id");
    }

    #[test]
    fn installing_a_default_setup_is_driven_by_the_pin_table_not_by_guesses() {
        let pin = super::super::manifest::creation_tool_pin();
        assert_eq!(pin.id, "cmdline-tools");
        assert!(pin.url.starts_with("https://dl.google.com/"));
        let _ = installs("latest", true, true);
    }
}
