//! Parsing for the `$GITHUB_OUTPUT` file protocol.
//!
//! A step writes `name=value` lines into a file the runner points at; the runner
//! reads them back and they become `steps.<id>.outputs.<name>` for later steps
//! and `needs.<job>.outputs.<name>` for later jobs.
//!
//! # The two formats
//!
//! ```text
//! single=one line, and '=' is allowed after the first
//! multi<<DELIMITER
//! first line
//! second line
//! DELIMITER
//! ```
//!
//! Both are supported because both are common: multi-line output is what a
//! workflow uses for a commit message, a JSON document, or `git log`. A runner
//! that only understood the single-line form would silently produce truncated
//! values for roughly half of real workflows, and truncated JSON parses about
//! as well as no JSON.
//!
//! # What is deliberately strict
//!
//! * A line with no `=` and no `<<` is an **error**, not silence. GitHub treats
//!   a malformed line as a failed step; silently dropping it would leave a
//!   workflow that "worked" with a missing output, and the failure would surface
//!   two jobs later with no clue where it came from.
//! * A heredoc whose delimiter never appears is an error. Accepting the partial
//!   value would be the worst outcome: it looks like real data.
//! * The delimiter must match **exactly** — no trimming. GitHub compares the
//!   whole line, and trimming would accept a value that GitHub would reject,
//!   producing different behaviour on the two platforms for the same workflow.

use std::collections::HashMap;

/// Parse a `$GITHUB_OUTPUT` file's contents.
pub fn parse_github_output(content: &str) -> Result<HashMap<String, String>, String> {
    let mut values = HashMap::new();
    // `split('\n')` keeps a trailing empty line for a trailing newline; that is
    // harmless because an empty line outside a heredoc is skipped, but it also
    // means a heredoc terminating at EOF without a final newline still works.
    let mut lines = content.split('\n').enumerate().peekable();

    while let Some((index, raw)) = lines.next() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.is_empty() {
            continue;
        }
        let line_number = index + 1;

        if let Some((name, rest)) = line.split_once("<<") {
            // Heredoc form. `name` must be non-empty and `rest` is the
            // delimiter; both are used verbatim.
            let name = name.trim();
            if name.is_empty() {
                return Err(format!("line {line_number}: empty name in heredoc output"));
            }
            let delimiter = rest;
            if delimiter.is_empty() {
                return Err(format!(
                    "line {line_number}: heredoc for '{name}' has an empty delimiter"
                ));
            }

            let mut body = String::new();
            let mut closed = false;
            for (_, next_raw) in lines.by_ref() {
                let next = next_raw.strip_suffix('\r').unwrap_or(next_raw);
                if next == delimiter {
                    closed = true;
                    break;
                }
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(next);
            }
            if !closed {
                return Err(format!(
                    "line {line_number}: heredoc for '{name}' is missing its closing '{delimiter}'"
                ));
            }
            values.insert(name.to_string(), body);
            continue;
        }

        let Some((name, value)) = line.split_once('=') else {
            return Err(format!(
                "line {line_number}: expected 'name=value' or 'name<<DELIMITER', got {line:?}"
            ));
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(format!("line {line_number}: empty output name"));
        }
        // Only the first `=` separates; everything after it is the value,
        // untrimmed — a value that legitimately starts or ends with a space
        // must survive.
        values.insert(name.to_string(), value.to_string());
    }

    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(content: &str) -> HashMap<String, String> {
        parse_github_output(content).expect("valid output file")
    }

    #[test]
    fn single_line_values_keep_everything_after_the_first_equals() {
        let values = parse("version=1.2.3\nquery=a=b&c=d\n");
        assert_eq!(values["version"], "1.2.3");
        // A signature or a query string is full of `=`.
        assert_eq!(values["query"], "a=b&c=d");
    }

    #[test]
    fn a_value_that_starts_or_ends_with_spaces_is_preserved() {
        let values = parse("padded=  keep me  \n");
        assert_eq!(values["padded"], "  keep me  ");
    }

    #[test]
    fn heredoc_values_keep_their_newlines() {
        let content = "changelog<<EOF\n- first\n- second\n\n- fourth\nEOF\n";
        let values = parse(content);
        assert_eq!(values["changelog"], "- first\n- second\n\n- fourth");
    }

    #[test]
    fn single_line_and_heredoc_values_mix() {
        let content = "sha=abc123\nbody<<END\nline one\nline two\nEND\nnext=1\n";
        let values = parse(content);
        assert_eq!(values["sha"], "abc123");
        assert_eq!(values["body"], "line one\nline two");
        assert_eq!(values["next"], "1");
    }

    /// Content inside a heredoc is data: a `name=value`-looking line must not be
    /// interpreted as an output.
    #[test]
    fn a_heredoc_body_is_not_reparsed() {
        let content = "config<<EOF\nsecret=not-an-output\nEOF\n";
        let values = parse(content);
        assert_eq!(values.len(), 1);
        assert_eq!(values["config"], "secret=not-an-output");
        assert!(!values.contains_key("secret"));
    }

    #[test]
    fn an_empty_heredoc_is_an_empty_string_not_a_missing_output() {
        let values = parse("empty<<EOF\nEOF\n");
        assert_eq!(values.get("empty").map(String::as_str), Some(""));
    }

    #[test]
    fn a_missing_delimiter_is_an_error() {
        let error = parse_github_output("body<<EOF\nunterminated\n").unwrap_err();
        assert!(error.contains("missing its closing"), "{error}");
    }

    #[test]
    fn a_malformed_line_is_an_error_not_a_silent_skip() {
        let error = parse_github_output("just-a-word\n").unwrap_err();
        assert!(error.contains("expected"), "{error}");
        let error = parse_github_output("=no-name\n").unwrap_err();
        assert!(error.contains("empty output name"), "{error}");
    }

    /// GitHub compares the delimiter to the whole line. Trimming would accept
    /// what GitHub rejects, and the same workflow would behave differently on
    /// the two platforms.
    #[test]
    fn the_delimiter_must_match_exactly() {
        let error = parse_github_output("body<<EOF\nline\n  EOF\n").unwrap_err();
        assert!(error.contains("missing its closing"), "{error}");
    }

    #[test]
    fn crlf_line_endings_are_tolerated() {
        let values = parse("a=1\r\nb<<EOF\r\nx\r\ny\r\nEOF\r\n");
        assert_eq!(values["a"], "1");
        assert_eq!(values["b"], "x\ny");
    }

    #[test]
    fn a_later_assignment_overwrites_an_earlier_one() {
        // GitHub's behaviour: last write wins within one file.
        let values = parse("k=first\nk=second\n");
        assert_eq!(values["k"], "second");
    }

    #[test]
    fn an_empty_file_is_an_empty_map() {
        assert!(parse("").is_empty());
        assert!(parse("\n\n\n").is_empty());
    }
}
