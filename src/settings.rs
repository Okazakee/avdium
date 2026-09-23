//! Avdium's own persisted settings.
//!
//! Settings live in Avdium's XDG config directory and never contain the user's
//! SDK or AVD data. Writes are atomic: a same-directory temporary file is
//! written, flushed, and renamed over the target, so an interrupted write
//! cannot leave a half-written file behind.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Schema version for the settings document.
pub const SETTINGS_SCHEMA: u32 = 1;

/// Color theme preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    /// Follow the desktop environment.
    #[default]
    System,
    /// Force the light theme.
    Light,
    /// Force the dark theme.
    Dark,
}

impl Theme {
    /// Every variant, for the settings dropdown.
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// Label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Follow system",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

/// Which SDK provider Avdium prefers when both are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderPreference {
    /// Use Android CLI when present, otherwise classic tools.
    #[default]
    Auto,
    /// Require Android CLI.
    AndroidCli,
    /// Require the deprecated classic tools.
    CmdlineTools,
}

impl ProviderPreference {
    /// Every variant, for the settings dropdown.
    pub const ALL: [Self; 3] = [Self::Auto, Self::AndroidCli, Self::CmdlineTools];

    /// Label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Automatic",
            Self::AndroidCli => "Android CLI",
            Self::CmdlineTools => "Classic tools (sdkmanager, avdmanager)",
        }
    }
}

/// Log verbosity for Avdium's own logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LogLevel {
    /// Errors only.
    Error,
    /// Errors and warnings.
    Warn,
    /// Normal operation.
    #[default]
    Info,
    /// Verbose, including subprocess argument arrays.
    Debug,
    /// Everything.
    Trace,
}

impl LogLevel {
    /// Every variant, for the settings dropdown.
    pub const ALL: [Self; 5] = [
        Self::Error,
        Self::Warn,
        Self::Info,
        Self::Debug,
        Self::Trace,
    ];

    /// Label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "Error",
            Self::Warn => "Warning",
            Self::Info => "Info",
            Self::Debug => "Debug",
            Self::Trace => "Trace",
        }
    }

    /// Value passed to `env_logger`.
    pub fn as_filter(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

/// Persisted user settings.
///
/// Unknown keys are retained in [`Settings::extra`] so a newer Avdium version
/// does not silently destroy settings written by an older or newer one, and
/// missing keys fall back to their defaults so an older file still loads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Document schema version.
    pub schema: u32,
    /// Explicit SDK root chosen by the user.
    pub sdk_path: Option<PathBuf>,
    /// Explicit AVD home chosen by the user.
    pub avd_home: Option<PathBuf>,
    /// Theme preference.
    pub theme: Theme,
    /// Log verbosity.
    pub log_level: LogLevel,
    /// Provider preference.
    pub provider: ProviderPreference,
    /// Whether the doctor may test network reachability when the user asks.
    pub allow_network_checks: bool,
    /// Keys written by another version that Avdium does not understand yet.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: SETTINGS_SCHEMA,
            sdk_path: None,
            avd_home: None,
            theme: Theme::default(),
            log_level: LogLevel::default(),
            provider: ProviderPreference::default(),
            allow_network_checks: false,
            extra: serde_json::Map::new(),
        }
    }
}

/// Why a settings file could not be read as-is.
#[derive(Debug)]
pub enum SettingsError {
    /// The file exists but is not valid JSON, or does not match the schema.
    Corrupt {
        /// The parse failure.
        message: String,
    },
    /// The file could not be read for a reason other than absence.
    Io(std::io::Error),
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Corrupt { message } => write!(formatter, "settings are not readable: {message}"),
            Self::Io(error) => write!(formatter, "settings could not be read: {error}"),
        }
    }
}

impl std::error::Error for SettingsError {}

/// Outcome of loading settings, including anything the user should be told.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSettings {
    /// The settings in effect.
    pub settings: Settings,
    /// Problems that were recovered from, in user-facing language.
    pub warnings: Vec<String>,
    /// Where the corrupt file was preserved, when recovery happened.
    pub preserved_as: Option<PathBuf>,
}

impl Settings {
    /// Load settings, recovering from a corrupt or missing file.
    ///
    /// A missing file yields defaults with no warnings. A corrupt file is
    /// preserved next to the original so nothing the user wrote is lost, and a
    /// warning explains what happened.
    pub fn load(path: &Path) -> LoadedSettings {
        match Self::load_checked(path) {
            Ok(loaded) => loaded,
            Err(SettingsError::Io(error)) => LoadedSettings {
                settings: Self::default(),
                warnings: vec![format!("Could not read settings: {error}")],
                preserved_as: None,
            },
            Err(SettingsError::Corrupt { message }) => {
                let preserved = preserve_corrupt(path);
                let mut warnings = vec![format!(
                    "Settings were not readable and defaults were used: {message}"
                )];
                match &preserved {
                    Some(destination) => warnings.push(format!(
                        "The previous file was kept at {}.",
                        destination.display()
                    )),
                    None => warnings.push(
                        "The previous file could not be preserved. Check its permissions."
                            .to_owned(),
                    ),
                }
                LoadedSettings {
                    settings: Self::default(),
                    warnings,
                    preserved_as: preserved,
                }
            }
        }
    }

    /// Load settings, failing instead of recovering.
    pub fn load_checked(path: &Path) -> Result<LoadedSettings, SettingsError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LoadedSettings {
                    settings: Self::default(),
                    warnings: Vec::new(),
                    preserved_as: None,
                });
            }
            Err(error) => return Err(SettingsError::Io(error)),
        };

        let mut warnings = Vec::new();
        let settings: Settings =
            serde_json::from_str(&text).map_err(|error| SettingsError::Corrupt {
                message: error.to_string(),
            })?;

        if settings.schema > SETTINGS_SCHEMA {
            warnings.push(format!(
                "Settings were written by a newer Avdium (schema {}). Unknown keys are kept, \
                 but some settings may be ignored.",
                settings.schema
            ));
        }

        Ok(LoadedSettings {
            settings,
            warnings,
            preserved_as: None,
        })
    }

    /// Write settings atomically, creating the parent directory if needed.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let Some(parent) = path.parent() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "settings path has no parent directory",
            ));
        };
        std::fs::create_dir_all(parent)?;

        let mut document = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        document.push(b'\n');

        // Same-directory temporary file, so the rename stays on one filesystem.
        let temporary = path.with_extension("json.tmp");
        {
            let mut file = std::fs::File::create(&temporary)?;
            file.write_all(&document)?;
            file.flush()?;
            // The rename is atomic, but the contents must reach disk first or a
            // crash can leave an empty file in place of valid settings.
            file.sync_all()?;
        }
        std::fs::rename(&temporary, path)
    }

    /// Record an explicit SDK root.
    pub fn set_sdk_path(&mut self, path: Option<PathBuf>) {
        self.sdk_path = path;
    }

    /// Record an explicit AVD home.
    pub fn set_avd_home(&mut self, path: Option<PathBuf>) {
        self.avd_home = path;
    }
}

fn preserve_corrupt(path: &Path) -> Option<PathBuf> {
    let destination = path.with_extension("json.corrupt");
    std::fs::rename(path, &destination).ok()?;
    Some(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults_without_warnings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let loaded = Settings::load(&temp.path().join("settings.json"));
        assert_eq!(loaded.settings, Settings::default());
        assert!(loaded.warnings.is_empty());
        assert_eq!(loaded.preserved_as, None);
    }

    #[test]
    fn settings_round_trip_through_disk() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("settings.json");
        let settings = Settings {
            sdk_path: Some(PathBuf::from("/opt/sdk")),
            avd_home: Some(PathBuf::from("/opt/avds")),
            theme: Theme::Dark,
            provider: ProviderPreference::CmdlineTools,
            log_level: LogLevel::Debug,
            ..Settings::default()
        };
        settings.save(&path).expect("save");

        let loaded = Settings::load(&path);
        assert_eq!(loaded.settings, settings);
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn unknown_keys_survive_a_round_trip() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"schema":1,"theme":"dark","future_feature":{"a":[1,2,3]}}"#,
        )
        .expect("write");

        let loaded = Settings::load(&path);
        assert_eq!(loaded.settings.theme, Theme::Dark);
        assert!(loaded.settings.extra.contains_key("future_feature"));

        loaded.settings.save(&path).expect("save");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(
            text.contains("future_feature"),
            "unknown key was dropped: {text}"
        );
    }

    #[test]
    fn corrupt_settings_are_preserved_and_defaults_are_used() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("settings.json");
        std::fs::write(&path, "{ this is not json").expect("write");

        let loaded = Settings::load(&path);
        assert_eq!(loaded.settings, Settings::default());
        assert_eq!(loaded.warnings.len(), 2);
        let preserved = loaded.preserved_as.expect("corrupt file preserved");
        assert!(preserved.exists());
        assert_eq!(
            std::fs::read_to_string(&preserved).expect("read"),
            "{ this is not json"
        );
        assert!(!path.exists(), "the corrupt file must not remain in place");
    }

    #[test]
    fn a_newer_schema_is_kept_and_explained() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("settings.json");
        std::fs::write(&path, r#"{"schema":99,"theme":"light"}"#).expect("write");

        let loaded = Settings::load(&path);
        assert_eq!(loaded.settings.theme, Theme::Light);
        assert_eq!(loaded.warnings.len(), 1);
        assert!(loaded.warnings[0].contains("newer Avdium"));
    }

    #[test]
    fn saving_leaves_no_temporary_file_behind() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("nested").join("settings.json");
        Settings::default().save(&path).expect("save");
        assert!(path.exists());
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn an_interrupted_write_cannot_destroy_previous_settings() {
        // Simulate the failure mode: a leftover temporary file from a crashed
        // run must not affect loading the real file.
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("settings.json");
        let settings = Settings {
            theme: Theme::Light,
            ..Settings::default()
        };
        settings.save(&path).expect("save");
        std::fs::write(path.with_extension("json.tmp"), "garbage").expect("write leftover");

        let loaded = Settings::load(&path);
        assert_eq!(loaded.settings.theme, Theme::Light);
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn log_level_maps_to_a_filter_string() {
        assert_eq!(LogLevel::Warn.as_filter(), "warn");
        assert_eq!(LogLevel::Trace.as_filter(), "trace");
    }
}
