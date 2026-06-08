//! OCI Container Registry module.
//!
//! Implements OCI Distribution Specification v1.0:
//! - Content-addressed blob storage
//! - Docker V2 Schema 2 and OCI Image Spec manifest handling
//! - Chunked upload support

pub mod types;
pub mod storage;
pub mod manifest;

pub use storage::OciStorage;
pub use manifest::{Manifest, ManifestLayer, ManifestDescriptor, ParsedManifest};
pub use types::{
    Reference, TagListResponse, ErrorResponse, ErrorDetail,
    media_types, error_codes,
    API_VERSION, API_VERSION_HEADER,
};
