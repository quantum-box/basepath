use crate::{
    db::Tx,
    model::*,
    params,
    service::{new_id, now, only, text, title, version, Actor},
    storage::{authorize, role, sequence, value},
};
use chrono::{Duration, Utc};
use serde_json::{json, Value};

async fn workspace(tx: &mut Tx, w: &str) -> Result<Workspace> {
    let sql = format!(
        "SELECT body,tenant_id FROM workspaces WHERE id=?{}",
        tx.lock_reads()
    );
    let row = tx
        .fetch_optional(&sql, &params![w])
        .await?
        .ok_or_else(ApiError::missing)?;
    // The column is the tenant of record; the copy in the body is only what
    // the API hands back. Reading it back from the column keeps a rewritten
    // body from being able to move a workspace between tenants.
    let mut workspace: Workspace = serde_json::from_str(&row.text(0)?)?;
    workspace.tenant_id = row.text(1)?;
    Ok(workspace)
}
async fn owner(tx: &mut Tx, actor: &Actor, w: &str) -> Result<()> {
    authorize(tx, actor, w, false).await?;
    if actor.agent || role(tx, w, &actor.id).await?.as_deref() != Some("owner") {
        return Err(ApiError::new(
            403,
            "OWNER_REQUIRED",
            "この操作はオーナーだけが行えます",
        ));
    }
    Ok(())
}
fn shared(w: &Workspace) -> Result<()> {
    if w.scope == "個人" {
        return Err(ApiError::new(
            403,
            "PERSONAL_WORKSPACE",
            "個人のワークスペースは共有できません。新しい共有領域を作成してください",
        ));
    }
    Ok(())
}
async fn save_workspace(tx: &mut Tx, w: &mut Workspace) -> Result<()> {
    w.version += 1;
    tx.execute(
        "UPDATE workspaces SET body=? WHERE id=?",
        &params![serde_json::to_string(w)?, &w.id],
    )
    .await?;
    Ok(())
}
// The workspace name lives inside the workspace JSON body. It is parsed in
// Rust rather than with a JSON path expression so the same statement runs on
// SQLite and TiDB.
const INVITATION_SELECT: &str = "SELECT i.id,i.workspace_id,w.body,i.target_actor,i.role,i.status,i.created_by,i.created_at,i.expires_at,i.version FROM invitations i JOIN workspaces w ON w.id=i.workspace_id";
fn invitation_row(row: &crate::db::Row) -> Result<Invitation> {
    let workspace: Workspace = serde_json::from_str(&row.text(2)?)?;
    let mut invitation = Invitation {
        id: row.text(0)?,
        workspace_id: row.text(1)?,
        workspace_name: workspace.name,
        target_actor: row.text(3)?,
        role: row.text(4)?,
        status: row.text(5)?,
        created_by: row.text(6)?,
        created_at: row.text(7)?,
        expires_at: row.text(8)?,
        version: row.int(9)?,
    };
    if invitation.status == "pending" && invitation.expires_at < now() {
        invitation.status = "expired".into();
    }
    Ok(invitation)
}
async fn invitation(tx: &mut Tx, id: &str) -> Result<Invitation> {
    let sql = format!("{INVITATION_SELECT} WHERE i.id=?{}", tx.lock_reads());
    let row = tx
        .fetch_optional(&sql, &params![id])
        .await?
        .ok_or_else(ApiError::missing)?;
    invitation_row(&row)
}

// Authorization also runs before returning previously saved mutation responses.
pub(crate) async fn authorize_route(
    tx: &mut Tx,
    actor: &Actor,
    method: &str,
    p: &[&str],
) -> Result<()> {
    match p {
        ["v1", "workspaces", w] if method == "PATCH" => owner(tx, actor, w).await?,
        ["v1", "workspaces", w, "members", _] if method != "GET" => owner(tx, actor, w).await?,
        ["v1", "workspaces", w, "invitations", ..] if method != "GET" => {
            owner(tx, actor, w).await?
        }
        ["v1", "invitations", id, "accept" | "decline"] => {
            let i = invitation(tx, id).await?;
            if actor.agent || i.target_actor != actor.id {
                return Err(ApiError::missing());
            }
            // Where the tenant boundary is actually enforced for sharing.
            //
            // Issuing an invitation cannot check the target's tenants:
            // PathBase knows which Tachyon tenants *this* person belongs to,
            // not which ones somebody else does. Accepting can, because the
            // person accepting is the one making the request — so an
            // invitation only ever becomes membership when its recipient is
            // acting in the workspace's own tenant. An invitation addressed
            // across a tenant boundary is simply never redeemable.
            if crate::storage::workspace_tenant(tx, &i.workspace_id)
                .await?
                .is_none_or(|tenant| tenant.is_empty() || tenant != actor.tenant)
            {
                return Err(ApiError::missing());
            }
            if matches!(i.status.as_str(), "revoked" | "expired")
                || (p[3] == "accept" && i.status == "declined")
            {
                return Err(ApiError::new(
                    409,
                    "INVITATION_UNAVAILABLE",
                    "この招待は取り消されたか、有効期限が切れています",
                ));
            }
            // Replaying a previously accepted invitation cannot restore revoked access.
            if i.status == "accepted" {
                authorize(tx, actor, &i.workspace_id, false).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

// Idempotency preserves the original response, but an invitation response must not
// look usable after the underlying invitation has expired or been revoked.
pub(crate) async fn authorize_replay(
    tx: &mut Tx,
    method: &str,
    p: &[&str],
    response: &Value,
) -> Result<()> {
    if let ("POST", ["v1", "workspaces", workspace_id, "invitations"]) = (method, p) {
        let id = response["id"].as_str().ok_or_else(ApiError::missing)?;
        let current = invitation(tx, id).await?;
        if current.workspace_id != *workspace_id || current.status != "pending" {
            return Err(ApiError::new(
                409,
                "INVITATION_UNAVAILABLE",
                "この招待は取り消されたか、有効期限が切れています",
            ));
        }
    }
    Ok(())
}

async fn management(tx: &mut Tx, actor: &Actor, w: &str) -> Result<Value> {
    authorize(tx, actor, w, false).await?;
    let mut ws = workspace(tx, w).await?;
    ws.role = role(tx, w, &actor.id)
        .await?
        .ok_or_else(ApiError::missing)?;
    let rows = tx
        .fetch_all(
            "SELECT actor,role FROM memberships WHERE workspace_id=? \
             ORDER BY CASE WHEN role='owner' THEN 0 ELSE 1 END,actor",
            &params![w],
        )
        .await?;
    let members = rows
        .iter()
        .map(|row| {
            let actor_id = row.text(0)?;
            // PathBase does not copy a person's profile into its own
            // database.  The authenticated person's actual Tachyon name is
            // available to the browser through /v1/me; other members may be
            // named by the Tachyon/Field directory in a later integration.
            // Still return a stable, human-readable kind now so consumers do
            // not have to render a bare opaque actor id as if it were a name.
            let local_preview = actor.id == Actor::local().id;
            let display_name = if actor_id == actor.id && local_preview {
                "ローカルプレビュー（あなた）"
            } else if local_preview {
                "ローカルプレビュー"
            } else if actor_id == actor.id {
                "あなたのTachyonアカウント"
            } else {
                "Tachyonアカウント"
            };
            Ok(json!({
                "actor": actor_id,
                "display_name": display_name,
                "role": row.text(1)?
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    let invites = if ws.role == "owner" && !actor.agent {
        let rows = tx
            .fetch_all(
                &format!(
                    "{INVITATION_SELECT} WHERE i.workspace_id=? AND i.status='pending' \
                     AND i.expires_at>? ORDER BY i.created_at DESC"
                ),
                &params![w, now()],
            )
            .await?;
        rows.iter()
            .map(invitation_row)
            .collect::<Result<Vec<_>>>()?
    } else {
        vec![]
    };
    Ok(json!({"workspace":ws,"members":members,"invitations":invites}))
}
fn validate_role(body: &Value, allow_owner: bool) -> Result<String> {
    let r = text(body, "role");
    if !(["editor", "viewer"].contains(&r) || allow_owner && r == "owner") {
        return Err(ApiError::invalid("権限を選択してください"));
    }
    Ok(r.into())
}
async fn change_member(
    tx: &mut Tx,
    w: &mut Workspace,
    target: &str,
    next: Option<&str>,
) -> Result<()> {
    let previous = role(tx, &w.id, target)
        .await?
        .ok_or_else(ApiError::missing)?;
    if previous == "owner" && next != Some("owner") {
        let sql = format!(
            "SELECT actor FROM memberships WHERE workspace_id=? AND role='owner'{}",
            tx.lock_reads()
        );
        let owners = tx.fetch_all(&sql, &params![&w.id]).await?.len() as i64;
        if owners <= 1 {
            return Err(ApiError::new(
                409,
                "LAST_OWNER",
                "別のメンバーをオーナーにしてから変更してください",
            ));
        }
    }
    if let Some(next) = next {
        tx.execute(
            "UPDATE memberships SET role=? WHERE workspace_id=? AND actor=?",
            &params![next, &w.id, target],
        )
        .await?;
    } else {
        tx.execute(
            "DELETE FROM memberships WHERE workspace_id=? AND actor=?",
            &params![&w.id, target],
        )
        .await?;
        tx.execute(
            "UPDATE invitations SET status='revoked',version=version+1 \
             WHERE workspace_id=? AND target_actor=? AND status IN ('pending','accepted')",
            &params![&w.id, target],
        )
        .await?;
    }
    if next != Some("owner") {
        tx.execute(
            "UPDATE invitations SET status='revoked',version=version+1 \
             WHERE workspace_id=? AND created_by=? AND status='pending'",
            &params![&w.id, target],
        )
        .await?;
    }
    save_workspace(tx, w).await
}

pub(crate) fn routes(method: &str, p: &[&str]) -> bool {
    matches!(
        (method, p),
        ("POST", ["v1", "workspaces"])
            | ("PATCH", ["v1", "workspaces", _])
            | ("GET", ["v1", "workspaces", _, "members"])
            | ("POST", ["v1", "workspaces", _, "leave"])
            | ("PATCH" | "DELETE", ["v1", "workspaces", _, "members", _])
            | ("POST", ["v1", "workspaces", _, "invitations"])
            | ("DELETE", ["v1", "workspaces", _, "invitations", _])
            | ("GET", ["v1", "invitations"])
            | ("POST", ["v1", "invitations", _, "accept" | "decline"])
    )
}

pub(crate) async fn dispatch(
    tx: &mut Tx,
    actor: &Actor,
    method: &str,
    p: &[&str],
    b: &Value,
) -> Result<Value> {
    if actor.agent && method != "GET" {
        return Err(ApiError::new(
            403,
            "APPROVAL_REQUIRED",
            "共有と権限の変更は画面から行ってください",
        ));
    }
    authorize_route(tx, actor, method, p).await?;
    match (method, p) {
        ("POST", ["v1", "workspaces"]) => {
            only(b, &["name", "scope", "timezone"])?;
            let scope = text(b, "scope");
            let timezone = b["timezone"].as_str().unwrap_or("Asia/Tokyo");
            if !["チーム", "組織"].contains(&scope) || timezone.parse::<chrono_tz::Tz>().is_err()
            {
                return Err(ApiError::invalid("領域とタイムゾーンを確認してください"));
            }
            if actor.tenant.is_empty() {
                return Err(ApiError::new(
                    428,
                    "TENANT_SELECTION_REQUIRED",
                    "利用するTachyonテナントを選択してください",
                ));
            }
            // A workspace is created *into* the tenant its creator is acting
            // in, and never moves. That is the whole of how a workspace
            // acquires a tenant: there is no later step that could assign a
            // different one, and no request that can name one.
            let ws = Workspace {
                id: new_id("workspace"),
                name: title(b, "name", 100)?,
                tenant_id: actor.tenant.clone(),
                scope: scope.into(),
                timezone: timezone.into(),
                role: "owner".into(),
                local: actor.id == "local-owner",
                version: 1,
            };
            tx.execute(
                "INSERT INTO workspaces(id,body,seq,tenant_id) VALUES(?,?,?,?)",
                &params![
                    &ws.id,
                    serde_json::to_string(&ws)?,
                    sequence(),
                    &ws.tenant_id
                ],
            )
            .await?;
            tx.execute(
                "INSERT INTO memberships(workspace_id,actor,role) VALUES(?,?,'owner')",
                &params![&ws.id, &actor.id],
            )
            .await?;
            value(ws)
        }
        ("GET", ["v1", "invitations"]) => {
            let rows = tx
                .fetch_all(
                    &format!(
                        "{INVITATION_SELECT} WHERE i.target_actor=? AND i.status='pending' \
                         AND w.tenant_id=? AND i.expires_at>? ORDER BY i.created_at DESC"
                    ),
                    &params![&actor.id, &actor.tenant, now()],
                )
                .await?;
            value(
                rows.iter()
                    .map(invitation_row)
                    .collect::<Result<Vec<_>>>()?,
            )
        }
        ("POST", ["v1", "invitations", id, action]) => {
            only(b, &["expected_version"])?;
            let workspace_id = invitation(tx, id).await?.workspace_id;
            // The route carries no workspace segment, so the shared lock has
            // not been taken yet. Re-read the invitation behind it.
            tx.lock_workspace(&workspace_id).await?;
            let i = invitation(tx, id).await?;
            version(b, i.version)?;
            if i.status != "pending" {
                return Err(ApiError::new(
                    409,
                    "INVITATION_UNAVAILABLE",
                    "この招待は使用済みか、有効期限が切れています",
                ));
            }
            let mut ws = workspace(tx, &i.workspace_id).await?;
            shared(&ws)?;
            // A departed or demoted inviter cannot grant access through an old invitation.
            if role(tx, &ws.id, &i.created_by).await?.as_deref() != Some("owner") {
                return Err(ApiError::new(
                    409,
                    "INVITATION_UNAVAILABLE",
                    "招待者の権限が変わりました。新しい招待を受け取ってください",
                ));
            }
            let accepted = *action == "accept";
            if accepted {
                if role(tx, &ws.id, &actor.id).await?.is_some() {
                    return Err(ApiError::new(409, "ALREADY_MEMBER", "すでに参加しています"));
                }
                tx.execute(
                    "INSERT INTO memberships(workspace_id,actor,role) VALUES(?,?,?)",
                    &params![&ws.id, &actor.id, &i.role],
                )
                .await?;
            }
            let status = if accepted { "accepted" } else { "declined" };
            tx.execute(
                "UPDATE invitations SET status=?,version=version+1 WHERE id=?",
                &params![status, id],
            )
            .await?;
            save_workspace(tx, &mut ws).await?;
            tx.execute(
                "INSERT INTO audit(id,workspace_id,actor,origin,command,created_at,seq,connection) \
                 VALUES(?,?,?,'ui',?,?,?,'')",
                &params![
                    new_id("audit"),
                    &ws.id,
                    &actor.id,
                    format!("invitation {status} {id}"),
                    now(),
                    sequence()
                ],
            )
            .await?;
            Ok(json!({"workspace_id":ws.id,"status":status}))
        }
        ("GET", ["v1", "workspaces", w, "members"]) => management(tx, actor, w).await,
        (_, ["v1", "workspaces", w, ..]) => {
            authorize(tx, actor, w, false).await?;
            let mut ws = workspace(tx, w).await?;
            shared(&ws)?;
            version(b, ws.version)?;
            match (method, p) {
                ("PATCH", [_, _, _]) => {
                    only(b, &["name", "timezone", "expected_version"])?;
                    ws.name = title(b, "name", 100)?;
                    ws.timezone = title(b, "timezone", 100)?;
                    if ws.timezone.parse::<chrono_tz::Tz>().is_err() {
                        return Err(ApiError::invalid("タイムゾーンを確認してください"));
                    }
                    save_workspace(tx, &mut ws).await?;
                    value(ws)
                }
                ("PATCH" | "DELETE", [_, _, _, "members", target]) => {
                    only(
                        b,
                        if method == "PATCH" {
                            &["role", "expected_version"]
                        } else {
                            &["expected_version"]
                        },
                    )?;
                    let next = if method == "PATCH" {
                        Some(validate_role(b, true)?)
                    } else {
                        None
                    };
                    change_member(tx, &mut ws, target, next.as_deref()).await?;
                    Ok(json!({"workspace_id":w,"version":ws.version}))
                }
                ("POST", [_, _, _, "leave"]) => {
                    only(b, &["expected_version"])?;
                    let actor_id = actor.id.clone();
                    change_member(tx, &mut ws, &actor_id, None).await?;
                    Ok(json!({"left":true}))
                }
                ("POST", [_, _, _, "invitations"]) => {
                    only(b, &["target_actor", "role", "expected_version"])?;
                    if ws.local {
                        return Err(ApiError::new(
                            403,
                            "LOCAL_WORKSPACE",
                            "ローカル確認用の領域は他のアカウントに共有できません",
                        ));
                    }
                    let target = title(b, "target_actor", 100)?;
                    if !target.starts_with("us_")
                        || target.len() <= 3
                        || !target
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
                    {
                        return Err(ApiError::invalid(
                            "相手のTachyonユーザーID（us_から始まるID）を入力してください",
                        ));
                    }
                    if role(tx, w, &target).await?.is_some() {
                        return Err(ApiError::new(
                            409,
                            "ALREADY_MEMBER",
                            "このユーザーはすでに参加しています",
                        ));
                    }
                    let role = validate_role(b, false)?;
                    let sql = format!(
                        "SELECT 1 FROM invitations WHERE workspace_id=? AND target_actor=? \
                         AND status='pending' AND expires_at>?{}",
                        tx.lock_reads()
                    );
                    let existing = tx
                        .fetch_optional(&sql, &params![w, &target, now()])
                        .await?
                        .is_some();
                    if existing {
                        return Err(ApiError::new(
                            409,
                            "INVITATION_EXISTS",
                            "このユーザーへの招待は送信済みです。取り消してから再招待してください",
                        ));
                    }
                    let id = new_id("invite");
                    tx.execute(
                        "INSERT INTO invitations(id,workspace_id,target_actor,role,status,\
                         created_by,created_at,expires_at,version) \
                         VALUES(?,?,?,?,'pending',?,?,?,1)",
                        &params![
                            &id,
                            w,
                            target,
                            role,
                            &actor.id,
                            now(),
                            (Utc::now() + Duration::days(7)).to_rfc3339()
                        ],
                    )
                    .await?;
                    save_workspace(tx, &mut ws).await?;
                    value(invitation(tx, &id).await?)
                }
                ("DELETE", [_, _, _, "invitations", id]) => {
                    only(b, &["expected_version"])?;
                    let i = invitation(tx, id).await?;
                    if i.workspace_id != *w {
                        return Err(ApiError::missing());
                    }
                    if !["pending", "expired"].contains(&i.status.as_str()) {
                        return Err(ApiError::new(
                            409,
                            "INVITATION_UNAVAILABLE",
                            "この招待はすでに処理されています",
                        ));
                    }
                    tx.execute(
                        "UPDATE invitations SET status='revoked',version=version+1 WHERE id=?",
                        &params![id],
                    )
                    .await?;
                    save_workspace(tx, &mut ws).await?;
                    Ok(json!({"revoked":true}))
                }
                _ => Err(ApiError::missing()),
            }
        }
        _ => Err(ApiError::missing()),
    }
}
