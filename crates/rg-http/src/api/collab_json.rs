//! Request-body extractor of the collaboration write endpoints.
//!
//! Contract (design R14, acceptance M10): a write request containing a field the
//! server does not recognize is **rejected**, never silently ignored — a typo
//! such as `expected_versoin` must not turn into an unguarded write. Every
//! malformed body therefore answers `400` with a stable code instead of axum's
//! default plain-text `422`:
//!
//! | code | meaning |
//! |---|---|
//! | `UNKNOWN_FIELD` | a field the endpoint does not define (`details.field`) |
//! | `INVALID_JSON` | not syntactically valid JSON |
//! | `INVALID_REQUEST_BODY` | wrong type, missing required field, bad value |
//! | `UNKNOWN_ENUM_VALUE` | an unrecognized value of a safety-critical enum |

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use rg_core::collaboration::compat::{ContractEnum, Known, UnknownValue};
use serde::de::DeserializeOwned;

use crate::error::AppError;

/// `axum::Json` whose rejections are clean, coded 400s.
pub struct CollabJson<T>(pub T);

impl<S, T> FromRequest<S> for CollabJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(reject(rejection)),
        }
    }
}

fn reject(rejection: JsonRejection) -> AppError {
    match rejection {
        JsonRejection::JsonDataError(e) => {
            let text = e.body_text();
            match unknown_field(&text) {
                Some(field) => AppError::coded(
                    StatusCode::BAD_REQUEST,
                    "UNKNOWN_FIELD",
                    format!("unrecognized field `{field}`; the request was not applied"),
                )
                .with_details(serde_json::json!({ "field": field })),
                None => AppError::coded(
                    StatusCode::BAD_REQUEST,
                    "INVALID_REQUEST_BODY",
                    clean_message(&text),
                ),
            }
        }
        JsonRejection::JsonSyntaxError(_) => AppError::coded(
            StatusCode::BAD_REQUEST,
            "INVALID_JSON",
            "request body is not valid JSON",
        ),
        // Missing content type (415), oversized body (413), ...: keep the status.
        other => AppError::coded(
            other.status(),
            "INVALID_REQUEST_BODY",
            clean_message(&other.body_text()),
        ),
    }
}

/// `unknown field `x`, expected one of ...` → `x`.
fn unknown_field(text: &str) -> Option<String> {
    let rest = &text[text.find("unknown field `")? + "unknown field `".len()..];
    Some(rest[..rest.find('`')?].to_string())
}

/// Drop axum's boilerplate prefix and serde's `at line N column M` suffix.
fn clean_message(text: &str) -> String {
    let text = text
        .strip_prefix("Failed to deserialize the JSON body into the target type: ")
        .unwrap_or(text);
    let text = text.split(" at line ").next().unwrap_or(text);
    text.trim().to_string()
}

/// Fail closed on an enum-valued input: `None` passes, a recognized value
/// passes, anything else is a coded `UNKNOWN_ENUM_VALUE` 400.
pub(crate) fn require_known<T: ContractEnum>(
    field: &str,
    raw: Option<&str>,
) -> Result<(), AppError> {
    if let Some(raw) = raw {
        Known::<T>::parse(raw).require(field)?;
    }
    Ok(())
}

/// An unrecognized safety-critical enum value is a coded 400 listing the
/// allowed set; the value is echoed (it is the caller's own input).
impl From<UnknownValue> for AppError {
    fn from(e: UnknownValue) -> Self {
        AppError::coded(StatusCode::BAD_REQUEST, "UNKNOWN_ENUM_VALUE", e.to_string()).with_details(
            serde_json::json!({ "field": e.field, "value": e.value, "allowed": e.allowed }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_field_name() {
        assert_eq!(
            unknown_field("Failed to deserialize the JSON body into the target type: unknown field `foo`, expected one of `a`, `b` at line 1 column 8"),
            Some("foo".to_string())
        );
        assert_eq!(
            unknown_field("missing field `title` at line 1 column 2"),
            None
        );
    }

    #[test]
    fn message_is_stripped_of_boilerplate() {
        assert_eq!(
            clean_message("Failed to deserialize the JSON body into the target type: missing field `title` at line 1 column 2"),
            "missing field `title`"
        );
    }
}
