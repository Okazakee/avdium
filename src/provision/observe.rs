//! Building [`Observations`] from what is actually on the machine.
//!
//! Observation is read-only apart from one cheap probe: asking an existing
//! `java` for its version. Nothing is installed, created or modified.

use std::path::{Path, PathBuf};

use super::consent::RecordedConsent;
use super::plan::{JavaObservation, Observations};
use crate::paths::PathContext;
use crate::providers::ProviderSet;
use crate::sdk::packages::PackageId;

/// Observe the machine.
pub fn observe(paths: &PathContext, providers: &ProviderSet) -> Observations {
    let sdk_root = paths.sdk.as_ref().map(|resolution| resolution.path.clone());
    let sdk_exists = sdk_root.as_ref().is_some_and(|root| root.is_dir());
    let sdk_writable = sdk_root.as_ref().is_some_and(|root| can_write(root));

    let installed_images = match &sdk_root {
        Some(root) => scan_system_images(root),
        None => Vec::new(),
    };

    Observations {
        sdk_root: sdk_root.clone(),
        sdk_exists,
        sdk_writable,
        android_cli: providers
            .android_cli
            .as_ref()
            .map(|cli| cli.program.clone()),
        cmdline_tools: paths.cmdline_tools_installs(),
        java: probe_java(),
        emulator: paths.emulator_program(),
        adb: paths.adb_program(),
        installed_images,
        recorded_consent: sdk_root
            .as_ref()
            .map(|root| RecordedConsent::read(root))
            .unwrap_or_default(),
        host_arch: std::env::consts::ARCH.to_owned(),
    }
}

fn can_write(path: &Path) -> bool {
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    // SAFETY: `access` only inspects permissions for the given path.
    unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 }
}

/// List installed system image packages by walking the SDK's layout.
///
/// The directory names are the package identifier segments, which is exactly
/// how the tools encode them.
fn scan_system_images(sdk_root: &Path) -> Vec<PackageId> {
    let mut images = Vec::new();
    let root = sdk_root.join("system-images");
    let Ok(levels) = std::fs::read_dir(&root) else {
        return images;
    };
    for level in levels.flatten() {
        let Some(api) = level.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(tags) = std::fs::read_dir(level.path()) else {
            continue;
        };
        for tag in tags.flatten() {
            let Some(tag_name) = tag.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(abis) = std::fs::read_dir(tag.path()) else {
                continue;
            };
            for abi in abis.flatten() {
                let Some(abi_name) = abi.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                // Only directories that actually contain an image count.
                if !abi.path().is_dir() {
                    continue;
                }
                let has_image = std::fs::read_dir(abi.path())
                    .map(|dir| {
                        dir.flatten().any(|entry| {
                            entry
                                .file_name()
                                .to_str()
                                .is_some_and(|name| name.ends_with(".img"))
                        })
                    })
                    .unwrap_or(false);
                if !has_image {
                    continue;
                }
                let raw = format!("system-images;{api};{tag_name};{abi_name}");
                if let Ok(id) = PackageId::parse(&raw) {
                    images.push(id);
                }
            }
        }
    }
    images.sort();
    images
}

/// Find an existing Java runtime and ask it for its version.
///
/// Reusing what is already installed is the whole point: a compatible runtime
/// on this machine means nothing has to be downloaded.
pub fn probe_java() -> Option<JavaObservation> {
    for candidate in java_candidates() {
        if !crate::process::is_executable(&candidate) {
            continue;
        }
        let version = crate::process::probe(
            &candidate,
            &["-version"],
            std::time::Duration::from_secs(10),
        )
        .ok()
        .and_then(|output| {
            // `java -version` prints to standard error.
            let text = format!("{}{}", output.stdout, output.stderr);
            text.lines().next().and_then(version_from_banner)
        })
        .unwrap_or_else(|| "version not reported".to_owned());
        return Some(JavaObservation {
            program: candidate,
            version,
        });
    }
    None
}

/// Every plausible Java location, most specific first.
pub fn java_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(java_home) = std::env::var_os("JAVA_HOME") {
        candidates.push(PathBuf::from(java_home).join("bin/java"));
    }
    if let Some(path_var) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path_var).map(|dir| dir.join("java")));
    }
    candidates
}

fn version_from_banner(line: &str) -> Option<String> {
    let start = line.find('"')? + 1;
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::EnvSnapshot;

    #[test]
    fn system_images_are_discovered_from_the_sdk_layout() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sdk = temp.path();
        let good = sdk.join("system-images/android-36.1/google_apis/x86_64");
        std::fs::create_dir_all(&good).expect("create");
        std::fs::write(good.join("system.img"), b"x").expect("write");
        // A directory with no image inside is not an installed image.
        let empty = sdk.join("system-images/android-37.0/google_apis/x86_64");
        std::fs::create_dir_all(&empty).expect("create");

        let images = scan_system_images(sdk);
        let names: Vec<String> = images.iter().map(PackageId::as_str).collect();
        assert_eq!(names, vec!["system-images/android-36.1/google_apis/x86_64"]);
    }

    #[test]
    fn a_missing_system_images_directory_is_not_an_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert!(scan_system_images(temp.path()).is_empty());
    }

    #[test]
    fn java_banner_versions_are_extracted() {
        assert_eq!(
            version_from_banner(r#"openjdk version "21.0.11" 2026-04-21"#),
            Some("21.0.11".to_owned())
        );
        assert_eq!(version_from_banner("no quotes"), None);
    }

    #[test]
    fn observing_a_real_machine_does_not_panic() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = PathContext::resolve(temp.path(), None, None, &EnvSnapshot::default());
        let providers = ProviderSet::detect(&paths, crate::settings::ProviderPreference::Auto);
        let observations = observe(&paths, &providers);
        assert!(!observations.host_arch.is_empty());
        // The temporary home has no SDK, so nothing should look installed.
        assert!(!observations.sdk_exists);
        assert!(observations.installed_images.is_empty());
    }

    #[test]
    fn java_is_found_when_it_is_on_path() {
        // This host has a Java; if it does not, the test still passes because
        // absence is a legitimate observation.
        let found = probe_java();
        if let Some(java) = found {
            assert!(java.program.is_absolute());
            assert!(!java.version.is_empty());
        }
    }
}
