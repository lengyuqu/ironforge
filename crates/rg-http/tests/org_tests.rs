//! Integration tests for organization & team endpoints.
//!
//! These specifically guard against the singular/plural migration table-name
//! regression (entities expect `organizations`/`teams`/`organization_members`,
//! while the phase8 migration originally created singular tables). Before the
//! `m20260616_0000015_rename_org_team_plural` fix, every assertion here would
//! fail at runtime with "no such table".

mod common;

use common::{register_user, spawn_test_app};

const PW: &str = "Qz7$wRtm";

/// Register a user and return (token, user_id).
async fn register_full(base: &str, username: &str, email: &str) -> (String, i64) {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/users/register", base))
        .json(&serde_json::json!({"username": username, "email": email, "password": PW}))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "register failed: {}",
        resp.status()
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    (
        body["token"].as_str().unwrap().to_string(),
        body["user_id"].as_i64().unwrap(),
    )
}

#[tokio::test]
async fn test_create_and_list_org() {
    let base = spawn_test_app().await;
    let token = register_user(&base, "orgowner", "orgowner@example.com", PW).await;
    let client = reqwest::Client::new();

    // Create org — exercises the `organizations` table.
    let resp = client
        .post(format!("{}/api/v1/orgs", base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name": "acme", "display_name": "Acme Inc"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create_org should succeed");
    let org: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(org["name"], "acme");

    // List orgs — should contain the new org.
    let resp = client
        .get(format!("{}/api/v1/orgs", base))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let orgs: serde_json::Value = resp.json().await.unwrap();
    let names: Vec<&str> = orgs
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|o| o["name"].as_str())
        .collect();
    assert!(
        names.contains(&"acme"),
        "list_orgs should include 'acme', got {names:?}"
    );
}

#[tokio::test]
async fn test_add_org_member() {
    let base = spawn_test_app().await;
    let owner_token = register_user(&base, "memowner", "memowner@example.com", PW).await;
    let (_member_token, member_id) = register_full(&base, "teammate", "teammate@example.com").await;
    let client = reqwest::Client::new();

    client
        .post(format!("{}/api/v1/orgs", base))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"name": "globex"}))
        .send()
        .await
        .unwrap();

    // Add member — exercises the `organization_members` table.
    let resp = client
        .post(format!("{}/api/v1/orgs/globex/members", base))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"user_id": member_id, "role": "member"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "add_org_member should succeed");

    // List members — should include the newly added member.
    let resp = client
        .get(format!("{}/api/v1/orgs/globex/members", base))
        .bearer_auth(&owner_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let members: serde_json::Value = resp.json().await.unwrap();
    let ids: Vec<i64> = members
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["user_id"].as_i64())
        .collect();
    assert!(
        ids.contains(&member_id),
        "member list should include {member_id}, got {ids:?}"
    );
}

#[tokio::test]
async fn test_create_and_list_team() {
    let base = spawn_test_app().await;
    let token = register_user(&base, "teamowner", "teamowner@example.com", PW).await;
    let client = reqwest::Client::new();

    client
        .post(format!("{}/api/v1/orgs", base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name": "initech"}))
        .send()
        .await
        .unwrap();

    // Create team — exercises the `teams` table.
    let resp = client
        .post(format!("{}/api/v1/orgs/initech/teams", base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name": "developers", "permission": "write"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create_team should succeed");
    let team: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(team["name"], "developers");

    // List teams — should contain the new team.
    let resp = client
        .get(format!("{}/api/v1/orgs/initech/teams", base))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let teams: serde_json::Value = resp.json().await.unwrap();
    let names: Vec<&str> = teams
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(
        names.contains(&"developers"),
        "team list should include 'developers', got {names:?}"
    );
}

/// Regression: the org/team mutation endpoints used to perform **no
/// authorization at all** — the seven team handlers did not even take a
/// `HeaderMap`, and the member endpoints only checked "is logged in". An
/// anonymous caller could create an `admin` team, and any ordinary user could
/// promote themselves to org `owner` and then read/write every private repo in
/// the org.
#[tokio::test]
async fn org_team_and_membership_mutations_require_org_admin() {
    let base = spawn_test_app().await;
    let owner_token = register_user(&base, "secalice", "secalice@example.com", PW).await;
    let (outsider_token, outsider_id) =
        register_full(&base, "secmallory", "secmallory@example.com").await;
    let client = reqwest::Client::new();

    // Alice owns the org.
    let resp = client
        .post(format!("{}/api/v1/orgs", base))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"name": "victimco"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // 1) Anonymous team creation must be rejected (was 201 before the fix).
    let resp = client
        .post(format!("{}/api/v1/orgs/victimco/teams", base))
        .json(&serde_json::json!({"name": "pwn", "permission": "admin"}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        401,
        "anonymous team creation must be rejected"
    );

    // 2) A non-member must not be able to create a team either.
    let resp = client
        .post(format!("{}/api/v1/orgs/victimco/teams", base))
        .bearer_auth(&outsider_token)
        .json(&serde_json::json!({"name": "pwn", "permission": "admin"}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "non-member must not create a team, got {}",
        resp.status()
    );

    // 3) Privilege escalation: a non-member must not self-promote to owner.
    let resp = client
        .post(format!("{}/api/v1/orgs/victimco/members", base))
        .bearer_auth(&outsider_token)
        .json(&serde_json::json!({"user_id": outsider_id, "role": "owner"}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        403,
        "self-promotion to org owner must be rejected, got {}",
        resp.status()
    );

    // The escalation must not have taken effect.
    let resp = client
        .get(format!("{}/api/v1/orgs/victimco/members", base))
        .bearer_auth(&owner_token)
        .send()
        .await
        .unwrap();
    let members: serde_json::Value = resp.json().await.unwrap();
    let has_outsider = members
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["user_id"].as_i64() == Some(outsider_id));
    assert!(!has_outsider, "outsider must not be an org member");

    // 4) The legitimate owner is still able to administer the org.
    let resp = client
        .post(format!("{}/api/v1/orgs/victimco/teams", base))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"name": "developers", "permission": "write"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "owner must still create teams");
    let team: serde_json::Value = resp.json().await.unwrap();
    let team_id = team["id"].as_i64().unwrap();

    let resp = client
        .post(format!("{}/api/v1/orgs/victimco/teams/{team_id}/members", base))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"user_id": outsider_id, "role": "member"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "owner must still add team members");

    // 5) Anonymous and non-admin callers must not delete the team.
    let resp = client
        .delete(format!("{}/api/v1/orgs/victimco/teams/{team_id}", base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "anonymous team deletion must be rejected");

    let resp = client
        .delete(format!("{}/api/v1/orgs/victimco/teams/{team_id}", base))
        .bearer_auth(&outsider_token)
        .send()
        .await
        .unwrap();
    assert_ne!(
        resp.status(),
        200,
        "non-admin must not delete a team, got {}",
        resp.status()
    );

    // The owner can still delete it.
    let resp = client
        .delete(format!("{}/api/v1/orgs/victimco/teams/{team_id}", base))
        .bearer_auth(&owner_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "owner must still delete teams");
}

/// Regression: team read endpoints used to ignore the org name in the path and
/// performed no authorization at all. `get_team`/`list_team_members` must
/// (a) scope the team to the org in the URL and (b) refuse private-org reads
/// by non-members.
#[tokio::test]
async fn team_reads_are_scoped_and_private_orgs_are_not_readable() {
    let base = spawn_test_app().await;
    let owner_token = register_user(&base, "teamreadowner", "teamreadowner@example.com", PW).await;
    let (outsider_token, _) =
        register_full(&base, "teamreadout", "teamreadout@example.com").await;
    let client = reqwest::Client::new();

    // A private org with one team.
    let resp = client
        .post(format!("{}/api/v1/orgs", base))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"name": "privateteamco", "visibility": "private"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let resp = client
        .post(format!("{}/api/v1/orgs/privateteamco/teams", base))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"name": "secret", "permission": "read"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "owner must create a team");
    let team_id = resp.json::<serde_json::Value>().await.unwrap()["id"]
        .as_i64()
        .unwrap();

    // Anonymous and non-member reads of the private org are refused.
    let anon = client
        .get(format!("{}/api/v1/orgs/privateteamco/teams/{team_id}", base))
        .send()
        .await
        .unwrap();
    assert_eq!(anon.status(), 404, "anonymous read of a private org must fail");
    let outsider = client
        .get(format!("{}/api/v1/orgs/privateteamco/teams/{team_id}", base))
        .bearer_auth(&outsider_token)
        .send()
        .await
        .unwrap();
    assert_eq!(
        outsider.status(),
        404,
        "non-member read of a private org must fail"
    );

    // The path org name is authoritative: reaching the team through another
    // public org must 404 instead of leaking it.
    let resp = client
        .post(format!("{}/api/v1/orgs", base))
        .bearer_auth(&owner_token)
        .json(&serde_json::json!({"name": "publicteamco"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let crossed = client
        .get(format!("{}/api/v1/orgs/publicteamco/teams/{team_id}", base))
        .bearer_auth(&owner_token)
        .send()
        .await
        .unwrap();
    assert_eq!(
        crossed.status(),
        404,
        "a team must not be reachable through an unrelated org path"
    );

    // The legitimate owner can still read it through the right path.
    let ok = client
        .get(format!("{}/api/v1/orgs/privateteamco/teams/{team_id}", base))
        .bearer_auth(&owner_token)
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 200, "owner must still read their own team");

    // Privilege failures are 403, not 400/404.
    let forbidden = client
        .post(format!("{}/api/v1/orgs/publicteamco/teams", base))
        .bearer_auth(&outsider_token)
        .json(&serde_json::json!({"name": "nope"}))
        .send()
        .await
        .unwrap();
    assert_eq!(forbidden.status(), 403, "non-admin team creation must be 403");
}