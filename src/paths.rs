//! Where things live: SDK root, AVD home, and Avdium's own XDG directories.
//!
//! Resolution is a pure function of a home directory, saved settings and an
//! environment snapshot. That makes conflicting-environment behavior testable
//! without touching the real process environment.

use std::path::{Path, PathBuf};

/// Conventional fresh-SDK location on Linux.
pub const CONVENTIONAL_SDK_SUBDIR: &str = "Android/Sdk";

/// Where a resolved path came from. Recorded so the UI can explain the choice
/// instead of silently preferring one source over another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathOrigin {
    /// Explicit choice saved by the user in Avdium settings.
    Settings,
    /// `ANDROID_HOME`, the primary SDK environment variable.
    AndroidHome,
    /// `ANDROID_SDK_ROOT`, deprecated but still honored.
    AndroidSdkRoot,
    /// `ANDROID_AVD_HOME`.
    AndroidAvdHome,
    /// `ANDROID_USER_HOME`.
    AndroidUserHome,
    /// `ANDROID_EMULATOR_HOME`.
    AndroidEmulatorHome,
    /// A built-in default such as `~/Android/Sdk`.
    Conventional,
}

impl PathOrigin {
    /// Short label for diagnostics and UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Settings => "Avdium settings",
            Self::AndroidHome => "ANDROID_HOME",
            Self::AndroidSdkRoot => "ANDROID_SDK_ROOT (deprecated)",
            Self::AndroidAvdHome => "ANDROID_AVD_HOME",
            Self::AndroidUserHome => "ANDROID_USER_HOME",
            Self::AndroidEmulatorHome => "ANDROID_EMULATOR_HOME",
            Self::Conventional => "default location",
        }
    }

    /// Whether this origin is an environment variable.
    pub fn is_environment(self) -> bool {
        !matches!(self, Self::Settings | Self::Conventional)
    }
}

/// The subset of the process environment that affects path resolution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvSnapshot {
    /// `ANDROID_HOME`.
    pub android_home: Option<PathBuf>,
    /// `ANDROID_SDK_ROOT`.
    pub android_sdk_root: Option<PathBuf>,
    /// `ANDROID_USER_HOME`.
    pub android_user_home: Option<PathBuf>,
    /// `ANDROID_EMULATOR_HOME`.
    pub android_emulator_home: Option<PathBuf>,
    /// `ANDROID_AVD_HOME`.
    pub android_avd_home: Option<PathBuf>,
}

impl EnvSnapshot {
    /// Read the real process environment.
    pub fn from_process() -> Self {
        Self::from_lookup(|name| std::env::var_os(name).map(PathBuf::from))
    }

    /// Build from a lookup function. Empty values are treated as unset, which
    /// matches how the official tools behave.
    pub fn from_lookup(mut lookup: impl FnMut(&str) -> Option<PathBuf>) -> Self {
        let mut get = |name: &str| -> Option<PathBuf> {
            lookup(name).filter(|path| !path.as_os_str().is_empty())
        };
        Self {
            android_home: get("ANDROID_HOME"),
            android_sdk_root: get("ANDROID_SDK_ROOT"),
            android_user_home: get("ANDROID_USER_HOME"),
            android_emulator_home: get("ANDROID_EMULATOR_HOME"),
            android_avd_home: get("ANDROID_AVD_HOME"),
        }
    }
}

/// A path chosen from one source, with the sources that lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// The winning path.
    pub path: PathBuf,
    /// Where the winning path came from.
    pub origin: PathOrigin,
    /// Paths from other sources that point somewhere else. Never hidden.
    pub overridden: Vec<(PathOrigin, PathBuf)>,
}

impl Resolution {
    fn new(path: PathBuf, origin: PathOrigin) -> Self {
        Self {
            path,
            origin,
            overridden: Vec::new(),
        }
    }
}

/// A conflicting configuration that the UI must surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathConflict {
    /// What the conflict is about, for example `SDK root`.
    pub subject: &'static str,
    /// The value that wins.
    pub selected: (PathOrigin, PathBuf),
    /// The values that lose.
    pub ignored: Vec<(PathOrigin, PathBuf)>,
}

impl PathConflict {
    /// One-line explanation for the doctor and settings screens.
    pub fn summary(&self) -> String {
        let (origin, path) = &self.selected;
        let others = self
            .ignored
            .iter()
            .map(|(origin, path)| format!("{} ({})", path.display(), origin.label()))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{}: using {} ({}), ignoring {}",
            self.subject,
            path.display(),
            origin.label(),
            others
        )
    }
}

/// A `cmdline-tools` installation discovered under an SDK root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmdlineToolsInstall {
    /// Directory name, for example `latest` or `19.0`.
    pub version: String,
    /// The `bin` directory holding the launchers.
    pub bin: PathBuf,
}

impl CmdlineToolsInstall {
    /// Path to `sdkmanager`, if the launcher exists.
    pub fn sdkmanager(&self) -> Option<PathBuf> {
        launcher(&self.bin, "sdkmanager")
    }

    /// Path to `avdmanager`, if the launcher exists.
    pub fn avdmanager(&self) -> Option<PathBuf> {
        launcher(&self.bin, "avdmanager")
    }
}

fn launcher(bin: &Path, name: &str) -> Option<PathBuf> {
    let plain = bin.join(name);
    if plain.is_file() {
        return Some(plain);
    }
    let bat = bin.join(format!("{name}.bat"));
    if bat.is_file() {
        return Some(bat);
    }
    None
}

/// Resolved paths for one run, plus everything the user should know about how
/// they were chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathContext {
    /// The user's home directory.
    pub home: PathBuf,
    /// SDK root, when one could be resolved at all.
    pub sdk: Option<Resolution>,
    /// AVD home. Always has a value: AVD discovery must work even with no SDK.
    pub avd_home: PathBuf,
    /// Where `avd_home` came from.
    pub avd_home_origin: PathOrigin,
    /// `ANDROID_USER_HOME`, defaulting to `~/.android`.
    pub android_user_home: PathBuf,
    /// `ANDROID_EMULATOR_HOME`, defaulting to `ANDROID_USER_HOME`.
    pub emulator_home: PathBuf,
    /// Conflicts that were resolved by preference rather than agreement.
    pub conflicts: Vec<PathConflict>,
    /// Non-fatal problems worth showing in the doctor.
    pub warnings: Vec<String>,
}

impl PathContext {
    /// Resolve every path from explicit inputs.
    ///
    /// Precedence for the SDK root:
    /// 1. saved Avdium setting
    /// 2. `ANDROID_HOME`
    /// 3. `ANDROID_SDK_ROOT` (deprecated)
    /// 4. `~/Android/Sdk`
    ///
    /// Precedence for the AVD home, matching the documented Android behavior:
    /// 1. saved Avdium setting
    /// 2. `ANDROID_AVD_HOME`
    /// 3. `ANDROID_EMULATOR_HOME/avd`, where `ANDROID_EMULATOR_HOME` defaults
    ///    to `ANDROID_USER_HOME`
    /// 4. `ANDROID_USER_HOME/avd`, where `ANDROID_USER_HOME` defaults to
    ///    `~/.android`
    pub fn resolve(
        home: &Path,
        settings_sdk: Option<&Path>,
        settings_avd_home: Option<&Path>,
        env: &EnvSnapshot,
    ) -> Self {
        let mut conflicts = Vec::new();
        let mut warnings = Vec::new();

        // SDK root. Ordered from strongest to weakest preference.
        let mut sdk_candidates: Vec<(PathOrigin, PathBuf)> = Vec::new();
        if let Some(path) = settings_sdk {
            sdk_candidates.push((PathOrigin::Settings, path.to_path_buf()));
        }
        if let Some(path) = &env.android_home {
            sdk_candidates.push((PathOrigin::AndroidHome, path.clone()));
        }
        if let Some(path) = &env.android_sdk_root {
            sdk_candidates.push((PathOrigin::AndroidSdkRoot, path.clone()));
        }
        let conventional = home.join(CONVENTIONAL_SDK_SUBDIR);
        let sdk = select("SDK root", sdk_candidates, &mut conflicts)
            .or_else(|| Some(Resolution::new(conventional, PathOrigin::Conventional)));

        let sdk = sdk.map(|mut resolution| {
            if !resolution.path.is_dir() {
                warnings.push(format!(
                    "SDK root {} does not exist yet (from {}).",
                    resolution.path.display(),
                    resolution.origin.label()
                ));
            }
            resolution.overridden.clear();
            resolution
        });

        // ANDROID_HOME is primary; ANDROID_SDK_ROOT is deprecated.
        if let (Some(home_var), Some(root_var)) = (&env.android_home, &env.android_sdk_root)
            && home_var != root_var
        {
            warnings.push(format!(
                "ANDROID_HOME and the deprecated ANDROID_SDK_ROOT disagree. \
                 Android tooling is inconsistent, so the selected SDK may not be \
                 the one other tools use ({} vs {}).",
                home_var.display(),
                root_var.display()
            ));
        }

        // Android user and emulator homes.
        let android_user_home = env
            .android_user_home
            .clone()
            .unwrap_or_else(|| home.join(".android"));
        let emulator_home = env
            .android_emulator_home
            .clone()
            .unwrap_or_else(|| android_user_home.clone());

        // AVD home. An explicit Avdium setting wins over every environment
        // variable, and a disagreement is reported rather than hidden.
        let mut avd_candidates: Vec<(PathOrigin, PathBuf)> = Vec::new();
        if let Some(path) = settings_avd_home {
            avd_candidates.push((PathOrigin::Settings, path.to_path_buf()));
        }
        if let Some(path) = &env.android_avd_home {
            avd_candidates.push((PathOrigin::AndroidAvdHome, path.clone()));
        }
        let mut avd_conflicts = Vec::new();
        let selected_avd = select("AVD home", avd_candidates, &mut avd_conflicts);
        conflicts.extend(avd_conflicts);

        let (avd_home, avd_home_origin) = match selected_avd {
            Some(resolution) => (resolution.path, resolution.origin),
            None => {
                let origin = if env.android_emulator_home.is_some() {
                    PathOrigin::AndroidEmulatorHome
                } else if env.android_user_home.is_some() {
                    PathOrigin::AndroidUserHome
                } else {
                    PathOrigin::Conventional
                };
                (emulator_home.join("avd"), origin)
            }
        };

        Self {
            home: home.to_path_buf(),
            sdk,
            avd_home,
            avd_home_origin,
            android_user_home,
            emulator_home,
            conflicts,
            warnings,
        }
    }

    /// Resolve from the real environment.
    pub fn from_process(settings_sdk: Option<&Path>, settings_avd_home: Option<&Path>) -> Self {
        let home = home_dir().unwrap_or_else(|| PathBuf::from("/"));
        Self::resolve(
            &home,
            settings_sdk,
            settings_avd_home,
            &EnvSnapshot::from_process(),
        )
    }

    /// Environment overrides to pass to every child process so tools and Avdium
    /// agree on the same SDK and AVD locations.
    pub fn child_env(&self) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        let mut env: Vec<(std::ffi::OsString, std::ffi::OsString)> = Vec::new();
        if let Some(sdk) = &self.sdk {
            env.push(("ANDROID_HOME".into(), sdk.path.clone().into()));
            // Deprecated but still read by classic tools. Keep it consistent
            // with the selected SDK instead of leaving a stale value behind.
            env.push(("ANDROID_SDK_ROOT".into(), sdk.path.clone().into()));
        }
        env.push((
            "ANDROID_USER_HOME".into(),
            self.android_user_home.clone().into(),
        ));
        env.push((
            "ANDROID_EMULATOR_HOME".into(),
            self.emulator_home.clone().into(),
        ));
        env.push(("ANDROID_AVD_HOME".into(), self.avd_home.clone().into()));
        env
    }

    /// Path to the emulator executable, if the SDK has one.
    pub fn emulator_program(&self) -> Option<PathBuf> {
        let sdk = self.sdk.as_ref()?;
        let program = sdk.path.join("emulator").join("emulator");
        program.is_file().then_some(program)
    }

    /// Path to the ADB executable, if the SDK has one.
    pub fn adb_program(&self) -> Option<PathBuf> {
        let sdk = self.sdk.as_ref()?;
        let program = sdk.path.join("platform-tools").join("adb");
        program.is_file().then_some(program)
    }

    /// Every `cmdline-tools/<version>/bin` directory, `latest` first.
    pub fn cmdline_tools_installs(&self) -> Vec<CmdlineToolsInstall> {
        let Some(sdk) = &self.sdk else {
            return Vec::new();
        };
        let root = sdk.path.join("cmdline-tools");
        let Ok(entries) = std::fs::read_dir(&root) else {
            return Vec::new();
        };
        let mut installs: Vec<CmdlineToolsInstall> = entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| {
                let version = entry.file_name().to_string_lossy().into_owned();
                let bin = entry.path().join("bin");
                bin.is_dir().then_some(CmdlineToolsInstall { version, bin })
            })
            .collect();
        // `latest` is the conventional stable location, so prefer it, then sort
        // the rest by name descending so newer version directories come first.
        installs.sort_by(|a, b| match (a.version.as_str(), b.version.as_str()) {
            ("latest", "latest") => std::cmp::Ordering::Equal,
            ("latest", _) => std::cmp::Ordering::Less,
            (_, "latest") => std::cmp::Ordering::Greater,
            (a, b) => b.cmp(a),
        });
        installs
    }

    /// True when the resolved SDK looks like an Android SDK rather than an
    /// arbitrary directory.
    pub fn sdk_looks_usable(&self) -> bool {
        self.sdk
            .as_ref()
            .is_some_and(|resolution| looks_like_sdk(&resolution.path))
    }

    /// Human-readable description of the resolved SDK, for the UI.
    pub fn sdk_summary(&self) -> String {
        match &self.sdk {
            Some(resolution) => format!(
                "{} (from {})",
                resolution.path.display(),
                resolution.origin.label()
            ),
            None => "not found".to_owned(),
        }
    }
}

fn select(
    subject: &'static str,
    candidates: Vec<(PathOrigin, PathBuf)>,
    conflicts: &mut Vec<PathConflict>,
) -> Option<Resolution> {
    let mut iter = candidates.into_iter();
    let (origin, path) = iter.next()?;
    let overridden: Vec<(PathOrigin, PathBuf)> = iter.filter(|(_, other)| other != &path).collect();
    if !overridden.is_empty() {
        conflicts.push(PathConflict {
            subject,
            selected: (origin, path.clone()),
            ignored: overridden,
        });
    }
    Some(Resolution {
        path,
        origin,
        overridden: Vec::new(),
    })
}

/// Whether a directory contains recognizable SDK components.
pub fn looks_like_sdk(path: &Path) -> bool {
    const MARKERS: [&str; 6] = [
        "emulator",
        "platform-tools",
        "cmdline-tools",
        "platforms",
        "build-tools",
        "system-images",
    ];
    MARKERS.iter().any(|marker| path.join(marker).is_dir())
}

/// The user's home directory, from `HOME`.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// Avdium's own XDG directories. The user's SDK and AVDs never live here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    /// Configuration, including `settings.json`.
    pub config: PathBuf,
    /// Cached catalog documents and other regenerable data.
    pub cache: PathBuf,
    /// Local installation records.
    pub data: PathBuf,
    /// Launch records, logs and other run state.
    pub state: PathBuf,
}

impl AppDirs {
    /// Derive the directories from the environment.
    pub fn from_process() -> Option<Self> {
        let project = directories::ProjectDirs::from("dev", "okazakee", "Avdium")?;
        Some(Self {
            config: project.config_dir().to_path_buf(),
            cache: project.cache_dir().to_path_buf(),
            data: project.data_dir().to_path_buf(),
            state: project
                .state_dir()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| project.data_dir().to_path_buf()),
        })
    }

    /// Build from explicit roots. Used by tests.
    pub fn from_roots(
        config: impl Into<PathBuf>,
        cache: impl Into<PathBuf>,
        data: impl Into<PathBuf>,
        state: impl Into<PathBuf>,
    ) -> Self {
        Self {
            config: config.into(),
            cache: cache.into(),
            data: data.into(),
            state: state.into(),
        }
    }

    /// `settings.json`.
    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    /// Directory for private download staging.
    pub fn staging_dir(&self) -> PathBuf {
        self.cache.join("staging")
    }

    /// Directory for cached catalog documents.
    pub fn catalog_dir(&self) -> PathBuf {
        self.cache.join("catalog")
    }

    /// Directory for captured tool output and Avdium logs.
    pub fn logs_dir(&self) -> PathBuf {
        self.state.join("logs")
    }

    /// Directory for launch records that survive an application restart.
    pub fn launches_dir(&self) -> PathBuf {
        self.state.join("launches")
    }

    /// Create the directories that must exist before writing.
    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [&self.config, &self.cache, &self.data, &self.state] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> EnvSnapshot {
        EnvSnapshot::from_lookup(|name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| PathBuf::from(value))
        })
    }

    #[test]
    fn conventional_sdk_is_used_when_nothing_is_configured() {
        let context = PathContext::resolve(Path::new("/home/example"), None, None, &env(&[]));
        let sdk = context
            .sdk
            .expect("a conventional SDK path is always proposed");
        assert_eq!(sdk.path, PathBuf::from("/home/example/Android/Sdk"));
        assert_eq!(sdk.origin, PathOrigin::Conventional);
        assert_eq!(
            context.avd_home,
            PathBuf::from("/home/example/.android/avd")
        );
        assert_eq!(context.avd_home_origin, PathOrigin::Conventional);
        assert!(context.conflicts.is_empty());
    }

    #[test]
    fn settings_win_over_environment_and_the_conflict_is_reported() {
        let context = PathContext::resolve(
            Path::new("/home/example"),
            Some(Path::new("/opt/custom-sdk")),
            None,
            &env(&[("ANDROID_HOME", "/home/example/Android/Sdk")]),
        );
        let sdk = context.sdk.expect("SDK resolved");
        assert_eq!(sdk.path, PathBuf::from("/opt/custom-sdk"));
        assert_eq!(sdk.origin, PathOrigin::Settings);
        assert_eq!(context.conflicts.len(), 1);
        let conflict = &context.conflicts[0];
        assert_eq!(conflict.subject, "SDK root");
        assert_eq!(conflict.ignored.len(), 1);
        assert_eq!(conflict.ignored[0].0, PathOrigin::AndroidHome);
        assert!(conflict.summary().contains("/opt/custom-sdk"));
    }

    #[test]
    fn android_home_beats_deprecated_sdk_root_and_the_disagreement_is_a_warning() {
        let context = PathContext::resolve(
            Path::new("/home/example"),
            None,
            None,
            &env(&[("ANDROID_HOME", "/a"), ("ANDROID_SDK_ROOT", "/b")]),
        );
        assert_eq!(context.sdk.expect("SDK resolved").path, PathBuf::from("/a"));
        assert!(
            context
                .warnings
                .iter()
                .any(|warning| warning.contains("ANDROID_SDK_ROOT")),
            "expected an environment warning, got {:?}",
            context.warnings
        );
    }

    #[test]
    fn avd_home_precedence_follows_the_documented_order() {
        // ANDROID_AVD_HOME wins.
        let context = PathContext::resolve(
            Path::new("/home/example"),
            None,
            None,
            &env(&[
                ("ANDROID_USER_HOME", "/user"),
                ("ANDROID_EMULATOR_HOME", "/emu"),
                ("ANDROID_AVD_HOME", "/avds"),
            ]),
        );
        assert_eq!(context.avd_home, PathBuf::from("/avds"));
        assert_eq!(context.avd_home_origin, PathOrigin::AndroidAvdHome);

        // ANDROID_EMULATOR_HOME/avd is next.
        let context = PathContext::resolve(
            Path::new("/home/example"),
            None,
            None,
            &env(&[
                ("ANDROID_USER_HOME", "/user"),
                ("ANDROID_EMULATOR_HOME", "/emu"),
            ]),
        );
        assert_eq!(context.avd_home, PathBuf::from("/emu/avd"));
        assert_eq!(context.avd_home_origin, PathOrigin::AndroidEmulatorHome);

        // ANDROID_USER_HOME/avd is next.
        let context = PathContext::resolve(
            Path::new("/home/example"),
            None,
            None,
            &env(&[("ANDROID_USER_HOME", "/user")]),
        );
        assert_eq!(context.avd_home, PathBuf::from("/user/avd"));
        assert_eq!(context.avd_home_origin, PathOrigin::AndroidUserHome);
    }

    #[test]
    fn a_saved_avd_home_wins_over_android_avd_home() {
        let context = PathContext::resolve(
            Path::new("/home/example"),
            None,
            Some(Path::new("/chosen")),
            &env(&[("ANDROID_AVD_HOME", "/avds")]),
        );
        assert_eq!(context.avd_home, PathBuf::from("/chosen"));
        assert_eq!(context.avd_home_origin, PathOrigin::Settings);
        assert_eq!(context.conflicts.len(), 1);
        assert_eq!(context.conflicts[0].subject, "AVD home");
    }

    #[test]
    fn empty_environment_values_are_ignored() {
        let snapshot =
            EnvSnapshot::from_lookup(|name| (name == "ANDROID_HOME").then(|| PathBuf::from("")));
        assert_eq!(snapshot.android_home, None);
    }

    #[test]
    fn child_env_keeps_home_and_sdk_consistent() {
        let context = PathContext::resolve(
            Path::new("/home/example"),
            None,
            None,
            &env(&[("ANDROID_HOME", "/sdk"), ("ANDROID_SDK_ROOT", "/stale")]),
        );
        let child: std::collections::BTreeMap<String, PathBuf> = context
            .child_env()
            .into_iter()
            .map(|(key, value)| (key.to_string_lossy().into_owned(), PathBuf::from(value)))
            .collect();
        assert_eq!(child["ANDROID_HOME"], PathBuf::from("/sdk"));
        // The stale deprecated value must not leak through to children.
        assert_eq!(child["ANDROID_SDK_ROOT"], PathBuf::from("/sdk"));
        assert_eq!(
            child["ANDROID_AVD_HOME"],
            PathBuf::from("/home/example/.android/avd")
        );
    }

    #[test]
    fn cmdline_tools_discovers_versioned_directories_latest_first() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sdk = temp.path().join("sdk");
        for version in ["latest", "19.0", "18.0"] {
            std::fs::create_dir_all(sdk.join("cmdline-tools").join(version).join("bin"))
                .expect("create cmdline-tools layout");
        }
        // A directory without bin/ is not an installation.
        std::fs::create_dir_all(sdk.join("cmdline-tools").join("broken")).expect("create");
        std::fs::write(
            sdk.join("cmdline-tools")
                .join("19.0")
                .join("bin")
                .join("sdkmanager"),
            "",
        )
        .expect("write launcher");

        let context = PathContext::resolve(temp.path(), Some(&sdk), None, &env(&[]));
        let installs = context.cmdline_tools_installs();
        let versions: Vec<&str> = installs.iter().map(|i| i.version.as_str()).collect();
        assert_eq!(versions, vec!["latest", "19.0", "18.0"]);
        assert_eq!(
            installs[1].sdkmanager(),
            Some(
                sdk.join("cmdline-tools")
                    .join("19.0")
                    .join("bin")
                    .join("sdkmanager")
            )
        );
        assert_eq!(installs[0].avdmanager(), None);
    }

    #[test]
    fn sdk_marker_detection_accepts_partial_installs() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert!(!looks_like_sdk(temp.path()));
        std::fs::create_dir_all(temp.path().join("platform-tools")).expect("create");
        assert!(looks_like_sdk(temp.path()));
    }

    #[test]
    fn app_dirs_are_under_xdg_roots_not_the_sdk() {
        let dirs = AppDirs::from_roots("/c", "/ca", "/d", "/s");
        assert_eq!(dirs.settings_file(), PathBuf::from("/c/settings.json"));
        assert_eq!(dirs.staging_dir(), PathBuf::from("/ca/staging"));
        assert_eq!(dirs.catalog_dir(), PathBuf::from("/ca/catalog"));
        assert_eq!(dirs.logs_dir(), PathBuf::from("/s/logs"));
        assert_eq!(dirs.launches_dir(), PathBuf::from("/s/launches"));
    }
}
