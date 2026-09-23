//! Provisioning: deciding what to install, and installing it safely.
//!
//! Three responsibilities, deliberately separated:
//!
//! - [`manifest`] holds the reviewed pins. Changing what Avdium will accept is
//!   a source change, not a settings change.
//! - [`plan`] is a pure function from observations and goals to an ordered set
//!   of steps. It never proposes updating something that already exists, and a
//!   blocked step stays blocked rather than being rewritten into something that
//!   happens to work.
//! - [`consent`] records decisions against the exact license text that was
//!   displayed, and builds the standard-input answers from those decisions
//!   only.
//! - [`install`] executes one pin: stage, verify, extract, validate, promote,
//!   record.
//!
//! Nothing here runs on the UI thread.

pub mod consent;
pub mod install;
pub mod manifest;
pub mod plan;

pub use consent::{
    ConsentStatus, ConsentStore, LicenseDecision, RecordedConsent, RequiredLicense, TextIdentity,
};
pub use install::{
    Fetcher, HttpFetcher, InstallError, InstalledTool, Stage, ToolRecord, install_pinned,
};
pub use manifest::{ArchiveKind, DigestProvenance, PINS, ToolPin};
pub use plan::{Goals, JavaObservation, Observations, ProvisionPlan, Step, StepAction, StepStatus};
