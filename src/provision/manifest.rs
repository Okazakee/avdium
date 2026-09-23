//! Reviewed tool pins.
//!
//! Every downloadable artifact Avdium offers is pinned here with its expected
//! size, an expected digest, and **where that digest came from**. The
//! provenance matters: a digest computed from a fresh download records
//! identity, it does not authenticate the download. A digest copied from the
//! publisher's own metadata is a different, weaker kind of evidence than one
//! this project reviewed and reproduced.
//!
//! Pins live in source, not in the user's configuration, so a local install
//! cannot silently change what Avdium will accept next time. Local installation
//! state is recorded separately, in [`super::install::ToolRecord`].

use crate::download::{Digest, DigestKind};

/// How an archive is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    /// A ZIP archive.
    Zip,
    /// A gzip-compressed tar archive.
    TarGz,
    /// A single executable file, not an archive.
    RawBinary,
}

/// Where an expected digest came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestProvenance {
    /// Reproduced by this project by downloading the artifact and hashing it.
    ReproducedByProject,
    /// Copied from the publisher's own repository metadata.
    PublisherMetadata,
    /// No digest is available yet.
    Unestablished,
}

impl DigestProvenance {
    /// One-line description for the setup screen.
    pub fn label(self) -> &'static str {
        match self {
            Self::ReproducedByProject => "reproduced by this project",
            Self::PublisherMetadata => "from the publisher's repository metadata",
            Self::Unestablished => "not established",
        }
    }

    /// Whether a digest from this source is worth enforcing.
    ///
    /// Both established sources are enforced. They are not equally strong, and
    /// the UI says which one applies rather than presenting them as the same.
    pub fn is_enforceable(self) -> bool {
        !matches!(self, Self::Unestablished)
    }
}

/// One reviewed artifact pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPin {
    /// Stable identifier, used in records and logs.
    pub id: &'static str,
    /// Human-readable name.
    pub name: &'static str,
    /// Download URL.
    pub url: &'static str,
    /// Expected size in bytes, when the publisher declares one.
    pub size: Option<u64>,
    /// Expected digest, as published or reproduced.
    pub digest: Option<(&'static str, &'static str)>,
    /// Where the digest came from.
    pub provenance: DigestProvenance,
    /// How the artifact is packed.
    pub archive: ArchiveKind,
    /// The revision directory this installs into, when it is versioned.
    pub revision: Option<&'static str>,
    /// The repository license identifier this artifact requires, if any.
    pub license_ref: Option<&'static str>,
    /// Notes shown next to the download, for example a compatibility caveat.
    pub note: Option<&'static str>,
}

impl ToolPin {
    /// The expected digest, parsed.
    pub fn expected_digest(&self) -> Result<Option<Digest>, crate::download::DigestError> {
        let Some((kind, value)) = self.digest else {
            return Ok(None);
        };
        let Some(kind) = DigestKind::parse(kind) else {
            return Ok(None);
        };
        Ok(Some(Digest::parse(kind, value)?))
    }

    /// Whether the digest is strong enough to be worth pinning against.
    ///
    /// SHA-1 is enforced when it is all the publisher offers, but the caller is
    /// told it is weak.
    pub fn digest_is_strong(&self) -> bool {
        self.expected_digest()
            .ok()
            .flatten()
            .is_some_and(|digest| digest.kind.is_strong())
    }
}

/// Every reviewed pin.
///
/// Adding a pin means adding an entry here, which is a reviewable change,
/// rather than editing a file in the user's home.
pub const PINS: &[ToolPin] = &[
    ToolPin {
        id: "android-cli-launcher",
        name: "Android CLI launcher",
        url: "https://dl.google.com/android/cli/1.0.16406183/linux_x86_64/android",
        size: Some(5_067_072),
        digest: Some((
            "sha256",
            "1e7f2da1bb678bdb3c78a20b421a276f6b9eb639f0b6a17d46fa02f01a0b67b4",
        )),
        provenance: DigestProvenance::ReproducedByProject,
        archive: ArchiveKind::RawBinary,
        revision: None,
        license_ref: Some("android-sdk-license"),
        note: Some(
            "The launcher downloads its own runtime and payload on first use. Pinning the \
             launcher does not pin those, so Avdium treats the first run as a network \
             operation rather than a reproducible install.",
        ),
    },
    ToolPin {
        id: "cmdline-tools",
        name: "Android command line tools",
        url: "https://dl.google.com/android/repository/commandlinetools-linux-16111833_latest.zip",
        size: Some(181_052_239),
        // Google publishes SHA-1 only. It is enforced, and the UI states that
        // it is weak rather than presenting it as equivalent to a SHA-256 pin.
        digest: Some(("sha1", "e025545c62a8e64c7559119566a569fb1dec5f60")),
        provenance: DigestProvenance::PublisherMetadata,
        archive: ArchiveKind::Zip,
        revision: Some("23.0"),
        license_ref: Some("android-sdk-license"),
        note: Some(
            "These are the deprecated tools. They remain the only official way to create a \
             device from a chosen system image, so they are offered for that.",
        ),
    },
];

/// Look up a pin by identifier.
pub fn pin(id: &str) -> Option<&'static ToolPin> {
    PINS.iter().find(|pin| pin.id == id)
}

/// The pin used to create a device from a chosen image.
pub fn creation_tool_pin() -> &'static ToolPin {
    // Both are compile-time constants, so a missing entry is a programming
    // error rather than a runtime condition.
    pin("cmdline-tools").expect("the classic tools pin is defined above")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pin_parses_its_own_digest() {
        for pin in PINS {
            if pin.digest.is_none() {
                continue;
            }
            let digest = pin
                .expected_digest()
                .unwrap_or_else(|error| panic!("{}: {error}", pin.id))
                .unwrap_or_else(|| panic!("{}: no digest", pin.id));
            let expected = match digest.kind {
                DigestKind::Sha1 => 40,
                DigestKind::Sha256 => 64,
            };
            assert_eq!(digest.value.len(), expected, "{}", pin.id);
            assert!(
                digest.value.chars().all(|c| c.is_ascii_hexdigit()),
                "{} is not hex",
                pin.id
            );
        }
    }

    #[test]
    fn every_established_digest_declares_its_provenance() {
        for pin in PINS {
            if pin.digest.is_some() {
                assert!(
                    pin.provenance.is_enforceable(),
                    "{} has a digest but no usable provenance",
                    pin.id
                );
                assert_ne!(pin.provenance, DigestProvenance::Unestablished);
            }
        }
    }

    #[test]
    fn the_published_launcher_pin_matches_the_reproduced_download() {
        // Reproduced during M0: 5067072 bytes, SHA-256 matching the brief.
        let launcher = pin("android-cli-launcher").expect("launcher pin");
        assert_eq!(launcher.size, Some(5_067_072));
        let digest = launcher
            .expected_digest()
            .expect("digest")
            .expect("present");
        assert_eq!(digest.kind, DigestKind::Sha256);
        assert_eq!(
            digest.value,
            "1e7f2da1bb678bdb3c78a20b421a276f6b9eb639f0b6a17d46fa02f01a0b67b4"
        );
        assert_eq!(launcher.provenance, DigestProvenance::ReproducedByProject);
        assert!(launcher.digest_is_strong());
    }

    #[test]
    fn the_classic_tools_pin_records_that_its_digest_is_only_sha1() {
        let tools = creation_tool_pin();
        assert_eq!(tools.size, Some(181_052_239));
        assert_eq!(tools.revision, Some("23.0"));
        assert_eq!(tools.archive, ArchiveKind::Zip);
        // SHA-1 from the publisher's metadata, enforced but not strong.
        assert!(!tools.digest_is_strong());
        assert_eq!(tools.provenance, DigestProvenance::PublisherMetadata);
        assert!(tools.provenance.is_enforceable());
    }

    #[test]
    fn an_unknown_pin_is_not_invented() {
        assert!(pin("does-not-exist").is_none());
    }

    #[test]
    fn provenance_labels_are_user_facing() {
        assert!(
            DigestProvenance::ReproducedByProject
                .label()
                .contains("reproduced")
        );
        assert!(!DigestProvenance::Unestablished.is_enforceable());
    }

    #[test]
    fn pins_carry_no_secrets() {
        // A cheap guard against someone pasting a token into a URL.
        for pin in PINS {
            for needle in ["token", "key=", "secret", "password", "Bearer"] {
                assert!(
                    !pin.url.contains(needle),
                    "{} looks like it contains a credential",
                    pin.id
                );
            }
            assert!(pin.url.starts_with("https://"), "{} must use https", pin.id);
        }
    }
}
