//! The AVD model.
//!
//! An AVD is described by two files: a companion `<Name>.ini` in the AVD home,
//! and a `config.ini` inside the AVD's data directory. This module reads both
//! and records how the data directory was resolved, so a device whose files
//! point somewhere unexpected is never treated as an ordinary device.

use std::path::{Path, PathBuf};

use super::ini::{IniFile, IniProblem};

/// How an AVD's data directory was determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataDirOrigin {
    /// The companion file's `path` key, used as written.
    AbsolutePathKey,
    /// The companion file's `path.rel` key, resolved against the AVD home.
    RelativePathKey,
    /// No usable key, so the conventional `<name>.avd` next to the file.
    NameConvention,
    /// A key was present but could not be used safely.
    Unresolved,
}

impl DataDirOrigin {
    /// Label for diagnostics.
    pub fn label(self) -> &'static str {
        match self {
            Self::AbsolutePathKey => "companion `path`",
            Self::RelativePathKey => "companion `path.rel`",
            Self::NameConvention => "conventional location",
            Self::Unresolved => "unresolved",
        }
    }
}

/// Why an AVD cannot be used as-is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokenReason {
    /// The companion `.ini` could not be read.
    UnreadableCompanion {
        /// The underlying error.
        detail: String,
    },
    /// The companion `.ini` has lines that prevent a safe edit.
    MalformedCompanion {
        /// The problems found.
        problems: Vec<IniProblem>,
    },
    /// The data directory named by the companion file does not exist.
    MissingDataDir {
        /// The directory that was expected.
        path: PathBuf,
    },
    /// `config.ini` is missing from an existing data directory.
    MissingConfig {
        /// The file that was expected.
        path: PathBuf,
    },
    /// The companion file's path keys are unusable.
    UnresolvedDataDir {
        /// The raw value that could not be used.
        raw: String,
    },
    /// The system image referenced by the configuration is not installed.
    MissingImage {
        /// The expected image directory.
        path: PathBuf,
    },
    /// No `target` is recorded, so the device cannot be matched to an image.
    MissingTarget,
}

impl BrokenReason {
    /// One-line explanation for the UI.
    pub fn summary(&self) -> String {
        match self {
            Self::UnreadableCompanion { detail } => {
                format!("The AVD definition could not be read: {detail}")
            }
            Self::MalformedCompanion { problems } => format!(
                "The AVD definition has {} problem(s), for example: {}",
                problems.len(),
                problems
                    .first()
                    .map(IniProblem::summary)
                    .unwrap_or_default()
            ),
            Self::MissingDataDir { path } => {
                format!("The AVD data directory {} does not exist.", path.display())
            }
            Self::MissingConfig { path } => {
                format!("{} is missing from the AVD data directory.", path.display())
            }
            Self::UnresolvedDataDir { raw } => format!(
                "The AVD definition points at a data directory that cannot be resolved: {raw}"
            ),
            Self::MissingImage { path } => {
                format!("The system image {} is not installed.", path.display())
            }
            Self::MissingTarget => {
                "The AVD has no target, so its image cannot be checked.".to_owned()
            }
        }
    }
}

/// Everything needed to load an AVD besides the companion file itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct AvdLoadContext<'a> {
    /// SDK root. Used only to check whether the referenced image is installed,
    /// so a missing SDK never prevents discovery.
    pub sdk_root: Option<&'a Path>,
    /// Base directory for a relative `path` or `path.rel` value.
    ///
    /// The Android tools write `path.rel` relative to the user home, which is
    /// the parent of the conventional `avd` directory, not relative to the AVD
    /// home itself. The companion file in the project brief is the concrete
    /// example: with the AVD home at `~/.android/avd`, it records
    /// `path.rel=avd/Pixel_9.avd`.
    ///
    /// When `None`, the parent of the companion file's directory is used.
    pub relative_base: Option<&'a Path>,
}

impl<'a> AvdLoadContext<'a> {
    /// A context for a device whose image should not be checked.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the SDK root used for the image check.
    pub fn with_sdk_root(mut self, sdk_root: Option<&'a Path>) -> Self {
        self.sdk_root = sdk_root;
        self
    }

    /// Set the base for relative paths.
    pub fn with_relative_base(mut self, base: &'a Path) -> Self {
        self.relative_base = Some(base);
        self
    }
}

/// One Android virtual device.
#[derive(Debug, Clone, PartialEq)]
pub struct Avd {
    /// The AVD name, which is also the companion file stem.
    pub name: String,
    /// The companion `<Name>.ini` path.
    pub ini_path: PathBuf,
    /// The AVD's data directory.
    pub data_dir: PathBuf,
    /// How the data directory was determined.
    pub data_dir_origin: DataDirOrigin,
    /// `config.ini` inside the data directory.
    pub config_path: PathBuf,
    /// The parsed companion file.
    pub companion: IniFile,
    /// The parsed `config.ini`, empty when it could not be read.
    pub config: IniFile,
    /// Everything that stops this device from working.
    pub broken: Vec<BrokenReason>,
}

impl Avd {
    /// Read an AVD from its companion `.ini` path.
    ///
    /// A missing SDK never prevents discovery: it only means the referenced
    /// image cannot be checked.
    pub fn load(ini_path: &Path, context: &AvdLoadContext<'_>) -> Self {
        let name = ini_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| ini_path.display().to_string());
        let avd_home = ini_path.parent().unwrap_or_else(|| Path::new("."));
        let mut broken = Vec::new();

        let (companion, companion_readable) = match std::fs::read_to_string(ini_path) {
            Ok(text) => (IniFile::parse(&text), true),
            Err(error) => {
                broken.push(BrokenReason::UnreadableCompanion {
                    detail: error.to_string(),
                });
                (IniFile::new(), false)
            }
        };

        if companion_readable {
            let problems = companion.problems();
            if !problems.is_empty() {
                broken.push(BrokenReason::MalformedCompanion { problems });
            }
        }

        let relative_base = context
            .relative_base
            .unwrap_or_else(|| avd_home.parent().unwrap_or(avd_home));
        let (data_dir, data_dir_origin) =
            resolve_data_dir(&companion, &name, avd_home, relative_base, &mut broken);
        let config_path = data_dir.join("config.ini");

        let mut config = IniFile::new();
        if data_dir.is_dir() {
            match std::fs::read_to_string(&config_path) {
                Ok(text) => config = IniFile::parse(&text),
                Err(_) => broken.push(BrokenReason::MissingConfig {
                    path: config_path.clone(),
                }),
            }
        } else if !broken
            .iter()
            .any(|reason| matches!(reason, BrokenReason::MissingDataDir { .. }))
        {
            broken.push(BrokenReason::MissingDataDir {
                path: data_dir.clone(),
            });
        }

        let mut avd = Self {
            name,
            ini_path: ini_path.to_path_buf(),
            data_dir,
            data_dir_origin,
            config_path,
            companion,
            config,
            broken,
        };

        if !avd.config.is_empty() {
            match avd.image_sysdir() {
                Some(sysdir) => {
                    if let Some(sdk_root) = context.sdk_root {
                        let image_dir = sdk_root.join(sysdir);
                        if !image_dir.is_dir() {
                            avd.broken
                                .push(BrokenReason::MissingImage { path: image_dir });
                        }
                    }
                }
                // The emulator can resolve an image from `target` alone, so an
                // absent `image.sysdir.1` is only a problem when there is no
                // target to fall back on.
                None => {
                    if avd.target().is_none() {
                        avd.broken.push(BrokenReason::MissingTarget);
                    }
                }
            }
        }

        avd
    }

    /// Whether anything about this device is wrong.
    pub fn is_broken(&self) -> bool {
        !self.broken.is_empty()
    }

    /// Display name, falling back to the AVD name.
    pub fn display_name(&self) -> &str {
        self.config
            .get_trimmed("avd.ini.displayname")
            .filter(|value| !value.is_empty())
            .unwrap_or(&self.name)
    }

    /// The `target` value, kept as a string. Values such as `android-36.1` are
    /// never parsed as integers.
    pub fn target(&self) -> Option<&str> {
        self.config
            .get_trimmed("target")
            .filter(|value| !value.is_empty())
    }

    /// The image ABI, for example `x86_64`.
    pub fn abi(&self) -> Option<&str> {
        self.config
            .get_trimmed("abi.type")
            .filter(|value| !value.is_empty())
            .or_else(|| {
                self.config
                    .get_trimmed("hw.cpu.arch")
                    .filter(|value| !value.is_empty())
            })
    }

    /// The CPU architecture reported by the hardware configuration.
    pub fn cpu_arch(&self) -> Option<&str> {
        self.config
            .get_trimmed("hw.cpu.arch")
            .filter(|value| !value.is_empty())
    }

    /// The image tag, for example `google_apis`.
    pub fn tag_id(&self) -> Option<&str> {
        self.config
            .get_trimmed("tag.id")
            .filter(|value| !value.is_empty())
    }

    /// The human-readable tag, for example `Google APIs`.
    pub fn tag_display(&self) -> Option<&str> {
        self.config
            .get_trimmed("tag.display")
            .filter(|value| !value.is_empty())
            .or_else(|| {
                self.config
                    .get_trimmed("tag.displaynames")
                    .filter(|value| !value.is_empty())
            })
    }

    /// The relative system image directory recorded in the configuration.
    pub fn image_sysdir(&self) -> Option<&str> {
        self.config
            .get_trimmed("image.sysdir.1")
            .filter(|value| !value.is_empty())
    }

    /// The absolute system image directory, when an SDK root is known.
    pub fn image_dir(&self, sdk_root: Option<&Path>) -> Option<PathBuf> {
        let sdk_root = sdk_root?;
        let sysdir = self.image_sysdir()?;
        Some(sdk_root.join(sysdir))
    }

    /// Configured RAM in MiB.
    pub fn ram_mb(&self) -> Option<u32> {
        parse_leading_u32(self.config.get_trimmed("hw.ramSize")?)
    }

    /// Configured CPU core count.
    pub fn cores(&self) -> Option<u32> {
        parse_leading_u32(self.config.get_trimmed("hw.cpu.ncore")?)
    }

    /// Configured graphics mode, for example `auto`.
    pub fn gpu_mode(&self) -> Option<&str> {
        self.config
            .get_trimmed("hw.gpu.mode")
            .filter(|value| !value.is_empty())
    }

    /// Whether hardware keyboard input is enabled.
    pub fn keyboard_enabled(&self) -> Option<bool> {
        parse_yes_no(self.config.get_trimmed("hw.keyboard")?)
    }

    /// Whether the configuration claims Play Store support.
    ///
    /// This is a recorded flag, not proof: the installed image's capabilities
    /// are authoritative.
    pub fn play_store_flag(&self) -> Option<bool> {
        parse_yes_no(self.config.get_trimmed("PlayStore.enabled")?)
    }

    /// The recorded device profile name, for example `pixel_9`.
    pub fn device_name(&self) -> Option<&str> {
        self.config
            .get_trimmed("hw.device.name")
            .filter(|value| !value.is_empty())
    }

    /// One-line description of the image, for the device list.
    pub fn image_label(&self) -> String {
        let target = self.target().unwrap_or("unknown target");
        let tag = self.tag_display().or_else(|| self.tag_id());
        let abi = self.abi();
        match (tag, abi) {
            (Some(tag), Some(abi)) => format!("{target} {tag} {abi}"),
            (Some(tag), None) => format!("{target} {tag}"),
            (None, Some(abi)) => format!("{target} {abi}"),
            (None, None) => target.to_owned(),
        }
    }

    /// The first broken reason, if any.
    pub fn primary_problem(&self) -> Option<&BrokenReason> {
        self.broken.first()
    }
}

fn resolve_data_dir(
    companion: &IniFile,
    name: &str,
    avd_home: &Path,
    relative_base: &Path,
    broken: &mut Vec<BrokenReason>,
) -> (PathBuf, DataDirOrigin) {
    if let Some(raw) = companion
        .get_trimmed("path")
        .filter(|value| !value.is_empty())
    {
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            return (path, DataDirOrigin::AbsolutePathKey);
        }
        // A relative `path` is resolved against the user home, never against
        // the process working directory, which is never what it means.
        return (
            resolve_relative(&path, relative_base, avd_home),
            DataDirOrigin::RelativePathKey,
        );
    }

    if let Some(raw) = companion
        .get_trimmed("path.rel")
        .filter(|value| !value.is_empty())
    {
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            broken.push(BrokenReason::UnresolvedDataDir {
                raw: raw.to_owned(),
            });
            return (
                avd_home.join(format!("{name}.avd")),
                DataDirOrigin::Unresolved,
            );
        }
        return (
            resolve_relative(&path, relative_base, avd_home),
            DataDirOrigin::RelativePathKey,
        );
    }

    (
        avd_home.join(format!("{name}.avd")),
        DataDirOrigin::NameConvention,
    )
}

/// Resolve a relative data directory against the user home.
///
/// Some third-party writers record a path relative to the AVD home instead, so
/// when the documented base does not produce an existing directory and the AVD
/// home does, the existing directory is used. Only an existing directory is
/// ever accepted, so this cannot silently pick a nonexistent target.
fn resolve_relative(path: &Path, relative_base: &Path, avd_home: &Path) -> PathBuf {
    let from_base = relative_base.join(path);
    if from_base.is_dir() {
        return from_base;
    }
    let from_home = avd_home.join(path);
    if from_home.is_dir() {
        return from_home;
    }
    from_base
}

fn parse_leading_u32(value: &str) -> Option<u32> {
    let digits: String = value
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

fn parse_yes_no(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "yes" | "true" | "1" => Some(true),
        "no" | "false" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "\
AvdId=Pixel_9
PlayStore.enabled=false
abi.type=x86_64
avd.ini.displayname=Pixel 9
hw.cpu.arch=x86_64
hw.cpu.ncore=6
hw.device.name=pixel_9
hw.gpu.mode=auto
hw.keyboard=yes
hw.ramSize=8192
image.sysdir.1=system-images/android-36.1/google_apis/x86_64/
tag.display=Google APIs
tag.id=google_apis
target=android-36.1
";

    fn write_avd(root: &Path, name: &str, companion: &str, config: Option<&str>) -> PathBuf {
        let home = root.join("avd");
        let data = home.join(format!("{name}.avd"));
        std::fs::create_dir_all(&data).expect("create data dir");
        if let Some(config) = config {
            std::fs::write(data.join("config.ini"), config).expect("write config");
        }
        let ini = home.join(format!("{name}.ini"));
        std::fs::write(&ini, companion).expect("write companion");
        ini
    }

    #[test]
    fn a_complete_device_is_not_broken() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sdk = temp.path().join("sdk");
        std::fs::create_dir_all(sdk.join("system-images/android-36.1/google_apis/x86_64"))
            .expect("create image");
        let ini = write_avd(
            temp.path(),
            "Pixel_9",
            "avd.ini.encoding=UTF-8\npath.rel=avd/Pixel_9.avd\ntarget=android-36.1\n",
            Some(CONFIG),
        );

        let avd = Avd::load(&ini, &AvdLoadContext::new().with_sdk_root(Some(&sdk)));
        assert!(!avd.is_broken(), "unexpected problems: {:?}", avd.broken);
        assert_eq!(avd.name, "Pixel_9");
        assert_eq!(avd.display_name(), "Pixel 9");
        assert_eq!(avd.target(), Some("android-36.1"));
        assert_eq!(avd.abi(), Some("x86_64"));
        assert_eq!(avd.tag_id(), Some("google_apis"));
        assert_eq!(avd.ram_mb(), Some(8192));
        assert_eq!(avd.cores(), Some(6));
        assert_eq!(avd.gpu_mode(), Some("auto"));
        assert_eq!(avd.keyboard_enabled(), Some(true));
        assert_eq!(avd.play_store_flag(), Some(false));
        assert_eq!(avd.device_name(), Some("pixel_9"));
        assert_eq!(avd.data_dir_origin, DataDirOrigin::RelativePathKey);
        assert_eq!(avd.image_label(), "android-36.1 Google APIs x86_64");
    }

    #[test]
    fn a_missing_image_is_reported_without_blocking_discovery() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sdk = temp.path().join("sdk");
        std::fs::create_dir_all(&sdk).expect("create sdk");
        let ini = write_avd(
            temp.path(),
            "Pixel_9",
            "path.rel=avd/Pixel_9.avd\ntarget=android-36.1\n",
            Some(CONFIG),
        );

        let avd = Avd::load(&ini, &AvdLoadContext::new().with_sdk_root(Some(&sdk)));
        assert!(avd.is_broken());
        assert!(matches!(
            avd.primary_problem(),
            Some(BrokenReason::MissingImage { .. })
        ));
        assert!(
            avd.primary_problem()
                .expect("problem")
                .summary()
                .contains("not installed")
        );
    }

    #[test]
    fn discovery_works_with_no_sdk_at_all() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ini = write_avd(
            temp.path(),
            "Pixel_9",
            "path.rel=avd/Pixel_9.avd\ntarget=android-36.1\n",
            Some(CONFIG),
        );
        let avd = Avd::load(&ini, &AvdLoadContext::new());
        assert!(!avd.is_broken());
        assert_eq!(avd.image_dir(None), None);
    }

    #[test]
    fn a_missing_data_directory_is_reported() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("avd");
        std::fs::create_dir_all(&home).expect("create home");
        let ini = home.join("Ghost.ini");
        std::fs::write(&ini, "path.rel=avd/Ghost.avd\n").expect("write");

        let avd = Avd::load(&ini, &AvdLoadContext::new());
        assert!(matches!(
            avd.primary_problem(),
            Some(BrokenReason::MissingDataDir { .. })
        ));
    }

    #[test]
    fn an_existing_data_directory_without_config_is_reported() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ini = write_avd(temp.path(), "Bare", "path.rel=avd/Bare.avd\n", None);
        let avd = Avd::load(&ini, &AvdLoadContext::new());
        assert!(matches!(
            avd.primary_problem(),
            Some(BrokenReason::MissingConfig { .. })
        ));
    }

    #[test]
    fn an_absolute_path_key_is_used_as_written() {
        let temp = tempfile::tempdir().expect("tempdir");
        let external = temp.path().join("external-device");
        std::fs::create_dir_all(&external).expect("create");
        std::fs::write(external.join("config.ini"), CONFIG).expect("write config");
        let home = temp.path().join("avd");
        std::fs::create_dir_all(&home).expect("create home");
        let ini = home.join("Imported.ini");
        std::fs::write(&ini, format!("path={}\n", external.display())).expect("write");

        let avd = Avd::load(&ini, &AvdLoadContext::new());
        assert_eq!(avd.data_dir, external);
        assert_eq!(avd.data_dir_origin, DataDirOrigin::AbsolutePathKey);
    }

    #[test]
    fn a_relative_path_key_is_resolved_against_the_user_home_not_the_cwd() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ini = write_avd(temp.path(), "Rel", "path=somewhere/Rel.avd\n", Some(CONFIG));
        // No base was supplied, so the parent of the companion's directory is
        // used, which is the user home in the conventional layout.
        let avd = Avd::load(&ini, &AvdLoadContext::new());
        assert_eq!(avd.data_dir, temp.path().join("somewhere/Rel.avd"));
        assert_eq!(avd.data_dir_origin, DataDirOrigin::RelativePathKey);
        assert!(
            !avd.data_dir
                .starts_with(std::env::current_dir().expect("cwd"))
        );
    }

    #[test]
    fn a_relative_path_is_taken_from_the_avd_home_when_that_directory_exists() {
        let temp = tempfile::tempdir().expect("tempdir");
        // A writer that recorded the path relative to the AVD home instead.
        let ini = write_avd(temp.path(), "Rel", "path=Rel.avd\n", Some(CONFIG));
        let avd = Avd::load(&ini, &AvdLoadContext::new());
        assert_eq!(avd.data_dir, temp.path().join("avd/Rel.avd"));
        assert!(avd.data_dir.is_dir());
        assert!(
            avd.broken.is_empty(),
            "unexpected problems: {:?}",
            avd.broken
        );
    }

    #[test]
    fn a_device_with_a_target_but_no_image_directory_is_not_broken() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ini = write_avd(
            temp.path(),
            "Targeted",
            "path.rel=avd/Targeted.avd\n",
            Some("target=android-36\nabi.type=x86_64\n"),
        );
        let avd = Avd::load(&ini, &AvdLoadContext::new());
        assert!(
            avd.broken.is_empty(),
            "unexpected problems: {:?}",
            avd.broken
        );
        assert_eq!(avd.target(), Some("android-36"));
    }

    #[test]
    fn a_device_with_neither_a_target_nor_an_image_is_reported() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ini = write_avd(
            temp.path(),
            "Untargeted",
            "path.rel=avd/Untargeted.avd\n",
            Some("abi.type=x86_64\n"),
        );
        let avd = Avd::load(&ini, &AvdLoadContext::new());
        assert!(matches!(
            avd.primary_problem(),
            Some(BrokenReason::MissingTarget)
        ));
    }

    #[test]
    fn a_malformed_companion_file_is_reported_but_still_loaded() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ini = write_avd(
            temp.path(),
            "Odd",
            "path.rel=avd/Odd.avd\nnot a pair\n",
            Some(CONFIG),
        );
        let avd = Avd::load(&ini, &AvdLoadContext::new());
        assert!(matches!(
            avd.primary_problem(),
            Some(BrokenReason::MalformedCompanion { .. })
        ));
        // The usable part of the file is still applied.
        assert_eq!(avd.data_dir_origin, DataDirOrigin::RelativePathKey);
    }

    #[test]
    fn numeric_values_with_suffixes_parse_leniently() {
        assert_eq!(parse_leading_u32("8192"), Some(8192));
        assert_eq!(parse_leading_u32(" 1024M "), Some(1024));
        assert_eq!(parse_leading_u32(""), None);
        assert_eq!(parse_leading_u32("auto"), None);
    }

    #[test]
    fn yes_no_parsing_is_case_insensitive_and_rejects_nonsense() {
        assert_eq!(parse_yes_no("YES"), Some(true));
        assert_eq!(parse_yes_no("false"), Some(false));
        assert_eq!(parse_yes_no("maybe"), None);
    }
}
