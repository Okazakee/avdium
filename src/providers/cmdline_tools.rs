//! Classic tools adapter (`sdkmanager`, `avdmanager`).
//!
//! Both tools are deprecated and redirect users to Android CLI, but they remain
//! the only official way to create a device from a user-chosen system image.
//! Avdium therefore keeps a narrow adapter for exactly that, plus a fallback
//! for package listing when no other provider is present.
//!
//! **Validation status:** no `cmdline-tools` installation exists on the
//! development host, so nothing in this module has been exercised against a
//! real build. The command shapes below come from the official documentation.
//! `docs/VALIDATION.md` records this as a gap. Do not treat these arguments as
//! verified until a real install is used.

use std::path::PathBuf;

use super::{Capabilities, LicenseBoundary};
use crate::process::{CommandSpec, StdinMode};
use crate::sdk::packages::{PackageId, SeparatorStyle};

/// Classic tools adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmdlineTools {
    /// The `cmdline-tools/<version>` directory name.
    pub version: String,
    /// The `bin` directory.
    pub bin: PathBuf,
    /// Path to `sdkmanager`, when present.
    pub sdkmanager: Option<PathBuf>,
    /// Path to `avdmanager`, when present.
    pub avdmanager: Option<PathBuf>,
}

impl CmdlineTools {
    /// The Java executable to use, when one is configured.
    ///
    /// Classic tools are Java programs. Avdium never assumes a system Java
    /// install: the caller decides which runtime to use, or none.
    pub fn java(&self) -> Option<PathBuf> {
        None
    }

    /// Whether a Java runtime is required for this provider.
    pub fn requires_java(&self) -> bool {
        true
    }

    /// `sdkmanager --list`.
    pub fn list_packages_spec(&self) -> Option<CommandSpec> {
        let program = self.sdkmanager.clone()?;
        Some(
            CommandSpec::new(program)
                .args(["--list", "--channel=0"])
                // Classic tools read `ANDROID_SDK_ROOT` and `ANDROID_HOME`;
                // the caller passes both consistently.
                .stdin(StdinMode::Closed),
        )
    }

    /// `sdkmanager --licenses`, which prints each license text in turn.
    pub fn licenses_spec(&self) -> Option<CommandSpec> {
        let program = self.sdkmanager.clone()?;
        Some(
            CommandSpec::new(program)
                .arg("--licenses")
                .stdin(StdinMode::Closed),
        )
    }

    /// `sdkmanager <id>`.
    ///
    /// `answers` is written to standard input. It must contain only the
    /// decisions covered by an explicit consent step, never an unbounded
    /// stream of `y`.
    pub fn install_spec(&self, id: &PackageId, answers: Option<&str>) -> Option<CommandSpec> {
        let program = self.sdkmanager.clone()?;
        let spec = CommandSpec::new(program).arg(id.classic());
        Some(match answers {
            Some(text) => spec.stdin(StdinMode::Text(text.to_owned())),
            None => spec.stdin(StdinMode::Closed),
        })
    }

    /// `sdkmanager --uninstall <id>`.
    pub fn uninstall_spec(&self, id: &PackageId) -> Option<CommandSpec> {
        let program = self.sdkmanager.clone()?;
        Some(
            CommandSpec::new(program)
                .args(["--uninstall", &id.classic()])
                .stdin(StdinMode::Closed),
        )
    }

    /// `sdkmanager --update`.
    pub fn update_spec(&self) -> Option<CommandSpec> {
        let program = self.sdkmanager.clone()?;
        Some(
            CommandSpec::new(program)
                .arg("--update")
                .stdin(StdinMode::Closed),
        )
    }

    /// `avdmanager list avd`.
    pub fn list_devices_spec(&self) -> Option<CommandSpec> {
        let program = self.avdmanager.clone()?;
        Some(
            CommandSpec::new(program)
                .args(["list", "avd"])
                .stdin(StdinMode::Closed),
        )
    }

    /// `avdmanager list device`.
    pub fn list_profiles_spec(&self) -> Option<CommandSpec> {
        let program = self.avdmanager.clone()?;
        Some(
            CommandSpec::new(program)
                .args(["list", "device"])
                .stdin(StdinMode::Closed),
        )
    }

    /// `avdmanager create avd`.
    ///
    /// `avdmanager` asks whether to create a custom hardware profile. Answering
    /// that prompt is part of the command, not an interactive surprise, so the
    /// answer is written explicitly and then standard input is closed.
    pub fn create_spec(&self, request: &CreateRequest) -> Option<CommandSpec> {
        let program = self.avdmanager.clone()?;
        let mut spec = CommandSpec::new(program).args(["create", "avd", "-n", &request.name]);
        spec = spec.args(["-k", &request.image.classic()]);
        if let Some(profile) = &request.profile {
            spec = spec.args(["-d", profile]);
        }
        if request.force {
            spec = spec.arg("--force");
        }
        Some(spec.stdin(StdinMode::Text("no\n".to_owned())))
    }

    /// `avdmanager delete avd -n <name>`.
    pub fn delete_spec(&self, name: &str) -> Option<CommandSpec> {
        let program = self.avdmanager.clone()?;
        Some(
            CommandSpec::new(program)
                .args(["delete", "avd", "-n", name])
                .stdin(StdinMode::Closed),
        )
    }

    /// What this provider can do.
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            list_packages: self.sdkmanager.is_some(),
            install_packages: self.sdkmanager.is_some(),
            update_packages: self.sdkmanager.is_some(),
            remove_packages: self.sdkmanager.is_some(),
            list_devices: self.avdmanager.is_some(),
            create_with_profile: self.avdmanager.is_some(),
            create_with_image: self.avdmanager.is_some(),
            list_profiles: self.avdmanager.is_some(),
            remove_device: self.avdmanager.is_some(),
            rename_device: false,
            start_device: false,
            stop_device: false,
            license_boundary: LicenseBoundary::PerPackageText,
        }
    }

    /// The separator style this provider expects.
    pub fn separator_style(&self) -> SeparatorStyle {
        SeparatorStyle::Semicolon
    }
}

/// A device creation request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateRequest {
    /// The AVD name.
    pub name: String,
    /// The system image package to base the device on.
    pub image: PackageId,
    /// An optional hardware profile id.
    pub profile: Option<String>,
    /// Replace an existing device with the same name.
    pub force: bool,
}

impl CreateRequest {
    /// A request for a name and image with no explicit profile.
    pub fn new(name: impl Into<String>, image: PackageId) -> Self {
        Self {
            name: name.into(),
            image,
            profile: None,
            force: false,
        }
    }

    /// Set the hardware profile.
    pub fn with_profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    /// Allow replacing an existing device.
    pub fn with_force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> CmdlineTools {
        CmdlineTools {
            version: "latest".to_owned(),
            bin: PathBuf::from("/sdk/cmdline-tools/latest/bin"),
            sdkmanager: Some(PathBuf::from("/sdk/cmdline-tools/latest/bin/sdkmanager")),
            avdmanager: Some(PathBuf::from("/sdk/cmdline-tools/latest/bin/avdmanager")),
        }
    }

    #[test]
    fn classic_identifiers_use_semicolons() {
        let id = PackageId::parse("system-images/android-36.1/google_apis/x86_64").expect("parse");
        let spec = tools().install_spec(&id, None).expect("spec");
        assert!(
            spec.args
                .iter()
                .any(|arg| arg == "system-images;android-36.1;google_apis;x86_64"),
            "{}",
            spec.preview()
        );
    }

    #[test]
    fn install_answers_are_written_explicitly_and_never_unbounded() {
        let id = PackageId::parse("platforms/android-36").expect("parse");
        let spec = tools().install_spec(&id, Some("y\n")).expect("spec");
        assert_eq!(spec.stdin, StdinMode::Text("y\n".to_owned()));
    }

    #[test]
    fn install_without_answers_closes_standard_input() {
        let id = PackageId::parse("platforms/android-36").expect("parse");
        let spec = tools().install_spec(&id, None).expect("spec");
        assert_eq!(spec.stdin, StdinMode::Closed);
    }

    #[test]
    fn create_uses_the_chosen_image_and_answers_the_hardware_prompt() {
        let image = PackageId::parse("system-images/android-36.1/google_apis_playstore/x86_64")
            .expect("parse");
        let request = CreateRequest::new("Avdium_Test", image).with_profile("pixel_9");
        let spec = tools().create_spec(&request).expect("spec");
        let args: Vec<String> = spec
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "create",
                "avd",
                "-n",
                "Avdium_Test",
                "-k",
                "system-images;android-36.1;google_apis_playstore;x86_64",
                "-d",
                "pixel_9"
            ]
        );
        // The custom hardware profile prompt must be answered, not left hanging.
        assert_eq!(spec.stdin, StdinMode::Text("no\n".to_owned()));
    }

    #[test]
    fn create_without_a_profile_omits_the_flag() {
        let image =
            PackageId::parse("system-images/android-36.1/google_apis/x86_64").expect("parse");
        let spec = tools()
            .create_spec(&CreateRequest::new("Plain", image))
            .expect("spec");
        assert!(
            !spec.args.iter().any(|arg| arg == "-d"),
            "{}",
            spec.preview()
        );
    }

    #[test]
    fn force_is_only_passed_when_requested() {
        let image =
            PackageId::parse("system-images/android-36.1/google_apis/x86_64").expect("parse");
        let request = CreateRequest::new("Dup", image).with_force(true);
        let spec = tools().create_spec(&request).expect("spec");
        assert!(spec.args.iter().any(|arg| arg == "--force"));
    }

    #[test]
    fn delete_names_the_device_explicitly() {
        let spec = tools().delete_spec("Avdium_Test").expect("spec");
        let args: Vec<String> = spec
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["delete", "avd", "-n", "Avdium_Test"]);
    }

    #[test]
    fn license_listing_is_a_separate_step() {
        let spec = tools().licenses_spec().expect("spec");
        assert!(spec.args.iter().any(|arg| arg == "--licenses"));
        assert_eq!(spec.stdin, StdinMode::Closed);
    }

    #[test]
    fn capabilities_follow_which_executables_exist() {
        let full = tools().capabilities();
        assert!(full.create_with_image);
        assert!(full.list_packages);
        assert!(!full.start_device);
        assert!(!full.rename_device);
        assert_eq!(full.license_boundary, LicenseBoundary::PerPackageText);
        assert!(full.license_boundary.allows_consent_flow());

        let avdmanager_only = CmdlineTools {
            sdkmanager: None,
            ..tools()
        };
        let partial = avdmanager_only.capabilities();
        assert!(!partial.list_packages);
        assert!(partial.create_with_image);
        assert!(avdmanager_only.list_packages_spec().is_none());

        let sdkmanager_only = CmdlineTools {
            avdmanager: None,
            ..tools()
        };
        let partial = sdkmanager_only.capabilities();
        assert!(partial.list_packages);
        assert!(!partial.create_with_image);
    }

    #[test]
    fn classic_tools_require_java() {
        assert!(tools().requires_java());
        assert_eq!(tools().java(), None);
    }
}
