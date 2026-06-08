pub mod adapter;
pub mod adapters;
pub mod oci;
pub mod storage;
pub mod service;

pub use adapter::{ExtractedMetadata, PackageAdapter, get_adapter};
pub use adapters::cargo::{build_sparse_index, build_sparse_index_entry};
pub use adapters::maven::{build_maven_metadata_xml, MavenVersionEntry};
pub use adapters::npm::{build_npm_metadata, NpmVersionInfo};
pub use adapters::pypi::{build_simple_repository_html, PyPIVersionEntry};
pub use adapters::nuget::{
    build_service_index, build_registration_index, build_search_results,
    NuGetRegistrationEntry, NuGetSearchResult,
};
pub use adapters::rubygems::{
    build_dependencies_json, build_gem_info_json,
    RubyGemsDependencyEntry, RubyGemsVersionEntry, RubyGemsDep,
};
pub use adapters::helm::{build_helm_index, HelmIndexEntry};
pub use service::{
    PackageDetail, PackageSummary, PublishInfo, PublishResult, VersionDetail, FileDetail,
    package_types,
};
pub use storage::PackageStorage;
