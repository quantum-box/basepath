use pathbase_api::{
    model::*,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Ages an invitation past its expiry without waiting seven days.
async fn expire_invitation(service: &Service, id: &str) {
    let mut tx = service.db.begin_write().await.unwrap();
    tx.execute(
        "UPDATE invitations SET expires_at='2000-01-01T00:00:00+00:00' WHERE id=?",
        &pathbase_api::params![id],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

fn actor(id: &str) -> Actor {
    Actor {
        id: id.into(),
        agent: false,
        connection: None,
    }
}
async fn req(s: &Service, a: &str, method: &str, path: &str, body: Value) -> Result<Value> {
    s.handle(
        &actor(a),
        method,
        path,
        &HashMap::new(),
        body,
        Some(&uuid::Uuid::new_v4().to_string()),
    )
    .await
}
async fn setup() -> (tempfile::TempDir, Service, String) {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(&dir.path().join("shared.sqlite3").to_string_lossy())
        .await
        .unwrap();
    s.provision_personal(&actor("us_owner")).await.unwrap();
    s.provision_personal(&actor("us_guest")).await.unwrap();
    let w = req(
        &s,
        "us_owner",
        "POST",
        "/v1/workspaces",
        json!({"name":"読書会","scope":"チーム"}),
    )
    .await
    .unwrap();
    (dir, s, w["id"].as_str().unwrap().into())
}
async fn revision(s: &Service, w: &str, a: &str) -> i64 {
    req(
        s,
        a,
        "GET",
        &format!("/v1/workspaces/{w}/members"),
        json!({}),
    )
    .await
    .unwrap()["workspace"]["version"]
        .as_i64()
        .unwrap()
}
async fn invite(s: &Service, w: &str, a: &str, target: &str, role: &str) -> Value {
    let version = revision(s, w, a).await;
    req(
        s,
        a,
        "POST",
        &format!("/v1/workspaces/{w}/invitations"),
        json!({"target_actor":target,"role":role,"expected_version":version}),
    )
    .await
    .unwrap()
}
async fn accept(s: &Service, i: &Value, a: &str) -> Result<Value> {
    req(
        s,
        a,
        "POST",
        &format!("/v1/invitations/{}/accept", i["id"].as_str().unwrap()),
        json!({"expected_version":1}),
    )
    .await
}
async fn member_change(
    s: &Service,
    w: &str,
    a: &str,
    target: &str,
    next: Option<&str>,
) -> Result<Value> {
    let version = revision(s, w, a).await;
    let mut b = json!({"expected_version":version});
    if let Some(role) = next {
        b["role"] = json!(role);
    }
    req(
        s,
        a,
        if next.is_some() { "PATCH" } else { "DELETE" },
        &format!("/v1/workspaces/{w}/members/{target}"),
        b,
    )
    .await
}

#[tokio::test]
async fn invitation_membership_revocation_and_restart_keep_personal_data_private() {
    let (dir, s, w) = setup().await;
    let own = req(&s, "us_owner", "GET", "/v1/workspaces", json!({}))
        .await
        .unwrap();
    let personal = own
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["scope"] == "個人")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let private = format!("/v1/workspaces/{personal}/items");
    req(
        &s,
        "us_owner",
        "POST",
        &private,
        json!({"title":"個人の日記"}),
    )
    .await
    .unwrap();
    let base = format!("/v1/workspaces/{w}");
    let i = invite(&s, &w, "us_owner", "us_guest", "editor").await;
    assert_eq!(
        req(
            &s,
            "us_guest",
            "GET",
            &format!("{base}/snapshot"),
            json!({})
        )
        .await
        .unwrap_err()
        .status,
        404
    );
    assert_eq!(accept(&s, &i, "us_intruder").await.unwrap_err().status, 404);
    let accept_path = format!("/v1/invitations/{}/accept", i["id"].as_str().unwrap());
    let first = s
        .handle(
            &actor("us_guest"),
            "POST",
            &accept_path,
            &HashMap::new(),
            json!({"expected_version":1}),
            Some("accept"),
        )
        .await
        .unwrap();
    assert_eq!(first["status"], "accepted");
    assert_eq!(
        req(&s, "us_guest", "GET", &private, json!({}))
            .await
            .unwrap_err()
            .status,
        404
    );
    let input = json!({"title":"みんなで読む本"});
    let path = format!("{base}/items");
    s.handle(
        &actor("us_guest"),
        "POST",
        &path,
        &HashMap::new(),
        input.clone(),
        Some("saved"),
    )
    .await
    .unwrap();
    let peer = Service::open(&dir.path().join("shared.sqlite3").to_string_lossy())
        .await
        .unwrap();
    member_change(&peer, &w, "us_owner", "us_guest", None)
        .await
        .unwrap();
    assert_eq!(
        s.handle(
            &actor("us_guest"),
            "POST",
            &path,
            &HashMap::new(),
            input,
            Some("saved")
        )
        .await
        .unwrap_err()
        .status,
        404
    );
    assert!(s
        .handle(
            &actor("us_guest"),
            "POST",
            &accept_path,
            &HashMap::new(),
            json!({"expected_version":1}),
            Some("accept")
        )
        .await
        .is_err());
    assert!(accept(&s, &i, "us_guest").await.is_err());
    for agent in [false, true] {
        assert_eq!(
            s.handle(
                &Actor {
                    id: "us_guest".into(),
                    agent,
                    connection: None,
                },
                "GET",
                &format!("{base}/snapshot"),
                &HashMap::new(),
                json!({}),
                None
            )
            .await
            .unwrap_err()
            .status,
            404
        );
    }
    let reopened = Service::open(&dir.path().join("shared.sqlite3").to_string_lossy())
        .await
        .unwrap();
    assert_eq!(
        req(
            &reopened,
            "us_owner",
            "GET",
            &format!("{base}/snapshot"),
            json!({})
        )
        .await
        .unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn viewers_can_read_and_leave_but_cannot_write_or_manage() {
    let (_dir, s, w) = setup().await;
    accept(
        &s,
        &invite(&s, &w, "us_owner", "us_guest", "viewer").await,
        "us_guest",
    )
    .await
    .unwrap();
    let base = format!("/v1/workspaces/{w}");
    assert!(req(
        &s,
        "us_guest",
        "GET",
        &format!("{base}/snapshot"),
        json!({})
    )
    .await
    .is_ok());
    assert_eq!(
        req(
            &s,
            "us_guest",
            "POST",
            &format!("{base}/items"),
            json!({"title":"不可"})
        )
        .await
        .unwrap_err()
        .status,
        403
    );
    assert_eq!(
        member_change(&s, &w, "us_guest", "us_owner", Some("editor"))
            .await
            .unwrap_err()
            .status,
        403
    );
    let version = revision(&s, &w, "us_guest").await;
    req(
        &s,
        "us_guest",
        "POST",
        &format!("{base}/leave"),
        json!({"expected_version":version}),
    )
    .await
    .unwrap();
    assert_eq!(
        req(&s, "us_guest", "GET", &format!("{base}/members"), json!({}))
            .await
            .unwrap_err()
            .status,
        404
    );
}

#[tokio::test]
async fn owner_transfer_prevents_lockout_and_rechecks_cached_admin_responses() {
    let (_dir, s, w) = setup().await;
    assert_eq!(
        member_change(&s, &w, "us_owner", "us_owner", None)
            .await
            .unwrap_err()
            .code,
        "LAST_OWNER"
    );
    let before = revision(&s, &w, "us_owner").await;
    accept(
        &s,
        &invite(&s, &w, "us_owner", "us_guest", "editor").await,
        "us_guest",
    )
    .await
    .unwrap();
    assert_eq!(
        req(
            &s,
            "us_owner",
            "PATCH",
            &format!("/v1/workspaces/{w}"),
            json!({"name":"古い操作","timezone":"Asia/Tokyo","expected_version":before})
        )
        .await
        .unwrap_err()
        .code,
        "VERSION_CONFLICT"
    );
    let path = format!("/v1/workspaces/{w}/members/us_guest");
    let version = revision(&s, &w, "us_owner").await;
    let body = json!({"role":"owner","expected_version":version});
    s.handle(
        &actor("us_owner"),
        "PATCH",
        &path,
        &HashMap::new(),
        body.clone(),
        Some("promote"),
    )
    .await
    .unwrap();
    let stale_invite = invite(&s, &w, "us_owner", "us_future", "viewer").await;
    member_change(&s, &w, "us_guest", "us_owner", Some("editor"))
        .await
        .unwrap();
    assert_eq!(
        s.handle(
            &actor("us_owner"),
            "PATCH",
            &path,
            &HashMap::new(),
            body,
            Some("promote")
        )
        .await
        .unwrap_err()
        .code,
        "OWNER_REQUIRED"
    );
    assert!(accept(&s, &stale_invite, "us_future").await.is_err());
    assert_eq!(
        member_change(&s, &w, "us_guest", "us_guest", Some("viewer"))
            .await
            .unwrap_err()
            .code,
        "LAST_OWNER"
    );
}

#[tokio::test]
async fn invitations_are_targeted_expiring_revocable_and_not_agent_controlled() {
    let (_dir, s, w) = setup().await;
    let i = invite(&s, &w, "us_owner", "us_guest", "editor").await;
    let base = format!("/v1/workspaces/{w}");
    let version = revision(&s, &w, "us_owner").await;
    let b = json!({"target_actor":"us_guest","role":"editor","expected_version":version});
    assert_eq!(
        req(
            &s,
            "us_owner",
            "POST",
            &format!("{base}/invitations"),
            b.clone()
        )
        .await
        .unwrap_err()
        .code,
        "INVITATION_EXISTS"
    );
    assert_eq!(
        req(&s, "us_intruder", "GET", "/v1/invitations", json!({}))
            .await
            .unwrap(),
        json!([])
    );
    assert_eq!(
        req(&s, "us_guest", "GET", "/v1/invitations", json!({}))
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let ai = Actor {
        id: "us_owner".into(),
        agent: true,
        connection: None,
    };
    assert_eq!(
        s.handle(
            &ai,
            "POST",
            &format!("{base}/invitations"),
            &HashMap::new(),
            b,
            Some("ai-invite")
        )
        .await
        .unwrap_err()
        .code,
        "APPROVAL_REQUIRED"
    );
    assert!(s.handle(&ai,"POST",&format!("{base}/changesets/preview"),&HashMap::new(),json!({"operations":[{"method":"POST","path":format!("{base}/invitations"),"body":{}}]}),Some("ai-plan")).await.is_err());
    expire_invitation(&s, i["id"].as_str().unwrap()).await;
    assert_eq!(
        accept(&s, &i, "us_guest").await.unwrap_err().code,
        "INVITATION_UNAVAILABLE"
    );
    let second = invite(&s, &w, "us_owner", "us_guest", "viewer").await;
    let version = revision(&s, &w, "us_owner").await;
    req(
        &s,
        "us_owner",
        "DELETE",
        &format!("{base}/invitations/{}", second["id"].as_str().unwrap()),
        json!({"expected_version":version}),
    )
    .await
    .unwrap();
    assert!(accept(&s, &second, "us_guest").await.is_err());
    let third = invite(&s, &w, "us_owner", "us_guest", "viewer").await;
    req(
        &s,
        "us_guest",
        "POST",
        &format!("/v1/invitations/{}/decline", third["id"].as_str().unwrap()),
        json!({"expected_version":1}),
    )
    .await
    .unwrap();
    assert!(accept(&s, &third, "us_guest").await.is_err());
    assert_eq!(
        req(&s, "us_guest", "GET", "/v1/invitations", json!({}))
            .await
            .unwrap(),
        json!([])
    );
}

#[tokio::test]
async fn revoked_or_expired_invitation_creation_replay_never_looks_pending() {
    let (_dir, s, w) = setup().await;
    let base = format!("/v1/workspaces/{w}");
    let version = revision(&s, &w, "us_owner").await;
    let body = json!({
        "target_actor":"us_guest",
        "role":"editor",
        "expected_version":version
    });
    let first = s
        .handle(
            &actor("us_owner"),
            "POST",
            &format!("{base}/invitations"),
            &HashMap::new(),
            body.clone(),
            Some("create-guest-invite"),
        )
        .await
        .unwrap();
    let version = revision(&s, &w, "us_owner").await;
    req(
        &s,
        "us_owner",
        "DELETE",
        &format!("{base}/invitations/{}", first["id"].as_str().unwrap()),
        json!({"expected_version":version}),
    )
    .await
    .unwrap();
    assert_eq!(
        s.handle(
            &actor("us_owner"),
            "POST",
            &format!("{base}/invitations"),
            &HashMap::new(),
            body,
            Some("create-guest-invite"),
        )
        .await
        .unwrap_err()
        .code,
        "INVITATION_UNAVAILABLE"
    );

    let owner_version = revision(&s, &w, "us_owner").await;
    let expired_body = json!({
        "target_actor":"us_future",
        "role":"viewer",
        "expected_version":owner_version
    });
    let expired = s
        .handle(
            &actor("us_owner"),
            "POST",
            &format!("{base}/invitations"),
            &HashMap::new(),
            expired_body.clone(),
            Some("create-future-invite"),
        )
        .await
        .unwrap();
    expire_invitation(&s, expired["id"].as_str().unwrap()).await;
    assert_eq!(
        s.handle(
            &actor("us_owner"),
            "POST",
            &format!("{base}/invitations"),
            &HashMap::new(),
            expired_body,
            Some("create-future-invite"),
        )
        .await
        .unwrap_err()
        .code,
        "INVITATION_UNAVAILABLE"
    );
    // A fresh key can replace an expired invitation, while the old invitation
    // remains unavailable to its recipient.
    let reinvite_body = json!({
        "target_actor":"us_future",
        "role":"viewer",
        "expected_version":revision(&s, &w, "us_owner").await
    });
    assert!(req(
        &s,
        "us_owner",
        "POST",
        &format!("{base}/invitations"),
        reinvite_body,
    )
    .await
    .is_ok());
    assert_eq!(
        accept(&s, &expired, "us_future").await.unwrap_err().code,
        "INVITATION_UNAVAILABLE"
    );
}

#[tokio::test]
async fn invitation_ids_cannot_cross_workspace_management_boundaries() {
    let (_dir, s, first_workspace) = setup().await;
    let second_workspace = req(
        &s,
        "us_owner",
        "POST",
        "/v1/workspaces",
        json!({"name":"別のチーム","scope":"チーム"}),
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let invitation = invite(&s, &second_workspace, "us_owner", "us_guest", "viewer").await;
    let delete_body = json!({"expected_version":revision(&s, &first_workspace, "us_owner").await});

    assert_eq!(
        req(
            &s,
            "us_owner",
            "DELETE",
            &format!(
                "/v1/workspaces/{first_workspace}/invitations/{}",
                invitation["id"].as_str().unwrap()
            ),
            delete_body,
        )
        .await
        .unwrap_err()
        .status,
        404
    );
    assert_eq!(
        accept(&s, &invitation, "us_guest").await.unwrap()["workspace_id"],
        second_workspace
    );
    assert_eq!(
        req(
            &s,
            "us_guest",
            "GET",
            &format!("/v1/workspaces/{first_workspace}/members"),
            json!({}),
        )
        .await
        .unwrap_err()
        .status,
        404
    );
}

#[tokio::test]
async fn local_and_personal_workspaces_cannot_be_shared_and_creation_is_idempotent() {
    let (dir, s, _w) = setup().await;
    let workspaces = req(&s, "us_owner", "GET", "/v1/workspaces", json!({}))
        .await
        .unwrap();
    let personal = workspaces
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["scope"] == "個人")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    assert_eq!(
        req(
            &s,
            "us_owner",
            "POST",
            &format!("/v1/workspaces/{personal}/invitations"),
            json!({"target_actor":"us_guest","role":"viewer","expected_version":1})
        )
        .await
        .unwrap_err()
        .code,
        "PERSONAL_WORKSPACE"
    );
    let local = Service::open(&dir.path().join("preview.sqlite3").to_string_lossy())
        .await
        .unwrap();
    local.initialize(false).await.unwrap();
    assert_eq!(
        req(
            &local,
            "local-owner",
            "POST",
            "/v1/workspaces/team/invitations",
            json!({"target_actor":"us_guest","role":"viewer","expected_version":1})
        )
        .await
        .unwrap_err()
        .code,
        "LOCAL_WORKSPACE"
    );
    let body = json!({"name":"新しい組織","scope":"組織"});
    let first = s
        .handle(
            &actor("us_owner"),
            "POST",
            "/v1/workspaces",
            &HashMap::new(),
            body.clone(),
            Some("create"),
        )
        .await
        .unwrap();
    let repeated = s
        .handle(
            &actor("us_owner"),
            "POST",
            "/v1/workspaces",
            &HashMap::new(),
            body,
            Some("create"),
        )
        .await
        .unwrap();
    assert_eq!(first, repeated);
    assert!(!first["local"].as_bool().unwrap());
}
