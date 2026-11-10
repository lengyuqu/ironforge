//! Password hashing and verification using Argon2id.
//!
//! Also includes a [`PasswordValidator`] for password strength checks
//! (Phase 22-D security hardening).

use crate::error::{CoreError, CoreResult};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand_core::OsRng;

/// Hash a plaintext password. Returns a PHC-format string (includes algorithm, params, salt, hash).
pub fn hash_password(password: &str) -> CoreResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| CoreError::internal(format!("password hashing failed: {}", e)))?;
    Ok(hash.to_string())
}

/// Verify a plaintext password against a stored PHC hash.
pub fn verify_password(password: &str, hash: &str) -> CoreResult<bool> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| CoreError::internal(format!("invalid password hash: {}", e)))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

// ── Password Strength Validation (Phase 22-D) ──────────────────────────────

/// Password validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordError {
    TooShort { min: usize, got: usize },
    TooLong { max: usize, got: usize },
    NoUppercase,
    NoLowercase,
    NoDigit,
    NoSpecialChar,
    ContainsWhitespace,
    /// The password, once its leading and trailing non-letter characters are
    /// stripped, is exactly one of [`COMMON_PASSWORDS`].
    ///
    /// Deliberately carries **no payload**: this error is rendered verbatim
    /// into the `POST /users/register` 400 response body, so keeping the
    /// plaintext here would echo the submitted password back to the caller
    /// (and to anything that logs the response).
    TooCommon,
    ContainsUsername,
}

impl std::fmt::Display for PasswordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort { min, got } => write!(
                f,
                "password must be at least {} characters (got {})",
                min, got
            ),
            Self::TooLong { max, got } => write!(
                f,
                "password must be at most {} characters (got {})",
                max, got
            ),
            Self::NoUppercase => write!(f, "password must contain at least one uppercase letter"),
            Self::NoLowercase => write!(f, "password must contain at least one lowercase letter"),
            Self::NoDigit => write!(f, "password must contain at least one digit"),
            Self::NoSpecialChar => write!(
                f,
                "password must contain at least one special character (!@#$%^&*...)"
            ),
            Self::ContainsWhitespace => write!(f, "password must not contain whitespace"),
            Self::TooCommon => write!(
                f,
                "password is too common — please choose a stronger one"
            ),
            Self::ContainsUsername => write!(f, "password must not contain the username"),
        }
    }
}

impl std::error::Error for PasswordError {}

/// Common/weak passwords to reject. Lowercase for case-insensitive matching.
/// Top 50 most common passwords from various breach analyses.
const COMMON_PASSWORDS: &[&str] = &[
    "password",
    "123456",
    "12345678",
    "qwerty",
    "abc123",
    "monkey",
    "1234567",
    "letmein",
    "trustno1",
    "dragon",
    "baseball",
    "iloveyou",
    "master",
    "sunshine",
    "ashley",
    "michael",
    "shadow",
    "123123",
    "654321",
    "superman",
    "qazwsx",
    "football",
    "password1",
    "password123",
    "welcome",
    "hello",
    "charlie",
    "donald",
    "admin",
    "administrator",
    "root",
    "toor",
    "pass",
    "test",
    "guest",
    "info",
    "mysql",
    "user",
    "ftp",
    "pi",
    "puppet",
    "ansible",
    "ec2-user",
    "vagrant",
    "ubuntu",
    "admin123",
    "root123",
    "test123",
];

/// Password validator with configurable rules.
#[derive(Debug, Clone)]
pub struct PasswordValidator {
    pub min_length: usize,
    pub max_length: usize,
    pub require_uppercase: bool,
    pub require_lowercase: bool,
    pub require_digit: bool,
    pub require_special: bool,
    pub reject_common: bool,
    pub reject_username: bool,
}

impl Default for PasswordValidator {
    fn default() -> Self {
        Self {
            min_length: 8,
            max_length: 128,
            require_uppercase: true,
            require_lowercase: true,
            require_digit: true,
            require_special: true,
            reject_common: true,
            reject_username: true,
        }
    }
}

impl PasswordValidator {
    /// Create a standard-strength validator (8+ chars, mixed case, digit, special).
    pub fn standard() -> Self {
        Self::default()
    }

    /// Create a strict validator (12+ chars, all requirements).
    pub fn strict() -> Self {
        Self {
            min_length: 12,
            ..Self::default()
        }
    }

    /// Create a lenient validator (only minimum length, no other rules).
    pub fn lenient() -> Self {
        Self {
            min_length: 6,
            require_uppercase: false,
            require_lowercase: false,
            require_digit: false,
            require_special: false,
            reject_common: false,
            reject_username: false,
            ..Self::default()
        }
    }

    /// Validate a password. Returns Ok(()) on success, Err on failure.
    pub fn validate(&self, password: &str) -> Result<(), PasswordError> {
        // Length checks — `len()` counts BYTES: 4 CJK chars (12 B) only just clear min_length 12, 43 (129 B) overshoot max_length 128; Display says "characters"
        if password.len() < self.min_length {
            return Err(PasswordError::TooShort {
                min: self.min_length,
                got: password.len(),
            });
        }
        if password.len() > self.max_length {
            return Err(PasswordError::TooLong {
                max: self.max_length,
                got: password.len(),
            });
        }

        // Whitespace check
        if password.chars().any(|c| c.is_whitespace()) {
            return Err(PasswordError::ContainsWhitespace);
        }

        // Character class checks
        if self.require_uppercase && !password.chars().any(|c| c.is_ascii_uppercase()) {
            return Err(PasswordError::NoUppercase);
        }
        if self.require_lowercase && !password.chars().any(|c| c.is_ascii_lowercase()) {
            return Err(PasswordError::NoLowercase);
        }
        if self.require_digit && !password.chars().any(|c| c.is_ascii_digit()) {
            return Err(PasswordError::NoDigit);
        }
        if self.require_special {
            // Common special characters
            const SPECIAL: &str = "!@#$%^&*()_+-=[]{}|;:,.<>?/~`'\"\\";
            if !password.chars().any(|c| SPECIAL.contains(c)) {
                return Err(PasswordError::NoSpecialChar);
            }
        }

        // Common password check. The candidate is the password's first letter
        // run, with any leading digits/symbols skipped first — `1Password1!`
        // and `!Password` must be compared as `password`, not as "" .
        //
        // Two tempting alternatives are both wrong here:
        //   * stripping *both* ends (`trim_matches`) turns `Welcome1!x` from a
        //     hit into a pass, because the trailing `x` stops the trim and the
        //     interior `1!` is never removed — a silent coverage regression;
        //   * taking the *longest* letter run rejects `Str0ng!Pass` by matching
        //     its trailing `pass`, which is a new false positive.
        // Skipping the prefix and keeping the first run changes exactly one
        // thing versus the old code: the run is no longer forced to start at
        // byte 0. Nothing that matched before stops matching.
        if self.reject_common {
            let lower = password.to_lowercase();
            let core: String = lower
                .trim_start_matches(|c: char| !c.is_ascii_alphabetic())
                .chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect();
            if !core.is_empty() && COMMON_PASSWORDS.contains(&core.as_str()) {
                return Err(PasswordError::TooCommon);
            }
            // The entries that are not pure letters ("abc123", "password1",
            // "ec2-user", and the all-digit ones) can never equal a letter-only
            // candidate, so they still need the whole-string branch.
            if COMMON_PASSWORDS.contains(&lower.as_str()) {
                return Err(PasswordError::TooCommon);
            }
        }

        Ok(())
    }

    /// Validate a password against a username (must not contain the username).
    pub fn validate_with_username(
        &self,
        password: &str,
        username: &str,
    ) -> Result<(), PasswordError> {
        self.validate(password)?;
        if self.reject_username
            && !username.is_empty()
            && password.to_lowercase().contains(&username.to_lowercase())
        {
            return Err(PasswordError::ContainsUsername);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify() {
        let hash = hash_password("hunter2").unwrap();
        assert!(verify_password("hunter2", &hash).unwrap());
        assert!(!verify_password("wrong", &hash).unwrap());
    }

    #[test]
    fn test_password_validator_default() {
        let v = PasswordValidator::default();
        assert!(v.validate("Str0ng!Pass").is_ok());
        assert!(v.validate("weak").is_err());
        assert!(v.validate("alllowercase1!").is_err()); // no uppercase
        assert!(v.validate("ALLUPPERCASE1!").is_err()); // no lowercase
        assert!(v.validate("NoDigits!Pass").is_err()); // no digit
        assert!(v.validate("NoSpecial1Pass").is_err()); // no special
    }

    #[test]
    fn test_password_validator_too_common() {
        let v = PasswordValidator::default();
        assert!(v.validate("Good!Pass1").is_ok());
        // Common passwords with proper char classes should still be rejected
        assert!(matches!(
            v.validate("Password1!"),
            Err(PasswordError::TooCommon)
        ));
        assert!(matches!(
            v.validate("Admin123!"),
            Err(PasswordError::TooCommon)
        ));
        assert!(matches!(
            v.validate("Qwerty1!"),
            Err(PasswordError::TooCommon)
        ));
        assert!(matches!(
            v.validate("Welcome1!"),
            Err(PasswordError::TooCommon)
        ));
        assert!(matches!(
            v.validate("Test123!"),
            Err(PasswordError::TooCommon)
        ));
    }

    /// Regression: the check used to take only the *leading* alphabetic run, so
    /// any password starting with a digit or a symbol skipped the lookup
    /// entirely while still passing every character-class rule.
    ///
    /// These inputs were confirmed to register successfully over HTTP before
    /// the fix (`POST /api/v1/users/register` → 201); they must now be rejected.
    #[test]
    fn test_password_common_word_not_bypassed_by_prefix_decoration() {
        let v = PasswordValidator::default();
        for pwd in [
            "1Password1!",  // leading digit
            "!Password1",   // leading symbol
            "0Admin123!",   // leading digit
            "1Qwerty1!",    // leading digit
            "_Welcome1!",   // leading underscore
            "Admin@123",    // trailing symbol+digits
            // Boundary case from the review: a listed word up front, then a
            // suffix that ends in a letter. Stripping both ends (or the whole
            // interior) would reduce this to `welcome1!x` / `welcomex` and let
            // it through — a regression against the pre-fix behaviour, which
            // caught it via the leading run.
            "Welcome1!x",
            "1Welcome1!x",  // same, with the prefix hole also closed
        ] {
            assert!(
                matches!(v.validate(pwd), Err(PasswordError::TooCommon)),
                "{pwd:?} must be rejected as too common"
            );
        }
    }

    /// Regression: the candidate is the *first* letter run, so interior digits
    /// and symbols must not be eaten. A "longest letter run" scan would reject
    /// `Str0ng!Pass` by matching its trailing `pass`.
    #[test]
    fn test_password_common_word_check_leaves_interior_intact() {
        let v = PasswordValidator::default();
        for pwd in ["Str0ng!Pass", "Good!Pass1", "MyPassw0rd!2026", "Tr0ub4dor&3"] {
            assert!(
                v.validate(pwd).is_ok(),
                "{pwd:?} must stay accepted — its digits/symbols are interior or trailing"
            );
        }
    }

    /// The error is rendered verbatim into the register endpoint's 400 body, so
    /// it must never carry the submitted password.
    #[test]
    fn test_too_common_display_does_not_echo_password() {
        let v = PasswordValidator::default();
        let submitted = "1Password1!";
        let err = v.validate(submitted).unwrap_err();
        let rendered = err.to_string();
        assert!(
            !rendered.contains(submitted) && !rendered.contains("Password1!"),
            "Display must not echo the submitted password, got: {rendered}"
        );
        assert!(
            rendered.contains("too common"),
            "message should still say why, got: {rendered}"
        );
    }

    #[test]
    fn test_password_validator_username_check() {
        let v = PasswordValidator::default();
        assert!(v.validate_with_username("Str0ng!Pass", "alice").is_ok());
        assert!(matches!(
            v.validate_with_username("Alice!123", "alice"),
            Err(PasswordError::ContainsUsername)
        ));
    }

    #[test]
    fn test_password_validator_strict() {
        let v = PasswordValidator::strict();
        assert!(v.validate("VeryStr0ng!Pass").is_ok());
        assert!(matches!(
            v.validate("Str0ng!Pass"),
            Err(PasswordError::TooShort { min: 12, .. })
        ));
    }

    #[test]
    fn test_password_validator_lenient() {
        let v = PasswordValidator::lenient();
        assert!(v.validate("simple").is_ok());
        assert!(v.validate("weak").is_err()); // too short
    }

    #[test]
    fn test_password_validator_whitespace() {
        let v = PasswordValidator::default();
        assert!(matches!(
            v.validate("Str0ng! Pass"),
            Err(PasswordError::ContainsWhitespace)
        ));
        assert!(matches!(
            v.validate("Str0ng\tPass"),
            Err(PasswordError::ContainsWhitespace)
        ));
    }

    #[test]
    fn test_password_validator_length_limits() {
        let v = PasswordValidator::default();
        let long = "A".repeat(200);
        assert!(matches!(
            v.validate(&long),
            Err(PasswordError::TooLong { .. })
        ));
    }
}
