//! Platform-specific abstractions for cross-platform compatibility.
//!
//! This module provides unified interfaces for operations that differ
//! between Unix and Windows (paths, process management, file permissions, etc.).

pub mod path;
pub mod process;
pub mod fs;

// Re-export commonly used items
pub use path::{repo_path, expand_home, temp_dir};
pub use process::{execute_script};
pub use fs::{set_executable, is_executable};
