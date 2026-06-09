//! Prometheus metrics endpoint for IronForge.
//!
//! Provides `/metrics` endpoint returning metrics in Prometheus text format.
//! Uses the `prometheus` crate (default-features = false to avoid OpenSSL).

use std::sync::OnceLock;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use prometheus::{Registry, TextEncoder};

use crate::AppState;

/// Global Prometheus registry (lazy-initialized).
pub static REGISTRY: OnceLock<Registry> = OnceLock::new();

/// HTTP request metrics.
pub mod http_requests {
    use prometheus::{Histogram, HistogramOpts, IntCounter, IntCounterVec, IntGauge, Opts, Registry};
    use std::sync::OnceLock;

    /// Counter: total HTTP requests by method, route, status.
    pub static REQUEST_COUNT: OnceLock<IntCounterVec> = OnceLock::new();

    /// Histogram: request duration (seconds).
    pub static REQUEST_DURATION: OnceLock<Histogram> = OnceLock::new();

    /// Gauge: current in-flight requests.
    pub static IN_FLIGHT: OnceLock<IntGauge> = OnceLock::new();

    /// Register all HTTP request metrics with the registry.
    pub fn register(registry: &Registry) -> Result<(), prometheus::Error> {
        let request_count = IntCounterVec::new(
            prometheus::Opts::new("http_requests_total", "Total HTTP requests"),
            &["method", "route", "status"],
        )?;
        REQUEST_COUNT.set(request_count.clone()).map_err(|_| prometheus::Error::Msg("REQUEST_COUNT already set".into()))?;
        registry.register(Box::new(request_count))?;

        let request_duration = Histogram::with_opts(
            HistogramOpts::new("http_request_duration_seconds", "HTTP request duration in seconds")
                .buckets(vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
        )?;
        REQUEST_DURATION.set(request_duration.clone()).map_err(|_| prometheus::Error::Msg("REQUEST_DURATION already set".into()))?;
        registry.register(Box::new(request_duration))?;

        let in_flight = IntGauge::with_opts(Opts::new("http_requests_in_flight", "Current in-flight HTTP requests"))?;
        IN_FLIGHT.set(in_flight.clone()).map_err(|_| prometheus::Error::Msg("IN_FLIGHT already set".into()))?;
        registry.register(Box::new(in_flight))?;

        Ok(())
    }
}

/// Database metrics.
pub mod db {
    use prometheus::{Histogram, HistogramOpts, IntCounter, IntCounterVec, Opts, Registry};
    use std::sync::OnceLock;

    /// Counter: total database queries by operation.
    pub static QUERY_COUNT: OnceLock<IntCounterVec> = OnceLock::new();

    /// Histogram: database query duration (seconds).
    pub static QUERY_DURATION: OnceLock<Histogram> = OnceLock::new();

    /// Register all database metrics with the registry.
    pub fn register(registry: &Registry) -> Result<(), prometheus::Error> {
        let query_count = IntCounterVec::new(
            prometheus::Opts::new("db_queries_total", "Total database queries"),
            &["operation"],
        )?;
        QUERY_COUNT.set(query_count.clone()).map_err(|_| prometheus::Error::Msg("QUERY_COUNT already set".into()))?;
        registry.register(Box::new(query_count))?;

        let query_duration = Histogram::with_opts(
            HistogramOpts::new("db_query_duration_seconds", "Database query duration in seconds")
                .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0]),
        )?;
        QUERY_DURATION.set(query_duration.clone()).map_err(|_| prometheus::Error::Msg("QUERY_DURATION already set".into()))?;
        registry.register(Box::new(query_duration))?;

        Ok(())
    }
}

/// Git operation metrics.
pub mod git {
    use prometheus::{Histogram, HistogramOpts, IntCounter, IntCounterVec, Opts, Registry};
    use std::sync::OnceLock;

    /// Counter: total Git operations by type (clone, push, pull).
    pub static OPERATION_COUNT: OnceLock<IntCounterVec> = OnceLock::new();

    /// Histogram: Git operation duration (seconds).
    pub static OPERATION_DURATION: OnceLock<Histogram> = OnceLock::new();

    /// Register all Git metrics with the registry.
    pub fn register(registry: &Registry) -> Result<(), prometheus::Error> {
        let operation_count = IntCounterVec::new(
            prometheus::Opts::new("git_operations_total", "Total Git operations"),
            &["operation"],
        )?;
        OPERATION_COUNT.set(operation_count.clone()).map_err(|_| prometheus::Error::Msg("OPERATION_COUNT already set".into()))?;
        registry.register(Box::new(operation_count))?;

        let operation_duration = Histogram::with_opts(
            HistogramOpts::new("git_operation_duration_seconds", "Git operation duration in seconds")
                .buckets(vec![0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 300.0]),
        )?;
        OPERATION_DURATION.set(operation_duration.clone()).map_err(|_| prometheus::Error::Msg("OPERATION_DURATION already set".into()))?;
        registry.register(Box::new(operation_duration))?;

        Ok(())
    }
}

/// CI/CD metrics.
pub mod ci {
    use prometheus::{IntCounterVec, IntGauge, Opts, Registry};
    use std::sync::OnceLock;

    /// Counter: total CI pipelines by status.
    pub static PIPELINE_COUNT: OnceLock<IntCounterVec> = OnceLock::new();

    /// Counter: total CI jobs by status.
    pub static JOB_COUNT: OnceLock<IntCounterVec> = OnceLock::new();

    /// Gauge: current running jobs.
    pub static JOBS_RUNNING: OnceLock<IntGauge> = OnceLock::new();

    /// Register all CI metrics with the registry.
    pub fn register(registry: &Registry) -> Result<(), prometheus::Error> {
        let pipeline_count = IntCounterVec::new(
            prometheus::Opts::new("ci_pipelines_total", "Total CI pipelines"),
            &["status"],
        )?;
        PIPELINE_COUNT.set(pipeline_count.clone()).map_err(|_| prometheus::Error::Msg("PIPELINE_COUNT already set".into()))?;
        registry.register(Box::new(pipeline_count))?;

        let job_count = IntCounterVec::new(
            prometheus::Opts::new("ci_jobs_total", "Total CI jobs"),
            &["status"],
        )?;
        JOB_COUNT.set(job_count.clone()).map_err(|_| prometheus::Error::Msg("JOB_COUNT already set".into()))?;
        registry.register(Box::new(job_count))?;

        let jobs_running = IntGauge::with_opts(Opts::new("ci_jobs_running", "Current running CI jobs"))?;
        JOBS_RUNNING.set(jobs_running.clone()).map_err(|_| prometheus::Error::Msg("JOBS_RUNNING already set".into()))?;
        registry.register(Box::new(jobs_running))?;

        Ok(())
    }
}

/// Initialize the global Prometheus registry.
/// Call this once at server startup.
pub fn init_registry() -> Result<(), prometheus::Error> {
    let registry = Registry::new();

    // Register all metric groups
    http_requests::register(&registry)?;
    db::register(&registry)?;
    git::register(&registry)?;
    ci::register(&registry)?;

    REGISTRY.set(registry).map_err(|_| prometheus::Error::Msg("Registry already initialized".into()))?;

    Ok(())
}

/// GET /metrics — return Prometheus-formatted metrics.
pub async fn metrics_handler() -> impl IntoResponse {
    let registry = REGISTRY.get().expect("Metrics registry not initialized");

    let encoder = TextEncoder::new();
    let metric_families = registry.gather();

    match encoder.encode_to_string(&metric_families) {
        Ok(body) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.1.0; charset=utf-8")],
            body,
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(axum::http::header::CONTENT_TYPE, "text/plain")],
            format!("Error encoding metrics: {e}"),
        )
            .into_response(),
    }
}
