//! SDK package identifiers.
//!
//! Package IDs are structured strings, not numbers. Values such as
//! `android-36.1` and `x86_64` must survive parsing unchanged, and API levels
//! are never converted to integers.
//!
//! The two tool families serialize the same identifier differently:
//!
//! - classic tools use semicolons: `system-images;android-36.1;google_apis;x86_64`
//! - Android CLI uses slashes: `system-images/android-36.1/google_apis/x86_64`
//!
//! [`PackageId`] stores the canonical slash form and can render either.

use std::fmt;

/// Why an identifier could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageIdError {
    /// The identifier was empty or only whitespace.
    Empty,
    /// The identifier had an empty segment, for example a doubled separator.
    EmptySegment {
        /// The raw identifier.
        raw: String,
    },
    /// A segment tried to escape its position.
    UnsafeSegment {
        /// The offending segment.
        segment: String,
    },
}

impl fmt::Display for PackageIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(formatter, "package identifier is empty"),
            Self::EmptySegment { raw } => {
                write!(formatter, "package identifier `{raw}` has an empty segment")
            }
            Self::UnsafeSegment { segment } => {
                write!(
                    formatter,
                    "package identifier segment `{segment}` is not allowed"
                )
            }
        }
    }
}

impl std::error::Error for PackageIdError {}

/// What kind of SDK component an identifier refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageKind {
    /// `platforms/android-36`
    Platform,
    /// `build-tools/36.1.0`
    BuildTools,
    /// `system-images/android-36.1/google_apis/x86_64`
    SystemImage,
    /// `platform-tools`
    PlatformTools,
    /// `emulator`
    Emulator,
    /// `cmdline-tools/latest`
    CmdlineTools,
    /// `sources/android-36`
    Sources,
    /// `ndk/27.0.12077973`
    Ndk,
    /// `cmake/3.22.1`
    Cmake,
    /// `add-ons/...`
    AddOn,
    /// Anything else.
    Other,
}

impl PackageKind {
    /// Label for the package list.
    pub fn label(self) -> &'static str {
        match self {
            Self::Platform => "Platform",
            Self::BuildTools => "Build tools",
            Self::SystemImage => "System image",
            Self::PlatformTools => "Platform tools",
            Self::Emulator => "Emulator",
            Self::CmdlineTools => "Command line tools",
            Self::Sources => "Sources",
            Self::Ndk => "NDK",
            Self::Cmake => "CMake",
            Self::AddOn => "Add-on",
            Self::Other => "Other",
        }
    }
}

/// A parsed, canonicalized package identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PackageId {
    segments: Vec<String>,
}

impl PackageId {
    /// Parse an identifier written with either separator style.
    pub fn parse(raw: &str) -> Result<Self, PackageIdError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(PackageIdError::Empty);
        }
        if trimmed.starts_with('/') || trimmed.starts_with(';') {
            return Err(PackageIdError::EmptySegment {
                raw: trimmed.to_owned(),
            });
        }

        let mut segments = Vec::new();
        for segment in trimmed.split([';', '/']) {
            if segment.is_empty() {
                return Err(PackageIdError::EmptySegment {
                    raw: trimmed.to_owned(),
                });
            }
            if segment == "." || segment == ".." || segment.contains('\\') {
                return Err(PackageIdError::UnsafeSegment {
                    segment: segment.to_owned(),
                });
            }
            if segment.chars().any(char::is_whitespace) {
                return Err(PackageIdError::UnsafeSegment {
                    segment: segment.to_owned(),
                });
            }
            segments.push(segment.to_owned());
        }

        Ok(Self { segments })
    }

    /// The canonical slash form used by Android CLI.
    pub fn as_str(&self) -> String {
        self.segments.join("/")
    }

    /// The semicolon form used by classic tools.
    pub fn classic(&self) -> String {
        self.segments.join(";")
    }

    /// The identifier segments.
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// The first segment, which identifies the component kind.
    pub fn root(&self) -> &str {
        self.segments
            .first()
            .map(String::as_str)
            .unwrap_or_default()
    }

    /// What kind of component this is.
    pub fn kind(&self) -> PackageKind {
        match self.root() {
            "platforms" => PackageKind::Platform,
            "build-tools" => PackageKind::BuildTools,
            "system-images" => PackageKind::SystemImage,
            "platform-tools" => PackageKind::PlatformTools,
            "emulator" => PackageKind::Emulator,
            "cmdline-tools" => PackageKind::CmdlineTools,
            "sources" => PackageKind::Sources,
            "ndk" => PackageKind::Ndk,
            "cmake" => PackageKind::Cmake,
            "add-ons" => PackageKind::AddOn,
            _ => PackageKind::Other,
        }
    }

    /// The API level or platform token, when the identifier has one.
    ///
    /// Returned as a string: `android-36.1` is not an integer.
    pub fn api_token(&self) -> Option<&str> {
        match self.kind() {
            PackageKind::Platform | PackageKind::Sources => {
                self.segments.get(1).map(String::as_str)
            }
            PackageKind::SystemImage => self.segments.get(1).map(String::as_str),
            _ => None,
        }
    }

    /// The image tag, for example `google_apis`.
    pub fn image_tag(&self) -> Option<&str> {
        (self.kind() == PackageKind::SystemImage)
            .then(|| self.segments.get(2).map(String::as_str))
            .flatten()
    }

    /// The image ABI, for example `x86_64`.
    pub fn image_abi(&self) -> Option<&str> {
        (self.kind() == PackageKind::SystemImage)
            .then(|| self.segments.get(3).map(String::as_str))
            .flatten()
    }

    /// The system image directory this package installs into, relative to the
    /// SDK root. Used to match AVDs to installed images.
    pub fn image_relative_dir(&self) -> Option<String> {
        (self.kind() == PackageKind::SystemImage).then(|| self.as_str())
    }

    /// Render for the given provider style.
    pub fn render(&self, style: SeparatorStyle) -> String {
        match style {
            SeparatorStyle::Slash => self.as_str(),
            SeparatorStyle::Semicolon => self.classic(),
        }
    }

    /// Append a version selector for Android CLI, for example `id@4.0.0`.
    pub fn with_version(&self, version: &str) -> String {
        format!("{}@{version}", self.as_str())
    }

    /// Split an Android CLI `id@version` argument.
    pub fn split_version(raw: &str) -> (&str, Option<&str>) {
        match raw.rsplit_once('@') {
            Some((id, version)) if !version.is_empty() && !id.is_empty() => (id, Some(version)),
            _ => (raw, None),
        }
    }
}

impl fmt::Display for PackageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.as_str())
    }
}

impl std::str::FromStr for PackageId {
    type Err = PackageIdError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::parse(raw)
    }
}

/// Which separator a provider expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeparatorStyle {
    /// Android CLI.
    Slash,
    /// Classic `sdkmanager`.
    Semicolon,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_separator_styles_parse_to_the_same_identifier() {
        let classic = PackageId::parse("system-images;android-36.1;google_apis;x86_64")
            .expect("classic form");
        let modern =
            PackageId::parse("system-images/android-36.1/google_apis/x86_64").expect("slash form");
        assert_eq!(classic, modern);
        assert_eq!(
            classic.as_str(),
            "system-images/android-36.1/google_apis/x86_64"
        );
        assert_eq!(
            classic.classic(),
            "system-images;android-36.1;google_apis;x86_64"
        );
    }

    #[test]
    fn api_identifiers_are_never_parsed_as_integers() {
        let id = PackageId::parse("platforms/android-36.1").expect("parse");
        assert_eq!(id.api_token(), Some("android-36.1"));
        assert_eq!(id.as_str(), "platforms/android-36.1");

        let id = PackageId::parse("system-images/android-37.0/google_apis_playstore_ps16k/x86_64")
            .expect("parse");
        assert_eq!(id.api_token(), Some("android-37.0"));
        assert_eq!(id.image_tag(), Some("google_apis_playstore_ps16k"));
        assert_eq!(id.image_abi(), Some("x86_64"));
    }

    #[test]
    fn a_single_segment_package_is_valid() {
        let id = PackageId::parse("emulator").expect("parse");
        assert_eq!(id.kind(), PackageKind::Emulator);
        assert_eq!(id.segments().len(), 1);
        assert_eq!(id.as_str(), "emulator");
    }

    #[test]
    fn empty_and_unsafe_identifiers_are_rejected() {
        assert_eq!(PackageId::parse(""), Err(PackageIdError::Empty));
        assert_eq!(PackageId::parse("   "), Err(PackageIdError::Empty));
        assert!(matches!(
            PackageId::parse("platforms//android-36"),
            Err(PackageIdError::EmptySegment { .. })
        ));
        assert!(matches!(
            PackageId::parse("/platforms/android-36"),
            Err(PackageIdError::EmptySegment { .. })
        ));
        assert!(matches!(
            PackageId::parse("platforms/../../etc"),
            Err(PackageIdError::UnsafeSegment { .. })
        ));
        assert!(matches!(
            PackageId::parse("platforms/.."),
            Err(PackageIdError::UnsafeSegment { .. })
        ));
        assert!(matches!(
            PackageId::parse("platforms/android 36"),
            Err(PackageIdError::UnsafeSegment { .. })
        ));
    }

    #[test]
    fn version_suffixes_round_trip() {
        assert_eq!(
            PackageId::split_version("emulator@37.1.11"),
            ("emulator", Some("37.1.11"))
        );
        assert_eq!(PackageId::split_version("emulator"), ("emulator", None));
        assert_eq!(PackageId::split_version("@"), ("@", None));
        let id = PackageId::parse("system-images/android-36.1/google_apis/x86_64").expect("parse");
        assert_eq!(
            id.with_version("4.0.0"),
            "system-images/android-36.1/google_apis/x86_64@4.0.0"
        );
    }

    #[test]
    fn kinds_are_classified_from_the_first_segment() {
        let cases = [
            ("platforms/android-36", PackageKind::Platform),
            ("build-tools/36.1.0", PackageKind::BuildTools),
            ("platform-tools", PackageKind::PlatformTools),
            ("cmdline-tools/latest", PackageKind::CmdlineTools),
            ("sources/android-36", PackageKind::Sources),
            ("ndk/27.1.12297006", PackageKind::Ndk),
            ("cmake/3.22.1", PackageKind::Cmake),
            ("add-ons/google_apis_playstore/36.1", PackageKind::AddOn),
            ("something/else", PackageKind::Other),
        ];
        for (raw, kind) in cases {
            assert_eq!(PackageId::parse(raw).expect(raw).kind(), kind, "{raw}");
        }
    }

    #[test]
    fn an_image_relative_dir_matches_the_avd_config_convention() {
        let id = PackageId::parse("system-images/android-36.1/google_apis/x86_64").expect("parse");
        assert_eq!(
            id.image_relative_dir().expect("image"),
            "system-images/android-36.1/google_apis/x86_64"
        );
        let platform = PackageId::parse("platforms/android-36").expect("parse");
        assert_eq!(platform.image_relative_dir(), None);
    }

    #[test]
    fn rendering_honors_the_requested_separator() {
        let id = PackageId::parse("build-tools;36.1.0").expect("parse");
        assert_eq!(id.render(SeparatorStyle::Slash), "build-tools/36.1.0");
        assert_eq!(id.render(SeparatorStyle::Semicolon), "build-tools;36.1.0");
    }

    #[test]
    fn identifiers_sort_deterministically() {
        let mut ids: Vec<PackageId> = ["platforms/android-36", "build-tools/36.1.0", "emulator"]
            .iter()
            .map(|raw| PackageId::parse(raw).expect("parse"))
            .collect();
        ids.sort();
        let rendered: Vec<String> = ids.iter().map(PackageId::as_str).collect();
        assert_eq!(
            rendered,
            vec!["build-tools/36.1.0", "emulator", "platforms/android-36"]
        );
    }
}
