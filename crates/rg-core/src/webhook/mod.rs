//! Webhook service — push/PR/issue event triggers and delivery.
//!
//! `service` logs and delivers; `outbox` is the fallback that re-delivers events
//! logged but never handed to delivery (#15, step 7).
pub mod outbox;
pub mod service;
