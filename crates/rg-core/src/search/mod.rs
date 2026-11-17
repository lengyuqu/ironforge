//! Search service — FTS5 global search across repos, issues, wiki pages, and code.

pub mod code_indexer;
pub mod dialect;
pub mod indexer;
pub mod map;
pub mod outline;
pub mod service;
pub mod symbol_indexer;
pub mod symbol_query;
