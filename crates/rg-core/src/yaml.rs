//! YAML reading and writing for workflow files, issue templates and package
//! metadata.
//!
//! One place chooses the parser and its settings, so every caller sees the same
//! dialect. The parser is `serde-saphyr`, a pure-Rust (`forbid(unsafe_code)`)
//! implementation of YAML 1.2; it replaced `serde_yaml`, whose libyaml port
//! (`unsafe-libyaml`) is unmaintained.
//!
//! Dynamic YAML is held in [`Value`], which is `serde_json::Value`. JSON has no
//! composite or non-string keys, which workflow files and package metadata do
//! not use; mappings come back with their keys sorted, so code must not depend
//! on mapping order (typed maps in the target struct decide that).
//!
//! Settings, and how they differ from the parser's defaults:
//!
//! * `strict_booleans`: only `true`/`false` are booleans. Without it `yes`,
//!   `no`, `on` and `off` become booleans, which would turn an `on:` workflow
//!   trigger value or a `no` answer into something else.
//! * non-finite floats (`.inf`, `.nan`) pass through (JSON turns them into
//!   `null`) instead of failing the whole document.
//!
//! Behaviour worth knowing, all different from `serde_yaml`:
//!
//! * merge keys (`<<: *anchor`) are applied, as Gitea's and GitHub's parsers do;
//! * custom tags (`!ruby/object:Gem::Specification`) are ignored and the tagged
//!   node is read as the plain node it wraps;
//! * a number with a leading zero (`010`) or underscores (`1_000`) is a number
//!   rather than a string; quote it when it must stay text.

use serde::{de::DeserializeOwned, Serialize};

/// A dynamically typed YAML value.
pub use serde_json::Value;

/// Why a document could not be read.
pub use serde_saphyr::Error;

/// Why a value could not be written as YAML.
pub use serde_saphyr::ser::Error as WriteError;

fn options() -> serde_saphyr::Options {
    let mut options = serde_saphyr::Options::default();
    options.strict_booleans = true;
    options.non_finite_float_policy = serde_saphyr::NonFiniteFloatPolicy::PassThrough;
    options
}

/// Parse a single YAML document.
pub fn from_str<T: DeserializeOwned>(input: &str) -> Result<T, Error> {
    serde_saphyr::from_str_with_options(input, options())
}

/// Write a value as a YAML document.
pub fn to_string<T: Serialize>(value: &T) -> Result<String, WriteError> {
    serde_saphyr::to_string(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    fn value(input: &str) -> Value {
        from_str(input).unwrap()
    }

    #[test]
    fn only_true_and_false_are_booleans() {
        let v = value("a: yes\nb: no\nc: on\nd: off\ne: y\nf: true\ng: False\n");
        assert_eq!(v["a"], "yes");
        assert_eq!(v["b"], "no");
        assert_eq!(v["c"], "on");
        assert_eq!(v["d"], "off");
        assert_eq!(v["e"], "y");
        assert_eq!(v["f"], true);
        assert_eq!(v["g"], false);
    }

    #[test]
    fn the_on_key_of_a_workflow_stays_a_string_key() {
        let v = value("on:\n  push:\n    branches: [main]\n");
        assert_eq!(v["on"]["push"]["branches"][0], "main");
    }

    #[test]
    fn scalars_keep_the_types_workflows_rely_on() {
        let v = value(
            "a: '123'\nb: 20\nc: 3.10\nd: 0x1F\ne: 0o14\nf: ~\ng:\nh: 2024-01-02\ni: 1.2.3\n",
        );
        assert_eq!(v["a"], "123");
        assert_eq!(v["b"], 20);
        // The classic trap, unchanged: an unquoted 3.10 is the float 3.1.
        assert_eq!(v["c"], 3.1);
        assert_eq!(v["d"], 31);
        assert_eq!(v["e"], 12);
        assert!(v["f"].is_null());
        assert!(v["g"].is_null());
        assert_eq!(v["h"], "2024-01-02");
        assert_eq!(v["i"], "1.2.3");
    }

    #[test]
    fn non_finite_floats_do_not_fail_the_document() {
        let v = value("a: .inf\nb: -.inf\nc: .nan\nd: 1\n");
        assert!(v["a"].is_null() && v["b"].is_null() && v["c"].is_null());
        assert_eq!(v["d"], 1);
    }

    #[test]
    fn merge_keys_are_applied() {
        let v = value("base: &b {x: 1, y: 2}\nuse:\n  <<: *b\n  z: 3\n");
        assert_eq!(v["use"]["x"], 1);
        assert_eq!(v["use"]["y"], 2);
        assert_eq!(v["use"]["z"], 3);
    }

    #[test]
    fn ruby_tagged_gem_metadata_reads_as_the_plain_mapping() {
        let v = value(
            "--- !ruby/object:Gem::Specification\nname: rack\nversion: !ruby/object:Gem::Version\n  version: \"2.2.4\"\nlicenses:\n- MIT\n",
        );
        assert_eq!(v["name"], "rack");
        assert_eq!(v["version"]["version"], "2.2.4");
        assert_eq!(v["licenses"][0], "MIT");
    }

    #[test]
    fn malformed_input_is_an_error_with_a_position() {
        let error = from_str::<Value>("a: [1, 2\nb: 3\n")
            .unwrap_err()
            .to_string();
        assert!(error.contains("line"), "no position in: {error}");
    }

    #[test]
    fn duplicate_keys_and_extra_documents_are_rejected() {
        assert!(from_str::<Value>("a: 1\na: 2\n").is_err());
        assert!(from_str::<Value>("a: 1\n---\nb: 2\n").is_err());
    }

    #[test]
    fn an_alias_bomb_is_rejected_instead_of_expanded() {
        // Each level refers to the one before it ten times: a few hundred bytes
        // that would expand to 10^8 scalars. Workflow files come from
        // repositories, so this must fail fast.
        let mut doc = String::from("l0: &l0 [x, x, x, x, x, x, x, x, x, x]\n");
        for level in 1..9 {
            let previous = format!("*l{}", level - 1);
            let items = vec![previous; 10].join(", ");
            doc.push_str(&format!("l{level}: &l{level} [{items}]\n"));
        }
        assert!(from_str::<Value>(&doc).is_err());
    }

    #[test]
    fn an_empty_document_is_null() {
        assert!(from_str::<Value>("").unwrap().is_null());
    }

    #[test]
    fn written_yaml_has_no_directive_and_reads_back() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Doc {
            name: String,
            tags: Vec<String>,
        }
        let doc = Doc {
            name: "x".into(),
            tags: vec!["a".into(), "b".into()],
        };
        let text = to_string(&doc).unwrap();
        assert!(!text.starts_with('%'), "YAML directive in output: {text}");
        assert_eq!(from_str::<Doc>(&text).unwrap(), doc);
    }
}
