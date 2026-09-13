use pathbase_api::{
    model::*,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

fn actor(id: &str) -> Actor {
    Actor {
        id: id.into(),
        agent: false,
    }
}
fn req(s: &Service, a: &str, method: &str, path: &str, body: Value) -> Result<Value> {
    s.handle(
        &actor(a),
        method,
        path,
        &HashMap::new(),
        body,
        Some(&uuid::Uuid::new_v4().to_string()),
    )
}
fn setup() -> (tempfile::TempDir, Service, String) {
    let dir = tempfile::tempdir().unwrap();
    let s = Service::open(&dir.path().join("shared.sqlite3")).unwrap();
    s.provision_personal(&actor("us_owner")).unwrap();
    s.provision_personal(&actor("us_guest")).unwrap();
    let w = req(
        &s,
        "us_owner",
        "POST",
        "/v1/workspaces",
        json!({"name":"読書会","scope":"チーム"}),
    )
    .unwrap();
    (dir, s, w["id"].as_str().unwrap().into())
}
fn revision(s: &Service, w: &str, a: &str) -> i64 {
    req(
        s,
        a,
        "GET",
        &format!("/v1/workspaces/{w}/members"),
        json!({}),
    )
    .unwrap()["workspace"]["version"]
        .as_i64()
        .unwrap()
}
fn invite(s: &Service, w: &str, a: &str, target: &str, role: &str) -> Value {
    req(
        s,
        a,
        "POST",
        &format!("/v1/workspaces/{w}/invitations"),
        json!({"target_actor":target,"role":role,"expected_version":revision(s,w,a)}),
    )
    .unwrap()
}
fn accept(s: &Service, i: &Value, a: &str) -> Result<Value> {
    req(
        s,
        a,
        "POST",
        &format!("/v1/invitations/{}/accept", i["id"].as_str().unwrap()),
        json!({"expected_version":1}),
    )
}
fn member_change(s: &Service, w: &str, a: &str, target: &str, next: Option<&str>) -> Result<Value> {
    let mut b = json!({"expected_version":revision(s,w,a)});
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
}

#[test]
fn invitation_membership_revocation_and_restart_keep_personal_data_private() {
    let (dir, s, w) = setup();
    let own = req(&s, "us_owner", "GET", "/v1/workspaces", json!({})).unwrap();
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
    .unwrap();
    let base = format!("/v1/workspaces/{w}");
    let i = invite(&s, &w, "us_owner", "us_guest", "editor");
    assert_eq!(
        req(
            &s,
            "us_guest",
            "GET",
            &format!("{base}/snapshot"),
            json!({})
        )
        .unwrap_err()
        .status,
        404
    );
    assert_eq!(accept(&s, &i, "us_intruder").unwrap_err().status, 404);
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
        .unwrap();
    assert_eq!(first["status"], "accepted");
    assert_eq!(
        req(&s, "us_guest", "GET", &private, json!({}))
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
    .unwrap();
    let peer = Service::open(&dir.path().join("shared.sqlite3")).unwrap();
    member_change(&peer, &w, "us_owner", "us_guest", None).unwrap();
    assert_eq!(
        s.handle(
            &actor("us_guest"),
            "POST",
            &path,
            &HashMap::new(),
            input,
            Some("saved")
        )
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
        .is_err());
    assert!(accept(&s, &i, "us_guest").is_err());
    for agent in [false, true] {
        assert_eq!(
            s.handle(
                &Actor {
                    id: "us_guest".into(),
                    agent
                },
                "GET",
                &format!("{base}/snapshot"),
                &HashMap::new(),
                json!({}),
                None
            )
            .unwrap_err()
            .status,
            404
        );
    }
    let reopened = Service::open(&dir.path().join("shared.sqlite3")).unwrap();
    assert_eq!(
        req(
            &reopened,
            "us_owner",
            "GET",
            &format!("{base}/snapshot"),
            json!({})
        )
        .unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn viewers_can_read_and_leave_but_cannot_write_or_manage() {
    let (_dir, s, w) = setup();
    accept(
        &s,
        &invite(&s, &w, "us_owner", "us_guest", "viewer"),
        "us_guest",
    )
    .unwrap();
    let base = format!("/v1/workspaces/{w}");
    assert!(req(
        &s,
        "us_guest",
        "GET",
        &format!("{base}/snapshot"),
        json!({})
    )
    .is_ok());
    assert_eq!(
        req(
            &s,
            "us_guest",
            "POST",
            &format!("{base}/items"),
            json!({"title":"不可"})
        )
        .unwrap_err()
        .status,
        403
    );
    assert_eq!(
        member_change(&s, &w, "us_guest", "us_owner", Some("editor"))
            .unwrap_err()
            .status,
        403
    );
    req(
        &s,
        "us_guest",
        "POST",
        &format!("{base}/leave"),
        json!({"expected_version":revision(&s,&w,"us_guest")}),
    )
    .unwrap();
    assert_eq!(
        req(&s, "us_guest", "GET", &format!("{base}/members"), json!({}))
            .unwrap_err()
            .status,
        404
    );
}

#[test]
fn owner_transfer_prevents_lockout_and_rechecks_cached_admin_responses() {
    let (_dir, s, w) = setup();
    assert_eq!(
        member_change(&s, &w, "us_owner", "us_owner", None)
            .unwrap_err()
            .code,
        "LAST_OWNER"
    );
    let before = revision(&s, &w, "us_owner");
    accept(
        &s,
        &invite(&s, &w, "us_owner", "us_guest", "editor"),
        "us_guest",
    )
    .unwrap();
    assert_eq!(
        req(
            &s,
            "us_owner",
            "PATCH",
            &format!("/v1/workspaces/{w}"),
            json!({"name":"古い操作","timezone":"Asia/Tokyo","expected_version":before})
        )
        .unwrap_err()
        .code,
        "VERSION_CONFLICT"
    );
    let path = format!("/v1/workspaces/{w}/members/us_guest");
    let body = json!({"role":"owner","expected_version":revision(&s,&w,"us_owner")});
    s.handle(
        &actor("us_owner"),
        "PATCH",
        &path,
        &HashMap::new(),
        body.clone(),
        Some("promote"),
    )
    .unwrap();
    let stale_invite = invite(&s, &w, "us_owner", "us_future", "viewer");
    member_change(&s, &w, "us_guest", "us_owner", Some("editor")).unwrap();
    assert_eq!(
        s.handle(
            &actor("us_owner"),
            "PATCH",
            &path,
            &HashMap::new(),
            body,
            Some("promote")
        )
        .unwrap_err()
        .code,
        "OWNER_REQUIRED"
    );
    assert!(accept(&s, &stale_invite, "us_future").is_err());
    assert_eq!(
        member_change(&s, &w, "us_guest", "us_guest", Some("viewer"))
            .unwrap_err()
            .code,
        "LAST_OWNER"
    );
}

#[test]
fn invitations_are_targeted_expiring_revocable_and_not_agent_controlled() {
    let (_dir, s, w) = setup();
    let i = invite(&s, &w, "us_owner", "us_guest", "editor");
    let base = format!("/v1/workspaces/{w}");
    let b = json!({"target_actor":"us_guest","role":"editor","expected_version":revision(&s,&w,"us_owner")});
    assert_eq!(
        req(
            &s,
            "us_owner",
            "POST",
            &format!("{base}/invitations"),
            b.clone()
        )
        .unwrap_err()
        .code,
        "INVITATION_EXISTS"
    );
    assert_eq!(
        req(&s, "us_intruder", "GET", "/v1/invitations", json!({})).unwrap(),
        json!([])
    );
    assert_eq!(
        req(&s, "us_guest", "GET", "/v1/invitations", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let ai = Actor {
        id: "us_owner".into(),
        agent: true,
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
        .unwrap_err()
        .code,
        "APPROVAL_REQUIRED"
    );
    assert!(s.handle(&ai,"POST",&format!("{base}/changesets/preview"),&HashMap::new(),json!({"operations":[{"method":"POST","path":format!("{base}/invitations"),"body":{}}]}),Some("ai-plan")).is_err());
    s.db.lock()
        .unwrap()
        .execute(
            "UPDATE invitations SET expires_at='2000-01-01T00:00:00+00:00' WHERE id=?1",
            [i["id"].as_str().unwrap()],
        )
        .unwrap();
    assert_eq!(
        accept(&s, &i, "us_guest").unwrap_err().code,
        "INVITATION_UNAVAILABLE"
    );
    let second = invite(&s, &w, "us_owner", "us_guest", "viewer");
    req(
        &s,
        "us_owner",
        "DELETE",
        &format!("{base}/invitations/{}", second["id"].as_str().unwrap()),
        json!({"expected_version":revision(&s,&w,"us_owner")}),
    )
    .unwrap();
    assert!(accept(&s, &second, "us_guest").is_err());
    let third = invite(&s, &w, "us_owner", "us_guest", "viewer");
    req(
        &s,
        "us_guest",
        "POST",
        &format!("/v1/invitations/{}/decline", third["id"].as_str().unwrap()),
        json!({"expected_version":1}),
    )
    .unwrap();
    assert!(accept(&s, &third, "us_guest").is_err());
    assert_eq!(
        req(&s, "us_guest", "GET", "/v1/invitations", json!({})).unwrap(),
        json!([])
    );
}

#[test]
fn revoked_or_expired_invitation_creation_replay_never_looks_pending() {
    let (_dir, s, w) = setup();
    let base = format!("/v1/workspaces/{w}");
    let body = json!({
        "target_actor":"us_guest",
        "role":"editor",
        "expected_version":revision(&s,&w,"us_owner")
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
        .unwrap();
    req(
        &s,
        "us_owner",
        "DELETE",
        &format!("{base}/invitations/{}", first["id"].as_str().unwrap()),
        json!({"expected_version":revision(&s,&w,"us_owner")}),
    )
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
        .unwrap_err()
        .code,
        "INVITATION_UNAVAILABLE"
    );

    let expired_body = json!({
        "target_actor":"us_future",
        "role":"viewer",
        "expected_version":revision(&s,&w,"us_owner")
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
        .unwrap();
    s.db.lock()
        .unwrap()
        .execute(
            "UPDATE invitations SET expires_at='2000-01-01T00:00:00+00:00' WHERE id=?1",
            [expired["id"].as_str().unwrap()],
        )
        .unwrap();
    assert_eq!(
        s.handle(
            &actor("us_owner"),
            "POST",
            &format!("{base}/invitations"),
            &HashMap::new(),
            expired_body,
            Some("create-future-invite"),
        )
        .unwrap_err()
        .code,
        "INVITATION_UNAVAILABLE"
    );
    // A fresh key can replace an expired invitation, while the old invitation
    // remains unavailable to its recipient.
    assert!(req(
        &s,
        "us_owner",
        "POST",
        &format!("{base}/invitations"),
        json!({
            "target_actor":"us_future",
            "role":"viewer",
            "expected_version":revision(&s,&w,"us_owner")
        }),
    )
    .is_ok());
    assert_eq!(
        accept(&s, &expired, "us_future").unwrap_err().code,
        "INVITATION_UNAVAILABLE"
    );
}

#[test]
fn invitation_ids_cannot_cross_workspace_management_boundaries() {
    let (_dir, s, first_workspace) = setup();
    let second_workspace = req(
        &s,
        "us_owner",
        "POST",
        "/v1/workspaces",
        json!({"name":"別のチーム","scope":"チーム"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let invitation = invite(&s, &second_workspace, "us_owner", "us_guest", "viewer");

    assert_eq!(
        req(
            &s,
            "us_owner",
            "DELETE",
            &format!(
                "/v1/workspaces/{first_workspace}/invitations/{}",
                invitation["id"].as_str().unwrap()
            ),
            json!({"expected_version":revision(&s,&first_workspace,"us_owner")}),
        )
        .unwrap_err()
        .status,
        404
    );
    assert_eq!(
        accept(&s, &invitation, "us_guest").unwrap()["workspace_id"],
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
        .unwrap_err()
        .status,
        404
    );
}

#[test]
fn local_and_personal_workspaces_cannot_be_shared_and_creation_is_idempotent() {
    let (dir, s, _w) = setup();
    let workspaces = req(&s, "us_owner", "GET", "/v1/workspaces", json!({})).unwrap();
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
        .unwrap_err()
        .code,
        "PERSONAL_WORKSPACE"
    );
    let local = Service::open(&dir.path().join("preview.sqlite3")).unwrap();
    local.initialize(false).unwrap();
    assert_eq!(
        req(
            &local,
            "local-owner",
            "POST",
            "/v1/workspaces/team/invitations",
            json!({"target_actor":"us_guest","role":"viewer","expected_version":1})
        )
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
        .unwrap();
    assert_eq!(first, repeated);
    assert!(!first["local"].as_bool().unwrap());
}
