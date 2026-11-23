//! Mirror service — sync and manage repository mirrors from remote sources.
/// Push mirror: forward this repository's refs to a remote host.
pub mod push_service;
/// Pull mirror: keep a copy of a remote repository here.
pub mod service;
