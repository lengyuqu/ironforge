//! Verify request extraction statuses described by the OpenAPI contract.
mod common;

#[tokio::test]
async fn malformed_mutation_bodies_match_openapi_statuses() {
    let base = common::spawn_test_app().await;
    let (token, _) =
        common::register_full(&base, "contract_owner", "contract_owner@example.com").await;
    common::create_repo(&base, &token, "contract_repo").await;
    let client = reqwest::Client::new();
    let root = format!("{base}/api/v1/repos/contract_owner/contract_repo");
    for (method, path, malformed) in [
        (
            reqwest::Method::PATCH,
            "",
            serde_json::json!({"description":"missing version"}),
        ),
        (
            reqwest::Method::POST,
            "/wiki",
            serde_json::json!({"title":"missing content"}),
        ),
        (
            reqwest::Method::PATCH,
            "/wiki/Home",
            serde_json::json!({"content":"missing version"}),
        ),
        (
            reqwest::Method::PATCH,
            "/pulls/1",
            serde_json::json!({"draft":"not a bool"}),
        ),
        (
            reqwest::Method::POST,
            "/pipelines",
            serde_json::json!({"ref_name":123}),
        ),
    ] {
        let malformed_response = client
            .request(method.clone(), format!("{root}{path}"))
            .bearer_auth(&token)
            .json(&malformed)
            .send()
            .await
            .unwrap();
        assert_eq!(
            malformed_response.status(),
            422,
            "{method} {path}: malformed body"
        );
        let wrong_type_response = client
            .request(method.clone(), format!("{root}{path}"))
            .bearer_auth(&token)
            .header("content-type", "text/plain")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(
            wrong_type_response.status(),
            415,
            "{method} {path}: content type"
        );
    }
}
