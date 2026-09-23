//! License consent.
//!
//! Three rules drive this module:
//!
//! 1. **A marker is not consent.** A file named `tos_displayed` records that
//!    something was shown, not that anyone agreed. Avdium never reads it as
//!    agreement.
//! 2. **A decision is bound to exact text.** The decision stores a digest of
//!    the text that was actually displayed. If the publisher changes the text,
//!    the digest changes and the old decision no longer covers it. That is the
//!    point.
//! 3. **Only covered answers are sent.** Avdium builds the standard-input
//!    response from the decisions that exist, and refuses to answer at all if
//!    any required license is uncovered. It never pipes an unbounded stream of
//!    `y` into a tool.
//!
//! Consent already validly recorded by the official tooling is respected.
//! The SDK keeps `<sdk>/licenses/<id>` files containing hashes of accepted
//! texts, so a license whose text hashes to a recorded value is treated as
//! already accepted and is not asked again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest as _, Sha256};

/// The digest of the text that was displayed when a decision was made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextIdentity {
    /// SHA-256 of the exact text.
    pub sha256: String,
    /// SHA-1 of the exact text, kept because the official tools record SHA-1
    /// hashes in `<sdk>/licenses`.
    pub sha1: String,
}

impl TextIdentity {
    /// Compute the identity of a license text.
    pub fn of(text: &str) -> Self {
        Self {
            sha256: hex(&Sha256::digest(text.as_bytes())),
            sha1: hex(&Sha1::digest(text.as_bytes())),
        }
    }

    /// A short form for logs. Never the full text.
    pub fn short(&self) -> String {
        self.sha256[..16].to_owned()
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

/// One recorded decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseDecision {
    /// The repository license identifier, for example `android-sdk-license`.
    pub license_id: String,
    /// The identity of the text that was displayed.
    pub text: TextIdentity,
    /// When the decision was recorded, as a Unix timestamp.
    pub decided_at_unix: u64,
    /// The package identifiers this decision was made for.
    ///
    /// Recorded so the answer sent to a tool can be limited to what the user
    /// actually agreed to.
    pub packages: Vec<String>,
}

impl LicenseDecision {
    /// Whether this decision covers the given text.
    pub fn covers(&self, text: &str) -> bool {
        self.text == TextIdentity::of(text)
    }
}

/// Consent recorded by the official tooling, read from the SDK.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordedConsent {
    /// License identifier to the set of accepted SHA-1 text hashes.
    pub accepted: BTreeMap<String, Vec<String>>,
}

impl RecordedConsent {
    /// Read `<sdk>/licenses`.
    ///
    /// Each file is named after a license identifier and contains one or more
    /// hashes, one per line. Anything unreadable is skipped: this is a
    /// convenience check, and failing to read it must not block a user.
    pub fn read(sdk_root: &Path) -> Self {
        let mut accepted: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let Ok(entries) = std::fs::read_dir(sdk_root.join("licenses")) else {
            return Self { accepted };
        };
        for entry in entries.flatten() {
            if !entry.path().is_file() {
                continue;
            }
            let Some(license_id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(contents) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            let hashes: Vec<String> = contents
                .lines()
                .map(|line| line.trim().to_ascii_lowercase())
                .filter(|line| !line.is_empty() && line.chars().all(|c| c.is_ascii_hexdigit()))
                .collect();
            if !hashes.is_empty() {
                accepted.insert(license_id, hashes);
            }
        }
        Self { accepted }
    }

    /// Whether this text has already been accepted through the official tools.
    pub fn covers(&self, license_id: &str, text: &str) -> bool {
        let identity = TextIdentity::of(text);
        self.accepted
            .get(license_id)
            .is_some_and(|hashes| hashes.iter().any(|hash| hash == &identity.sha1))
    }
}

/// Why a license does or does not need a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentStatus {
    /// Avdium recorded a decision for this exact text.
    DecidedByUser,
    /// The official tooling already recorded acceptance of this exact text.
    AlreadyRecorded,
    /// The text has changed since the recorded decision.
    TextChanged,
    /// No decision exists.
    Missing,
}

impl ConsentStatus {
    /// Whether an answer may be sent for this license.
    pub fn is_satisfied(self) -> bool {
        matches!(self, Self::DecidedByUser | Self::AlreadyRecorded)
    }

    /// Label for the setup screen.
    pub fn label(self) -> &'static str {
        match self {
            Self::DecidedByUser => "accepted",
            Self::AlreadyRecorded => "already accepted by the installed tooling",
            Self::TextChanged => "the text changed since it was accepted",
            Self::Missing => "needs a decision",
        }
    }
}

/// The persisted decisions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsentStore {
    /// Decisions in the order they were made.
    pub decisions: Vec<LicenseDecision>,
    /// Whether the user declined, keyed by license identifier.
    ///
    /// A refusal is recorded so Avdium does not re-ask in the same session and
    /// can explain why provisioning is blocked.
    #[serde(default)]
    pub refusals: Vec<String>,
}

impl ConsentStore {
    /// Load from a file, falling back to an empty store.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Save atomically.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut document = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        document.push(b'\n');
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, document)?;
        std::fs::rename(&temporary, path)
    }

    /// Record a decision for the exact text.
    pub fn record(&mut self, license_id: &str, text: &str, packages: &[String]) {
        let identity = TextIdentity::of(text);
        self.refusals.retain(|id| id != license_id);
        // Replace any earlier decision for the same license so the newest
        // decision, made against the newest text, is the one that counts.
        self.decisions
            .retain(|decision| decision.license_id != license_id);
        self.decisions.push(LicenseDecision {
            license_id: license_id.to_owned(),
            text: identity,
            decided_at_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0),
            packages: packages.to_vec(),
        });
    }

    /// Record a refusal.
    pub fn refuse(&mut self, license_id: &str) {
        if !self.refusals.iter().any(|id| id == license_id) {
            self.refusals.push(license_id.to_owned());
        }
    }

    /// Whether the user declined this license.
    pub fn is_refused(&self, license_id: &str) -> bool {
        self.refusals.iter().any(|id| id == license_id)
    }

    /// The status of one license.
    pub fn status(
        &self,
        license_id: &str,
        text: &str,
        recorded: &RecordedConsent,
    ) -> ConsentStatus {
        let identity = TextIdentity::of(text);
        if let Some(decision) = self
            .decisions
            .iter()
            .find(|decision| decision.license_id == license_id)
        {
            return if decision.text == identity {
                ConsentStatus::DecidedByUser
            } else {
                // The user accepted something, but not this text.
                ConsentStatus::TextChanged
            };
        }
        if recorded.covers(license_id, text) {
            return ConsentStatus::AlreadyRecorded;
        }
        ConsentStatus::Missing
    }

    /// The standard-input answers for a set of required licenses.
    ///
    /// Returns the exact text to send, one answer per required license, or the
    /// list of licenses that still need a decision. Never produces an answer
    /// for a license that is not covered.
    pub fn answers_for(
        &self,
        required: &[(String, String)],
        recorded: &RecordedConsent,
    ) -> Result<String, Vec<String>> {
        let mut answers = String::new();
        let mut missing = Vec::new();
        for (license_id, text) in required {
            match self.status(license_id, text, recorded) {
                ConsentStatus::DecidedByUser | ConsentStatus::AlreadyRecorded => {
                    answers.push_str("y\n");
                }
                ConsentStatus::TextChanged | ConsentStatus::Missing => {
                    missing.push(license_id.clone());
                }
            }
        }
        if missing.is_empty() {
            Ok(answers)
        } else {
            Err(missing)
        }
    }

    /// A decision by identifier, for display.
    pub fn decision(&self, license_id: &str) -> Option<&LicenseDecision> {
        self.decisions
            .iter()
            .find(|decision| decision.license_id == license_id)
    }
}

/// A license the caller must handle before it may act.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredLicense {
    /// Repository identifier.
    pub id: String,
    /// The exact text that must be shown before a decision is taken.
    pub text: String,
}

impl RequiredLicense {
    /// Build from a repository license entry.
    pub fn from_catalog(license: &crate::sdk::License) -> Self {
        Self {
            id: license.id.clone(),
            text: license.text.clone(),
        }
    }

    /// A short, non-secret summary for logs.
    ///
    /// The full text is never logged.
    pub fn summary(&self) -> String {
        format!(
            "{} ({} characters, {})",
            self.id,
            self.text.len(),
            TextIdentity::of(&self.text).short()
        )
    }
}

/// The default location of Avdium's consent records.
pub fn consent_file(dirs: &crate::paths::AppDirs) -> PathBuf {
    dirs.data.join("licenses.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT_A: &str = "Terms and Conditions\n\nThis is the Android SDK License Agreement.";
    const TEXT_B: &str =
        "Terms and Conditions\n\nThis is the Android SDK License Agreement, revised.";

    #[test]
    fn a_decision_covers_the_exact_text_and_not_a_revision() {
        let mut store = ConsentStore::default();
        store.record("android-sdk-license", TEXT_A, &["emulator".to_owned()]);

        let recorded = RecordedConsent::default();
        assert_eq!(
            store.status("android-sdk-license", TEXT_A, &recorded),
            ConsentStatus::DecidedByUser
        );
        // The same license, different text, is not covered.
        assert_eq!(
            store.status("android-sdk-license", TEXT_B, &recorded),
            ConsentStatus::TextChanged
        );
    }

    #[test]
    fn an_unknown_license_is_missing_not_assumed() {
        let store = ConsentStore::default();
        assert_eq!(
            store.status(
                "android-sdk-preview-license",
                TEXT_A,
                &RecordedConsent::default()
            ),
            ConsentStatus::Missing
        );
    }

    #[test]
    fn recorded_consent_from_the_official_tooling_is_respected() {
        let mut recorded = RecordedConsent::default();
        recorded.accepted.insert(
            "android-sdk-license".to_owned(),
            vec![TextIdentity::of(TEXT_A).sha1.clone()],
        );

        let store = ConsentStore::default();
        assert_eq!(
            store.status("android-sdk-license", TEXT_A, &recorded),
            ConsentStatus::AlreadyRecorded
        );
        assert!(
            store
                .status("android-sdk-license", TEXT_A, &recorded)
                .is_satisfied()
        );
        // A revised text is not covered by the old hash.
        assert_eq!(
            store.status("android-sdk-license", TEXT_B, &recorded),
            ConsentStatus::Missing
        );
    }

    #[test]
    fn recorded_consent_is_read_from_the_sdk_licenses_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let licenses = temp.path().join("licenses");
        std::fs::create_dir_all(&licenses).expect("create");
        std::fs::write(
            licenses.join("android-sdk-license"),
            format!("{}\n{}\n", TextIdentity::of(TEXT_A).sha1, "0".repeat(40)),
        )
        .expect("write");
        // A directory, an empty file and a non-hex file are all skipped.
        std::fs::create_dir_all(licenses.join("a-directory")).expect("create");
        std::fs::write(licenses.join("empty"), "").expect("write");
        std::fs::write(licenses.join("garbage"), "not a hash").expect("write");

        let recorded = RecordedConsent::read(temp.path());
        assert!(recorded.covers("android-sdk-license", TEXT_A));
        assert!(!recorded.covers("android-sdk-license", TEXT_B));
        assert!(!recorded.covers("android-sdk-preview-license", TEXT_A));
        assert!(!recorded.accepted.contains_key("garbage"));
        assert!(!recorded.accepted.contains_key("empty"));
    }

    #[test]
    fn a_missing_licenses_directory_is_not_an_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert!(RecordedConsent::read(temp.path()).accepted.is_empty());
    }

    #[test]
    fn answers_are_only_produced_when_every_required_license_is_covered() {
        let mut store = ConsentStore::default();
        store.record("android-sdk-license", TEXT_A, &[]);
        let recorded = RecordedConsent::default();

        let required = vec![
            ("android-sdk-license".to_owned(), TEXT_A.to_owned()),
            ("android-sdk-preview-license".to_owned(), TEXT_B.to_owned()),
        ];

        // One license is uncovered, so no answer at all is produced.
        let error = store
            .answers_for(&required, &recorded)
            .expect_err("must refuse");
        assert_eq!(error, vec!["android-sdk-preview-license".to_owned()]);

        store.record("android-sdk-preview-license", TEXT_B, &[]);
        let answers = store.answers_for(&required, &recorded).expect("covered");
        assert_eq!(answers, "y\ny\n");
    }

    #[test]
    fn answers_are_exactly_one_line_per_required_license() {
        let mut store = ConsentStore::default();
        store.record("android-sdk-license", TEXT_A, &[]);
        let required = vec![("android-sdk-license".to_owned(), TEXT_A.to_owned())];
        let answers = store
            .answers_for(&required, &RecordedConsent::default())
            .expect("covered");
        assert_eq!(answers.lines().count(), 1);
        assert_eq!(answers, "y\n");
        // Never an unbounded stream.
        assert!(!answers.contains("yes yes"));
    }

    #[test]
    fn a_refusal_is_remembered_and_does_not_produce_an_answer() {
        let mut store = ConsentStore::default();
        store.refuse("android-sdk-license");
        assert!(store.is_refused("android-sdk-license"));
        let required = vec![("android-sdk-license".to_owned(), TEXT_A.to_owned())];
        assert!(
            store
                .answers_for(&required, &RecordedConsent::default())
                .is_err()
        );

        // Accepting later clears the refusal.
        store.record("android-sdk-license", TEXT_A, &[]);
        assert!(!store.is_refused("android-sdk-license"));
        assert!(
            store
                .answers_for(&required, &RecordedConsent::default())
                .is_ok()
        );
    }

    #[test]
    fn deciding_again_replaces_the_earlier_decision() {
        let mut store = ConsentStore::default();
        store.record("android-sdk-license", TEXT_A, &["emulator".to_owned()]);
        store.record(
            "android-sdk-license",
            TEXT_B,
            &["platform-tools".to_owned()],
        );
        assert_eq!(store.decisions.len(), 1);
        assert_eq!(store.decisions[0].text, TextIdentity::of(TEXT_B));
        assert_eq!(
            store.decisions[0].packages,
            vec!["platform-tools".to_owned()]
        );
    }

    #[test]
    fn decisions_round_trip_through_disk() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("licenses.json");
        let mut store = ConsentStore::default();
        store.record("android-sdk-license", TEXT_A, &["emulator".to_owned()]);
        store.refuse("android-sdk-preview-license");
        store.save(&path).expect("save");

        let loaded = ConsentStore::load(&path);
        assert_eq!(loaded, store);
        assert_eq!(
            loaded.status("android-sdk-license", TEXT_A, &RecordedConsent::default()),
            ConsentStatus::DecidedByUser
        );
    }

    #[test]
    fn a_corrupt_or_missing_store_yields_no_decisions_rather_than_a_panic() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            ConsentStore::load(&temp.path().join("missing.json")),
            ConsentStore::default()
        );
        let path = temp.path().join("broken.json");
        std::fs::write(&path, "{not json").expect("write");
        assert_eq!(ConsentStore::load(&path), ConsentStore::default());
    }

    #[test]
    fn a_stored_decision_never_contains_the_license_text() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("licenses.json");
        let mut store = ConsentStore::default();
        store.record("android-sdk-license", TEXT_A, &[]);
        store.save(&path).expect("save");

        let document = std::fs::read_to_string(&path).expect("read");
        assert!(
            !document.contains("Android SDK License Agreement"),
            "the text must not be stored"
        );
        assert!(document.contains(&TextIdentity::of(TEXT_A).sha256));
    }

    #[test]
    fn required_license_summaries_do_not_leak_the_text() {
        let license = RequiredLicense {
            id: "android-sdk-license".to_owned(),
            text: TEXT_A.to_owned(),
        };
        let summary = license.summary();
        assert!(summary.contains("android-sdk-license"));
        assert!(!summary.contains("License Agreement"));
    }
}
