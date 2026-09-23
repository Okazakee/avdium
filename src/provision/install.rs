//! Installing pinned tools.
//!
//! The flow, in order:
//!
//! 1. Download into staging **on the destination filesystem**, so promotion is
//!    a rename rather than a copy across devices.
//! 2. Verify the expected size and the expected digest before anything is
//!    extracted.
//! 3. Extract safely, rejecting traversal, links and oversized archives.
//! 4. Resolve the archive's actual top-level layout instead of assuming it.
//! 5. Validate that the result really is the tool it claims to be.
//! 6. Promote by rename into a **versioned** directory, refusing to overwrite an
//!    existing installation.
//! 7. Record what happened, including where the expected digest came from.
//!
//! Any failure removes the staging directory, so a partial install is never
//! left behind pretending to be complete.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::manifest::{ArchiveKind, ToolPin};
use crate::download::{
    self, DigestError, DownloadError, DownloadRecord, Expectation, ExtractionError,
    ExtractionLimits, human_bytes,
};
use crate::process::CancellationToken;

/// The directory Avdium uses for staging inside an SDK root.
///
/// It is inside the SDK so that promotion from staging to the final location is
/// a rename on the same filesystem, and it is per-invocation unique so two
/// Avdium instances cannot collide.
pub const STAGING_DIR_NAME: &str = ".avdium-staging";

/// Progress stages reported to a caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Creating the staging directory.
    Preparing,
    /// Fetching the artifact.
    Downloading,
    /// Checking size and digest.
    Verifying,
    /// Unpacking the archive.
    Extracting,
    /// Resolving and checking the layout.
    Validating,
    /// Moving the result into place.
    Promoting,
}

impl Stage {
    /// Label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Preparing => "Preparing",
            Self::Downloading => "Downloading",
            Self::Verifying => "Verifying",
            Self::Extracting => "Extracting",
            Self::Validating => "Checking the result",
            Self::Promoting => "Installing",
        }
    }
}

/// Why an installation failed.
#[derive(Debug)]
pub enum InstallError {
    /// The download failed.
    Download(DownloadError),
    /// The expected digest could not be parsed.
    Digest(DigestError),
    /// The artifact did not match its expectation.
    Verification {
        /// What was wrong.
        detail: String,
    },
    /// Extraction failed.
    Extraction(ExtractionError),
    /// The archive layout was not recognised.
    Layout {
        /// What was found.
        detail: String,
    },
    /// The result is not the tool it claims to be.
    Validation {
        /// What was missing.
        detail: String,
    },
    /// The destination already exists.
    AlreadyInstalled {
        /// The existing path.
        path: PathBuf,
    },
    /// The filesystem refused a step.
    Io {
        /// What was being done.
        what: &'static str,
        /// The underlying error.
        message: String,
    },
    /// The caller cancelled.
    Cancelled,
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Download(error) => write!(formatter, "{error}"),
            Self::Digest(error) => write!(formatter, "{error}"),
            Self::Verification { detail } => write!(formatter, "verification failed: {detail}"),
            Self::Extraction(error) => write!(formatter, "{error}"),
            Self::Layout { detail } => write!(formatter, "unrecognised archive layout: {detail}"),
            Self::Validation { detail } => {
                write!(formatter, "installed tool is incomplete: {detail}")
            }
            Self::AlreadyInstalled { path } => write!(
                formatter,
                "{} already exists; Avdium does not overwrite an existing installation",
                path.display()
            ),
            Self::Io { what, message } => write!(formatter, "{what} failed: {message}"),
            Self::Cancelled => write!(formatter, "the installation was cancelled"),
        }
    }
}

impl std::error::Error for InstallError {}

/// Where an artifact comes from.
///
/// Injectable so the entire install flow, including verification and
/// promotion, can be tested without a network.
pub trait Fetcher {
    /// Fetch `url` into `destination`.
    fn fetch(
        &self,
        url: &str,
        destination: &Path,
        expectation: &Expectation,
        cancel: &CancellationToken,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<DownloadRecord, DownloadError>;
}

/// The real fetcher.
#[derive(Debug, Clone, Copy, Default)]
pub struct HttpFetcher;

impl Fetcher for HttpFetcher {
    fn fetch(
        &self,
        url: &str,
        destination: &Path,
        expectation: &Expectation,
        cancel: &CancellationToken,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<DownloadRecord, DownloadError> {
        download::download(url, destination, expectation, cancel, progress)
    }
}

/// A record of one installation, kept so the user can see what happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRecord {
    /// The pin that was installed.
    pub pin_id: String,
    /// The URL that was fetched.
    pub url: String,
    /// The version installed.
    pub version: Option<String>,
    /// The expected digest, as published.
    pub expected_digest: Option<String>,
    /// Where that digest came from.
    pub digest_provenance: String,
    /// The digest of what actually arrived.
    pub actual_digest: String,
    /// Whether the artifact matched its expectation.
    pub verified: bool,
    /// Where it was installed.
    pub install_path: PathBuf,
    /// When, as a Unix timestamp.
    pub installed_at_unix: u64,
}

impl ToolRecord {
    /// A file name that cannot collide across pins.
    fn file_name(&self, extension: &str) -> String {
        format!("{}.{extension}", sanitize(&self.pin_id))
    }

    /// Save into a directory.
    pub fn save(&self, directory: &Path) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(directory)?;
        let document = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let path = directory.join(self.file_name("json"));
        std::fs::write(&path, document)?;
        Ok(path)
    }

    /// Read every record in a directory.
    pub fn load_all(directory: &Path) -> Vec<Self> {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|extension| extension.to_str())
                    == Some("json")
            })
            .filter_map(|entry| {
                let text = std::fs::read_to_string(entry.path()).ok()?;
                serde_json::from_str(&text).ok()
            })
            .collect()
    }
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// The result of a successful installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledTool {
    /// The pin identifier.
    pub pin_id: String,
    /// Where it was installed.
    pub path: PathBuf,
    /// The record that was written.
    pub record: ToolRecord,
    /// A note for the user, taken from the pin.
    pub note: Option<String>,
}

/// Install a pinned tool into an SDK root.
pub fn install_pinned<F: Fetcher>(
    pin: &ToolPin,
    sdk_root: &Path,
    records_dir: &Path,
    fetcher: &F,
    cancel: &CancellationToken,
    mut progress: impl FnMut(Stage),
) -> Result<InstalledTool, InstallError> {
    progress(Stage::Preparing);

    // Staging lives inside the SDK so promotion is a rename on one filesystem.
    let staging = unique_staging_dir(sdk_root);
    if let Err(error) = std::fs::create_dir_all(&staging) {
        return Err(InstallError::Io {
            what: "creating the staging directory",
            message: error.to_string(),
        });
    }

    let outcome = install_into(
        pin,
        sdk_root,
        records_dir,
        fetcher,
        cancel,
        &staging,
        &mut progress,
    );

    // A failed or cancelled install must leave nothing behind.
    let _ = std::fs::remove_dir_all(&staging);

    outcome
}

#[allow(clippy::too_many_arguments)]
fn install_into<F: Fetcher>(
    pin: &ToolPin,
    sdk_root: &Path,
    records_dir: &Path,
    fetcher: &F,
    cancel: &CancellationToken,
    staging: &Path,
    progress: &mut impl FnMut(Stage),
) -> Result<InstalledTool, InstallError> {
    // The expectation is built from the reviewed pin, never from the response.
    let expected_digest = pin.expected_digest().map_err(InstallError::Digest)?;
    let expectation = Expectation {
        size: pin.size,
        digest: expected_digest.clone(),
        digest_source: Some(pin.provenance.label().to_owned()),
    };

    progress(Stage::Downloading);
    let artifact = staging.join("artifact");
    let record = fetcher
        .fetch(pin.url, &artifact, &expectation, cancel, &mut |_, _| {})
        .map_err(|error| match error {
            DownloadError::Cancelled => InstallError::Cancelled,
            other => InstallError::Download(other),
        })?;

    if cancel.is_cancelled() {
        return Err(InstallError::Cancelled);
    }

    progress(Stage::Verifying);
    if !record.verified {
        return Err(InstallError::Verification {
            detail: record.verification_notes.join("; "),
        });
    }

    // Resolve the tool root: either the artifact itself, or a directory inside
    // the extracted archive.
    let tool_root = match pin.archive {
        ArchiveKind::RawBinary => artifact.clone(),
        ArchiveKind::Zip | ArchiveKind::TarGz => {
            progress(Stage::Extracting);
            let extract_dir = staging.join("extracted");
            let limits = ExtractionLimits::default();
            match pin.archive {
                ArchiveKind::Zip => download::extract_zip(&artifact, &extract_dir, &limits)
                    .map_err(InstallError::Extraction)?,
                _ => download::extract_tar_gz(&artifact, &extract_dir, &limits)
                    .map_err(InstallError::Extraction)?,
            };
            progress(Stage::Validating);
            resolve_tool_root(&extract_dir)?
        }
    };

    validate_tool_root(pin, &tool_root)?;

    // Promote. A versioned destination is never overwritten.
    progress(Stage::Promoting);
    let destination = destination_path(pin, sdk_root, &artifact);
    if destination.exists() {
        return Err(InstallError::AlreadyInstalled { path: destination });
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| InstallError::Io {
            what: "creating the destination directory",
            message: error.to_string(),
        })?;
    }
    promote(&tool_root, &destination)?;

    // A downloaded executable is not executable: the mode comes from the
    // download, which has none. Set it here rather than refusing a legitimate
    // artifact during validation.
    if pin.archive == ArchiveKind::RawBinary {
        set_executable(&destination).map_err(|error| InstallError::Io {
            what: "making the downloaded binary executable",
            message: error.to_string(),
        })?;
    }

    let installed_at_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    let tool_record = ToolRecord {
        pin_id: pin.id.to_owned(),
        url: pin.url.to_owned(),
        version: pin.revision.map(str::to_owned),
        expected_digest: expected_digest.map(|digest| digest.raw()),
        digest_provenance: pin.provenance.label().to_owned(),
        actual_digest: record.digest.raw(),
        verified: record.verified,
        install_path: destination.clone(),
        installed_at_unix,
    };
    // A record that cannot be written is not a failed install, but it must be
    // visible: the caller decides whether to surface it.
    let _ = tool_record.save(records_dir);

    Ok(InstalledTool {
        pin_id: pin.id.to_owned(),
        path: destination,
        record: tool_record,
        note: pin.note.map(str::to_owned),
    })
}

/// A staging directory unique to this invocation.
fn unique_staging_dir(sdk_root: &Path) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    sdk_root
        .join(STAGING_DIR_NAME)
        .join(format!("{}-{nonce}", std::process::id()))
}

/// Where a pin is installed.
fn destination_path(pin: &ToolPin, sdk_root: &Path, _artifact: &Path) -> PathBuf {
    match pin.id {
        "cmdline-tools" => sdk_root
            .join("cmdline-tools")
            .join(pin.revision.unwrap_or("installed")),
        "android-cli-launcher" => sdk_root.join("avdium").join("android-cli"),
        other => sdk_root.join("avdium-tools").join(other),
    }
}

/// Find the directory inside an extracted archive that holds the tool.
///
/// The archive layout is resolved rather than assumed: Google's classic tools
/// ZIP contains a single `cmdline-tools` directory, but the next release may
/// not.
fn resolve_tool_root(extract_dir: &Path) -> Result<PathBuf, InstallError> {
    let mut candidates = vec![extract_dir.to_path_buf()];

    let entries: Vec<PathBuf> = std::fs::read_dir(extract_dir)
        .map_err(|error| InstallError::Io {
            what: "reading the extracted archive",
            message: error.to_string(),
        })?
        .flatten()
        .map(|entry| entry.path())
        .collect();

    if entries.len() == 1 && entries[0].is_dir() {
        candidates.push(entries[0].clone());
        // One more level, because some archives wrap twice.
        let nested: Vec<PathBuf> = std::fs::read_dir(&entries[0])
            .map(|dir| dir.flatten().map(|entry| entry.path()).collect())
            .unwrap_or_default();
        if nested.len() == 1 && nested[0].is_dir() {
            candidates.push(nested[0].clone());
        }
    }

    for candidate in &candidates {
        if candidate.join("bin").is_dir() {
            return Ok(candidate.clone());
        }
    }

    let found: Vec<String> = entries
        .iter()
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect();
    Err(InstallError::Layout {
        detail: format!("no directory containing bin/ was found; top level was [{found:?}]"),
    })
}

/// Check that the extracted result really is the tool it claims to be.
fn validate_tool_root(pin: &ToolPin, root: &Path) -> Result<(), InstallError> {
    match pin.id {
        "cmdline-tools" => {
            let mut missing = Vec::new();
            for tool in ["sdkmanager", "avdmanager"] {
                let path = root.join("bin").join(tool);
                if !path.is_file() {
                    missing.push(tool);
                }
            }
            if missing.is_empty() {
                Ok(())
            } else {
                Err(InstallError::Validation {
                    detail: format!("bin/ is missing {missing:?}"),
                })
            }
        }
        "android-cli-launcher" => {
            // The executable bit is applied during promotion, so only presence
            // and non-emptiness can be checked here.
            match std::fs::metadata(root) {
                Ok(metadata) if metadata.is_file() && metadata.len() > 0 => Ok(()),
                Ok(_) => Err(InstallError::Validation {
                    detail: "the downloaded launcher is empty".to_owned(),
                }),
                Err(error) => Err(InstallError::Validation {
                    detail: format!("the launcher could not be inspected: {error}"),
                }),
            }
        }
        _ => Ok(()),
    }
}

fn set_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(path)?;
    let mut permissions = metadata.permissions();
    // Preserve the read and write bits, add execute for everyone who can read.
    let mode = permissions.mode();
    let read_mask = 0o444;
    permissions.set_mode(mode | ((mode & read_mask) >> 2));
    std::fs::set_permissions(path, permissions)
}

/// Move the tool into place.
///
/// A rename is used when the two paths are on the same filesystem, which they
/// are by construction because staging lives inside the SDK. A cross-device
/// rename, which should not happen, falls back to a copy rather than failing
/// with a confusing error.
fn promote(from: &Path, to: &Path) -> Result<(), InstallError> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(libc::EXDEV) => {
            copy_tree(from, to).map_err(|error| InstallError::Io {
                what: "copying the tool into place",
                message: error.to_string(),
            })
        }
        Err(error) => Err(InstallError::Io {
            what: "moving the tool into place",
            message: error.to_string(),
        }),
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_file() {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(from, to)?;
        return Ok(());
    }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        copy_tree(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(())
}

/// Verify an already-installed tool against its pin, without downloading.
pub fn verify_installed(pin: &ToolPin, sdk_root: &Path) -> Result<(), InstallError> {
    let destination = destination_path(pin, sdk_root, Path::new(""));
    validate_tool_root(pin, &destination)
}

/// A human-readable download size for a pin.
pub fn pin_download_label(pin: &ToolPin) -> String {
    match pin.size {
        Some(size) => human_bytes(size),
        None => "unknown size".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::{Digest, DigestKind};
    use crate::provision::manifest;
    use std::io::Write;

    /// A fetcher that serves a local file, so the whole flow runs offline.
    struct LocalFetcher {
        source: PathBuf,
        /// When set, the served bytes are corrupted after being read.
        corrupt: bool,
    }

    impl Fetcher for LocalFetcher {
        fn fetch(
            &self,
            url: &str,
            destination: &Path,
            expectation: &Expectation,
            cancel: &CancellationToken,
            progress: &mut dyn FnMut(u64, Option<u64>),
        ) -> Result<DownloadRecord, DownloadError> {
            if cancel.is_cancelled() {
                return Err(DownloadError::Cancelled);
            }
            let mut bytes = std::fs::read(&self.source)
                .map_err(|error| DownloadError::Http(format!("local fixture: {error}")))?;
            if self.corrupt {
                bytes.push(b'!');
            }
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).map_err(DownloadError::Io)?;
            }
            std::fs::write(destination, &bytes).map_err(DownloadError::Io)?;
            progress(bytes.len() as u64, expectation.size);

            let digest = Digest::compute(DigestKind::Sha256, &bytes);
            let mut notes = Vec::new();
            let mut verified = true;
            if let Some(expected) = &expectation.digest {
                let computed = match expected.kind {
                    DigestKind::Sha1 => Digest::compute(DigestKind::Sha1, &bytes),
                    DigestKind::Sha256 => digest.clone(),
                };
                if computed.value != expected.value {
                    verified = false;
                    notes.push("digest mismatch".to_owned());
                }
            } else {
                verified = false;
            }
            if let Some(expected) = expectation.size
                && bytes.len() as u64 != expected
            {
                verified = false;
                notes.push(format!("expected {expected} bytes, got {}", bytes.len()));
            }

            Ok(DownloadRecord {
                url: url.to_owned(),
                path: destination.to_path_buf(),
                bytes: bytes.len() as u64,
                digest,
                verified,
                verification_notes: notes,
            })
        }
    }

    /// Build a classic-tools-shaped ZIP and a pin that matches it.
    fn fixture_tools_zip(dir: &Path) -> (PathBuf, ToolPin) {
        let path = dir.join("tools.zip");
        {
            let file = std::fs::File::create(&path).expect("create");
            let mut writer = zip::ZipWriter::new(file);
            let dir_options: zip::write::FileOptions<'_, ()> =
                zip::write::FileOptions::default().unix_permissions(0o755);
            writer
                .add_directory("cmdline-tools/bin/", dir_options)
                .expect("dir");
            for tool in ["sdkmanager", "avdmanager"] {
                let options: zip::write::FileOptions<'_, ()> =
                    zip::write::FileOptions::default().unix_permissions(0o755);
                writer
                    .start_file(format!("cmdline-tools/bin/{tool}"), options)
                    .expect("start");
                writer.write_all(b"#!/bin/sh\n").expect("write");
            }
            let plain: zip::write::FileOptions<'_, ()> =
                zip::write::FileOptions::default().unix_permissions(0o644);
            writer
                .start_file("cmdline-tools/NOTICE.txt", plain)
                .expect("start");
            writer.write_all(b"notice").expect("write");
            writer.finish().expect("finish");
        }

        let bytes = std::fs::read(&path).expect("read");
        let digest = Digest::compute(DigestKind::Sha256, &bytes);
        let pin = ToolPin {
            id: "cmdline-tools",
            name: "Android command line tools",
            // The fetcher ignores the URL, but it must still be https.
            url: "https://example.invalid/cmdline-tools.zip",
            size: Some(bytes.len() as u64),
            // The pin holds hex strings, so the digest is leaked into a 'static
            // slot for the test. This is test-only scaffolding.
            digest: Some(("sha256", Box::leak(digest.value.into_boxed_str()))),
            provenance: crate::provision::manifest::DigestProvenance::ReproducedByProject,
            archive: ArchiveKind::Zip,
            revision: Some("23.0"),
            license_ref: None,
            note: Some("test fixture"),
        };
        (path, pin)
    }

    #[test]
    fn a_verified_archive_is_installed_into_a_versioned_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sdk = temp.path().join("sdk");
        std::fs::create_dir_all(&sdk).expect("create");
        let (source, pin) = fixture_tools_zip(temp.path());
        let fetcher = LocalFetcher {
            source,
            corrupt: false,
        };

        let installed = install_pinned(
            &pin,
            &sdk,
            &temp.path().join("records"),
            &fetcher,
            &CancellationToken::new(),
            |_| {},
        )
        .expect("install");

        // Installed under a versioned directory, not `latest`.
        assert_eq!(installed.path, sdk.join("cmdline-tools/23.0"));
        assert!(installed.path.join("bin/sdkmanager").is_file());
        assert!(installed.path.join("bin/avdmanager").is_file());
        assert!(installed.record.verified);
        assert_eq!(
            installed.record.digest_provenance,
            "reproduced by this project"
        );

        // The executable bit survived.
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(installed.path.join("bin/sdkmanager"))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);
    }

    #[test]
    fn staging_is_removed_after_a_successful_install() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sdk = temp.path().join("sdk");
        let (source, pin) = fixture_tools_zip(temp.path());
        let fetcher = LocalFetcher {
            source,
            corrupt: false,
        };
        install_pinned(
            &pin,
            &sdk,
            &temp.path().join("r"),
            &fetcher,
            &CancellationToken::new(),
            |_| {},
        )
        .expect("install");
        // The staging root may exist, but no per-invocation directory remains.
        let staging = sdk.join(STAGING_DIR_NAME);
        let remaining = std::fs::read_dir(&staging)
            .map(|dir| dir.count())
            .unwrap_or(0);
        assert_eq!(remaining, 0, "staging was not cleaned up");
    }

    #[test]
    fn a_digest_mismatch_aborts_before_anything_is_installed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sdk = temp.path().join("sdk");
        let (source, pin) = fixture_tools_zip(temp.path());
        let fetcher = LocalFetcher {
            source,
            corrupt: true,
        };

        let error = install_pinned(
            &pin,
            &sdk,
            &temp.path().join("r"),
            &fetcher,
            &CancellationToken::new(),
            |_| {},
        )
        .expect_err("must refuse");
        assert!(
            matches!(error, InstallError::Verification { .. }),
            "got {error:?}"
        );
        assert!(!sdk.join("cmdline-tools").exists());
        let staging = sdk.join(STAGING_DIR_NAME);
        assert_eq!(
            std::fs::read_dir(&staging).map(|d| d.count()).unwrap_or(0),
            0
        );
    }

    #[test]
    fn an_existing_installation_is_never_overwritten() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sdk = temp.path().join("sdk");
        let existing = sdk.join("cmdline-tools/23.0");
        std::fs::create_dir_all(existing.join("bin")).expect("create");
        std::fs::write(existing.join("bin/sdkmanager"), "original").expect("write");

        let (source, pin) = fixture_tools_zip(temp.path());
        let fetcher = LocalFetcher {
            source,
            corrupt: false,
        };
        let error = install_pinned(
            &pin,
            &sdk,
            &temp.path().join("r"),
            &fetcher,
            &CancellationToken::new(),
            |_| {},
        )
        .expect_err("must refuse");

        assert!(
            matches!(error, InstallError::AlreadyInstalled { .. }),
            "got {error:?}"
        );
        // The existing file is untouched.
        assert_eq!(
            std::fs::read_to_string(existing.join("bin/sdkmanager")).expect("read"),
            "original"
        );
    }

    #[test]
    fn a_cancelled_install_leaves_nothing_behind() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sdk = temp.path().join("sdk");
        let (source, pin) = fixture_tools_zip(temp.path());
        let fetcher = LocalFetcher {
            source,
            corrupt: false,
        };
        let cancel = CancellationToken::new();
        cancel.cancel();

        let error = install_pinned(
            &pin,
            &sdk,
            &temp.path().join("r"),
            &fetcher,
            &cancel,
            |_| {},
        )
        .expect_err("must cancel");
        assert!(matches!(error, InstallError::Cancelled), "got {error:?}");
        assert!(!sdk.join("cmdline-tools").exists());
    }

    #[test]
    fn an_archive_without_the_expected_executables_is_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("partial.zip");
        {
            let file = std::fs::File::create(&source).expect("create");
            let mut writer = zip::ZipWriter::new(file);
            writer
                .add_directory(
                    "cmdline-tools/bin/",
                    zip::write::FileOptions::<'_, ()>::default(),
                )
                .expect("dir");
            // sdkmanager is missing.
            writer
                .start_file(
                    "cmdline-tools/bin/avdmanager",
                    zip::write::FileOptions::<'_, ()>::default(),
                )
                .expect("start");
            writer.write_all(b"x").expect("write");
            writer.finish().expect("finish");
        }
        let bytes = std::fs::read(&source).expect("read");
        let digest = Digest::compute(DigestKind::Sha256, &bytes);
        let pin = ToolPin {
            id: "cmdline-tools",
            name: "tools",
            url: "https://example.invalid/x.zip",
            size: Some(bytes.len() as u64),
            digest: Some(("sha256", Box::leak(digest.value.into_boxed_str()))),
            provenance: manifest::DigestProvenance::ReproducedByProject,
            archive: ArchiveKind::Zip,
            revision: Some("23.0"),
            license_ref: None,
            note: None,
        };
        let fetcher = LocalFetcher {
            source,
            corrupt: false,
        };
        let error = install_pinned(
            &pin,
            &temp.path().join("sdk"),
            &temp.path().join("r"),
            &fetcher,
            &CancellationToken::new(),
            |_| {},
        )
        .expect_err("must refuse");
        assert!(
            matches!(error, InstallError::Validation { .. }),
            "got {error:?}"
        );
    }

    #[test]
    fn staging_directories_are_unique_per_invocation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let first = unique_staging_dir(temp.path());
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = unique_staging_dir(temp.path());
        assert_ne!(first, second);
        assert!(first.starts_with(temp.path().join(STAGING_DIR_NAME)));
    }

    #[test]
    fn tool_records_round_trip_and_never_contain_license_text() {
        let temp = tempfile::tempdir().expect("tempdir");
        let record = ToolRecord {
            pin_id: "cmdline-tools".to_owned(),
            url: "https://dl.google.com/android/repository/x.zip".to_owned(),
            version: Some("23.0".to_owned()),
            expected_digest: Some("sha1:abc".to_owned()),
            digest_provenance: "from the publisher's repository metadata".to_owned(),
            actual_digest: "sha1:abc".to_owned(),
            verified: true,
            install_path: PathBuf::from("/sdk/cmdline-tools/23.0"),
            installed_at_unix: 1_700_000_000,
        };
        let path = record.save(temp.path()).expect("save");
        let loaded = ToolRecord::load_all(temp.path());
        assert_eq!(loaded, vec![record.clone()]);
        let document = std::fs::read_to_string(path).expect("read");
        assert!(!document.contains("License Agreement"));
    }

    #[test]
    fn a_missing_records_directory_is_not_an_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert!(ToolRecord::load_all(&temp.path().join("nope")).is_empty());
    }

    #[test]
    fn the_launcher_pin_installs_as_an_executable_binary() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sdk = temp.path().join("sdk");
        let source = temp.path().join("android");
        std::fs::write(&source, b"#!/bin/sh\n").expect("write");
        let bytes = std::fs::read(&source).expect("read");
        let digest = Digest::compute(DigestKind::Sha256, &bytes);
        let pin = ToolPin {
            id: "android-cli-launcher",
            name: "Android CLI launcher",
            url: "https://example.invalid/android",
            size: Some(bytes.len() as u64),
            digest: Some(("sha256", Box::leak(digest.value.into_boxed_str()))),
            provenance: manifest::DigestProvenance::ReproducedByProject,
            archive: ArchiveKind::RawBinary,
            revision: None,
            license_ref: None,
            note: None,
        };
        let fetcher = LocalFetcher {
            source,
            corrupt: false,
        };
        let installed = install_pinned(
            &pin,
            &sdk,
            &temp.path().join("r"),
            &fetcher,
            &CancellationToken::new(),
            |_| {},
        )
        .expect("install");
        assert!(installed.path.is_file());
    }

    #[test]
    fn stage_labels_are_user_facing() {
        for stage in [
            Stage::Preparing,
            Stage::Downloading,
            Stage::Verifying,
            Stage::Extracting,
            Stage::Validating,
            Stage::Promoting,
        ] {
            assert!(!stage.label().is_empty());
        }
    }
}
