//! `server.json`: the manifest of an MCP server, as the official MCP Registry
//! describes it.
//!
//! The registry stores the **manifest only**. It does not host the server, does
//! not fetch any URL in it, and does not look inside the packages it points at.
//! Everything in a manifest is what its publisher *claims*.
//!
//! # What is checked, and where the rule comes from
//!
//! Checked against the published JSON Schema (`schemas/2025-12-11`, read on
//! 2026-11-21):
//!
//! * `name`, `description`, `version` are required; `name` matches
//!   `^[a-zA-Z0-9.-]+/[a-zA-Z0-9._-]+$` and is 3–200 characters, `description` is
//!   1–100 characters, `version` is at most 255, `title` is 1–100.
//! * A package needs `registryType`, `identifier` and `transport`; `version` is
//!   not `latest`; `fileSha256` is 64 lower-case hex digits.
//! * A transport is `stdio`, or `streamable-http` / `sse` with a `url`. A remote
//!   is `streamable-http` / `sse` with a `url`. URLs are `http(s)`.
//! * An argument is `positional` (with a `value` or `valueHint`) or `named`
//!   (with a `name`); headers and environment variables have a `name`.
//!
//! Rules this registry adds (they are **stricter than the schema**):
//!
//! * `version` must be one canonical semantic version with no build metadata,
//!   because a registry that resolves constraints must be able to order
//!   versions. (The schema says "SHOULD follow semantic versioning".)
//! * At least one of `packages` / `remotes` is present, so a manifest names
//!   something a client can run or call.
//! * A remote or http transport URL is `https`, or `http` only for `localhost`,
//!   `127.0.0.1` or `[::1]` (reported as `insecure_local`); it carries no
//!   `user:password@`.
//! * A package `version` is one version, not a range (the schema words this only
//!   in prose).
//! * Size limits the schema does not have: the manifest at most 128 KiB,
//!   50 packages, 20 remotes.
//!
//! Left alone on purpose: a `registryType` this code does not know is kept and
//! flagged `known_registry_type: false`; fields it does not know are kept.
//! Template URLs (`{var}` in place of the scheme) appear in the repository's
//! draft schema but not in the published one, and are refused.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Largest canonical manifest, in bytes.
pub const MAX_MANIFEST_BYTES: usize = 128 * 1024;
pub const MAX_PACKAGES: usize = 50;
pub const MAX_REMOTES: usize = 20;
/// Longest `$schema` string that is recorded with a version.
pub const MAX_DECLARED_SCHEMA_BYTES: usize = 512;

/// `registryType` values this code knows. Others are accepted and flagged.
const KNOWN_REGISTRY_TYPES: &[&str] = &["npm", "pypi", "oci", "nuget", "mcpb", "cargo"];

/// A manifest the registry refuses, with a stable machine-readable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError {
    pub code: &'static str,
    pub message: String,
    pub too_large: bool,
}

impl ManifestError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            too_large: false,
        }
    }
}

type Result<T> = std::result::Result<T, ManifestError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSummary {
    pub registry_type: String,
    pub known_registry_type: bool,
    pub identifier: String,
    pub version: Option<String>,
    /// `stdio`, `streamable-http` or `sse`.
    pub transport: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSummary {
    /// `streamable-http` or `sse`.
    pub transport: String,
    pub url: String,
    /// `true` for a plain-`http` URL that points at this machine.
    pub insecure_local: bool,
}

/// What a client can do with the server, read off the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointSummary {
    pub packages: Vec<PackageSummary>,
    pub remotes: Vec<RemoteSummary>,
}

/// A manifest that passed validation, in the form the registry stores.
#[derive(Debug, Clone)]
pub struct Analysis {
    /// `namespace/server`.
    pub name: String,
    pub version: String,
    pub description: String,
    pub title: Option<String>,
    /// The `$schema` the publisher declared, recorded as written. Never fetched.
    pub declared_schema: Option<String>,
    /// The manifest with object keys sorted and no insignificant whitespace.
    /// These exact bytes are stored, and their digest is what immutability is
    /// judged on, so key order and whitespace in the request do not matter.
    pub canonical: Vec<u8>,
    /// `sha256:` of `canonical`.
    pub digest: String,
    pub endpoints: EndpointSummary,
}

/// Validate a manifest and produce what the registry stores.
pub fn analyze(manifest: &Value) -> Result<Analysis> {
    let object = manifest.as_object().ok_or_else(|| {
        ManifestError::new("MCP_MANIFEST_INVALID", "the manifest must be a JSON object")
    })?;

    let canonical = canonical_bytes(manifest);
    if canonical.len() > MAX_MANIFEST_BYTES {
        let mut error = ManifestError::new(
            "MCP_MANIFEST_TOO_LARGE",
            format!("a manifest may be at most {MAX_MANIFEST_BYTES} bytes"),
        );
        error.too_large = true;
        return Err(error);
    }

    let name = required_str(object, "name", "MCP_NAME_INVALID")?;
    check_name(name)?;

    let description = required_str(object, "description", "MCP_DESCRIPTION_INVALID")?;
    let description_chars = description.chars().count();
    if description.trim().is_empty() || description_chars > 100 {
        return Err(ManifestError::new(
            "MCP_DESCRIPTION_INVALID",
            "description must be 1 to 100 characters and not blank",
        ));
    }

    let version = required_str(object, "version", "MCP_VERSION_INVALID")?;
    check_version(version)?;

    let title = match object.get("title") {
        None => None,
        Some(Value::String(title)) => {
            let chars = title.chars().count();
            if title.trim().is_empty() || chars > 100 {
                return Err(ManifestError::new(
                    "MCP_MANIFEST_INVALID",
                    "title must be 1 to 100 characters and not blank",
                ));
            }
            Some(title.clone())
        }
        Some(_) => {
            return Err(ManifestError::new(
                "MCP_MANIFEST_INVALID",
                "title must be a string",
            ))
        }
    };

    let declared_schema = match object.get("$schema") {
        None => None,
        Some(Value::String(schema)) if schema.len() <= MAX_DECLARED_SCHEMA_BYTES => {
            Some(schema.clone())
        }
        Some(_) => {
            return Err(ManifestError::new(
                "MCP_MANIFEST_INVALID",
                format!("$schema must be a string of at most {MAX_DECLARED_SCHEMA_BYTES} bytes"),
            ))
        }
    };

    if let Some(site) = object.get("websiteUrl") {
        let site = site.as_str().ok_or_else(|| {
            ManifestError::new("MCP_MANIFEST_INVALID", "websiteUrl must be a string")
        })?;
        parse_url(site, "websiteUrl").map_err(|m| ManifestError::new("MCP_MANIFEST_INVALID", m))?;
    }
    check_repository(object.get("repository"))?;

    let packages = array_of(object, "packages", MAX_PACKAGES)?;
    let remotes = array_of(object, "remotes", MAX_REMOTES)?;
    if packages.is_empty() && remotes.is_empty() {
        return Err(ManifestError::new(
            "MCP_NO_ENDPOINT",
            "a manifest needs at least one entry in `packages` or `remotes`",
        ));
    }

    let mut endpoints = EndpointSummary {
        packages: Vec::new(),
        remotes: Vec::new(),
    };
    for (index, package) in packages.iter().enumerate() {
        endpoints
            .packages
            .push(check_package(package).map_err(|m| in_item("packages", index, m))?);
    }
    for (index, remote) in remotes.iter().enumerate() {
        endpoints
            .remotes
            .push(check_remote(remote).map_err(|m| in_item("remotes", index, m))?);
    }

    let digest = format!("sha256:{}", hex::encode(Sha256::digest(&canonical)));
    Ok(Analysis {
        name: name.to_string(),
        version: version.to_string(),
        description: description.to_string(),
        title,
        declared_schema,
        canonical,
        digest,
        endpoints,
    })
}

fn in_item(field: &str, index: usize, mut error: ManifestError) -> ManifestError {
    error.message = format!("{field}[{index}]: {}", error.message);
    error
}

// ── Canonical form ───────────────────────────────────────────────────────

/// `value` as JSON with object keys sorted, independent of how `serde_json` was
/// built (its object type keeps insertion order under the `preserve_order`
/// feature, which another crate may switch on).
pub fn canonical_bytes(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push(b'{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.extend(serde_json::to_vec(key).unwrap_or_default());
                out.push(b':');
                write_canonical(&map[key], out);
            }
            out.push(b'}');
        }
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        scalar => out.extend(serde_json::to_vec(scalar).unwrap_or_default()),
    }
}

// ── Top level ────────────────────────────────────────────────────────────

type Object = serde_json::Map<String, Value>;

fn required_str<'a>(object: &'a Object, key: &str, code: &'static str) -> Result<&'a str> {
    match object.get(key) {
        Some(Value::String(text)) => Ok(text),
        Some(_) => Err(ManifestError::new(code, format!("{key} must be a string"))),
        None => Err(ManifestError::new(code, format!("{key} is required"))),
    }
}

fn array_of<'a>(object: &'a Object, key: &str, max: usize) -> Result<&'a [Value]> {
    match object.get(key) {
        None => Ok(&[]),
        Some(Value::Array(items)) if items.len() <= max => Ok(items),
        Some(Value::Array(_)) => Err(ManifestError::new(
            "MCP_MANIFEST_INVALID",
            format!("{key} may have at most {max} entries"),
        )),
        Some(_) => Err(ManifestError::new(
            "MCP_MANIFEST_INVALID",
            format!("{key} must be an array"),
        )),
    }
}

fn check_name(name: &str) -> Result<()> {
    let bad = |message: &str| Err(ManifestError::new("MCP_NAME_INVALID", message.to_string()));
    let chars = name.chars().count();
    if !(3..=200).contains(&chars) {
        return bad("name must be 3 to 200 characters");
    }
    let mut parts = name.split('/');
    let (Some(namespace), Some(server), None) = (parts.next(), parts.next(), parts.next()) else {
        return bad("name must be `namespace/server`: exactly one `/`");
    };
    let namespace_ok = !namespace.is_empty()
        && namespace
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    let server_ok = !server.is_empty()
        && server
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-');
    if !namespace_ok || !server_ok {
        return bad(
            "name must match ^[a-zA-Z0-9.-]+/[a-zA-Z0-9._-]+$ (namespace letters, digits, `.` `-`; \
             server letters, digits, `.` `_` `-`)",
        );
    }
    // The pattern allows `.` and `..`; the name is used as a storage path
    // segment and a URL segment, where those mean something else.
    if namespace.bytes().all(|b| b == b'.') || server.bytes().all(|b| b == b'.') {
        return bad("a name segment may not consist only of dots");
    }
    Ok(())
}

/// One canonical semantic version, no build metadata.
pub fn check_version(version: &str) -> Result<()> {
    let parsed = semver::Version::parse(version).map_err(|error| {
        ManifestError::new(
            "MCP_VERSION_INVALID",
            format!("version {version:?} is not a semantic version: {error}"),
        )
    })?;
    if !parsed.build.is_empty() {
        return Err(ManifestError::new(
            "MCP_VERSION_INVALID",
            "build metadata (+...) is not allowed: it does not take part in version ordering, \
             so two versions could not be told apart",
        ));
    }
    if parsed.to_string() != version || version.len() > 255 {
        return Err(ManifestError::new(
            "MCP_VERSION_INVALID",
            format!("write the version as {parsed}, not {version:?}"),
        ));
    }
    Ok(())
}

fn check_repository(repository: Option<&Value>) -> Result<()> {
    let Some(repository) = repository else {
        return Ok(());
    };
    let invalid = |m: &str| Err(ManifestError::new("MCP_MANIFEST_INVALID", m.to_string()));
    let Some(object) = repository.as_object() else {
        return invalid("repository must be an object");
    };
    let (Some(Value::String(url)), Some(Value::String(source))) =
        (object.get("url"), object.get("source"))
    else {
        return invalid("repository needs string `url` and `source`");
    };
    if source.is_empty() {
        return invalid("repository.source must not be empty");
    }
    parse_url(url, "repository.url").map_err(|m| ManifestError::new("MCP_MANIFEST_INVALID", m))?;
    Ok(())
}

// ── URLs ─────────────────────────────────────────────────────────────────

/// An absolute `http(s)` URL with a host and no embedded credentials.
fn parse_url(text: &str, field: &str) -> std::result::Result<reqwest::Url, String> {
    if text.len() > 2048 || text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("{field} is not a valid URL"));
    }
    let url = reqwest::Url::parse(text).map_err(|_| format!("{field} is not a valid URL"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(format!("{field} must be an http(s) URL with a host"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(format!(
            "{field} must not contain credentials (`user:password@`)"
        ));
    }
    Ok(url)
}

/// Whether `url` is plain `http` to this machine. `Err` when it is `http` to
/// anywhere else.
fn require_https_or_local(url: &reqwest::Url, field: &str) -> std::result::Result<bool, String> {
    if url.scheme() == "https" {
        return Ok(false);
    }
    // `host_str` keeps the brackets of an IPv6 literal.
    match url.host_str() {
        Some("localhost" | "127.0.0.1" | "[::1]") => Ok(true),
        _ => Err(format!(
            "{field} must be https (plain http is allowed only for localhost, 127.0.0.1 and [::1])"
        )),
    }
}

// ── Packages and remotes ─────────────────────────────────────────────────

fn object_in<'a>(value: &'a Value, what: &str) -> Result<&'a Object> {
    value.as_object().ok_or_else(|| {
        ManifestError::new("MCP_MANIFEST_INVALID", format!("{what} must be an object"))
    })
}

fn package_err(message: impl Into<String>) -> ManifestError {
    ManifestError::new("MCP_PACKAGE_INVALID", message)
}

fn nonempty_str<'a>(
    object: &'a Object,
    key: &str,
    max: usize,
    make: fn(String) -> ManifestError,
) -> Result<&'a str> {
    match object.get(key) {
        Some(Value::String(text))
            if !text.trim().is_empty()
                && text.len() <= max
                && !text.chars().any(|c| c.is_control()) =>
        {
            Ok(text)
        }
        Some(Value::String(_)) => Err(make(format!(
            "{key} must be 1 to {max} bytes, not blank, with no control characters"
        ))),
        Some(_) => Err(make(format!("{key} must be a string"))),
        None => Err(make(format!("{key} is required"))),
    }
}

fn check_package(value: &Value) -> Result<PackageSummary> {
    let object = object_in(value, "a package")?;
    let registry_type = nonempty_str(object, "registryType", 64, package_err)?;
    let identifier = nonempty_str(object, "identifier", 1024, package_err)?;

    let version = match object.get("version") {
        None => None,
        Some(Value::String(version)) => {
            check_package_version(version)?;
            Some(version.clone())
        }
        Some(_) => return Err(package_err("version must be a string")),
    };

    if let Some(sha) = object.get("fileSha256") {
        let ok = sha.as_str().is_some_and(|s| {
            s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        });
        if !ok {
            return Err(package_err(
                "fileSha256 must be 64 lower-case hexadecimal digits",
            ));
        }
    }
    if let Some(base) = object.get("registryBaseUrl") {
        let base = base
            .as_str()
            .ok_or_else(|| package_err("registryBaseUrl must be a string"))?;
        parse_url(base, "registryBaseUrl").map_err(package_err)?;
    }

    let transport = match object.get("transport") {
        Some(transport) => check_transport(transport, true)?.0,
        None => return Err(package_err("transport is required")),
    };

    check_arguments(object.get("runtimeArguments"), "runtimeArguments")?;
    check_arguments(object.get("packageArguments"), "packageArguments")?;
    check_named(object.get("environmentVariables"), "environmentVariables")?;

    Ok(PackageSummary {
        known_registry_type: KNOWN_REGISTRY_TYPES.contains(&registry_type),
        registry_type: registry_type.to_string(),
        identifier: identifier.to_string(),
        version,
        transport,
    })
}

/// A package's `version` names one version. The schema says so only in prose
/// ("version ranges are rejected"), and forbids `latest` itself.
fn check_package_version(version: &str) -> Result<()> {
    let range_like = version.bytes().any(|b| {
        matches!(b, b'^' | b'~' | b'<' | b'>' | b'*' | b'|' | b' ' | b',') || b.is_ascii_control()
    }) || version.ends_with(".x")
        || version.ends_with(".X");
    if version.is_empty() || version.len() > 255 || version == "latest" || range_like {
        return Err(package_err(
            "version must be one exact version, not `latest` or a range",
        ));
    }
    Ok(())
}

/// A transport object: its type, and whether it carries a URL that was checked.
fn check_transport(value: &Value, in_package: bool) -> Result<(String, Option<bool>)> {
    let err = |m: String| {
        if in_package {
            package_err(format!("transport: {m}"))
        } else {
            ManifestError::new("MCP_REMOTE_INVALID", m)
        }
    };
    let object = value
        .as_object()
        .ok_or_else(|| err("must be an object".into()))?;
    let kind = match object.get("type") {
        Some(Value::String(kind)) => kind.as_str(),
        _ => return Err(err("type is required".into())),
    };
    let allowed: &[&str] = if in_package {
        &["stdio", "streamable-http", "sse"]
    } else {
        &["streamable-http", "sse"]
    };
    if !allowed.contains(&kind) {
        return Err(err(format!("type must be one of {}", allowed.join(", "))));
    }
    if kind == "stdio" {
        return Ok((kind.to_string(), None));
    }
    let url = match object.get("url") {
        Some(Value::String(url)) => url,
        _ => return Err(err(format!("a {kind} transport needs a url"))),
    };
    let parsed = parse_url(url, "url").map_err(err)?;
    let insecure_local = require_https_or_local(&parsed, "url").map_err(err)?;
    check_named(object.get("headers"), "headers").map_err(|e| err(e.message))?;
    Ok((kind.to_string(), Some(insecure_local)))
}

fn check_remote(value: &Value) -> Result<RemoteSummary> {
    let (transport, insecure_local) = check_transport(value, false)?;
    let url = value
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(RemoteSummary {
        transport,
        url,
        insecure_local: insecure_local.unwrap_or(false),
    })
}

/// `headers` / `environmentVariables`: a list of objects that each have a name.
fn check_named(value: Option<&Value>, field: &str) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let items = value
        .as_array()
        .ok_or_else(|| package_err(format!("{field} must be an array")))?;
    if items.len() > 200 {
        return Err(package_err(format!("{field} may have at most 200 entries")));
    }
    for item in items {
        let named = item
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|n| !n.is_empty());
        if !named {
            return Err(package_err(format!("every entry of {field} needs a name")));
        }
    }
    Ok(())
}

/// `runtimeArguments` / `packageArguments`: positional or named.
fn check_arguments(value: Option<&Value>, field: &str) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let items = value
        .as_array()
        .ok_or_else(|| package_err(format!("{field} must be an array")))?;
    if items.len() > 200 {
        return Err(package_err(format!("{field} may have at most 200 entries")));
    }
    for item in items {
        let kind = item.get("type").and_then(Value::as_str);
        let has = |key: &str| item.get(key).and_then(Value::as_str).is_some();
        let ok = match kind {
            Some("positional") => has("value") || has("valueHint"),
            Some("named") => has("name"),
            _ => false,
        };
        if !ok {
            return Err(package_err(format!(
                "{field}: each argument is `positional` (with a value or valueHint) or `named` \
                 (with a name)"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn remote_manifest() -> Value {
        json!({
            "$schema": "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json",
            "name": "io.github.acme/weather",
            "description": "Weather lookups.",
            "version": "1.2.3",
            "remotes": [{"type": "streamable-http", "url": "https://mcp.acme.example/weather"}]
        })
    }

    fn package_manifest() -> Value {
        json!({
            "name": "io.github.acme/weather",
            "description": "Weather lookups.",
            "version": "1.2.3",
            "packages": [{
                "registryType": "npm",
                "identifier": "@acme/weather-mcp",
                "version": "1.2.3",
                "transport": {"type": "stdio"},
                "environmentVariables": [{"name": "API_KEY", "isSecret": true}],
                "packageArguments": [{"type": "named", "name": "--region", "value": "eu"}]
            }]
        })
    }

    fn code(manifest: Value) -> &'static str {
        analyze(&manifest).expect_err("should be refused").code
    }

    #[test]
    fn a_remote_and_a_package_manifest_are_accepted() {
        let a = analyze(&remote_manifest()).unwrap();
        assert_eq!(a.name, "io.github.acme/weather");
        assert_eq!(a.endpoints.remotes[0].transport, "streamable-http");
        assert!(!a.endpoints.remotes[0].insecure_local);
        assert!(a.declared_schema.is_some());

        let b = analyze(&package_manifest()).unwrap();
        assert_eq!(b.endpoints.packages[0].registry_type, "npm");
        assert!(b.endpoints.packages[0].known_registry_type);
        assert_eq!(b.endpoints.packages[0].transport, "stdio");
    }

    #[test]
    fn the_digest_does_not_depend_on_key_order_or_spacing() {
        let a = analyze(&remote_manifest()).unwrap();
        let reordered: Value = serde_json::from_str(
            r#"{ "remotes": [ {"url": "https://mcp.acme.example/weather", "type": "streamable-http"} ],
                 "version": "1.2.3", "description": "Weather lookups.",
                 "name": "io.github.acme/weather",
                 "$schema": "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json" }"#,
        )
        .unwrap();
        let b = analyze(&reordered).unwrap();
        assert_eq!(a.canonical, b.canonical);
        assert_eq!(a.digest, b.digest);

        let mut changed = remote_manifest();
        changed["description"] = json!("Weather lookups!");
        assert_ne!(analyze(&changed).unwrap().digest, a.digest);
    }

    #[test]
    fn required_fields_and_their_limits() {
        for field in ["name", "description", "version"] {
            let mut m = remote_manifest();
            m.as_object_mut().unwrap().remove(field);
            assert!(analyze(&m).is_err(), "{field} is required");
        }
        let mut m = remote_manifest();
        m["description"] = json!("x".repeat(101));
        assert_eq!(code(m), "MCP_DESCRIPTION_INVALID");
        let mut m = remote_manifest();
        m["description"] = json!("   ");
        assert_eq!(code(m), "MCP_DESCRIPTION_INVALID");
        let mut m = remote_manifest();
        m["title"] = json!("t".repeat(101));
        assert_eq!(code(m), "MCP_MANIFEST_INVALID");
        assert_eq!(code(json!([1, 2])), "MCP_MANIFEST_INVALID");
    }

    #[test]
    fn names_follow_the_published_pattern() {
        for good in ["a/b", "io.github.u/weather", "com.example-1/srv_name.v2"] {
            let mut m = remote_manifest();
            m["name"] = json!(good);
            assert!(analyze(&m).is_ok(), "{good}");
        }
        for bad in [
            "weather", "a/b/c", "/b", "a/", "a b/c", "a/b c", "a_b/c", "ü/c", "../x", "x/..",
            "./x", "ab",
        ] {
            let mut m = remote_manifest();
            m["name"] = json!(bad);
            assert_eq!(code(m), "MCP_NAME_INVALID", "{bad}");
        }
        let mut m = remote_manifest();
        m["name"] = json!(format!("{}/x", "a".repeat(200)));
        assert_eq!(code(m), "MCP_NAME_INVALID");
    }

    #[test]
    fn versions_are_one_canonical_semver() {
        for good in ["0.0.1", "1.2.3", "1.0.0-rc.1"] {
            assert!(check_version(good).is_ok(), "{good}");
        }
        for bad in [
            "1",
            "1.2",
            "v1.2.3",
            "^1.2.3",
            "~1.2.3",
            "1.2.x",
            ">=1.0.0",
            "latest",
            "1.2.3+build",
            "01.2.3",
            "",
        ] {
            assert_eq!(
                check_version(bad).unwrap_err().code,
                "MCP_VERSION_INVALID",
                "{bad}"
            );
        }
    }

    #[test]
    fn something_to_run_or_call_is_required() {
        let mut m = remote_manifest();
        m.as_object_mut().unwrap().remove("remotes");
        assert_eq!(code(m), "MCP_NO_ENDPOINT");
        let mut m = remote_manifest();
        m["remotes"] = json!([]);
        assert_eq!(code(m), "MCP_NO_ENDPOINT");
        let mut m = remote_manifest();
        m["remotes"] = json!("https://x.example");
        assert_eq!(code(m), "MCP_MANIFEST_INVALID");
    }

    #[test]
    fn remote_urls_are_https_or_local_and_carry_no_credentials() {
        let try_url = |url: &str| {
            let mut m = remote_manifest();
            m["remotes"][0]["url"] = json!(url);
            analyze(&m)
        };
        assert!(try_url("https://mcp.example/x").is_ok());
        for local in [
            "http://localhost:8080/mcp",
            "http://127.0.0.1/mcp",
            "http://[::1]:3000/mcp",
        ] {
            let a = try_url(local).unwrap();
            assert!(a.endpoints.remotes[0].insecure_local, "{local}");
        }
        for bad in [
            "http://mcp.example/x",
            "http://localhost.evil.example/x",
            "ftp://mcp.example/x",
            "https://user:pw@mcp.example/x",
            "https://user@mcp.example/x",
            "not a url",
            "{base}/mcp",
            "https://mcp.example/a b",
        ] {
            let e = try_url(bad).expect_err(bad);
            assert_eq!(e.code, "MCP_REMOTE_INVALID", "{bad}");
        }
    }

    #[test]
    fn remote_types_and_headers() {
        let mut m = remote_manifest();
        m["remotes"][0]["type"] = json!("stdio");
        assert_eq!(code(m), "MCP_REMOTE_INVALID");
        let mut m = remote_manifest();
        m["remotes"][0]["type"] = json!("sse");
        assert!(analyze(&m).is_ok());
        let mut m = remote_manifest();
        m["remotes"][0]["headers"] = json!([{"value": "x"}]);
        assert_eq!(code(m), "MCP_REMOTE_INVALID");
        let mut m = remote_manifest();
        m["remotes"] = json!(vec![
            json!({"type": "sse", "url": "https://a.example/x"});
            MAX_REMOTES + 1
        ]);
        assert_eq!(code(m), "MCP_MANIFEST_INVALID");
    }

    #[test]
    fn package_rules() {
        let edit = |f: &dyn Fn(&mut Value)| {
            let mut m = package_manifest();
            f(&mut m["packages"][0]);
            analyze(&m)
        };
        for key in ["registryType", "identifier", "transport"] {
            let e = edit(&|p| {
                p.as_object_mut().unwrap().remove(key);
            })
            .expect_err(key);
            assert_eq!(e.code, "MCP_PACKAGE_INVALID", "{key}");
        }
        for bad in ["latest", "^1.2.3", "~1.2", ">=1", "1.x", "1.2.*", "1 || 2"] {
            let e = edit(&|p| p["version"] = json!(bad)).expect_err(bad);
            assert_eq!(e.code, "MCP_PACKAGE_INVALID", "{bad}");
        }
        assert!(edit(&|p| p["version"] = json!("2024.10.1")).is_ok());
        assert!(edit(&|p| p["fileSha256"] = json!("a".repeat(64))).is_ok());
        for bad in ["A".repeat(64), "a".repeat(63), "zz".repeat(32)] {
            let e = edit(&|p| p["fileSha256"] = json!(bad)).expect_err("sha");
            assert_eq!(e.code, "MCP_PACKAGE_INVALID");
        }
        let e = edit(&|p| p["transport"] = json!({"type": "streamable-http"})).unwrap_err();
        assert_eq!(e.code, "MCP_PACKAGE_INVALID");
        assert!(edit(&|p| {
            p["transport"] = json!({"type": "sse", "url": "https://x.example/sse"})
        })
        .is_ok());
        let e = edit(&|p| p["runtimeArguments"] = json!([{"type": "named"}])).unwrap_err();
        assert_eq!(e.code, "MCP_PACKAGE_INVALID");
        let e = edit(&|p| p["packageArguments"] = json!([{"type": "positional"}])).unwrap_err();
        assert_eq!(e.code, "MCP_PACKAGE_INVALID");
        let e = edit(&|p| p["environmentVariables"] = json!([{"isSecret": true}])).unwrap_err();
        assert_eq!(e.code, "MCP_PACKAGE_INVALID");
        // The index is named in the message.
        assert!(e.message.starts_with("packages[0]:"), "{}", e.message);
    }

    #[test]
    fn an_unknown_registry_type_is_kept_and_flagged() {
        let mut m = package_manifest();
        m["packages"][0]["registryType"] = json!("brand-new-registry");
        let a = analyze(&m).unwrap();
        assert!(!a.endpoints.packages[0].known_registry_type);
        assert_eq!(a.endpoints.packages[0].registry_type, "brand-new-registry");
    }

    #[test]
    fn unknown_fields_are_kept_in_the_stored_manifest() {
        let mut m = remote_manifest();
        m["x-extra"] = json!({"anything": [1, 2, 3]});
        let a = analyze(&m).unwrap();
        let stored: Value = serde_json::from_slice(&a.canonical).unwrap();
        assert_eq!(stored["x-extra"]["anything"][2], 3);
    }

    #[test]
    fn the_size_limits_hold() {
        let mut m = remote_manifest();
        m["x-big"] = json!("x".repeat(MAX_MANIFEST_BYTES));
        let e = analyze(&m).unwrap_err();
        assert_eq!(e.code, "MCP_MANIFEST_TOO_LARGE");
        assert!(e.too_large);
        let mut m = package_manifest();
        let one = m["packages"][0].clone();
        m["packages"] = Value::Array(vec![one; MAX_PACKAGES + 1]);
        assert_eq!(code(m), "MCP_MANIFEST_INVALID");
        let mut m = remote_manifest();
        m["$schema"] = json!("x".repeat(MAX_DECLARED_SCHEMA_BYTES + 1));
        assert_eq!(code(m), "MCP_MANIFEST_INVALID");
    }

    #[test]
    fn repository_and_website_must_be_http_urls() {
        let mut m = remote_manifest();
        m["repository"] = json!({"url": "https://github.com/acme/weather", "source": "github"});
        m["websiteUrl"] = json!("https://acme.example");
        assert!(analyze(&m).is_ok());
        let mut m = remote_manifest();
        m["repository"] = json!({"url": "https://github.com/acme/weather"});
        assert_eq!(code(m), "MCP_MANIFEST_INVALID");
        let mut m = remote_manifest();
        m["websiteUrl"] = json!("javascript:alert(1)");
        assert_eq!(code(m), "MCP_MANIFEST_INVALID");
    }
}
