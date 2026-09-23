//! Downloads, integrity checks and safe archive extraction.
//!
//! Two things this module refuses to do:
//!
//! - **Trust a download because it arrived.** A digest computed from a fresh
//!   download records identity, it does not authenticate it. The expected
//!   digest and where it came from are recorded separately from what was
//!   actually computed, so a caller can tell the difference.
//! - **Extract an archive blindly.** Absolute paths, `..` components, symlinks
//!   and hardlinks are rejected, and the total extracted size is bounded.

use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use sha1::Sha1;
use sha2::{Digest as _, Sha256};

use crate::process::CancellationToken;

/// Which digest algorithm a value uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestKind {
    /// SHA-1. Google publishes these, but SHA-1 is weak.
    Sha1,
    /// SHA-256.
    Sha256,
}

impl DigestKind {
    /// Parse an algorithm name as the repository writes it.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().replace('-', "").as_str() {
            "sha1" => Some(Self::Sha1),
            "sha256" => Some(Self::Sha256),
            _ => None,
        }
    }

    /// Canonical name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
        }
    }

    /// Whether this algorithm is considered strong enough to pin against.
    pub fn is_strong(self) -> bool {
        matches!(self, Self::Sha256)
    }
}

/// A digest value together with its algorithm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digest {
    /// The algorithm.
    pub kind: DigestKind,
    /// Lowercase hex digest.
    pub value: String,
}

impl Digest {
    /// Build a digest from a hexadecimal string.
    pub fn parse(kind: DigestKind, value: &str) -> Result<Self, DigestError> {
        let value = value.trim().to_ascii_lowercase();
        let expected_length = match kind {
            DigestKind::Sha1 => 40,
            DigestKind::Sha256 => 64,
        };
        if value.len() != expected_length || !value.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(DigestError::Malformed {
                kind: kind.as_str(),
                value,
                expected_length,
            });
        }
        Ok(Self { kind, value })
    }

    /// Compute the digest of some bytes.
    pub fn compute(kind: DigestKind, bytes: &[u8]) -> Self {
        match kind {
            DigestKind::Sha1 => Self {
                kind,
                value: hex(&Sha1::digest(bytes)),
            },
            DigestKind::Sha256 => Self {
                kind,
                value: hex(&Sha256::digest(bytes)),
            },
        }
    }

    /// Whether these bytes match.
    pub fn matches(&self, bytes: &[u8]) -> bool {
        Self::compute(self.kind, bytes).value == self.value
    }

    /// A short form for logs.
    pub fn short(&self) -> String {
        format!(
            "{}:{}",
            self.kind.as_str(),
            &self.value[..self.value.len().min(16)]
        )
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

/// Why a digest could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DigestError {
    /// The value was not a valid digest for its algorithm.
    Malformed {
        /// The algorithm name.
        kind: &'static str,
        /// The offending value.
        value: String,
        /// How long the value should have been.
        expected_length: usize,
    },
}

impl std::fmt::Display for DigestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed {
                kind,
                value,
                expected_length,
            } => write!(
                formatter,
                "`{value}` is not a {kind} digest ({expected_length} hex characters expected)"
            ),
        }
    }
}

impl std::error::Error for DigestError {}

/// What a caller expects a download to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expectation {
    /// Expected size in bytes, when the source declares one.
    pub size: Option<u64>,
    /// Expected digest.
    pub digest: Option<Digest>,
    /// Where the expected digest came from, for example
    /// `repository2-3.xml` or `reviewed project manifest`.
    ///
    /// Recorded because a digest without provenance is not evidence.
    pub digest_source: Option<String>,
}

impl Expectation {
    /// No expectations beyond the URL itself.
    pub fn none() -> Self {
        Self {
            size: None,
            digest: None,
            digest_source: None,
        }
    }
}

/// What actually arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRecord {
    /// The URL that was fetched.
    pub url: String,
    /// Where the bytes were written.
    pub path: PathBuf,
    /// How many bytes arrived.
    pub bytes: u64,
    /// The digest computed from what arrived.
    pub digest: Digest,
    /// Whether the result matched every supplied expectation.
    pub verified: bool,
    /// Problems found while verifying, in user-facing language.
    pub verification_notes: Vec<String>,
}

impl DownloadRecord {
    /// A one-line summary for the log.
    pub fn summary(&self) -> String {
        format!(
            "{} bytes from {} ({}{})",
            self.bytes,
            self.url,
            self.digest.short(),
            if self.verified {
                ", verified"
            } else {
                ", not verified"
            }
        )
    }
}

/// Why a download failed.
#[derive(Debug)]
pub enum DownloadError {
    /// The request could not be made or the response was an error status.
    Http(String),
    /// The destination could not be written.
    Io(io::Error),
    /// The download was cancelled.
    Cancelled,
    /// The body was larger than the declared size.
    TooLarge {
        /// Bytes received.
        received: u64,
        /// Bytes expected.
        expected: u64,
    },
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(message) => write!(formatter, "download failed: {message}"),
            Self::Io(error) => write!(formatter, "download could not be written: {error}"),
            Self::Cancelled => write!(formatter, "download was cancelled"),
            Self::TooLarge { received, expected } => write!(
                formatter,
                "download sent {received} bytes but only {expected} were expected"
            ),
        }
    }
}

impl std::error::Error for DownloadError {}

/// How often the progress callback is invoked, in bytes.
const PROGRESS_CHUNK: u64 = 256 * 1024;

/// Download a URL to a file, reporting progress and honoring cancellation.
///
/// The file is written directly at `destination`, which the caller must place
/// in a private staging directory.
pub fn download(
    url: &str,
    destination: &Path,
    expectation: &Expectation,
    cancel: &CancellationToken,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<DownloadRecord, DownloadError> {
    if cancel.is_cancelled() {
        return Err(DownloadError::Cancelled);
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("avdium/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| DownloadError::Http(error.to_string()))?;

    let mut response = client
        .get(url)
        .send()
        .map_err(|error| DownloadError::Http(error.to_string()))?;

    if !response.status().is_success() {
        return Err(DownloadError::Http(format!(
            "server returned {}",
            response.status()
        )));
    }

    let declared = response.content_length();
    if let (Some(declared), Some(expected)) = (declared, expectation.size)
        && declared > expected
    {
        return Err(DownloadError::TooLarge {
            received: declared,
            expected,
        });
    }

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(DownloadError::Io)?;
    }
    let mut file = std::fs::File::create(destination).map_err(DownloadError::Io)?;

    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut received: u64 = 0;
    let mut last_report: u64 = 0;

    loop {
        if cancel.is_cancelled() {
            drop(file);
            let _ = std::fs::remove_file(destination);
            return Err(DownloadError::Cancelled);
        }
        let read = match response.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(DownloadError::Http(error.to_string())),
        };
        let chunk = &buffer[..read];
        file.write_all(chunk).map_err(DownloadError::Io)?;
        sha1.update(chunk);
        sha256.update(chunk);
        received += read as u64;

        if let Some(expected) = expectation.size
            && received > expected
        {
            return Err(DownloadError::TooLarge { received, expected });
        }

        if received - last_report >= PROGRESS_CHUNK {
            last_report = received;
            progress(received, expectation.size.or(declared));
        }
    }

    file.flush().map_err(DownloadError::Io)?;
    // The bytes must be on disk before the caller promotes the file.
    file.sync_all().map_err(DownloadError::Io)?;
    drop(file);
    progress(received, expectation.size.or(declared));

    let computed_sha1 = Digest {
        kind: DigestKind::Sha1,
        value: hex(&sha1.finalize()),
    };
    let computed_sha256 = Digest {
        kind: DigestKind::Sha256,
        value: hex(&sha256.finalize()),
    };

    let mut verification_notes = Vec::new();
    let mut verified = true;

    if let Some(expected) = expectation.size
        && received != expected
    {
        verified = false;
        verification_notes.push(format!("expected {expected} bytes but received {received}"));
    }

    let digest = match &expectation.digest {
        Some(expected) => {
            let computed = match expected.kind {
                DigestKind::Sha1 => &computed_sha1,
                DigestKind::Sha256 => &computed_sha256,
            };
            if computed.value != expected.value {
                verified = false;
                verification_notes.push(format!(
                    "{} digest mismatch: expected {}, computed {}",
                    expected.kind.as_str(),
                    expected.value,
                    computed.value
                ));
            }
            computed.clone()
        }
        None => {
            verified = false;
            verification_notes.push(
                "no expected digest was supplied, so the download is recorded but not \
                 authenticated"
                    .to_owned(),
            );
            computed_sha256
        }
    };

    Ok(DownloadRecord {
        url: url.to_owned(),
        path: destination.to_path_buf(),
        bytes: received,
        digest,
        verified,
        verification_notes,
    })
}

/// Limits applied while extracting an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionLimits {
    /// Largest total uncompressed size accepted.
    pub max_total_bytes: u64,
    /// Largest number of entries accepted.
    pub max_entries: usize,
}

impl Default for ExtractionLimits {
    fn default() -> Self {
        Self {
            max_total_bytes: 4 * 1024 * 1024 * 1024,
            max_entries: 20_000,
        }
    }
}

/// What was extracted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExtractionReport {
    /// Files written.
    pub files: usize,
    /// Directories created.
    pub directories: usize,
    /// Total uncompressed bytes written.
    pub bytes: u64,
    /// The top-level entry names, so callers can resolve archive layout rather
    /// than assuming it.
    pub top_level: Vec<String>,
}

/// Why extraction failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractionError {
    /// The archive could not be read.
    Read(String),
    /// An entry tried to escape the destination.
    UnsafePath {
        /// The offending entry name.
        entry: String,
    },
    /// A symlink or hardlink was present.
    LinkNotAllowed {
        /// The offending entry name.
        entry: String,
    },
    /// The archive exceeded a limit.
    LimitExceeded {
        /// Which limit.
        limit: &'static str,
    },
    /// The destination could not be written.
    Write(String),
}

impl std::fmt::Display for ExtractionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(message) => write!(formatter, "archive could not be read: {message}"),
            Self::UnsafePath { entry } => {
                write!(
                    formatter,
                    "archive entry `{entry}` points outside the destination"
                )
            }
            Self::LinkNotAllowed { entry } => {
                write!(
                    formatter,
                    "archive entry `{entry}` is a link, which is not extracted"
                )
            }
            Self::LimitExceeded { limit } => {
                write!(formatter, "archive exceeded the {limit} limit")
            }
            Self::Write(message) => write!(formatter, "archive could not be written: {message}"),
        }
    }
}

impl std::error::Error for ExtractionError {}

/// Validate an archive entry path.
///
/// Rejects absolute paths, drive-like prefixes and any `..` component. The
/// returned path is relative to the destination.
pub fn safe_relative_path(entry: &str) -> Result<PathBuf, ExtractionError> {
    let normalized = entry.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err(ExtractionError::UnsafePath {
            entry: entry.to_owned(),
        });
    }
    let mut result = PathBuf::new();
    for component in Path::new(&normalized).components() {
        match component {
            Component::Normal(part) => result.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ExtractionError::UnsafePath {
                    entry: entry.to_owned(),
                });
            }
        }
    }
    if result.as_os_str().is_empty() {
        return Err(ExtractionError::UnsafePath {
            entry: entry.to_owned(),
        });
    }
    Ok(result)
}

/// Extract a ZIP archive into a destination directory.
pub fn extract_zip(
    archive_path: &Path,
    destination: &Path,
    limits: &ExtractionLimits,
) -> Result<ExtractionReport, ExtractionError> {
    let file = std::fs::File::open(archive_path)
        .map_err(|error| ExtractionError::Read(error.to_string()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| ExtractionError::Read(error.to_string()))?;

    if archive.len() > limits.max_entries {
        return Err(ExtractionError::LimitExceeded {
            limit: "entry count",
        });
    }

    let mut report = ExtractionReport::default();
    let mut top_level: Vec<String> = Vec::new();

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| ExtractionError::Read(error.to_string()))?;
        let raw_name = entry.name().to_owned();
        let relative = safe_relative_path(&raw_name)?;

        // `unix_mode` carries the executable bit for tool archives.
        let mode = entry.unix_mode();
        let is_directory = entry.is_dir();

        if let Some(first) = relative.components().next()
            && let Component::Normal(name) = first
        {
            let name = name.to_string_lossy().into_owned();
            if !top_level.contains(&name) {
                top_level.push(name);
            }
        }

        let target = destination.join(&relative);
        if is_directory {
            std::fs::create_dir_all(&target)
                .map_err(|error| ExtractionError::Write(error.to_string()))?;
            report.directories += 1;
            continue;
        }

        report.bytes = report.bytes.saturating_add(entry.size());
        if report.bytes > limits.max_total_bytes {
            return Err(ExtractionError::LimitExceeded {
                limit: "uncompressed size",
            });
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| ExtractionError::Write(error.to_string()))?;
        }
        let mut output = std::fs::File::create(&target)
            .map_err(|error| ExtractionError::Write(error.to_string()))?;
        io::copy(&mut entry, &mut output)
            .map_err(|error| ExtractionError::Write(error.to_string()))?;
        output
            .flush()
            .map_err(|error| ExtractionError::Write(error.to_string()))?;
        drop(output);

        if let Some(mode) = mode {
            set_mode(&target, mode);
        }
        report.files += 1;
    }

    report.top_level = top_level;
    Ok(report)
}

/// Extract a `.tar.gz` archive into a destination directory.
pub fn extract_tar_gz(
    archive_path: &Path,
    destination: &Path,
    limits: &ExtractionLimits,
) -> Result<ExtractionReport, ExtractionError> {
    let file = std::fs::File::open(archive_path)
        .map_err(|error| ExtractionError::Read(error.to_string()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    let mut report = ExtractionReport::default();
    let mut top_level: Vec<String> = Vec::new();

    let entries = archive
        .entries()
        .map_err(|error| ExtractionError::Read(error.to_string()))?;
    for entry in entries {
        let mut entry = entry.map_err(|error| ExtractionError::Read(error.to_string()))?;
        if report.files + report.directories > limits.max_entries {
            return Err(ExtractionError::LimitExceeded {
                limit: "entry count",
            });
        }
        let header = entry.header().clone();
        let entry_type = header.entry_type();
        let raw_name = entry
            .path()
            .map_err(|error| ExtractionError::Read(error.to_string()))?
            .to_string_lossy()
            .into_owned();
        let relative = safe_relative_path(&raw_name)?;

        if entry_type.is_symlink() || entry_type.is_hard_link() {
            return Err(ExtractionError::LinkNotAllowed { entry: raw_name });
        }

        if let Some(first) = relative.components().next()
            && let Component::Normal(name) = first
        {
            let name = name.to_string_lossy().into_owned();
            if !top_level.contains(&name) {
                top_level.push(name);
            }
        }

        let target = destination.join(&relative);
        if entry_type.is_dir() {
            std::fs::create_dir_all(&target)
                .map_err(|error| ExtractionError::Write(error.to_string()))?;
            report.directories += 1;
            continue;
        }
        if !entry_type.is_file() {
            // Devices, fifos and other special entries are not extracted.
            continue;
        }

        let size = header.size().unwrap_or(0);
        report.bytes = report.bytes.saturating_add(size);
        if report.bytes > limits.max_total_bytes {
            return Err(ExtractionError::LimitExceeded {
                limit: "uncompressed size",
            });
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| ExtractionError::Write(error.to_string()))?;
        }
        let mut output = std::fs::File::create(&target)
            .map_err(|error| ExtractionError::Write(error.to_string()))?;
        io::copy(&mut entry, &mut output)
            .map_err(|error| ExtractionError::Write(error.to_string()))?;
        output
            .flush()
            .map_err(|error| ExtractionError::Write(error.to_string()))?;
        drop(output);

        if let Ok(mode) = header.mode() {
            set_mode(&target, mode);
        }
        report.files += 1;
    }

    report.top_level = top_level;
    Ok(report)
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    // Only the permission bits are meaningful here.
    let permissions = std::fs::Permissions::from_mode(mode & 0o7777);
    let _ = std::fs::set_permissions(path, permissions);
}

/// Render a byte count for humans.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [(&str, u64); 4] = [
        ("GiB", 1024 * 1024 * 1024),
        ("MiB", 1024 * 1024),
        ("KiB", 1024),
        ("B", 1),
    ];
    for (unit, size) in UNITS {
        if bytes >= size {
            let value = bytes as f64 / size as f64;
            return if unit == "B" {
                format!("{bytes} B")
            } else {
                format!("{value:.1} {unit}")
            };
        }
    }
    "0 B".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests_are_computed_and_checked() {
        let bytes = b"hello";
        let sha1 = Digest::compute(DigestKind::Sha1, bytes);
        assert_eq!(sha1.value, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
        assert!(sha1.matches(bytes));
        assert!(!sha1.matches(b"hello!"));

        let sha256 = Digest::compute(DigestKind::Sha256, bytes);
        assert_eq!(
            sha256.value,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn digest_parsing_validates_length_and_alphabet() {
        let ok = Digest::parse(DigestKind::Sha1, "AAF4C61DDCC5E8A2DABEDE0F3B482CD9AEA9434D")
            .expect("uppercase is accepted and normalized");
        assert_eq!(ok.value, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");

        assert!(Digest::parse(DigestKind::Sha1, "abc").is_err());
        assert!(Digest::parse(DigestKind::Sha256, &"a".repeat(40)).is_err());
        assert!(Digest::parse(DigestKind::Sha1, &"z".repeat(40)).is_err());
    }

    #[test]
    fn sha1_is_recorded_but_not_treated_as_strong() {
        assert!(!DigestKind::Sha1.is_strong());
        assert!(DigestKind::Sha256.is_strong());
        assert_eq!(DigestKind::parse("SHA-1"), Some(DigestKind::Sha1));
        assert_eq!(DigestKind::parse("sha256"), Some(DigestKind::Sha256));
        assert_eq!(DigestKind::parse("md5"), None);
    }

    #[test]
    fn safe_paths_are_accepted_and_escapes_are_rejected() {
        assert_eq!(
            safe_relative_path("cmdline-tools/bin/sdkmanager").expect("safe"),
            PathBuf::from("cmdline-tools/bin/sdkmanager")
        );
        assert_eq!(
            safe_relative_path("./a/b").expect("safe"),
            PathBuf::from("a/b")
        );
        for unsafe_entry in [
            "/etc/passwd",
            "../../etc/passwd",
            "a/../../b",
            "..",
            "a\\..\\..\\b",
            "",
            ".",
        ] {
            assert!(
                safe_relative_path(unsafe_entry).is_err(),
                "{unsafe_entry} should be rejected"
            );
        }
    }

    #[test]
    fn a_zip_with_traversal_is_rejected_and_nothing_escapes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let archive_path = temp.path().join("evil.zip");
        {
            let file = std::fs::File::create(&archive_path).expect("create");
            let mut writer = zip::ZipWriter::new(file);
            let options: zip::write::FileOptions<'_, ()> =
                zip::write::FileOptions::default().unix_permissions(0o644);
            writer.start_file("../escaped.txt", options).expect("start");
            writer.write_all(b"should not be written").expect("write");
            writer.finish().expect("finish");
        }

        let destination = temp.path().join("out");
        let error = extract_zip(&archive_path, &destination, &ExtractionLimits::default())
            .expect_err("traversal must be rejected");
        assert!(matches!(error, ExtractionError::UnsafePath { .. }));
        assert!(!temp.path().join("escaped.txt").exists());
        assert!(
            !destination.exists()
                || std::fs::read_dir(&destination)
                    .expect("read")
                    .next()
                    .is_none()
        );
    }

    #[test]
    fn a_zip_extracts_files_directories_and_the_executable_bit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let archive_path = temp.path().join("tools.zip");
        {
            let file = std::fs::File::create(&archive_path).expect("create");
            let mut writer = zip::ZipWriter::new(file);
            let dir_options: zip::write::FileOptions<'_, ()> =
                zip::write::FileOptions::default().unix_permissions(0o755);
            writer
                .add_directory("cmdline-tools/bin/", dir_options)
                .expect("dir");
            let file_options: zip::write::FileOptions<'_, ()> =
                zip::write::FileOptions::default().unix_permissions(0o755);
            writer
                .start_file("cmdline-tools/bin/sdkmanager", file_options)
                .expect("start");
            writer.write_all(b"#!/bin/sh\n").expect("write");
            let plain: zip::write::FileOptions<'_, ()> =
                zip::write::FileOptions::default().unix_permissions(0o644);
            writer
                .start_file("cmdline-tools/NOTICE.txt", plain)
                .expect("start");
            writer.write_all(b"notice").expect("write");
            writer.finish().expect("finish");
        }

        let destination = temp.path().join("out");
        let report = extract_zip(&archive_path, &destination, &ExtractionLimits::default())
            .expect("extract");
        assert_eq!(report.files, 2);
        assert_eq!(report.directories, 1);
        assert_eq!(report.top_level, vec!["cmdline-tools".to_owned()]);

        use std::os::unix::fs::PermissionsExt;
        let launcher = destination.join("cmdline-tools/bin/sdkmanager");
        let mode = std::fs::metadata(&launcher)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "the executable bit must survive");
        let notice = destination.join("cmdline-tools/NOTICE.txt");
        let mode = std::fs::metadata(&notice)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0, "plain files must not become executable");
    }

    #[test]
    fn an_oversized_archive_is_rejected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let archive_path = temp.path().join("big.zip");
        {
            let file = std::fs::File::create(&archive_path).expect("create");
            let mut writer = zip::ZipWriter::new(file);
            let options: zip::write::FileOptions<'_, ()> =
                zip::write::FileOptions::default().unix_permissions(0o644);
            writer.start_file("big.bin", options).expect("start");
            writer.write_all(&vec![0u8; 4096]).expect("write");
            writer.finish().expect("finish");
        }
        let limits = ExtractionLimits {
            max_total_bytes: 1024,
            max_entries: 100,
        };
        let error =
            extract_zip(&archive_path, &temp.path().join("out"), &limits).expect_err("size limit");
        assert_eq!(
            error,
            ExtractionError::LimitExceeded {
                limit: "uncompressed size"
            }
        );
    }

    #[test]
    fn tar_gz_extraction_rejects_symlinks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let archive_path = temp.path().join("links.tar.gz");
        {
            let file = std::fs::File::create(&archive_path).expect("create");
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
            let mut builder = tar::Builder::new(encoder);

            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            header.set_link_name("/etc/passwd").expect("link name");
            header.set_cksum();
            builder
                .append_data(&mut header, "escape", io::empty())
                .expect("append");

            let encoder = builder.into_inner().expect("into_inner");
            encoder.finish().expect("finish");
        }

        let error = extract_tar_gz(
            &archive_path,
            &temp.path().join("out"),
            &ExtractionLimits::default(),
        )
        .expect_err("symlink must be rejected");
        assert!(matches!(error, ExtractionError::LinkNotAllowed { .. }));
    }

    #[test]
    fn tar_gz_extraction_writes_files_and_preserves_modes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let archive_path = temp.path().join("jre.tar.gz");
        {
            let file = std::fs::File::create(&archive_path).expect("create");
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
            let mut builder = tar::Builder::new(encoder);

            let payload = b"#!/bin/sh\necho java\n";
            let mut header = tar::Header::new_gnu();
            header.set_size(payload.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "jre/bin/java", &payload[..])
                .expect("append");

            let encoder = builder.into_inner().expect("into_inner");
            encoder.finish().expect("finish");
        }

        let destination = temp.path().join("out");
        let report = extract_tar_gz(&archive_path, &destination, &ExtractionLimits::default())
            .expect("extract");
        assert_eq!(report.files, 1);
        assert_eq!(report.top_level, vec!["jre".to_owned()]);

        use std::os::unix::fs::PermissionsExt;
        let java = destination.join("jre/bin/java");
        let mode = std::fs::metadata(&java)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);
    }

    #[test]
    fn a_missing_archive_is_a_read_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        let error = extract_zip(
            &temp.path().join("nope.zip"),
            &temp.path().join("out"),
            &ExtractionLimits::default(),
        )
        .expect_err("missing");
        assert!(matches!(error, ExtractionError::Read(_)));
    }

    #[test]
    fn byte_counts_render_for_humans() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KiB");
        assert_eq!(human_bytes(353_795_619), "337.4 MiB");
        assert_eq!(human_bytes(5 * 1024 * 1024 * 1024), "5.0 GiB");
    }

    #[test]
    fn an_expectation_without_provenance_is_reported_as_unverified() {
        // A download that cannot be verified must say so rather than implying
        // success.
        let expectation = Expectation::none();
        assert!(expectation.digest.is_none());
        assert!(expectation.digest_source.is_none());
    }

    #[test]
    fn download_refuses_an_already_cancelled_request() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cancel = CancellationToken::new();
        cancel.cancel();
        let error = download(
            "https://example.invalid/x",
            &temp.path().join("x"),
            &Expectation::none(),
            &cancel,
            |_, _| {},
        )
        .expect_err("cancelled");
        assert!(matches!(error, DownloadError::Cancelled));
        assert!(!temp.path().join("x").exists());
    }

    /// Opt-in: performs a real network request.
    ///
    /// Run with `cargo test -- --ignored real_download`.
    #[test]
    #[ignore = "performs a real network request"]
    fn real_download_records_identity_and_size() {
        let temp = tempfile::tempdir().expect("tempdir");
        let destination = temp.path().join("android");
        let expectation = Expectation::none();
        let record = download(
            "https://dl.google.com/android/cli/1.0.16406183/linux_x86_64/android",
            &destination,
            &expectation,
            &CancellationToken::new(),
            |_, _| {},
        )
        .expect("download");

        // Recorded from the reviewed research; if this fails, the pinned input
        // changed and the pin must be re-verified rather than adjusted.
        assert_eq!(record.bytes, 5_067_072);
        assert_eq!(
            record.digest.value,
            "1e7f2da1bb678bdb3c78a20b421a276f6b9eb639f0b6a17d46fa02f01a0b67b4"
        );
        // No expected digest was supplied, so it is identity, not authenticity.
        assert!(!record.verified);
        assert!(
            record
                .verification_notes
                .iter()
                .any(|note| note.contains("not authenticated"))
        );
    }
}
