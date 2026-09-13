use crate::{
    model::*,
    service::{new_id, now, only, text, title, version, Actor},
    storage::{authorize, value},
};
use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

fn workspace(db: &Connection, w: &str) -> Result<Workspace> {
    let raw: Option<String> = db
        .query_row("SELECT body FROM workspaces WHERE id=?1", [w], |r| r.get(0))
        .optional()?;
    Ok(serde_json::from_str(&raw.ok_or_else(ApiError::missing)?)?)
}
fn role(db: &Connection, w: &str, actor: &str) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT role FROM memberships WHERE workspace_id=?1 AND actor=?2",
            params![w, actor],
            |r| r.get(0),
        )
        .optional()?)
}
fn owner(db: &Connection, actor: &Actor, w: &str) -> Result<()> {
    authorize(db, &actor.id, w, false)?;
    if actor.agent || role(db, w, &actor.id)?.as_deref() != Some("owner") {
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
fn save_workspace(db: &Connection, w: &mut Workspace) -> Result<()> {
    w.version += 1;
    db.execute(
        "UPDATE workspaces SET body=?2 WHERE id=?1",
        params![w.id, serde_json::to_string(w)?],
    )?;
    Ok(())
}
fn invitation_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Invitation> {
    let mut i = Invitation {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        workspace_name: r.get(2)?,
        target_actor: r.get(3)?,
        role: r.get(4)?,
        status: r.get(5)?,
        created_by: r.get(6)?,
        created_at: r.get(7)?,
        expires_at: r.get(8)?,
        version: r.get(9)?,
    };
    if i.status == "pending" && i.expires_at < now() {
        i.status = "expired".into();
    }
    Ok(i)
}
const INVITATION_SELECT: &str = "SELECT i.id,i.workspace_id,json_extract(w.body,'$.name'),i.target_actor,i.role,i.status,i.created_by,i.created_at,i.expires_at,i.version FROM invitations i JOIN workspaces w ON w.id=i.workspace_id";
fn invitation(db: &Connection, id: &str) -> Result<Invitation> {
    db.query_row(
        &format!("{INVITATION_SELECT} WHERE i.id=?1"),
        [id],
        invitation_row,
    )
    .optional()?
    .ok_or_else(ApiError::missing)
}

// Authorization also runs before returning previously saved mutation responses.
pub(crate) fn authorize_route(
    db: &Connection,
    actor: &Actor,
    method: &str,
    p: &[&str],
) -> Result<()> {
    match p {
        ["v1", "workspaces", w] if method == "PATCH" => owner(db, actor, w)?,
        ["v1", "workspaces", w, "members", _] if method != "GET" => owner(db, actor, w)?,
        ["v1", "workspaces", w, "invitations", ..] if method != "GET" => owner(db, actor, w)?,
        ["v1", "invitations", id, "accept" | "decline"] => {
            let i = invitation(db, id)?;
            if actor.agent || i.target_actor != actor.id {
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
                authorize(db, &actor.id, &i.workspace_id, false)?;
            }
        }
        _ => {}
    }
    Ok(())
}

// Idempotency preserves the original response, but an invitation response must not
// look usable after the underlying invitation has expired or been revoked.
pub(crate) fn authorize_replay(
    db: &Connection,
    method: &str,
    p: &[&str],
    response: &Value,
) -> Result<()> {
    if let ("POST", ["v1", "workspaces", workspace_id, "invitations"]) = (method, p) {
        let id = response["id"].as_str().ok_or_else(ApiError::missing)?;
        let current = invitation(db, id)?;
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

fn management(db: &Connection, actor: &Actor, w: &str) -> Result<Value> {
    authorize(db, &actor.id, w, false)?;
    let mut ws = workspace(db, w)?;
    ws.role = role(db, w, &actor.id)?.ok_or_else(ApiError::missing)?;
    let mut q = db.prepare(
        "SELECT actor,role FROM memberships WHERE workspace_id=?1 ORDER BY role='owner' DESC,actor",
    )?;
    let members = q
        .query_map([w], |r| {
            Ok(json!({"actor":r.get::<_,String>(0)?,"role":r.get::<_,String>(1)?}))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let invites = if ws.role == "owner" && !actor.agent {
        let mut q = db.prepare(&format!("{INVITATION_SELECT} WHERE i.workspace_id=?1 AND i.status='pending' AND i.expires_at>?2 ORDER BY i.created_at DESC"))?;
        let rows = q.query_map(params![w, now()], invitation_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
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
fn change_member(
    db: &Connection,
    w: &mut Workspace,
    target: &str,
    next: Option<&str>,
) -> Result<()> {
    let previous = role(db, &w.id, target)?.ok_or_else(ApiError::missing)?;
    if previous == "owner" && next != Some("owner") {
        let owners: i64 = db.query_row(
            "SELECT COUNT(*) FROM memberships WHERE workspace_id=?1 AND role='owner'",
            [&w.id],
            |r| r.get(0),
        )?;
        if owners <= 1 {
            return Err(ApiError::new(
                409,
                "LAST_OWNER",
                "別のメンバーをオーナーにしてから変更してください",
            ));
        }
    }
    if let Some(next) = next {
        db.execute(
            "UPDATE memberships SET role=?3 WHERE workspace_id=?1 AND actor=?2",
            params![w.id, target, next],
        )?;
    } else {
        db.execute(
            "DELETE FROM memberships WHERE workspace_id=?1 AND actor=?2",
            params![w.id, target],
        )?;
        db.execute("UPDATE invitations SET status='revoked',version=version+1 WHERE workspace_id=?1 AND target_actor=?2 AND status IN ('pending','accepted')", params![w.id,target])?;
    }
    if next != Some("owner") {
        db.execute("UPDATE invitations SET status='revoked',version=version+1 WHERE workspace_id=?1 AND created_by=?2 AND status='pending'", params![w.id,target])?;
    }
    save_workspace(db, w)
}

pub(crate) fn dispatch(
    db: &Connection,
    actor: &Actor,
    method: &str,
    p: &[&str],
    b: &Value,
) -> Option<Result<Value>> {
    let is_route = matches!(
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
    );
    if !is_route {
        return None;
    }
    Some((|| {
        if actor.agent && method != "GET" {
            return Err(ApiError::new(
                403,
                "APPROVAL_REQUIRED",
                "共有と権限の変更は画面から行ってください",
            ));
        }
        authorize_route(db, actor, method, p)?;
        match (method, p) {
            ("POST", ["v1", "workspaces"]) => {
                only(b, &["name", "scope", "timezone"])?;
                let scope = text(b, "scope");
                let timezone = b["timezone"].as_str().unwrap_or("Asia/Tokyo");
                if !["チーム", "組織"].contains(&scope)
                    || timezone.parse::<chrono_tz::Tz>().is_err()
                {
                    return Err(ApiError::invalid("領域とタイムゾーンを確認してください"));
                }
                let ws = Workspace {
                    id: new_id("workspace"),
                    name: title(b, "name", 100)?,
                    scope: scope.into(),
                    timezone: timezone.into(),
                    role: "owner".into(),
                    local: actor.id == "local-owner",
                    version: 1,
                };
                db.execute(
                    "INSERT INTO workspaces VALUES(?1,?2)",
                    params![ws.id, serde_json::to_string(&ws)?],
                )?;
                db.execute(
                    "INSERT INTO memberships VALUES(?1,?2,'owner')",
                    params![ws.id, actor.id],
                )?;
                value(ws)
            }
            ("GET", ["v1", "invitations"]) => {
                let mut q = db.prepare(&format!("{INVITATION_SELECT} WHERE i.target_actor=?1 AND i.status='pending' AND i.expires_at>?2 ORDER BY i.created_at DESC"))?;
                let rows = q
                    .query_map(params![actor.id, now()], invitation_row)?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                value(rows)
            }
            ("POST", ["v1", "invitations", id, action]) => {
                only(b, &["expected_version"])?;
                let i = invitation(db, id)?;
                version(b, i.version)?;
                if i.status != "pending" {
                    return Err(ApiError::new(
                        409,
                        "INVITATION_UNAVAILABLE",
                        "この招待は使用済みか、有効期限が切れています",
                    ));
                }
                let mut ws = workspace(db, &i.workspace_id)?;
                shared(&ws)?;
                // A departed or demoted inviter cannot grant access through an old invitation.
                if role(db, &ws.id, &i.created_by)?.as_deref() != Some("owner") {
                    return Err(ApiError::new(
                        409,
                        "INVITATION_UNAVAILABLE",
                        "招待者の権限が変わりました。新しい招待を受け取ってください",
                    ));
                }
                let accepted = *action == "accept";
                if accepted {
                    if role(db, &ws.id, &actor.id)?.is_some() {
                        return Err(ApiError::new(409, "ALREADY_MEMBER", "すでに参加しています"));
                    }
                    db.execute(
                        "INSERT INTO memberships VALUES(?1,?2,?3)",
                        params![ws.id, actor.id, i.role],
                    )?;
                }
                let status = if accepted { "accepted" } else { "declined" };
                db.execute(
                    "UPDATE invitations SET status=?2,version=version+1 WHERE id=?1",
                    params![id, status],
                )?;
                save_workspace(db, &mut ws)?;
                db.execute(
                    "INSERT INTO audit VALUES(?1,?2,?3,'ui',?4,?5)",
                    params![
                        new_id("audit"),
                        ws.id,
                        actor.id,
                        format!("invitation {status} {id}"),
                        now()
                    ],
                )?;
                Ok(json!({"workspace_id":ws.id,"status":status}))
            }
            ("GET", ["v1", "workspaces", w, "members"]) => management(db, actor, w),
            (_, ["v1", "workspaces", w, ..]) => {
                authorize(db, &actor.id, w, false)?;
                let mut ws = workspace(db, w)?;
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
                        save_workspace(db, &mut ws)?;
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
                        change_member(db, &mut ws, target, next.as_deref())?;
                        Ok(json!({"workspace_id":w,"version":ws.version}))
                    }
                    ("POST", [_, _, _, "leave"]) => {
                        only(b, &["expected_version"])?;
                        change_member(db, &mut ws, &actor.id, None)?;
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
                        if role(db, w, &target)?.is_some() {
                            return Err(ApiError::new(
                                409,
                                "ALREADY_MEMBER",
                                "このユーザーはすでに参加しています",
                            ));
                        }
                        let role = validate_role(b, false)?;
                        let existing: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM invitations WHERE workspace_id=?1 AND target_actor=?2 AND status='pending' AND expires_at>?3)",params![w,target,now()],|r|r.get(0))?;
                        if existing {
                            return Err(ApiError::new(409,"INVITATION_EXISTS","このユーザーへの招待は送信済みです。取り消してから再招待してください"));
                        }
                        let id = new_id("invite");
                        db.execute(
                            "INSERT INTO invitations VALUES(?1,?2,?3,?4,'pending',?5,?6,?7,1)",
                            params![
                                id,
                                w,
                                target,
                                role,
                                actor.id,
                                now(),
                                (Utc::now() + Duration::days(7)).to_rfc3339()
                            ],
                        )?;
                        save_workspace(db, &mut ws)?;
                        value(invitation(db, &id)?)
                    }
                    ("DELETE", [_, _, _, "invitations", id]) => {
                        only(b, &["expected_version"])?;
                        let i = invitation(db, id)?;
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
                        db.execute(
                            "UPDATE invitations SET status='revoked',version=version+1 WHERE id=?1",
                            [id],
                        )?;
                        save_workspace(db, &mut ws)?;
                        Ok(json!({"revoked":true}))
                    }
                    _ => Err(ApiError::missing()),
                }
            }
            _ => Err(ApiError::missing()),
        }
    })())
}
