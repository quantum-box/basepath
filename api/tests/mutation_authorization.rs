//! Server-side authorization regressions for graph mutations.
//!
//! The UI uses the same item-create endpoint for a child and a sibling (a
//! sibling is created under the selected item's parent).  These tests keep
//! the important boundary at the API: owners and editors may mutate their
//! workspace, viewers may not, and neither a workspace id nor an item id from
//! another workspace can be used as a capability.
use pathbase_api::{
    model::Result,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;

const TENANT: &str = pathbase_api::service::LOCAL_TENANT;

fn person(id: &str) -> Actor {
    Actor::person(id, TENANT)
}

async fn service() -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(
        &dir.path()
            .join("mutation-authorization.sqlite3")
            .to_string_lossy(),
    )
    .await
    .unwrap();
    (dir, service)
}

async fn call(
    service: &Service,
    actor: &Actor,
    method: &str,
    path: &str,
    body: Value,
    key: &str,
) -> Result<Value> {
    service
        .handle(actor, method, path, &HashMap::new(), body, Some(key))
        .await
}

async fn get(service: &Service, actor: &Actor, path: &str) -> Result<Value> {
    service
        .handle(actor, "GET", path, &HashMap::new(), json!({}), None)
        .await
}

async fn add_membership(service: &Service, workspace: &str, actor: &str, role: &str) {
    let mut tx = service.db.begin_write().await.unwrap();
    tx.execute(
        "INSERT INTO memberships(workspace_id,actor,role) VALUES(?,?,?)",
        &pathbase_api::params![workspace, actor, role],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

async fn workspace(service: &Service, actor: &Actor, name: &str, key: &str) -> String {
    call(
        service,
        actor,
        "POST",
        "/v1/workspaces",
        json!({"name":name,"scope":"チーム"}),
        key,
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn item(
    service: &Service,
    actor: &Actor,
    workspace: &str,
    kind: &str,
    title: &str,
    parent_id: Option<&str>,
    key: &str,
) -> String {
    let mut body = json!({"kind":kind,"title":title});
    if let Some(parent_id) = parent_id {
        body["parent_id"] = json!(parent_id);
    }
    call(
        service,
        actor,
        "POST",
        &format!("/v1/workspaces/{workspace}/items"),
        body,
        key,
    )
    .await
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn breakdown(service: &Service, actor: &Actor, workspace: &str, root: &str) -> Value {
    get(
        service,
        actor,
        &format!("/v1/workspaces/{workspace}/items/{root}/breakdown"),
    )
    .await
    .unwrap()
}

async fn complete(
    service: &Service,
    actor: &Actor,
    workspace: &str,
    action: &str,
    key: &str,
) -> Result<Value> {
    call(
        service,
        actor,
        "POST",
        &format!("/v1/workspaces/{workspace}/actions/{action}/complete"),
        json!({
            "expected_version": 1,
            "local_date": "2026-09-20",
            "completed_at": "2026-09-20T01:00:00Z"
        }),
        key,
    )
    .await
}

#[tokio::test]
async fn child_and_sibling_mutations_require_write_membership_and_stay_in_workspace() {
    let (_dir, service) = service().await;
    let owner = person("owner-a");
    let editor = person("editor-a");
    let viewer = person("viewer-a");
    let other_owner = person("owner-b");
    let workspace_a = workspace(&service, &owner, "A", "workspace-a").await;
    let workspace_b = workspace(&service, &other_owner, "B", "workspace-b").await;
    add_membership(&service, &workspace_a, &editor.id, "editor").await;
    add_membership(&service, &workspace_a, &viewer.id, "viewer").await;

    let parent_a = item(
        &service,
        &owner,
        &workspace_a,
        "outcome",
        "A の親",
        None,
        "parent-a",
    )
    .await;
    let parent_b = item(
        &service,
        &other_owner,
        &workspace_b,
        "outcome",
        "B の親",
        None,
        "parent-b",
    )
    .await;
    let child_a = item(
        &service,
        &owner,
        &workspace_a,
        "initiative",
        "A の既存子",
        Some(&parent_a),
        "child-a",
    )
    .await;

    // The owner and editor use the same server path for both UI concepts:
    // direct child creation and sibling creation (same parent_id).
    for (actor, label) in [(&owner, "owner"), (&editor, "editor")] {
        let child = item(
            &service,
            actor,
            &workspace_a,
            "initiative",
            &format!("{label} の子"),
            Some(&parent_a),
            &format!("{label}-child"),
        )
        .await;
        let sibling = item(
            &service,
            actor,
            &workspace_a,
            "initiative",
            &format!("{label} の兄弟"),
            Some(&parent_a),
            &format!("{label}-sibling"),
        )
        .await;
        assert_ne!(child, sibling);
    }
    let tree = breakdown(&service, &owner, &workspace_a, &parent_a).await;
    let children = tree["nodes"].as_array().unwrap();
    assert_eq!(
        children.len(),
        6,
        "root plus five children should be visible"
    );
    assert!(children
        .iter()
        .skip(1)
        .all(|node| node["parent_id"] == parent_a));

    // A viewer can read the graph but cannot create either kind of relative
    // item. Authorization runs before item validation and before any write.
    for (index, label) in ["子", "兄弟"].into_iter().enumerate() {
        let denied = call(
            &service,
            &viewer,
            "POST",
            &format!("/v1/workspaces/{workspace_a}/items"),
            json!({
                "kind":"initiative",
                "title":format!("viewer の{label}"),
                "parent_id":parent_a
            }),
            &format!("viewer-relative-{index}"),
        )
        .await
        .unwrap_err();
        assert_eq!(denied.status, 403);
    }
    let after_viewer = breakdown(&service, &viewer, &workspace_a, &parent_a).await;
    assert_eq!(after_viewer["nodes"].as_array().unwrap().len(), 6);

    // A child id is not portable to a sibling-order mutation in another
    // workspace, even when the caller owns that other workspace.
    let foreign_order = call(
        &service,
        &other_owner,
        "POST",
        &format!("/v1/workspaces/{workspace_b}/items/{parent_b}/children"),
        json!({"order":[child_a]}),
        "foreign-sibling-order",
    )
    .await
    .unwrap_err();
    assert_eq!(foreign_order.status, 422);

    // Membership on A does not authorize the same actor on B. This checks
    // the workspace segment itself for child creation and sibling ordering.
    for (index, actor) in [&owner, &editor, &viewer].into_iter().enumerate() {
        let denied_child = call(
            &service,
            actor,
            "POST",
            &format!("/v1/workspaces/{workspace_b}/items"),
            json!({"kind":"initiative","title":"越境子","parent_id":parent_b}),
            &format!("cross-child-{index}"),
        )
        .await
        .unwrap_err();
        assert_eq!(denied_child.status, 404);

        let denied_sibling = call(
            &service,
            actor,
            "POST",
            &format!("/v1/workspaces/{workspace_b}/items/{parent_b}/children"),
            json!({"order":[]}),
            &format!("cross-sibling-{index}"),
        )
        .await
        .unwrap_err();
        assert_eq!(denied_sibling.status, 404);
    }

    // A B owner cannot attach an A parent: validation looks up both ends in
    // B and the transaction rolls back the provisional item.
    let foreign_parent = call(
        &service,
        &other_owner,
        "POST",
        &format!("/v1/workspaces/{workspace_b}/items"),
        json!({"kind":"initiative","title":"外部親","parent_id":parent_a}),
        "foreign-parent",
    )
    .await
    .unwrap_err();
    assert_eq!(foreign_parent.status, 404);
    let b_tree = breakdown(&service, &other_owner, &workspace_b, &parent_b).await;
    assert_eq!(b_tree["nodes"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn action_completion_requires_write_membership_and_does_not_cross_workspaces() {
    let (_dir, service) = service().await;
    let owner = person("owner-a");
    let editor = person("editor-a");
    let viewer = person("viewer-a");
    let other_owner = person("owner-b");
    let workspace_a = workspace(&service, &owner, "A", "action-workspace-a").await;
    let workspace_b = workspace(&service, &other_owner, "B", "action-workspace-b").await;
    add_membership(&service, &workspace_a, &editor.id, "editor").await;
    add_membership(&service, &workspace_a, &viewer.id, "viewer").await;

    let owner_action = item(
        &service,
        &owner,
        &workspace_a,
        "action",
        "owner が完了",
        None,
        "owner-action",
    )
    .await;
    let editor_action = item(
        &service,
        &editor,
        &workspace_a,
        "action",
        "editor が完了",
        None,
        "editor-action",
    )
    .await;
    let viewer_action = item(
        &service,
        &owner,
        &workspace_a,
        "action",
        "viewer は完了不可",
        None,
        "viewer-action",
    )
    .await;
    let foreign_action = item(
        &service,
        &owner,
        &workspace_a,
        "action",
        "別 workspace から見えない",
        None,
        "foreign-action",
    )
    .await;
    let action_b = item(
        &service,
        &other_owner,
        &workspace_b,
        "action",
        "B の行動",
        None,
        "action-b",
    )
    .await;

    let owner_done = complete(
        &service,
        &owner,
        &workspace_a,
        &owner_action,
        "owner-complete",
    )
    .await
    .unwrap();
    assert_eq!(owner_done["item"]["state"], "done");
    let editor_done = complete(
        &service,
        &editor,
        &workspace_a,
        &editor_action,
        "editor-complete",
    )
    .await
    .unwrap();
    assert_eq!(editor_done["item"]["state"], "done");

    let viewer_denied = complete(
        &service,
        &viewer,
        &workspace_a,
        &viewer_action,
        "viewer-complete",
    )
    .await
    .unwrap_err();
    assert_eq!(viewer_denied.status, 403);
    assert_eq!(
        get(
            &service,
            &viewer,
            &format!("/v1/workspaces/{workspace_a}/items/{viewer_action}"),
        )
        .await
        .unwrap()["state"],
        "active"
    );

    // None of the A memberships grants access to B's action route.
    for (index, actor) in [&owner, &editor, &viewer].into_iter().enumerate() {
        let denied = complete(
            &service,
            actor,
            &workspace_b,
            &action_b,
            &format!("cross-action-{index}"),
        )
        .await
        .unwrap_err();
        assert_eq!(denied.status, 404);
    }

    // Even a B owner cannot use the B route to complete an item stored in A.
    let foreign_denied = complete(
        &service,
        &other_owner,
        &workspace_b,
        &foreign_action,
        "foreign-action-id",
    )
    .await
    .unwrap_err();
    assert_eq!(foreign_denied.status, 404);
    assert_eq!(
        get(
            &service,
            &owner,
            &format!("/v1/workspaces/{workspace_a}/items/{foreign_action}"),
        )
        .await
        .unwrap()["state"],
        "active"
    );

    // The B owner can still complete B's own action, proving the negative
    // cases did not poison the workspace or idempotency records.
    let b_done = complete(
        &service,
        &other_owner,
        &workspace_b,
        &action_b,
        "b-owner-complete",
    )
    .await
    .unwrap();
    assert_eq!(b_done["item"]["state"], "done");
}
