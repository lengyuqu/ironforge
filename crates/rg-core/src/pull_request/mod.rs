//! Pull request service — create, diff, merge strategies, fork PR support.
pub mod gix_patch;
pub mod merge_queue;
pub mod service;

pub use merge_queue::*;
pub use service::*;
