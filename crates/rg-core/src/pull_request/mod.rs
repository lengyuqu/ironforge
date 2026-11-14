//! Pull request service — create, diff, merge strategies, fork PR support.
pub mod gix_patch;
pub mod merge_queue;
pub mod readiness;
pub mod repair;
pub mod service;
pub mod update_branch;

pub use merge_queue::*;
pub use repair::*;
pub use service::*;
