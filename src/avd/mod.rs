//! Device model, INI handling and read-only discovery.

pub mod ini;
pub mod model;
pub mod repository;

pub use ini::{IniChange, IniEntry, IniFile, IniProblem, Newline};
pub use model::{Avd, AvdLoadContext, BrokenReason, DataDirOrigin};
pub use repository::{AvdRepository, Discovery};
