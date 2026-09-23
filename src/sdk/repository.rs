//! The official SDK repository catalog.
//!
//! Parses the namespace-prefixed XML Google publishes. Two properties matter
//! for safety and are enforced here rather than assumed:
//!
//! - **No entity expansion.** `<!DOCTYPE` is rejected outright, so a document
//!   cannot declare custom entities. Only the five predefined XML entities and
//!   numeric character references resolve; anything else is an error. That
//!   closes the XXE and billion-laughs class of problems without relying on a
//!   library default.
//! - **Bounded input.** Documents larger than [`MAX_CATALOG_BYTES`] are
//!   rejected before parsing starts.
//!
//! Selection is by host, channel and revision, never by document order.

use std::fmt;

use quick_xml::Reader;
use quick_xml::events::Event;

use super::packages::PackageId;

/// Largest catalog document accepted.
pub const MAX_CATALOG_BYTES: usize = 32 * 1024 * 1024;

/// Why a catalog could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    /// The document exceeded [`MAX_CATALOG_BYTES`].
    TooLarge {
        /// The actual size.
        bytes: usize,
        /// The limit.
        limit: usize,
    },
    /// The document was not well-formed XML.
    Xml {
        /// Byte offset where the problem was detected.
        position: usize,
        /// The parser's message.
        message: String,
    },
    /// A `<!DOCTYPE` declaration was present.
    DoctypeNotAllowed,
    /// An entity other than the predefined five or a numeric reference.
    UnknownEntity {
        /// The entity name, without the `&` and `;`.
        name: String,
    },
    /// A numeric character reference was not a valid code point.
    BadCharacterReference {
        /// The reference text.
        text: String,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { bytes, limit } => {
                write!(
                    formatter,
                    "catalog is {bytes} bytes, larger than the {limit} byte limit"
                )
            }
            Self::Xml { position, message } => {
                write!(
                    formatter,
                    "catalog is not valid XML at byte {position}: {message}"
                )
            }
            Self::DoctypeNotAllowed => write!(
                formatter,
                "catalog contains a DOCTYPE declaration, which is not accepted"
            ),
            Self::UnknownEntity { name } => {
                write!(
                    formatter,
                    "catalog references an unsupported entity: &{name};"
                )
            }
            Self::BadCharacterReference { text } => {
                write!(
                    formatter,
                    "catalog has an invalid character reference: &{text};"
                )
            }
        }
    }
}

impl std::error::Error for CatalogError {}

/// A package revision.
///
/// Revisions are ordered fields, not semantic versions, and `preview` marks a
/// preview build rather than a pre-release suffix on a version number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Revision {
    /// Major component.
    pub major: u64,
    /// Minor component.
    pub minor: u64,
    /// Micro component.
    pub micro: u64,
    /// Preview number, when this is a preview build.
    pub preview: Option<u64>,
}

impl Revision {
    /// Whether this is a preview build.
    pub fn is_preview(&self) -> bool {
        self.preview.is_some()
    }
}

impl Ord for Revision {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.micro).cmp(&(other.major, other.minor, other.micro))
    }
}

impl PartialOrd for Revision {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Revision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.micro)?;
        if let Some(preview) = self.preview {
            write!(formatter, "-preview{preview}")?;
        }
        Ok(())
    }
}

/// A published archive checksum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checksum {
    /// Algorithm name, for example `sha1`.
    pub kind: String,
    /// Hex digest.
    pub value: String,
}

impl Checksum {
    /// Whether this checksum uses an algorithm Avdium verifies.
    ///
    /// SHA-1 is recorded because Google publishes it, but it is weak. A
    /// SHA-256 pin from the repository manifest is preferred when present.
    pub fn is_strong(&self) -> bool {
        matches!(
            self.kind.to_ascii_lowercase().as_str(),
            "sha256" | "sha-256"
        )
    }
}

/// One downloadable archive for a package.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Archive {
    /// Target operating system, empty when the archive is host-independent.
    pub host_os: Option<String>,
    /// Target architecture, empty when the archive is host-independent.
    pub host_arch: Option<String>,
    /// Archive size in bytes.
    pub size: Option<u64>,
    /// Published checksum.
    pub checksum: Option<Checksum>,
    /// Download URL, usually relative to the repository base.
    pub url: Option<String>,
}

impl Archive {
    /// Whether this archive can be used on a host.
    pub fn matches_host(&self, host_os: &str, host_arch: &str) -> bool {
        let os_matches = self
            .host_os
            .as_deref()
            .is_none_or(|value| value.eq_ignore_ascii_case(host_os));
        let arch_matches = self
            .host_arch
            .as_deref()
            .is_none_or(|value| value.is_empty() || value.eq_ignore_ascii_case(host_arch));
        os_matches && arch_matches
    }
}

/// A license text published by the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct License {
    /// Repository identifier, for example `android-sdk-license`.
    pub id: String,
    /// The exact text a user must be shown before consent.
    pub text: String,
}

/// A release channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    /// Repository identifier, for example `channel-0`.
    pub id: String,
    /// Channel name, for example `stable`.
    pub name: String,
}

impl Channel {
    /// Whether this channel is the stable channel.
    pub fn is_stable(&self) -> bool {
        self.name.eq_ignore_ascii_case("stable") || self.id == "channel-0"
    }
}

/// One package published in the catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePackage {
    /// The raw package path, in classic semicolon form.
    pub path: String,
    /// The parsed identifier, when the path is valid.
    pub id: Option<PackageId>,
    /// Human-readable name.
    pub display_name: Option<String>,
    /// The revision offered.
    pub revision: Revision,
    /// The channel this revision belongs to.
    pub channel_id: Option<String>,
    /// The license the package requires.
    pub license_ref: Option<String>,
    /// Whether the repository marks the package obsolete.
    pub obsolete: bool,
    /// Available archives.
    pub archives: Vec<Archive>,
}

impl RemotePackage {
    /// The first archive usable on a host.
    pub fn archive_for_host(&self, host_os: &str, host_arch: &str) -> Option<&Archive> {
        self.archives
            .iter()
            .find(|archive| archive.matches_host(host_os, host_arch))
    }

    /// The smallest declared download size for a host, when sizes are present.
    pub fn download_size_for_host(&self, host_os: &str, host_arch: &str) -> Option<u64> {
        self.archive_for_host(host_os, host_arch)
            .and_then(|archive| archive.size)
    }
}

/// A parsed catalog.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Catalog {
    /// Packages in document order.
    pub packages: Vec<RemotePackage>,
    /// License texts.
    pub licenses: Vec<License>,
    /// Release channels.
    pub channels: Vec<Channel>,
}

impl Catalog {
    /// Look up a license by identifier.
    pub fn license(&self, id: &str) -> Option<&License> {
        self.licenses.iter().find(|license| license.id == id)
    }

    /// Look up a channel by identifier.
    pub fn channel(&self, id: &str) -> Option<&Channel> {
        self.channels.iter().find(|channel| channel.id == id)
    }

    /// Whether a package is on an acceptable channel.
    pub fn channel_allowed(&self, package: &RemotePackage, include_preview: bool) -> bool {
        if package.revision.is_preview() && !include_preview {
            return false;
        }
        match package.channel_id.as_deref() {
            None => true,
            Some(id) => match self.channel(id) {
                Some(channel) => include_preview || channel.is_stable(),
                None => include_preview,
            },
        }
    }

    /// Packages usable on a host, newest revision first.
    ///
    /// When the same path is published more than once, only the newest allowed
    /// revision is returned, so callers never pick by accident of document
    /// order.
    pub fn select_for_host(
        &self,
        host_os: &str,
        host_arch: &str,
        include_preview: bool,
    ) -> Vec<&RemotePackage> {
        let mut selected: Vec<&RemotePackage> = Vec::new();
        for package in &self.packages {
            if package.obsolete || !self.channel_allowed(package, include_preview) {
                continue;
            }
            if package.archive_for_host(host_os, host_arch).is_none() {
                continue;
            }
            match selected
                .iter_mut()
                .find(|existing| existing.path == package.path)
            {
                Some(existing) if package.revision > existing.revision => *existing = package,
                Some(_) => {}
                None => selected.push(package),
            }
        }
        selected.sort_by(|a, b| {
            b.revision
                .cmp(&a.revision)
                .then_with(|| a.path.cmp(&b.path))
        });
        selected
    }

    /// Packages that require a given license.
    pub fn packages_using_license(&self, license_id: &str) -> Vec<&RemotePackage> {
        self.packages
            .iter()
            .filter(|package| package.license_ref.as_deref() == Some(license_id))
            .collect()
    }
}

/// A host target for archive selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTarget {
    /// Catalog host OS token, for example `linux`.
    pub os: String,
    /// Catalog host architecture token, for example `x64`.
    pub arch: String,
}

impl HostTarget {
    /// The host this build runs on.
    pub fn current() -> Self {
        Self {
            os: std::env::consts::OS.to_owned(),
            arch: catalog_arch_token(std::env::consts::ARCH),
        }
    }
}

/// Map a Rust architecture name to the catalog's token.
pub fn catalog_arch_token(arch: &str) -> String {
    match arch {
        "x86_64" => "x64".to_owned(),
        "aarch64" => "arm64-v8a".to_owned(),
        other => other.to_owned(),
    }
}

/// Parse a catalog document.
pub fn parse_catalog(xml: &str) -> Result<Catalog, CatalogError> {
    if xml.len() > MAX_CATALOG_BYTES {
        return Err(CatalogError::TooLarge {
            bytes: xml.len(),
            limit: MAX_CATALOG_BYTES,
        });
    }

    let mut reader = Reader::from_str(xml);
    reader.config_mut().check_end_names = true;

    let mut catalog = Catalog::default();
    let mut stack: Vec<String> = Vec::new();
    let mut saw_element = false;
    let mut text = String::new();
    let mut attributes: Vec<(String, String)> = Vec::new();

    let mut package: Option<RemotePackage> = None;
    let mut archive: Option<Archive> = None;
    let mut revision = Revision::default();
    let mut license_id: Option<String> = None;
    let mut channel_id: Option<String> = None;

    loop {
        let event = match reader.read_event() {
            Ok(event) => event,
            Err(error) => {
                return Err(CatalogError::Xml {
                    position: reader.buffer_position() as usize,
                    message: error.to_string(),
                });
            }
        };

        match event {
            Event::Start(start) => {
                saw_element = true;
                let name = local_name_of(start.name().as_ref());
                text.clear();
                attributes.clear();
                for attribute in start.attributes().flatten() {
                    let key = local_name_of(attribute.key.as_ref()).to_owned();
                    let value = unescape(attribute.value.as_ref())?;
                    attributes.push((key, value));
                }
                match name.as_str() {
                    "remotePackage" => {
                        package = Some(RemotePackage {
                            path: attribute_value(&attributes, "path").unwrap_or_default(),
                            id: None,
                            display_name: None,
                            revision: Revision::default(),
                            channel_id: None,
                            license_ref: None,
                            obsolete: false,
                            archives: Vec::new(),
                        });
                    }
                    "archive" => archive = Some(Archive::default()),
                    "revision" => revision = Revision::default(),
                    "license" => license_id = attribute_value(&attributes, "id"),
                    "channel" => channel_id = attribute_value(&attributes, "id"),
                    _ => {}
                }
                stack.push(name);
            }
            Event::Empty(empty) => {
                saw_element = true;
                let name = local_name_of(empty.name().as_ref());
                let mut local_attributes: Vec<(String, String)> = Vec::new();
                for attribute in empty.attributes().flatten() {
                    let key = local_name_of(attribute.key.as_ref()).to_owned();
                    let value = unescape(attribute.value.as_ref())?;
                    local_attributes.push((key, value));
                }
                // Self-closing elements carry no text, so they are handled here.
                match name.as_str() {
                    "uses-license" => {
                        if let Some(package) = package.as_mut() {
                            package.license_ref = attribute_value(&local_attributes, "ref");
                        }
                    }
                    "channelRef" => {
                        if let Some(package) = package.as_mut() {
                            package.channel_id = attribute_value(&local_attributes, "ref");
                        }
                    }
                    "obsolete" => {
                        if let Some(package) = package.as_mut() {
                            package.obsolete = true;
                        }
                    }
                    _ => {}
                }
            }
            Event::End(end) => {
                let name = local_name_of(end.name().as_ref());
                let parent = stack.iter().rev().nth(1).cloned().unwrap_or_default();
                let trimmed = text.trim();

                match name.as_str() {
                    "revision" => {
                        if let Some(package) = package.as_mut() {
                            package.revision = revision;
                        }
                    }
                    "major" => revision.major = parse_u64(trimmed),
                    "minor" => revision.minor = parse_u64(trimmed),
                    "micro" => revision.micro = parse_u64(trimmed),
                    "preview" => revision.preview = Some(parse_u64(trimmed)),
                    "display-name" => {
                        if let Some(package) = package.as_mut() {
                            package.display_name =
                                Some(trimmed.to_owned()).filter(|value| !value.is_empty());
                        }
                    }
                    "size" => {
                        if let Some(archive) = archive.as_mut() {
                            archive.size = trimmed.parse::<u64>().ok();
                        }
                    }
                    "checksum" => {
                        if let Some(archive) = archive.as_mut() {
                            let kind = attribute_value(&attributes, "type")
                                .unwrap_or_else(|| "sha1".to_owned());
                            if !trimmed.is_empty() {
                                archive.checksum = Some(Checksum {
                                    kind,
                                    value: trimmed.to_owned(),
                                });
                            }
                        }
                    }
                    "url" => {
                        if let Some(archive) = archive.as_mut() {
                            archive.url =
                                Some(trimmed.to_owned()).filter(|value| !value.is_empty());
                        }
                    }
                    "host-os" if parent == "archive" => {
                        if let Some(archive) = archive.as_mut() {
                            archive.host_os =
                                Some(trimmed.to_owned()).filter(|value| !value.is_empty());
                        }
                    }
                    "host-arch" if parent == "archive" => {
                        if let Some(archive) = archive.as_mut() {
                            archive.host_arch =
                                Some(trimmed.to_owned()).filter(|value| !value.is_empty());
                        }
                    }
                    "archive" => {
                        if let (Some(package), Some(archive)) = (package.as_mut(), archive.take()) {
                            package.archives.push(archive);
                        }
                    }
                    "remotePackage" => {
                        if let Some(mut finished) = package.take() {
                            finished.id = PackageId::parse(&finished.path).ok();
                            catalog.packages.push(finished);
                        }
                    }
                    "license" => {
                        if let Some(id) = license_id.take() {
                            catalog.licenses.push(License {
                                id,
                                text: text.clone(),
                            });
                        }
                    }
                    "channel" => {
                        if let Some(id) = channel_id.take() {
                            catalog.channels.push(Channel {
                                id,
                                name: trimmed.to_owned(),
                            });
                        }
                    }
                    _ => {}
                }

                stack.pop();
                text.clear();
                attributes.clear();
            }
            Event::Text(chunk) => text.push_str(chunk.as_ref()),
            Event::CData(chunk) => text.push_str(chunk.as_ref()),
            Event::GeneralRef(reference) => {
                let name = reference.as_ref();
                text.push_str(&resolve_reference(name)?);
            }
            Event::DocType(_) => return Err(CatalogError::DoctypeNotAllowed),
            Event::Eof => break,
            _ => {}
        }
    }

    check_well_formed(&stack, saw_element, reader.buffer_position() as usize)?;
    Ok(catalog)
}

/// Validate that the document had a root element and that every opened element
/// was closed.
fn check_well_formed(
    stack: &[String],
    saw_element: bool,
    position: usize,
) -> Result<(), CatalogError> {
    if !saw_element {
        return Err(CatalogError::Xml {
            position,
            message: "the document contains no elements".to_owned(),
        });
    }
    if stack.is_empty() {
        return Ok(());
    }
    Err(CatalogError::Xml {
        position,
        message: format!("unclosed element <{}>", stack.join(" / ")),
    })
}

/// The local part of a possibly namespace-prefixed name.
fn local_name_of(name: &str) -> String {
    let local = match name.rsplit_once(':') {
        Some((_, local)) => local,
        None => name,
    };
    // quick-xml may also present names in `{namespace}local` form.
    match local.rsplit_once('}') {
        Some((_, local)) => local.to_owned(),
        None => local.to_owned(),
    }
}

fn attribute_value(attributes: &[(String, String)], key: &str) -> Option<String> {
    attributes
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
}

fn parse_u64(text: &str) -> u64 {
    text.trim().parse().unwrap_or(0)
}

/// Resolve one entity or character reference.
fn resolve_reference(name: &str) -> Result<String, CatalogError> {
    match name {
        "amp" => Ok("&".to_owned()),
        "lt" => Ok("<".to_owned()),
        "gt" => Ok(">".to_owned()),
        "quot" => Ok("\"".to_owned()),
        "apos" => Ok("'".to_owned()),
        _ => {
            let digits =
                if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16)
                        .ok()
                        .map(|code| (code, name.to_owned()))
                } else if let Some(decimal) = name.strip_prefix('#') {
                    decimal
                        .parse::<u32>()
                        .ok()
                        .map(|code| (code, name.to_owned()))
                } else {
                    None
                };
            match digits {
                Some((code, text)) => char::from_u32(code)
                    .map(|character| character.to_string())
                    .ok_or(CatalogError::BadCharacterReference { text }),
                // Anything else, including a DTD-declared entity, is refused
                // rather than expanded.
                None => Err(CatalogError::UnknownEntity {
                    name: name.to_owned(),
                }),
            }
        }
    }
}

/// Resolve every entity in a raw attribute value.
fn unescape(raw: &str) -> Result<String, CatalogError> {
    if !raw.contains('&') {
        return Ok(raw.to_owned());
    }
    let mut output = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(index) = rest.find('&') {
        output.push_str(&rest[..index]);
        let after = &rest[index + 1..];
        let Some(end) = after.find(';') else {
            // An unterminated `&` is literal text in XML.
            output.push('&');
            rest = after;
            continue;
        };
        let name = &after[..end];
        output.push_str(&resolve_reference(name)?);
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn excerpt() -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/catalog/repository2-3.excerpt.xml");
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path:?}: {error}"))
    }

    #[test]
    fn the_real_excerpt_parses() {
        let catalog = parse_catalog(&excerpt()).expect("parse real catalog excerpt");
        assert_eq!(catalog.packages.len(), 6);
        assert_eq!(catalog.channels.len(), 4);
        assert_eq!(catalog.licenses.len(), 2);

        let emulator = catalog
            .packages
            .iter()
            .find(|package| package.path == "emulator")
            .expect("emulator package");
        assert_eq!(
            emulator.revision,
            Revision {
                major: 37,
                minor: 3,
                micro: 1,
                preview: None
            }
        );
        assert_eq!(emulator.channel_id.as_deref(), Some("channel-2"));
        assert_eq!(
            emulator.license_ref.as_deref(),
            Some("android-sdk-preview-license")
        );
        assert_eq!(emulator.display_name.as_deref(), Some("Android Emulator"));
        assert_eq!(emulator.archives.len(), 4);

        let linux = emulator
            .archive_for_host("linux", "x64")
            .expect("linux archive");
        assert_eq!(linux.size, Some(353_795_619));
        assert_eq!(
            linux.checksum.as_ref().expect("checksum").value,
            "07431598c6a52d971f63ef4c76b4f1217ce6b64c"
        );
        assert_eq!(
            linux.url.as_deref(),
            Some("emulator-linux_x64-16373887.zip")
        );
        assert!(!linux.checksum.as_ref().expect("checksum").is_strong());
    }

    #[test]
    fn package_identifiers_survive_parsing() {
        let catalog = parse_catalog(&excerpt()).expect("parse");
        let ids: Vec<String> = catalog
            .packages
            .iter()
            .filter_map(|package| package.id.as_ref().map(PackageId::as_str))
            .collect();
        assert!(ids.contains(&"platforms/android-36".to_owned()));
        assert!(ids.contains(&"build-tools/36.1.0".to_owned()));
        assert!(ids.contains(&"ndk/30.0.16138531".to_owned()));
    }

    #[test]
    fn channels_are_read_with_their_names() {
        let catalog = parse_catalog(&excerpt()).expect("parse");
        let stable = catalog.channel("channel-0").expect("stable channel");
        assert_eq!(stable.name, "stable");
        assert!(stable.is_stable());
        assert!(!catalog.channel("channel-2").expect("dev").is_stable());
    }

    #[test]
    fn license_text_is_available_in_full_for_a_consent_screen() {
        let catalog = parse_catalog(&excerpt()).expect("parse");
        let license = catalog.license("android-sdk-license").expect("license");
        assert!(license.text.len() > 10_000, "license text looks truncated");
        assert!(
            license
                .text
                .contains("Android Software Development Kit License Agreement")
        );
    }

    #[test]
    fn stable_selection_excludes_preview_and_dev_channels() {
        let catalog = parse_catalog(&excerpt()).expect("parse");
        let stable = catalog.select_for_host("linux", "x64", false);
        let paths: Vec<&str> = stable.iter().map(|package| package.path.as_str()).collect();
        // The excerpt's emulator entry is on the dev channel.
        assert!(
            !paths.contains(&"emulator"),
            "dev channel leaked into stable selection: {paths:?}"
        );
        assert!(paths.contains(&"platform-tools"));
        assert!(paths.contains(&"build-tools;36.1.0"));
        assert!(paths.contains(&"platforms;android-36"));
    }

    #[test]
    fn preview_selection_includes_the_dev_channel() {
        let catalog = parse_catalog(&excerpt()).expect("parse");
        let preview = catalog.select_for_host("linux", "x64", true);
        let paths: Vec<&str> = preview
            .iter()
            .map(|package| package.path.as_str())
            .collect();
        assert!(paths.contains(&"emulator"));
        assert!(paths.contains(&"ndk;30.0.16138531"));
    }

    #[test]
    fn selection_uses_the_host_not_document_order() {
        let catalog = parse_catalog(&excerpt()).expect("parse");
        let windows = catalog.select_for_host("windows", "x64", true);
        let emulator = windows
            .iter()
            .find(|package| package.path == "emulator")
            .expect("emulator has a windows archive");
        assert_eq!(
            emulator
                .archive_for_host("windows", "x64")
                .expect("archive")
                .host_os
                .as_deref(),
            Some("windows")
        );
        assert!(emulator.archive_for_host("linux", "x64").is_some());
        assert!(emulator.archive_for_host("freebsd", "x64").is_none());

        // macOS has no arm64 emulator archive in the excerpt.
        let macos_arm = catalog.select_for_host("macosx", "arm64-v8a", true);
        assert!(!macos_arm.iter().any(|package| package.path == "emulator"));
    }

    #[test]
    fn host_independent_packages_match_every_host() {
        let catalog = parse_catalog(&excerpt()).expect("parse");
        let platform = catalog
            .packages
            .iter()
            .find(|package| package.path == "platforms;android-36")
            .expect("platform");
        let archive = platform
            .archive_for_host("linux", "x64")
            .expect("host independent");
        assert_eq!(archive.host_os, None);
        assert!(platform.archive_for_host("windows", "x64").is_some());
    }

    #[test]
    fn packages_are_grouped_by_license() {
        let catalog = parse_catalog(&excerpt()).expect("parse");
        let preview = catalog.packages_using_license("android-sdk-preview-license");
        let paths: Vec<&str> = preview
            .iter()
            .map(|package| package.path.as_str())
            .collect();
        assert!(paths.contains(&"emulator"));
        assert!(paths.contains(&"ndk;30.0.16138531"));
    }

    #[test]
    fn revisions_order_by_fields_and_reject_preview_when_asked() {
        let low = Revision {
            major: 36,
            minor: 2,
            micro: 12,
            preview: None,
        };
        let high = Revision {
            major: 37,
            minor: 1,
            micro: 11,
            preview: None,
        };
        assert!(high > low);
        assert_eq!(high.to_string(), "37.1.11");
        let preview = Revision {
            major: 30,
            minor: 0,
            micro: 1,
            preview: Some(3),
        };
        assert!(preview.is_preview());
        assert_eq!(preview.to_string(), "30.0.1-preview3");
    }

    #[test]
    fn a_doctype_is_refused_outright() {
        let xml = r#"<!DOCTYPE foo [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>
<sdk:sdk-repository xmlns:sdk="n"><remotePackage path="a"><display-name>&xxe;</display-name></remotePackage></sdk:sdk-repository>"#;
        assert_eq!(parse_catalog(xml), Err(CatalogError::DoctypeNotAllowed));
    }

    #[test]
    fn unknown_entities_are_refused_rather_than_expanded() {
        let xml = r#"<r><remotePackage path="a"><display-name>&custom;</display-name></remotePackage></r>"#;
        match parse_catalog(xml) {
            Err(CatalogError::UnknownEntity { name }) => assert_eq!(name, "custom"),
            other => panic!("expected an unknown entity error, got {other:?}"),
        }
    }

    #[test]
    fn predefined_and_numeric_entities_resolve() {
        let xml = r#"<r><remotePackage path="a"><display-name>a &amp; b &#65; &#x42; &lt;c&gt;</display-name></remotePackage></r>"#;
        let catalog = parse_catalog(xml).expect("parse");
        assert_eq!(
            catalog.packages[0].display_name.as_deref(),
            Some("a & b A B <c>")
        );
    }

    #[test]
    fn entities_in_attribute_values_resolve() {
        let xml = r#"<r><remotePackage path="a&amp;b"></remotePackage></r>"#;
        let catalog = parse_catalog(xml).expect("parse");
        assert_eq!(catalog.packages[0].path, "a&b");
    }

    #[test]
    fn an_invalid_character_reference_is_an_error() {
        let xml = r#"<r><remotePackage path="a"><display-name>&#xD800;</display-name></remotePackage></r>"#;
        assert!(matches!(
            parse_catalog(xml),
            Err(CatalogError::BadCharacterReference { .. })
        ));
    }

    #[test]
    fn an_oversized_document_is_rejected_before_parsing() {
        let huge = "x".repeat(MAX_CATALOG_BYTES + 1);
        match parse_catalog(&huge) {
            Err(CatalogError::TooLarge { bytes, limit }) => {
                assert_eq!(bytes, MAX_CATALOG_BYTES + 1);
                assert_eq!(limit, MAX_CATALOG_BYTES);
            }
            other => panic!("expected a size error, got {other:?}"),
        }
    }

    #[test]
    fn malformed_xml_is_an_error_not_a_panic() {
        // An empty document and a bare-text document are both refused: a catalog
        // without a root element is not a catalog.
        for xml in [
            "<r><a></r>",
            "<r>",
            "<r><a></b></r>",
            "not xml at all",
            "",
            "   \n",
        ] {
            assert!(parse_catalog(xml).is_err(), "expected an error for {xml:?}");
        }
    }

    #[test]
    fn namespace_prefixes_are_stripped_from_element_and_attribute_names() {
        let xml = r#"<sdk:sdk-repository xmlns:sdk="n" xmlns:xsi="x"><sdk:remotePackage path="a"><sdk-common:display-name>Hello</sdk-common:display-name><type-details xsi:type="generic:genericDetailsType"/></sdk:remotePackage></sdk:sdk-repository>"#;
        let catalog = parse_catalog(xml).expect("parse");
        assert_eq!(catalog.packages.len(), 1);
        assert_eq!(catalog.packages[0].display_name.as_deref(), Some("Hello"));
    }

    #[test]
    fn obsolete_packages_are_marked_and_excluded_from_selection() {
        let xml = r#"<r><remotePackage path="old"><revision><major>1</major></revision><obsolete/><archives><archive><complete><url>a.zip</url></complete></archive></archives></remotePackage></r>"#;
        let catalog = parse_catalog(xml).expect("parse");
        assert!(catalog.packages[0].obsolete);
        assert!(catalog.select_for_host("linux", "x64", true).is_empty());
    }

    #[test]
    fn cdata_is_treated_as_text() {
        let xml = r#"<r><license id="a"><![CDATA[text with <angle> brackets & ampersand]]></license></r>"#;
        let catalog = parse_catalog(xml).expect("parse");
        assert_eq!(
            catalog.licenses[0].text,
            "text with <angle> brackets & ampersand"
        );
    }

    #[test]
    fn the_newest_revision_wins_for_a_duplicated_path() {
        let xml = r#"<r>
            <remotePackage path="p"><revision><major>1</major></revision><archives><archive><complete><url>old.zip</url></complete></archive></archives></remotePackage>
            <remotePackage path="p"><revision><major>2</major></revision><archives><archive><complete><url>new.zip</url></complete></archive></archives></remotePackage>
        </r>"#;
        let catalog = parse_catalog(xml).expect("parse");
        let selected = catalog.select_for_host("linux", "x64", true);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].revision.major, 2);
    }

    #[test]
    fn arch_tokens_match_the_catalog_vocabulary() {
        assert_eq!(catalog_arch_token("x86_64"), "x64");
        assert_eq!(catalog_arch_token("aarch64"), "arm64-v8a");
        assert_eq!(catalog_arch_token("riscv64"), "riscv64");
    }

    #[test]
    fn a_package_with_no_usable_archive_is_not_offered() {
        let xml = r#"<r><remotePackage path="p"><revision><major>1</major></revision><archives><archive><complete><url>a.zip</url></complete><host-os>plan9</host-os></archive></archives></remotePackage></r>"#;
        let catalog = parse_catalog(xml).expect("parse");
        assert!(catalog.select_for_host("linux", "x64", true).is_empty());
        assert_eq!(catalog.packages.len(), 1);
    }
}
