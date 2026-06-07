pub mod adapter;
pub mod adapters;
pub mod storage;
pub mod service;

pub use adapter::{ExtractedMetadata, PackageAdapter, get_adapter};
pub use adapters::cargo::{build_sparse_index, build_sparse_index_entry};
pub use adapters::npm::{build_npm_metadata, NpmVersionInfo};
pub use service::{
    PackageDetail, PackageSummary, PublishInfo, PublishResult, VersionDetail, FileDetail,
    package_types,
};
pub use storage::PackageStorage;
