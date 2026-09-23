//! AVD discovery.
//!
//! Discovery is read-only. It never writes, moves, or repairs anything, and it
//! works with no SDK installed so a device whose image is missing still shows
//! up with an explanation instead of disappearing.

use std::path::{Path, PathBuf};

use super::model::{Avd, AvdLoadContext};

/// The result of scanning an AVD home.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Discovery {
    /// Devices found, sorted by name.
    pub avds: Vec<Avd>,
    /// Problems with the AVD home itself, not with any single device.
    pub warnings: Vec<String>,
}

impl Discovery {
    /// Devices that cannot be used as-is.
    pub fn broken(&self) -> impl Iterator<Item = &Avd> {
        self.avds.iter().filter(|avd| avd.is_broken())
    }

    /// Look up a device by name.
    pub fn get(&self, name: &str) -> Option<&Avd> {
        self.avds.iter().find(|avd| avd.name == name)
    }
}

/// Reads AVDs from one directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvdRepository {
    /// The resolved AVD home.
    pub home: PathBuf,
    /// Base directory for relative `path` and `path.rel` values.
    ///
    /// This is the Android user or emulator home, which is the parent of the
    /// conventional `avd` directory. The application always supplies it from
    /// the resolved path context; the default is a convenience for callers that
    /// have nothing better.
    pub relative_base: PathBuf,
}

impl AvdRepository {
    /// Create a repository for an AVD home.
    pub fn new(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        // When the home is the conventional `<user home>/avd`, its parent is
        // the base the tools wrote their relative paths against.
        let relative_base = match home.file_name().and_then(|name| name.to_str()) {
            Some("avd") => home
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.clone()),
            _ => home.clone(),
        };
        Self {
            home,
            relative_base,
        }
    }

    /// Use an explicit base for relative paths.
    pub fn with_relative_base(mut self, base: impl Into<PathBuf>) -> Self {
        self.relative_base = base.into();
        self
    }

    /// Scan the AVD home.
    ///
    /// Only companion `<Name>.ini` files are treated as devices. A missing AVD
    /// home is not an error: it means there are no devices yet.
    pub fn discover(&self, sdk_root: Option<&Path>) -> Discovery {
        let mut warnings = Vec::new();

        let entries = match std::fs::read_dir(&self.home) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Discovery {
                    avds: Vec::new(),
                    warnings,
                };
            }
            Err(error) => {
                warnings.push(format!(
                    "The AVD home {} could not be read: {error}",
                    self.home.display()
                ));
                return Discovery {
                    avds: Vec::new(),
                    warnings,
                };
            }
        };

        let mut ini_paths: Vec<PathBuf> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("ini") {
                continue;
            }
            ini_paths.push(path);
        }
        ini_paths.sort();

        let context = AvdLoadContext::new()
            .with_sdk_root(sdk_root)
            .with_relative_base(&self.relative_base);
        let mut avds: Vec<Avd> = ini_paths
            .iter()
            .map(|path| Avd::load(path, &context))
            .collect();
        avds.sort_by(|a, b| a.name.cmp(&b.name));

        Discovery { avds, warnings }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_companion(home: &Path, name: &str) {
        std::fs::create_dir_all(home.join(format!("{name}.avd"))).expect("create data dir");
        std::fs::write(
            home.join(format!("{name}.avd")).join("config.ini"),
            "target=android-36\n",
        )
        .expect("write config");
        std::fs::write(
            home.join(format!("{name}.ini")),
            format!("path.rel=avd/{name}.avd\n"),
        )
        .expect("write companion");
    }

    #[test]
    fn a_missing_avd_home_is_not_an_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repository = AvdRepository::new(temp.path().join("does-not-exist"));
        let discovery = repository.discover(None);
        assert!(discovery.avds.is_empty());
        assert!(discovery.warnings.is_empty());
    }

    #[test]
    fn devices_are_found_and_sorted_by_name() {
        let temp = tempfile::tempdir().expect("tempdir");
        // The real layout: `<user home>/avd` holding `<Name>.ini` and
        // `<Name>.avd`, with `path.rel` written against the user home.
        let home = temp.path().join("avd");
        std::fs::create_dir_all(&home).expect("create avd home");
        write_companion(&home, "Zeta");
        write_companion(&home, "Alpha");
        write_companion(&home, "Mid");

        let discovery = AvdRepository::new(&home).discover(None);
        let names: Vec<&str> = discovery.avds.iter().map(|avd| avd.name.as_str()).collect();
        assert_eq!(names, vec!["Alpha", "Mid", "Zeta"]);
        assert_eq!(discovery.get("Mid").expect("found").name, "Mid");
        assert!(discovery.broken().next().is_none());
        assert_eq!(
            discovery.get("Mid").expect("found").data_dir,
            home.join("Mid.avd")
        );
    }

    #[test]
    fn unrelated_ini_files_are_ignored_and_data_directories_are_not_devices() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("avd");
        std::fs::create_dir_all(&home).expect("create avd home");
        write_companion(&home, "Real");
        // A data directory named like a device must not be scanned as one.
        std::fs::create_dir_all(home.join("Fake.avd")).expect("create");
        std::fs::write(home.join("notes.ini"), "unrelated=1\n").expect("write");

        let discovery = AvdRepository::new(&home).discover(None);
        let names: Vec<&str> = discovery.avds.iter().map(|avd| avd.name.as_str()).collect();
        // `notes.ini` is a companion-shaped file, so it is reported as a device
        // with a missing data directory rather than silently dropped.
        assert!(names.contains(&"Real"));
        assert!(names.contains(&"notes"));
        assert!(!names.contains(&"Fake"));
    }

    #[test]
    fn broken_devices_are_listed_with_reasons() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("avd");
        std::fs::create_dir_all(&home).expect("create avd home");
        std::fs::write(home.join("Ghost.ini"), "path.rel=avd/Ghost.avd\n").expect("write");

        let discovery = AvdRepository::new(&home).discover(None);
        assert_eq!(discovery.avds.len(), 1);
        let broken: Vec<&str> = discovery.broken().map(|avd| avd.name.as_str()).collect();
        assert_eq!(broken, vec!["Ghost"]);
    }

    #[test]
    fn an_unreadable_home_reports_a_warning_instead_of_panicking() {
        let temp = tempfile::tempdir().expect("tempdir");
        let file = temp.path().join("not-a-directory");
        std::fs::write(&file, "x").expect("write");

        let discovery = AvdRepository::new(&file).discover(None);
        assert!(discovery.avds.is_empty());
        assert_eq!(discovery.warnings.len(), 1);
    }
}
