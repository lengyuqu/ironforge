//! Unified API pagination support.
//!
//! All list endpoints accept `page` and `per_page` query parameters.
//! Response wraps data in `PaginatedResponse { data, pagination }`.

use axum::extract::Query;
use serde::{Deserialize, Serialize};

/// Default number of items per page.
const DEFAULT_PER_PAGE: u64 = 20;
/// Maximum number of items per page.
const MAX_PER_PAGE: u64 = 100;

/// Query parameters for pagination.
#[derive(Debug, Clone, Deserialize)]
pub struct PaginationParams {
    /// Page number (1-based). Default: 1
    #[serde(default = "default_page")]
    pub page: u64,
    /// Items per page. Default: 20, Max: 100
    #[serde(default = "default_per_page")]
    pub per_page: u64,
}

fn default_page() -> u64 {
    1
}

fn default_per_page() -> u64 {
    DEFAULT_PER_PAGE
}

impl PaginationParams {
    /// Create a new pagination params with defaults.
    pub fn new(page: u64, per_page: u64) -> Self {
        Self {
            page: page.max(1),
            per_page: per_page.clamp(1, MAX_PER_PAGE),
        }
    }

    /// Get the offset (0-based) for database queries.
    pub fn offset(&self) -> u64 {
        (self.page - 1) * self.per_page
    }

    /// Get the limit for database queries.
    pub fn limit(&self) -> u64 {
        self.per_page.min(MAX_PER_PAGE)
    }

    /// Clamp per_page to valid range.
    pub fn clamp(&self) -> Self {
        Self::new(self.page, self.per_page)
    }
}

/// Pagination metadata in response.
#[derive(Debug, Clone, Serialize)]
pub struct PaginationMeta {
    /// Current page number (1-based).
    pub page: u64,
    /// Items per page.
    pub per_page: u64,
    /// Total number of items.
    pub total: u64,
    /// Total number of pages.
    pub total_pages: u64,
    /// Whether there is a next page.
    pub has_next: bool,
    /// Whether there is a previous page.
    pub has_prev: bool,
}

impl PaginationMeta {
    /// Create pagination metadata from params and total count.
    pub fn from_params(params: &PaginationParams, total: u64) -> Self {
        let total_pages = if total == 0 {
            1
        } else {
            (total + params.per_page - 1) / params.per_page
        };

        Self {
            page: params.page,
            per_page: params.per_page,
            total,
            total_pages,
            has_next: params.page < total_pages,
            has_prev: params.page > 1,
        }
    }
}

/// Paginated response wrapper.
#[derive(Debug, Clone, Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub data: Vec<T>,
    pub pagination: PaginationMeta,
}

impl<T: Serialize> PaginatedResponse<T> {
    /// Create a new paginated response.
    pub fn new(data: Vec<T>, params: &PaginationParams, total: u64) -> Self {
        Self {
            data,
            pagination: PaginationMeta::from_params(params, total),
        }
    }
}
