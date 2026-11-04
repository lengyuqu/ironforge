//! Forward-compatibility helpers of the collaboration contract.
//!
//! Policy (design: `agent-collaboration-p2-design-2026-11.md` §6, R14):
//!
//! * Responses may gain optional fields and enum values in a minor version;
//!   clients ignore unknown response fields and unknown *non-critical* enum
//!   values.
//! * An unknown value of an enum that decides **authorization or a terminal
//!   state** is never guessed: it is `unknown` and handled fail-closed (deny,
//!   "not done", "not satisfied"), never as success.
//! * Write requests reject unrecognized fields (`deny_unknown_fields`) and
//!   unrecognized enum values, so a typo cannot silently change meaning.
//!
//! [`Known`] is the one place that rule is spelled out for enum-valued strings.

use serde::{Serialize, Serializer};

/// An enum carried as a string on the wire.
pub trait ContractEnum: Sized + Copy {
    /// Field-independent name used in messages (`task status`).
    const NAME: &'static str;
    /// Every value this build understands, in documentation order.
    const VALUES: &'static [&'static str];

    fn from_wire(raw: &str) -> Option<Self>;
    fn as_wire(self) -> &'static str;
}

/// A wire value that is either understood or kept verbatim as unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Known<T> {
    Known(T),
    Unknown(String),
}

/// An unrecognized value of a safety-critical enum, ready to be reported as a
/// stable machine-readable error (`UNKNOWN_ENUM_VALUE`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnknownValue {
    pub field: String,
    pub value: String,
    pub allowed: Vec<&'static str>,
}

impl std::fmt::Display for UnknownValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} has an unrecognized value; allowed: {}",
            self.field,
            self.allowed.join(", ")
        )
    }
}

impl std::error::Error for UnknownValue {}

impl<T: ContractEnum> Known<T> {
    pub fn parse(raw: &str) -> Self {
        match T::from_wire(raw) {
            Some(known) => Self::Known(known),
            None => Self::Unknown(raw.to_string()),
        }
    }

    pub fn known(&self) -> Option<T> {
        match self {
            Self::Known(value) => Some(*value),
            Self::Unknown(_) => None,
        }
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown(_))
    }

    /// Fail closed: only a recognized value passes.
    pub fn require(self, field: &str) -> Result<T, UnknownValue> {
        match self {
            Self::Known(value) => Ok(value),
            Self::Unknown(value) => Err(UnknownValue {
                field: field.to_string(),
                value,
                allowed: T::VALUES.to_vec(),
            }),
        }
    }
}

impl<T: ContractEnum> Serialize for Known<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Known(value) => s.serialize_str(value.as_wire()),
            Self::Unknown(_) => s.serialize_str("unknown"),
        }
    }
}

macro_rules! contract_enum {
    ($(#[$meta:meta])* $name:ident, $label:literal, { $($variant:ident => $wire:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($variant),+ }

        impl ContractEnum for $name {
            const NAME: &'static str = $label;
            const VALUES: &'static [&'static str] = &[$($wire),+];

            fn from_wire(raw: &str) -> Option<Self> {
                match raw { $($wire => Some(Self::$variant),)+ _ => None }
            }

            fn as_wire(self) -> &'static str {
                match self { $(Self::$variant => $wire),+ }
            }
        }
    };
}

contract_enum!(
    /// Task status. `Done` is terminal; an unknown status is never treated as it.
    TaskStatus, "task status", {
        Open => "open",
        InProgress => "in_progress",
        Waiting => "waiting",
        Blocked => "blocked",
        Done => "done",
    }
);

contract_enum!(
    /// Question status. Only `Open` accepts an answer; the others are terminal.
    QuestionStatus, "question status", {
        Open => "open",
        Answered => "answered",
        Cancelled => "cancelled",
    }
);

contract_enum!(
    /// Who may answer a question. Decides authorization.
    RecipientKind, "recipient kind", {
        RepoWriters => "repo_writers",
        User => "user",
        Session => "session",
    }
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values_round_trip() {
        for raw in TaskStatus::VALUES {
            let status = Known::<TaskStatus>::parse(raw).require("status").unwrap();
            assert_eq!(status.as_wire(), *raw);
        }
    }

    #[test]
    fn unknown_value_fails_closed_and_reports_allowed_set() {
        let err = Known::<RecipientKind>::parse("team")
            .require("recipient.kind")
            .unwrap_err();
        assert_eq!(err.value, "team");
        assert_eq!(err.allowed, vec!["repo_writers", "user", "session"]);
        assert!(Known::<RecipientKind>::parse("team").is_unknown());
        assert_eq!(Known::<RecipientKind>::parse("team").known(), None);
    }

    #[test]
    fn unknown_serializes_as_the_literal_unknown() {
        let v = serde_json::to_value(Known::<QuestionStatus>::parse("archived")).unwrap();
        assert_eq!(v, "unknown");
        let v = serde_json::to_value(Known::<QuestionStatus>::parse("open")).unwrap();
        assert_eq!(v, "open");
    }

    #[test]
    fn enums_agree_with_the_constants_the_services_validate_against() {
        assert_eq!(TaskStatus::VALUES, crate::collaboration::task::STATUSES);
        assert_eq!(
            RecipientKind::VALUES,
            [
                crate::collaboration::question::RECIPIENT_REPO_WRITERS,
                crate::collaboration::question::RECIPIENT_USER,
                crate::collaboration::question::RECIPIENT_SESSION,
            ]
        );
    }
}
