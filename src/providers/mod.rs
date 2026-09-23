//! SDK provider adapters.
//!
//! Avdium talks to two official tool families:
//!
//! - **Android CLI** (`android`), the current tool. It is the preferred
//!   provider for listing and package operations.
//! - **Classic tools** (`sdkmanager`, `avdmanager`), both deprecated but still
//!   the only official way to create a device from a chosen system image.
//!
//! Capabilities are modeled explicitly instead of being assumed. When a
//! provider cannot do something, the answer is "unsupported", never a silent
//! substitution of a different image, API level or device profile.

pub mod android_cli;
pub mod cmdline_tools;

pub use android_cli::{AndroidCli, AndroidCliInfo, InstallChannel, SdkListEntry, SdkListSection};
pub use cmdline_tools::CmdlineTools;

use std::path::PathBuf;

use crate::paths::PathContext;
use crate::settings::ProviderPreference;

/// How a provider exposes license consent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseBoundary {
    /// Exact per-package text can be shown before the decision, and only the
    /// answers covered by that decision are sent.
    PerPackageText,
    /// A set of exact texts can be shown and accepted as one enumerated group.
    EnumeratedGroup,
    /// The provider records consent without exposing a reviewable boundary.
    /// Provisioning that requires consent is blocked through this provider.
    Implicit,
    /// Not established for this provider.
    Unknown,
}

impl LicenseBoundary {
    /// Label for the capability matrix.
    pub fn label(self) -> &'static str {
        match self {
            Self::PerPackageText => "per-package license text",
            Self::EnumeratedGroup => "enumerated license group",
            Self::Implicit => "implicit, no reviewable boundary",
            Self::Unknown => "unknown",
        }
    }

    /// Whether consent-requiring operations may be performed.
    pub fn allows_consent_flow(self) -> bool {
        matches!(self, Self::PerPackageText | Self::EnumeratedGroup)
    }
}

/// What one provider can actually do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// List installed and available packages.
    pub list_packages: bool,
    /// Install a package.
    pub install_packages: bool,
    /// Update an installed package.
    pub update_packages: bool,
    /// Remove an installed package.
    pub remove_packages: bool,
    /// List devices.
    pub list_devices: bool,
    /// Create a device from a named hardware profile.
    pub create_with_profile: bool,
    /// Create a device from a user-chosen system image.
    pub create_with_image: bool,
    /// List the hardware profiles a device can be created from.
    pub list_profiles: bool,
    /// Remove a device.
    pub remove_device: bool,
    /// Rename a device in place.
    pub rename_device: bool,
    /// Start a device through this provider.
    pub start_device: bool,
    /// Stop a device through this provider.
    pub stop_device: bool,
    /// How license consent is handled.
    pub license_boundary: LicenseBoundary,
}

impl Capabilities {
    /// Every capability as a name and whether it is supported, for the About
    /// and Settings screens.
    pub fn matrix(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("List packages", self.list_packages),
            ("Install packages", self.install_packages),
            ("Update packages", self.update_packages),
            ("Remove packages", self.remove_packages),
            ("List devices", self.list_devices),
            ("Create from a profile", self.create_with_profile),
            ("Create from a chosen image", self.create_with_image),
            ("List device profiles", self.list_profiles),
            ("Remove a device", self.remove_device),
            ("Rename a device", self.rename_device),
            ("Start a device", self.start_device),
            ("Stop a device", self.stop_device),
        ]
    }

    /// Why a capability is unavailable, in user-facing language.
    ///
    /// Every `false` in the matrix must have an answer here, so the UI never
    /// shows a disabled control without a reason.
    pub fn unsupported_reason(&self, capability: &str) -> Option<String> {
        if self.supports(capability) {
            return None;
        }
        let reason = match capability {
            "Create from a chosen image" => {
                "This provider has no image selector, so the image cannot be chosen. \
                 The classic avdmanager adapter is required."
            }
            "Rename a device" => "Renaming is not implemented for this provider yet.",
            "Start a device" | "Stop a device" => {
                "Runtime start and stop are handled by the emulator and ADB directly, \
                 not through this provider."
            }
            _ => "Not supported by this provider.",
        };
        Some(reason.to_owned())
    }

    /// Whether a named capability is supported.
    pub fn supports(&self, capability: &str) -> bool {
        match capability {
            "List packages" => self.list_packages,
            "Install packages" => self.install_packages,
            "Update packages" => self.update_packages,
            "Remove packages" => self.remove_packages,
            "List devices" => self.list_devices,
            "Create from a profile" => self.create_with_profile,
            "Create from a chosen image" => self.create_with_image,
            "List device profiles" => self.list_profiles,
            "Remove a device" => self.remove_device,
            "Rename a device" => self.rename_device,
            "Start a device" => self.start_device,
            "Stop a device" => self.stop_device,
            _ => false,
        }
    }
}

/// A detected provider, or the absence of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provider {
    /// Android CLI.
    AndroidCli(AndroidCli),
    /// Classic `sdkmanager` and `avdmanager`.
    CmdlineTools(CmdlineTools),
}

impl Provider {
    /// Short name for logs and diagnostics.
    pub fn name(&self) -> &'static str {
        match self {
            Self::AndroidCli(_) => "Android CLI",
            Self::CmdlineTools(_) => "classic tools",
        }
    }

    /// What this provider can do.
    pub fn capabilities(&self) -> Capabilities {
        match self {
            Self::AndroidCli(cli) => cli.capabilities(),
            Self::CmdlineTools(tools) => tools.capabilities(),
        }
    }

    /// The Android CLI adapter, when this is one.
    pub fn as_android_cli(&self) -> Option<&AndroidCli> {
        match self {
            Self::AndroidCli(cli) => Some(cli),
            Self::CmdlineTools(_) => None,
        }
    }

    /// The classic tools adapter, when this is one.
    pub fn as_cmdline_tools(&self) -> Option<&CmdlineTools> {
        match self {
            Self::CmdlineTools(tools) => Some(tools),
            Self::AndroidCli(_) => None,
        }
    }
}

/// Everything detected on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderSet {
    /// Android CLI, when a launcher was found.
    pub android_cli: Option<AndroidCli>,
    /// Classic tools, when a `cmdline-tools` installation was found.
    pub cmdline_tools: Option<CmdlineTools>,
    /// How the provider was chosen.
    pub preference: ProviderPreference,
    /// Problems that stopped a provider from being detected.
    pub warnings: Vec<String>,
}

impl ProviderSet {
    /// Detect providers from a resolved path context.
    ///
    /// Detection is read-only and never runs a tool. Running a tool during
    /// startup could bootstrap caches or require network access.
    pub fn detect(paths: &PathContext, preference: ProviderPreference) -> Self {
        let mut warnings = Vec::new();

        let android_cli = detect_android_cli(paths);
        let cmdline_tools = detect_cmdline_tools(paths, &mut warnings);

        // Report an unavailable preference instead of failing quietly.
        match preference {
            ProviderPreference::AndroidCli if android_cli.is_none() && cmdline_tools.is_some() => {
                warnings.push(
                    "Android CLI was preferred but was not found, so the classic tools \
                     are used instead."
                        .to_owned(),
                );
            }
            ProviderPreference::CmdlineTools
                if cmdline_tools.is_none() && android_cli.is_some() =>
            {
                warnings.push(
                    "The classic tools were preferred but were not found, so Android CLI \
                     is used instead."
                        .to_owned(),
                );
            }
            _ => {}
        }

        if android_cli.is_none() && cmdline_tools.is_none() {
            warnings.push(
                "No Android SDK tool was found. Android CLI is expected at \
                 ~/.android/bin/android-cli, and classic tools under \
                 <sdk>/cmdline-tools/<version>/bin."
                    .to_owned(),
            );
        }

        Self {
            android_cli,
            cmdline_tools,
            preference,
            warnings,
        }
    }

    /// The provider to use, honoring the preference.
    ///
    /// An explicit preference that is unavailable falls back to the other
    /// provider rather than leaving the user with nothing. The fallback is
    /// reported by [`ProviderSet::warnings`] so it is never silent.
    pub fn selected(&self) -> Option<Provider> {
        let android_cli = || self.android_cli.clone().map(Provider::AndroidCli);
        let classic = || self.cmdline_tools.clone().map(Provider::CmdlineTools);
        match self.preference {
            ProviderPreference::AndroidCli => android_cli().or_else(classic),
            ProviderPreference::CmdlineTools => classic().or_else(android_cli),
            ProviderPreference::Auto => android_cli().or_else(classic),
        }
    }

    /// The provider capable of creating a device from a chosen image.
    ///
    /// This is deliberately separate from [`ProviderSet::selected`]: Android
    /// CLI is the better default for listing and packages, but it cannot do
    /// selected-image creation at all.
    pub fn creation_provider(&self) -> Option<Provider> {
        self.cmdline_tools.clone().map(Provider::CmdlineTools)
    }

    /// Whether any provider was found.
    pub fn is_empty(&self) -> bool {
        self.android_cli.is_none() && self.cmdline_tools.is_none()
    }

    /// One-line summary for the doctor.
    pub fn summary(&self) -> String {
        match (&self.android_cli, &self.cmdline_tools) {
            (Some(cli), Some(tools)) => format!(
                "Android CLI at {}, classic tools at {}",
                cli.program.display(),
                tools
                    .sdkmanager
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| tools.bin.display().to_string())
            ),
            (Some(cli), None) => format!("Android CLI at {}", cli.program.display()),
            (None, Some(tools)) => format!("classic tools at {}", tools.bin.display()),
            (None, None) => "no provider found".to_owned(),
        }
    }
}

/// Locate the Android CLI launcher.
///
/// The launcher is looked up in the locations the official installer uses,
/// plus `PATH`. Nothing is downloaded or executed here.
///
/// The resolved user home from the path context is used rather than the real
/// process home, so detection is consistent with the SDK that was resolved and
/// testable without touching a real home directory.
fn detect_android_cli(paths: &PathContext) -> Option<AndroidCli> {
    // The official installer puts the launcher in `$ANDROID_USER_HOME/bin`.
    let candidates: Vec<PathBuf> = vec![
        paths.android_user_home.join("bin").join("android-cli"),
        paths.android_user_home.join("bin").join("android"),
        // A user-level install in the resolved home that is not tied to
        // ANDROID_USER_HOME.
        paths.home.join(".android").join("bin").join("android-cli"),
        paths.home.join(".local").join("bin").join("android"),
    ];

    if let Some(found) = candidates
        .into_iter()
        .find(|path| crate::process::is_executable(path))
    {
        return Some(AndroidCli::new(found));
    }

    // Last resort: PATH.
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("android");
            if crate::process::is_executable(&candidate) {
                return Some(AndroidCli::new(candidate));
            }
        }
    }

    None
}

/// Locate classic tools under the SDK.
fn detect_cmdline_tools(paths: &PathContext, warnings: &mut Vec<String>) -> Option<CmdlineTools> {
    let installs = paths.cmdline_tools_installs();
    for install in installs {
        let sdkmanager = install.sdkmanager();
        let avdmanager = install.avdmanager();
        if sdkmanager.is_none() && avdmanager.is_none() {
            continue;
        }
        if sdkmanager.is_none() {
            warnings.push(format!(
                "cmdline-tools/{} has avdmanager but no sdkmanager. \
                 Package listing falls back to another provider.",
                install.version
            ));
        }
        return Some(CmdlineTools {
            version: install.version,
            bin: install.bin,
            sdkmanager,
            avdmanager,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::EnvSnapshot;
    use std::path::Path;

    fn empty_env() -> EnvSnapshot {
        EnvSnapshot::default()
    }

    #[test]
    fn no_providers_is_reported_with_an_explanation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = PathContext::resolve(temp.path(), None, None, &empty_env());
        let set = ProviderSet::detect(&paths, ProviderPreference::Auto);
        assert!(set.is_empty());
        assert_eq!(set.selected(), None);
        assert_eq!(set.creation_provider(), None);
        assert_eq!(set.warnings.len(), 1);
        assert!(set.warnings[0].contains("No Android SDK tool"));
    }

    #[test]
    fn android_cli_is_detected_in_the_android_user_home() {
        let temp = tempfile::tempdir().expect("tempdir");
        let bin = temp.path().join("bin");
        std::fs::create_dir_all(&bin).expect("create bin");
        let launcher = bin.join("android-cli");
        std::fs::write(&launcher, "#!/bin/sh\n").expect("write launcher");
        set_executable(&launcher);

        // The official installer puts the launcher in `$ANDROID_USER_HOME/bin`.
        let env = EnvSnapshot {
            android_user_home: Some(temp.path().to_path_buf()),
            ..EnvSnapshot::default()
        };
        let paths = PathContext::resolve(temp.path(), None, None, &env);
        let set = ProviderSet::detect(&paths, ProviderPreference::Auto);
        let cli = set.android_cli.as_ref().expect("android cli detected");
        assert_eq!(cli.program, launcher);
        assert_eq!(set.selected().expect("selected").name(), "Android CLI");
        assert!(
            set.creation_provider().is_none(),
            "Android CLI cannot create from an image"
        );
    }

    #[test]
    fn classic_tools_are_detected_and_are_the_creation_provider() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sdk = temp.path().join("sdk");
        let bin = sdk.join("cmdline-tools").join("latest").join("bin");
        std::fs::create_dir_all(&bin).expect("create bin");
        for tool in ["sdkmanager", "avdmanager"] {
            let path = bin.join(tool);
            std::fs::write(&path, "#!/bin/sh\n").expect("write tool");
            set_executable(&path);
        }

        let paths = PathContext::resolve(temp.path(), Some(&sdk), None, &empty_env());
        let set = ProviderSet::detect(&paths, ProviderPreference::Auto);
        let tools = set.cmdline_tools.clone().expect("classic tools detected");
        assert_eq!(tools.version, "latest");
        assert!(tools.sdkmanager.is_some());
        assert!(tools.avdmanager.is_some());
        // With no Android CLI present, classic tools are the default too.
        assert_eq!(set.selected().expect("selected").name(), "classic tools");
        let creation = set.creation_provider().expect("creation provider");
        assert!(creation.capabilities().create_with_image);
    }

    #[test]
    fn the_preference_is_honored_when_both_providers_exist() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sdk = temp.path().join("sdk");
        let tools_bin = sdk.join("cmdline-tools").join("19.0").join("bin");
        std::fs::create_dir_all(&tools_bin).expect("create bin");
        for tool in ["sdkmanager", "avdmanager"] {
            let path = tools_bin.join(tool);
            std::fs::write(&path, "#!/bin/sh\n").expect("write");
            set_executable(&path);
        }
        let user_bin = temp.path().join("bin");
        std::fs::create_dir_all(&user_bin).expect("create bin");
        let launcher = user_bin.join("android-cli");
        std::fs::write(&launcher, "#!/bin/sh\n").expect("write");
        set_executable(&launcher);

        let env = EnvSnapshot {
            android_user_home: Some(temp.path().to_path_buf()),
            ..EnvSnapshot::default()
        };
        let paths = PathContext::resolve(temp.path(), Some(&sdk), None, &env);
        assert_eq!(
            ProviderSet::detect(&paths, ProviderPreference::Auto)
                .selected()
                .expect("selected")
                .name(),
            "Android CLI"
        );
        assert_eq!(
            ProviderSet::detect(&paths, ProviderPreference::CmdlineTools)
                .selected()
                .expect("selected")
                .name(),
            "classic tools"
        );
        // An unavailable preference falls back rather than failing.
        let only_classic = ProviderSet {
            android_cli: None,
            cmdline_tools: ProviderSet::detect(&paths, ProviderPreference::Auto).cmdline_tools,
            preference: ProviderPreference::AndroidCli,
            warnings: Vec::new(),
        };
        assert_eq!(
            only_classic.selected().expect("fallback").name(),
            "classic tools"
        );
    }

    #[test]
    fn a_cmdline_tools_install_without_sdkmanager_warns() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sdk = temp.path().join("sdk");
        let bin = sdk.join("cmdline-tools").join("latest").join("bin");
        std::fs::create_dir_all(&bin).expect("create bin");
        let path = bin.join("avdmanager");
        std::fs::write(&path, "#!/bin/sh\n").expect("write");
        set_executable(&path);

        let paths = PathContext::resolve(temp.path(), Some(&sdk), None, &empty_env());
        let set = ProviderSet::detect(&paths, ProviderPreference::Auto);
        assert!(set.cmdline_tools.is_some());
        assert!(
            set.warnings
                .iter()
                .any(|warning| warning.contains("no sdkmanager"))
        );
    }

    #[test]
    fn the_android_cli_capability_matrix_matches_the_observed_help_output() {
        let cli = AndroidCli::new("/fake/android-cli");
        let capabilities = cli.capabilities();
        assert!(capabilities.list_packages);
        assert!(capabilities.create_with_profile);
        assert!(capabilities.list_profiles);
        // The installed binary's `emulator create --help` has no image selector.
        assert!(!capabilities.create_with_image);
        assert!(!capabilities.rename_device);
        assert_eq!(capabilities.license_boundary, LicenseBoundary::Implicit);
        assert!(!capabilities.license_boundary.allows_consent_flow());
        assert!(capabilities.unsupported_reason("Rename a device").is_some());
        assert!(capabilities.unsupported_reason("List packages").is_none());
    }

    #[test]
    fn every_unsupported_capability_has_a_reason() {
        for provider in [
            Provider::AndroidCli(AndroidCli::new("/fake/android-cli")),
            Provider::CmdlineTools(CmdlineTools {
                version: "latest".to_owned(),
                bin: PathBuf::from("/fake/bin"),
                sdkmanager: Some(PathBuf::from("/fake/bin/sdkmanager")),
                avdmanager: Some(PathBuf::from("/fake/bin/avdmanager")),
            }),
        ] {
            let capabilities = provider.capabilities();
            for (name, supported) in capabilities.matrix() {
                let reason = capabilities.unsupported_reason(name);
                if supported {
                    assert!(reason.is_none(), "{name} is supported but has a reason");
                } else {
                    assert!(reason.is_some(), "{name} is unsupported but has no reason");
                }
            }
        }
    }

    fn set_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod");
    }
}
