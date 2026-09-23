//! SDK package identifiers and the official repository catalog.

pub mod packages;
pub mod repository;

pub use packages::{PackageId, PackageIdError, PackageKind, SeparatorStyle};
pub use repository::{
    Archive, Catalog, CatalogError, Channel, Checksum, HostTarget, License, RemotePackage, Revision,
};
