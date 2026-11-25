//! CNB (cnb.cool) API client, for migrating a repository out of it.
//!
//! CNB is not a Gitea or GitLab derivative: it has its own OpenAPI with its own
//! shapes. Copying the Gitea client would produce something that compiles and
//! silently imports nothing, so this file is written against responses captured
//! from the live API instead:
//!
//! ```text
//! GET https://api.cnb.cool/{owner}/{repo}/-/issues?state=open|closed  → [ {...}, ... ]
//! GET https://api.cnb.cool/{owner}/{repo}/-/issues/{number} → { ..., "body": "..." }
//! GET https://api.cnb.cool/{owner}/{repo}/-/issues/{n}/comments
//! GET https://api.cnb.cool/{owner}/{repo}/-/pulls?state=all
//! ```
//!
//! Three things differ from every other source and each one has cost a real
//! migration in this codebase or would:
//!
//! * the response body is a **bare array**, not `{ "data": [...] }`
//! * `number` is a **string** (`"65"`), not a number
//! * the list endpoint carries **no body** — the text lives on the single-issue
//!   endpoint, so importing N issues costs N extra requests
//!
//! Authentication is `Authorization: Bearer <access_token>`; the token comes
//! from `~/.cnb/token`, which is a docker-style `{"auths": {"cnb.cool": {...}}}`
//! document rather than the token itself.

use anyhow::{Context, Result};

const DEFAULT_BASE: &str = "https://api.cnb.cool";
const PAGE_SIZE: usize = 100;

/// A repository identifier in CNB's `owner/repo` form.
#[derive(Debug, Clone)]
pub struct CnbRepo {
    pub owner: String,
    pub name: String,
}

impl CnbRepo {
    fn path(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CnbUser {
    pub username: String,
    #[serde(default)]
    pub nickname: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CnbLabel {
    #[serde(default)]
    pub name: Option<String>,
    /// Some endpoints send labels as bare strings instead of objects.
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CnbIssue {
    /// A string on CNB; every other source this project imports from uses a
    /// number. Kept as-is so the conversion is explicit at the call site.
    pub number: String,
    pub title: String,
    /// Only present on the single-issue endpoint.
    #[serde(default)]
    pub body: Option<String>,
    pub state: String,
    #[serde(default)]
    pub labels: Vec<serde_json::Value>,
    #[serde(default)]
    pub author: Option<CnbUser>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    /// Empty string, not null, when the issue is open.
    #[serde(default)]
    pub closed_at: String,
    #[serde(default)]
    pub comment_count: i64,
}

impl CnbIssue {
    /// CNB strings its numbers; anything unparseable becomes 0 so the importer
    /// can fall back to allocating one rather than dropping the issue.
    pub fn number_as_i64(&self) -> i64 {
        self.number.trim().parse().unwrap_or(0)
    }

    /// Label names, accepting both `{"name": "bug"}` and `"bug"`.
    pub fn label_names(&self) -> Vec<String> {
        self.labels
            .iter()
            .filter_map(|value| match value {
                serde_json::Value::String(name) => Some(name.clone()),
                serde_json::Value::Object(map) => {
                    map.get("name").and_then(|v| v.as_str()).map(str::to_string)
                }
                _ => None,
            })
            .collect()
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CnbComment {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub author: Option<CnbUser>,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CnbPullRef {
    /// A full ref (`refs/heads/main`), unlike GitHub's short branch name.
    #[serde(default)]
    pub r#ref: String,
    #[serde(default)]
    pub sha: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CnbPull {
    pub number: String,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    pub state: String,
    /// CNB reports merging separately; `state` stays `closed`.
    #[serde(default)]
    pub is_merged: bool,
    /// CNB calls a draft a "wip".
    #[serde(default)]
    pub is_wip: bool,
    #[serde(default)]
    pub head: Option<CnbPullRef>,
    #[serde(default)]
    pub base: Option<CnbPullRef>,
    #[serde(default)]
    pub labels: Vec<serde_json::Value>,
    #[serde(default)]
    pub author: Option<CnbUser>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub closed_at: String,
}

impl CnbPull {
    pub fn number_as_i64(&self) -> i64 {
        self.number.trim().parse().unwrap_or(0)
    }

    pub fn label_names(&self) -> Vec<String> {
        label_names_from(&self.labels)
    }
}

/// Accept both `{"name": "bug"}` and `"bug"`; a client that only handles one
/// shape drops every label silently.
fn label_names_from(labels: &[serde_json::Value]) -> Vec<String> {
    labels
        .iter()
        .filter_map(|value| match value {
            serde_json::Value::String(name) => Some(name.clone()),
            serde_json::Value::Object(map) => {
                map.get("name").and_then(|v| v.as_str()).map(str::to_string)
            }
            _ => None,
        })
        .collect()
}

/// Keep the first row per number.
///
/// An item that changes state between the `open` and `closed` calls would
/// otherwise be returned twice; the shared importer creates by number, so the
/// second copy always fails and the failure count lies about the source.
fn dedupe_by_number<T>(items: Vec<T>, number: impl Fn(&T) -> String) -> Vec<T> {
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(number(item)))
        .collect()
}

pub struct CnbClient {
    token: String,
    base: String,
    http: reqwest::Client,
}

impl CnbClient {
    pub fn new(token: String) -> Self {
        Self::with_base(token, DEFAULT_BASE.to_string())
    }

    pub fn with_base(token: String, base: String) -> Self {
        Self {
            token,
            base,
            http: reqwest::Client::new(),
        }
    }

    /// Build a client from the token file CNB's own CLI writes.
    ///
    /// The file is `{"auths": {"cnb.cool": {"access_token": ...}}}`, not a raw
    /// token: reading it as text and sending it as a bearer token produces a
    /// 401 that says nothing about the real mistake.
    pub fn token_from_cli_file(path: &std::path::Path) -> Result<String> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading CNB token file {}", path.display()))?;
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .with_context(|| format!("parsing CNB token file {}", path.display()))?;
        parsed
            .get("auths")
            .and_then(|auths| auths.get("cnb.cool"))
            .and_then(|entry| entry.get("access_token"))
            .and_then(|token| token.as_str())
            .map(str::to_string)
            .context("CNB token file has no auths[\"cnb.cool\"].access_token")
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base.trim_end_matches('/'), path);
        let response = self
            .http
            .get(&url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.token),
            )
            .header(reqwest::header::ACCEPT, "application/vnd.cnb.api+json")
            .send()
            .await
            .with_context(|| format!("CNB request failed: {path}"))?;
        let status = response.status();
        if !status.is_success() {
            // The body explains more than the status does (a 404 names the
            // resource), but it is the source platform's text, so it is kept out
            // of what the importing user sees and only logged.
            let body = response.text().await.unwrap_or_default();
            tracing::warn!(%status, path, body = %body.chars().take(300).collect::<String>(), "CNB API request failed");
            anyhow::bail!("CNB API error ({status}) for {path}");
        }
        response
            .json::<T>()
            .await
            .with_context(|| format!("CNB response did not match the expected shape: {path}"))
    }

    /// Walk a paginated list endpoint.
    ///
    /// CNB answers with a bare array and reports the total in `x-cnb-total`; the
    /// loop stops on a short page so it does not depend on that header.
    async fn paginate<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<Vec<T>> {
        let mut all = Vec::new();
        let mut page = 1usize;
        loop {
            let separator = if path.contains('?') { '&' } else { '?' };
            let page_path = format!("{path}{separator}page={page}&page_size={PAGE_SIZE}");
            let batch: Vec<T> = self.get_json(&page_path).await?;
            let len = batch.len();
            all.extend(batch);
            if len < PAGE_SIZE {
                break;
            }
            page += 1;
            // A source that ignores paging would otherwise loop forever.
            if page > 1000 {
                anyhow::bail!("CNB pagination did not terminate for {path}");
            }
        }
        Ok(all)
    }

    /// Every issue, whatever its state.
    ///
    /// Unlike `/pulls`, CNB's issue list does **not** accept `state=all`: it
    /// answers `400 {"errmsg":"bad request"}`. Omitting the parameter is not a
    /// way out either — it silently means `open`, so a repository whose issues
    /// are mostly closed (31 of 32 on the live test repo) imports a fraction
    /// and reports success. Only `open` and `closed` are accepted, so this
    /// asks twice and merges.
    pub async fn list_issues(&self, repo: &CnbRepo) -> Result<Vec<CnbIssue>> {
        let mut all = Vec::new();
        for state in ["open", "closed"] {
            let batch = self
                .paginate(&format!("/{}/-/issues?state={state}", repo.path()))
                .await?;
            all.extend(batch);
        }
        Ok(dedupe_by_number(all, |issue| issue.number.clone()))
    }

    /// Fetch one issue, which is where its `body` lives.
    pub async fn get_issue(&self, repo: &CnbRepo, number: &str) -> Result<CnbIssue> {
        self.get_json(&format!("/{}/-/issues/{number}", repo.path()))
            .await
    }

    pub async fn list_issue_comments(
        &self,
        repo: &CnbRepo,
        number: &str,
    ) -> Result<Vec<CnbComment>> {
        self.paginate(&format!("/{}/-/issues/{number}/comments", repo.path()))
            .await
    }

    /// Every pull request, whatever its state.
    ///
    /// CNB's default is **open only**, and a merged pull is `closed` — so a
    /// repository whose pull requests are all merged reports **zero** without
    /// `state=all`. The same trap exists on this project's own API; here it was
    /// found by asking the live endpoint for a count and getting 0 while the
    /// web UI showed 34.
    pub async fn list_pulls(&self, repo: &CnbRepo) -> Result<Vec<CnbPull>> {
        self.paginate(&format!("/{}/-/pulls?state=all", repo.path()))
            .await
    }

    pub async fn list_pull_comments(
        &self,
        repo: &CnbRepo,
        number: &str,
    ) -> Result<Vec<CnbComment>> {
        self.paginate(&format!("/{}/-/pulls/{number}/comments", repo.path()))
            .await
    }
}

/// Parse `https://cnb.cool/owner/repo` (with or without `.git`) into its parts.
pub fn parse_cnb_url(source_url: &str) -> Result<CnbRepo> {
    let url = source_url.trim_end_matches('/').trim_end_matches(".git");
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let mut segments = rest.split('/').filter(|segment| !segment.is_empty());
    let _host = segments.next();
    let owner = segments
        .next()
        .context("CNB URL has no owner segment")?
        .to_string();
    let name = segments
        .next()
        .context("CNB URL has no repository segment")?
        .to_string();
    Ok(CnbRepo { owner, name })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_parsed_with_or_without_git_and_scheme() {
        let repo = parse_cnb_url("https://cnb.cool/lengyuqu/ironforge").unwrap();
        assert_eq!(repo.owner, "lengyuqu");
        assert_eq!(repo.name, "ironforge");

        let repo = parse_cnb_url("https://cnb.cool/lengyuqu/ironforge.git").unwrap();
        assert_eq!(repo.name, "ironforge");

        assert!(parse_cnb_url("https://cnb.cool/onlyowner").is_err());
    }

    /// Captured from the live API: `number` is a string, labels are an empty
    /// array on an unlabelled issue, and `closed_at` is `""` rather than null.
    #[test]
    fn an_issue_payload_from_the_live_api_parses() {
        let payload = r##"{
            "number": "65",
            "state": "open",
            "state_reason": "open",
            "title": "[security] Fix CVE-2026-25537",
            "invisible": false,
            "author": {"username": "cnb.lengyuqu", "nickname": "x", "is_npc": false},
            "assignees": [],
            "labels": [],
            "comment_count": 1,
            "priority": "",
            "created_at": "2026-11-24T15:04:04Z",
            "updated_at": "2026-11-24T15:17:51Z",
            "started_at": "",
            "ended_at": "",
            "last_acted_at": "2026-11-24T15:17:51Z",
            "closed_at": "",
            "body": "heading then text"
        }"##;
        let issue: CnbIssue = serde_json::from_str(payload).expect("CNB issue must parse");
        assert_eq!(issue.number, "65");
        assert_eq!(issue.number_as_i64(), 65);
        assert_eq!(issue.body.as_deref(), Some("heading then text"));
        assert!(issue.label_names().is_empty());
        assert_eq!(
            issue.author.as_ref().map(|a| a.username.as_str()),
            Some("cnb.lengyuqu")
        );
    }

    /// Both shapes appear in the wild; a client that only handles objects drops
    /// every label silently.
    #[test]
    fn labels_are_read_from_objects_and_from_strings() {
        let payload = r#"{
            "number": "1", "title": "t", "state": "open",
            "labels": [{"name": "bug"}, "enhancement"]
        }"#;
        let issue: CnbIssue = serde_json::from_str(payload).unwrap();
        assert_eq!(issue.label_names(), vec!["bug", "enhancement"]);
    }

    /// `state=all` is a 400 on this endpoint, so the client asks twice; an
    /// issue that flips state between those calls must not be imported twice.
    #[test]
    fn duplicates_across_the_two_state_calls_are_collapsed() {
        let items = vec!["11".to_string(), "12".to_string(), "11".to_string()];
        assert_eq!(
            dedupe_by_number(items, |number| number.clone()),
            vec!["11".to_string(), "12".to_string()]
        );
    }

    #[test]
    fn the_cli_token_file_is_not_a_bare_token() {
        // The TempDir must outlive the file: binding it to a temporary drops it
        // (and the directory) at the end of the statement.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("token");
        std::fs::write(
            &file,
            r#"{"auths":{"cnb.cool":{"access_token":"abc123","expires_at":"2026-12-01"}}}"#,
        )
        .unwrap();
        assert_eq!(CnbClient::token_from_cli_file(&file).unwrap(), "abc123");

        std::fs::write(&file, r#"{"auths":{}}"#).unwrap();
        assert!(CnbClient::token_from_cli_file(&file).is_err());
    }
}
