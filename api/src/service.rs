use crate::{model::*, storage::*};
use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct Service {
    pub db: Arc<Mutex<Connection>>,
}
#[derive(Clone, Debug)]
pub struct Actor {
    pub id: String,
    pub agent: bool,
}
impl Actor {
    pub fn local() -> Self {
        Self {
            id: "local-owner".into(),
            agent: false,
        }
    }
}
pub fn now() -> String {
    Utc::now().to_rfc3339()
}
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4())
}
pub(crate) fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
pub(crate) fn title(v: &Value, key: &str, max: usize) -> Result<String> {
    let s = text(v, key).trim();
    if s.is_empty() || s.chars().count() > max {
        return Err(ApiError::invalid(&format!(
            "{key}は1〜{max}文字で入力してください"
        )));
    }
    Ok(s.into())
}
pub(crate) fn only(v: &Value, fields: &[&str]) -> Result<()> {
    let obj = v
        .as_object()
        .ok_or_else(|| ApiError::invalid("JSONオブジェクトを指定してください"))?;
    if let Some(k) = obj.keys().find(|k| !fields.contains(&k.as_str())) {
        return Err(ApiError::invalid(&format!(
            "変更できないフィールドです: {k}"
        )));
    }
    Ok(())
}
fn date(v: &Option<String>) -> Result<()> {
    if let Some(d) = v {
        if NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() || d.len() != 10 {
            return Err(ApiError::invalid("日付はYYYY-MM-DDで指定してください"));
        }
    }
    Ok(())
}
fn timestamp(s: &str) -> Result<()> {
    DateTime::parse_from_rfc3339(s)
        .map(|_| ())
        .map_err(|_| ApiError::invalid("日時はタイムゾーンを含むRFC3339で指定してください"))
}
pub(crate) fn version(v: &Value, current: i64) -> Result<()> {
    let n = v["expected_version"]
        .as_i64()
        .ok_or_else(|| ApiError::new(428, "VERSION_REQUIRED", "expected_versionが必要です"))?;
    if n != current {
        let mut e = ApiError::new(
            409,
            "VERSION_CONFLICT",
            "別の操作で更新されています。入力を保持したまま最新の内容を確認してください",
        );
        e.details = json!({"expected_version":n,"current_version":current});
        return Err(e);
    }
    Ok(())
}
fn fingerprint(method: &str, path: &str, body: &Value) -> String {
    format!("{:x}", Sha256::digest(format!("{method}\n{path}\n{body}")))
}
fn validate_item(db: &Connection, item: &Item) -> Result<()> {
    if item.title.trim().is_empty()
        || item.title.chars().count() > 200
        || item.description.chars().count() > 10000
        || item.fields.memo.chars().count() > 20000
    {
        return Err(ApiError::invalid(
            "タイトルまたは本文の長さを確認してください",
        ));
    }
    if !["idea", "outcome", "initiative", "action", "milestone"].contains(&item.kind.as_str()) {
        return Err(ApiError::invalid("不明な項目の種類です"));
    }
    if !["draft", "active", "paused", "done", "abandoned"].contains(&item.state.as_str()) {
        return Err(ApiError::invalid("不明な状態です"));
    }
    if item
        .fields
        .priority
        .as_deref()
        .is_some_and(|priority| !["low", "medium", "high", "urgent"].contains(&priority))
    {
        return Err(ApiError::invalid("優先度を確認してください"));
    }
    if let Some(assignee) = &item.fields.assignee_id {
        let member: Option<String> = db
            .query_row(
                "SELECT actor FROM memberships WHERE workspace_id=?1 AND actor=?2",
                params![item.workspace_id, assignee],
                |row| row.get(0),
            )
            .optional()?;
        if member.is_none() {
            return Err(ApiError::invalid(
                "担当者は現在のワークスペースメンバーから選択してください",
            ));
        }
        let workspace: Workspace = serde_json::from_str(&db.query_row(
            "SELECT body FROM workspaces WHERE id=?1",
            [&item.workspace_id],
            |row| row.get::<_, String>(0),
        )?)?;
        if workspace.scope == "個人" {
            let owner: String = db.query_row(
                "SELECT actor FROM memberships WHERE workspace_id=?1 AND role='owner' LIMIT 1",
                [&item.workspace_id],
                |row| row.get(0),
            )?;
            if assignee != &owner {
                return Err(ApiError::invalid("個人領域では本人だけを担当者にできます"));
            }
        }
    }
    date(&item.start_date)?;
    date(&item.due_date)?;
    date(&item.scheduled_date)?;
    if item
        .start_date
        .as_ref()
        .zip(item.due_date.as_ref())
        .is_some_and(|(s, e)| s > e)
    {
        return Err(ApiError::invalid("終了日は開始日以降にしてください"));
    }
    if let Some(t) = &item.scheduled_time {
        if NaiveTime::parse_from_str(t, "%H:%M").is_err() || t.len() != 5 {
            return Err(ApiError::invalid("時刻はHH:MMで指定してください"));
        }
    }
    if item
        .fields
        .self_assessment
        .is_some_and(|v| !v.is_finite() || !(0.0..=100.0).contains(&v))
    {
        return Err(ApiError::invalid("自己評価は0〜100で指定してください"));
    }
    if let Some(r) = &item.fields.recurrence {
        if item.kind != "action"
            || !["period_quota", "fixed_schedule"].contains(&r.mode.as_str())
            || r.times_per_week == 0
            || r.times_per_week > 7
            || r.timezone.parse::<chrono_tz::Tz>().is_err()
            || r.weekdays.iter().any(|d| *d > 6)
            || (r.mode == "fixed_schedule" && r.weekdays.is_empty())
        {
            return Err(ApiError::invalid(
                "習慣の回数・曜日・タイムゾーンを確認してください",
            ));
        }
    }
    if let Some(u) = &item.fields.external_url {
        if !(u.is_empty() || u.starts_with("https://") || u.starts_with("http://")) {
            return Err(ApiError::invalid(
                "参考リンクはHTTPまたはHTTPSのURLにしてください",
            ));
        }
    }
    if let Some(id) = &item.fields.next_action_id {
        let next: Item = get(db, &item.workspace_id, "items", id)?;
        if next.kind != "action" {
            return Err(ApiError::invalid("次の一歩には行動を指定してください"));
        }
    }
    Ok(())
}

fn create_notification(
    db: &Connection,
    item: &Item,
    recipient: &str,
    kind: &str,
    discriminator: &str,
    title: String,
) -> Result<()> {
    let digest = format!(
        "{:x}",
        Sha256::digest(format!(
            "{}\n{}\n{}\n{}\n{}",
            item.workspace_id, item.id, recipient, kind, discriminator
        ))
    );
    let notification = Notification {
        id: format!("notification_{}", &digest[..24]),
        workspace_id: item.workspace_id.clone(),
        recipient: recipient.into(),
        item_id: item.id.clone(),
        kind: kind.into(),
        title,
        created_at: now(),
        read_at: None,
    };
    db.execute(
        "INSERT OR IGNORE INTO documents(workspace_id,collection,id,body) VALUES(?1,'notifications',?2,?3)",
        params![item.workspace_id, notification.id, serde_json::to_string(&notification)?],
    )?;
    Ok(())
}

fn sync_due_notifications(db: &Connection, workspace_id: &str, actor: &str) -> Result<()> {
    let workspace: Workspace = serde_json::from_str(&db.query_row(
        "SELECT body FROM workspaces WHERE id=?1",
        [workspace_id],
        |row| row.get::<_, String>(0),
    )?)?;
    let timezone: chrono_tz::Tz = workspace
        .timezone
        .parse()
        .map_err(|_| ApiError::invalid("ワークスペースのタイムゾーンが不正です"))?;
    let today = Utc::now().with_timezone(&timezone).date_naive();
    for item in list::<Item>(db, workspace_id, "items")? {
        if item.archived_at.is_some()
            || item.state == "done"
            || item.fields.assignee_id.as_deref() != Some(actor)
        {
            continue;
        }
        let Some(raw_due) = item.due_date.as_deref() else {
            continue;
        };
        let due = NaiveDate::parse_from_str(raw_due, "%Y-%m-%d")
            .map_err(|_| ApiError::invalid("期限の日付が不正です"))?;
        let days = (due - today).num_days();
        let (kind, title) = match days {
            value if value < 0 => ("overdue", format!("期限超過: 「{}」", item.title)),
            0 => ("due_today", format!("今日が期限です: 「{}」", item.title)),
            1..=7 => ("due_soon", format!("7日以内が期限です: 「{}」", item.title)),
            _ => continue,
        };
        create_notification(db, &item, actor, kind, raw_due, title)?;
    }
    Ok(())
}
fn validate_relation(db: &Connection, r: &Relation) -> Result<()> {
    let _: Item = get(db, &r.workspace_id, "items", &r.source_id)?;
    let _: Item = get(db, &r.workspace_id, "items", &r.target_id)?;
    if r.source_id == r.target_id {
        return Err(ApiError::new(
            422,
            "CYCLE_DETECTED",
            "自分自身とは接続できません",
        ));
    }
    if !["part_of", "contributes_to", "depends_on", "relates_to"]
        .contains(&r.relation_type.as_str())
    {
        return Err(ApiError::invalid("不明な関係です"));
    }
    let all: Vec<Relation> = list(db, &r.workspace_id, "relations")?;
    let same: Vec<_> = all
        .iter()
        .filter(|x| x.id != r.id && x.relation_type == r.relation_type)
        .collect();
    if same.iter().any(|x| {
        (x.source_id == r.source_id && x.target_id == r.target_id)
            || (r.relation_type == "relates_to"
                && x.source_id == r.target_id
                && x.target_id == r.source_id)
    }) {
        return Err(ApiError::new(
            409,
            "DUPLICATE_RELATION",
            "この関係はすでに登録されています",
        ));
    }
    if r.relation_type == "part_of" && same.iter().any(|x| x.source_id == r.source_id) {
        return Err(ApiError::invalid("整理上の親は1つまでです"));
    }
    if ["part_of", "depends_on"].contains(&r.relation_type.as_str()) {
        let mut todo = vec![r.target_id.clone()];
        let mut visited = HashSet::new();
        while let Some(node) = todo.pop() {
            if node == r.source_id {
                return Err(ApiError::new(
                    422,
                    "CYCLE_DETECTED",
                    "循環する関係は追加できません",
                ));
            }
            if visited.insert(node.clone()) {
                todo.extend(
                    same.iter()
                        .filter(|x| x.source_id == node)
                        .map(|x| x.target_id.clone()),
                );
            }
        }
    }
    Ok(())
}

impl Service {
    pub fn provision_personal(&self, actor: &Actor) -> Result<()> {
        let db = self
            .db
            .lock()
            .map_err(|_| ApiError::new(500, "STORAGE_ERROR", "Storage unavailable"))?;
        let w = format!("personal-{:x}", Sha256::digest(actor.id.as_bytes()));
        let workspace = Workspace {
            id: w.clone(),
            name: "個人".into(),
            scope: "個人".into(),
            timezone: "Asia/Tokyo".into(),
            role: "owner".into(),
            local: false,
            version: 1,
        };
        db.execute(
            "INSERT OR IGNORE INTO workspaces VALUES(?1,?2)",
            params![w, serde_json::to_string(&workspace)?],
        )?;
        db.execute(
            "INSERT OR IGNORE INTO memberships VALUES(?1,?2,'owner')",
            params![w, actor.id],
        )?;
        Ok(())
    }

    pub fn open(path: &std::path::Path) -> Result<Self> {
        if let Some(p) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(p)
                .map_err(|_| ApiError::new(500, "STORAGE_ERROR", "保存先を作成できません"))?;
        }
        let db = Connection::open(path)?;
        crate::storage::migrate(&db)?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
        })
    }
    pub fn initialize(&self, demo: bool) -> Result<()> {
        let mut db = self
            .db
            .lock()
            .map_err(|_| ApiError::new(500, "STORAGE_ERROR", "保存処理を再起動してください"))?;
        if db.query_row("SELECT COUNT(*) FROM workspaces", [], |r| {
            r.get::<_, i64>(0)
        })? == 0
        {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            crate::seed::seed(&tx, demo)?;
            tx.commit()?;
        }
        Ok(())
    }
    pub fn handle(
        &self,
        actor: &Actor,
        method: &str,
        path: &str,
        query: &HashMap<String, String>,
        body: Value,
        key: Option<&str>,
    ) -> Result<Value> {
        self.handle_internal(actor, (method, path), query, body, key, None)
    }
    pub fn handle_derived(
        &self,
        actor: &Actor,
        original_path: &str,
        original_body: &Value,
        operation: Operation,
        key: Option<&str>,
    ) -> Result<Value> {
        self.handle_internal(
            actor,
            (&operation.method, &operation.path),
            &HashMap::new(),
            operation.body,
            key,
            Some(fingerprint("POST", original_path, original_body)),
        )
    }
    fn handle_internal(
        &self,
        actor: &Actor,
        route: (&str, &str),
        query: &HashMap<String, String>,
        body: Value,
        key: Option<&str>,
        fingerprint_override: Option<String>,
    ) -> Result<Value> {
        let (method, path) = route;
        let mut db = self
            .db
            .lock()
            .map_err(|_| ApiError::new(500, "STORAGE_ERROR", "保存処理を再起動してください"))?;
        let parts: Vec<_> = path.trim_matches('/').split('/').collect();
        let w = if parts.get(1) == Some(&"workspaces") {
            parts.get(2).copied().unwrap_or("")
        } else {
            ""
        };
        let suggestion_preview =
            parts.as_slice() == ["v1", "workspaces", w, "ai", "suggestions", "preview"];
        let notification_read = method == "PATCH"
            && parts.get(3) == Some(&"notifications")
            && parts.get(5) == Some(&"read");
        let workspace_write = method != "GET"
            && parts.as_slice() != ["v1", "workspaces", w, "leave"]
            && !notification_read
            && !suggestion_preview;
        if !w.is_empty() {
            authorize(&db, &actor.id, w, workspace_write)?;
        }
        if method == "GET" {
            if parts.get(3) == Some(&"snapshot") && !w.is_empty() {
                sync_due_notifications(&db, w, &actor.id)?;
            }
            let tx = db.transaction()?;
            return dispatch(&tx, actor, method, path, query, &body);
        }
        let key = key
            .filter(|k| !k.is_empty() && k.len() <= 200)
            .ok_or_else(|| {
                ApiError::new(400, "IDEMPOTENCY_KEY_REQUIRED", "Idempotency-Keyが必要です")
            })?;
        if actor.agent
            && !(parts.get(3) == Some(&"changesets")
                && (parts.get(4) == Some(&"preview") || parts.get(5) == Some(&"apply")))
        {
            return Err(ApiError::new(
                403,
                "APPROVAL_REQUIRED",
                "画面での承認が必要です",
            ));
        }
        let fp = fingerprint_override.unwrap_or_else(|| fingerprint(method, path, &body));
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Recheck inside the write transaction, including before an idempotent replay.
        if !w.is_empty() {
            authorize(&tx, &actor.id, w, workspace_write)?;
        }
        crate::collaboration::authorize_route(&tx, actor, method, &parts)?;
        let idempotency_actor = format!(
            "{}:{}",
            actor.id,
            if actor.agent { "agent" } else { "human" }
        );
        let prior:Option<(String,String)>=tx.query_row("SELECT fingerprint,response FROM idempotency WHERE actor=?1 AND workspace_id=?2 AND key=?3",params![idempotency_actor,w,key],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((hash, response)) = prior {
            if hash != fp {
                return Err(ApiError::new(
                    409,
                    "IDEMPOTENCY_CONFLICT",
                    "この再送キーは別の入力で使用されています",
                ));
            }
            let response = serde_json::from_str(&response)?;
            crate::collaboration::authorize_replay(&tx, method, &parts, &response)?;
            return Ok(response);
        }
        let result = dispatch(&tx, actor, method, path, query, &body)?;
        tx.execute(
            "INSERT INTO idempotency VALUES(?1,?2,?3,?4,?5,?6)",
            params![idempotency_actor, w, key, fp, result.to_string(), now()],
        )?;
        tx.execute(
            "INSERT INTO audit VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                new_id("audit"),
                w,
                actor.id,
                if actor.agent { "mcp" } else { "ui" },
                format!("{method} {path}"),
                now()
            ],
        )?;
        tx.commit()?;
        Ok(result)
    }
}

pub fn dispatch(
    db: &Connection,
    actor: &Actor,
    method: &str,
    path: &str,
    query: &HashMap<String, String>,
    body: &Value,
) -> Result<Value> {
    let p: Vec<_> = path.trim_matches('/').split('/').collect();
    if let Some(result) = crate::collaboration::dispatch(db, actor, method, &p, body) {
        return result;
    }
    match (method, p.as_slice()) {
        ("GET", ["v1", "me"]) => {
            return Ok(
                json!({"id":actor.id,"name":"やまだ はるか","mode":"local-preview","agent":actor.agent}),
            )
        }
        ("GET", ["v1", "workspaces"]) => return value(memberships(db, &actor.id)?),
        ("GET", ["v1", "templates"]) => return Ok(templates()),
        ("GET", ["v1", "settings"]) => {
            let raw: Option<String> = db
                .query_row(
                    "SELECT body FROM settings WHERE actor=?1",
                    [&actor.id],
                    |r| r.get(0),
                )
                .optional()?;
            return Ok(raw
                .map(|x| serde_json::from_str(&x))
                .transpose()?
                .unwrap_or(json!({"compact":false,"notifications":true,"timezone":"Asia/Tokyo"})));
        }
        ("PATCH", ["v1", "settings"]) if !actor.agent => {
            only(body, &["compact", "notifications", "timezone"])?;
            if body["compact"].as_bool().is_none()
                || body["notifications"].as_bool().is_none()
                || text(body, "timezone").parse::<chrono_tz::Tz>().is_err()
            {
                return Err(ApiError::invalid("設定値を確認してください"));
            }
            db.execute("INSERT INTO settings VALUES(?1,?2) ON CONFLICT(actor) DO UPDATE SET body=excluded.body",params![actor.id,body.to_string()])?;
            return Ok(body.clone());
        }
        _ => {}
    }
    if p.len() < 4 || p[0] != "v1" || p[1] != "workspaces" {
        return Err(ApiError::missing());
    }
    let w = p[2];
    let col = p[3];
    let id = p.get(4).copied().unwrap_or("");
    let suffix = p.get(5).copied().unwrap_or("");
    let suggestion_preview =
        p.as_slice() == ["v1", "workspaces", w, "ai", "suggestions", "preview"];
    let notification_read = method == "PATCH" && col == "notifications" && suffix == "read";
    authorize(
        db,
        &actor.id,
        w,
        method != "GET" && !suggestion_preview && !notification_read,
    )?;
    if actor.agent
        && method != "GET"
        && !(col == "changesets" && (id == "preview" || suffix == "apply"))
    {
        return Err(ApiError::new(
            403,
            "APPROVAL_REQUIRED",
            "この接続は提案モードです。変更をプレビューして画面で承認してください",
        ));
    }
    match (method, col, id, suffix) {
        ("POST", "ai", "suggestions", "preview") if !actor.agent => {
            crate::suggestions::preview(db, w, body)
        }
        ("GET", "snapshot", "", "") => {
            let cols = [
                "items",
                "relations",
                "records",
                "metrics",
                "observations",
                "views",
                "changesets",
            ];
            let mut result = json!({"workspace_id":w});
            for col in cols {
                result[col] = value(list::<Value>(db, w, col)?)?;
            }
            result["notifications"] = value(
                list::<Notification>(db, w, "notifications")?
                    .into_iter()
                    .filter(|notification| notification.recipient == actor.id)
                    .collect::<Vec<_>>(),
            )?;
            Ok(result)
        }
        ("GET", "graph", "", "") => {
            let limit = query
                .get("limit")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(200)
                .clamp(1, 200);
            let items: Vec<Item> = list(db, w, "items")?;
            let relations: Vec<Relation> = list(db, w, "relations")?;
            let items: Vec<_> = items
                .into_iter()
                .filter(|i| i.archived_at.is_none())
                .collect();
            let total = items.len();
            let items: Vec<_> = items.into_iter().take(limit).collect();
            let ids: HashSet<_> = items.iter().map(|i| i.id.as_str()).collect();
            let edges: Vec<_> = relations
                .into_iter()
                .filter(|r| {
                    ids.contains(r.source_id.as_str()) && ids.contains(r.target_id.as_str())
                })
                .collect();
            Ok(json!({"items":items,"relations":edges,"truncated":total>limit,"limit":limit}))
        }
        ("GET", "today", "", "") => {
            let d = query.get("local_date").cloned().unwrap_or_else(|| {
                Utc::now()
                    .with_timezone(&chrono_tz::Asia::Tokyo)
                    .date_naive()
                    .to_string()
            });
            date(&Some(d.clone()))?;
            let day = NaiveDate::parse_from_str(&d, "%Y-%m-%d").unwrap();
            let items: Vec<Item> = list(db, w, "items")?;
            let records: Vec<Record> = list(db, w, "records")?;
            let items: Vec<Value> = items
                .into_iter()
                .filter(|i| {
                    i.kind == "action"
                        && i.archived_at.is_none()
                        && i.state != "paused"
                        && i.state != "abandoned"
                        && i.state != "draft"
                        && match &i.fields.recurrence {
                            Some(r) => {
                                i.start_date.as_ref().is_none_or(|s| s <= &d)
                                    && i.due_date.as_ref().is_none_or(|e| e >= &d)
                                    && (r.mode == "period_quota"
                                        || r.weekdays.contains(
                                            &(day.weekday().num_days_from_monday() as u8),
                                        ))
                            }
                            None => i.scheduled_date.as_ref().is_none_or(|s| s == &d),
                        }
                })
                .map(|i| {
                    let occ = format!("{}:{d}", i.id);
                    let latest = records
                        .iter()
                        .rev()
                        .find(|r| r.occurrence_key.as_deref() == Some(&occ));
                    let completed = if i.fields.recurrence.is_some() {
                        latest.is_some_and(|r| r.record_type == "completion")
                    } else {
                        i.state == "done"
                    };
                    json!({"item":i,"completed":completed,"occurrence_key":occ})
                })
                .collect();
            Ok(json!({"local_date":d,"items":items}))
        }
        ("GET", "audit", "", "") => {
            let mut q=db.prepare("SELECT id,command,created_at,origin,actor FROM audit WHERE workspace_id=?1 ORDER BY rowid DESC LIMIT 100")?;
            let rows=q.query_map([w],|r|Ok(json!({"id":r.get::<_,String>(0)?,"command":r.get::<_,String>(1)?,"created_at":r.get::<_,String>(2)?,"origin":r.get::<_,String>(3)?,"actor":r.get::<_,String>(4)?})))?;
            value(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        }
        ("GET", col, "", "")
            if [
                "items",
                "relations",
                "records",
                "metrics",
                "observations",
                "views",
                "changesets",
            ]
            .contains(&col) =>
        {
            let mut items: Vec<Value> = list(db, w, col)?;
            items.retain(|i| {
                (col != "items"
                    || query.get("archived").is_some_and(|v| v == "true")
                    || i["archived_at"].is_null())
                    && query
                        .get("query")
                        .is_none_or(|q| text(i, "title").to_lowercase().contains(&q.to_lowercase()))
                    && query.get("kind").is_none_or(|k| text(i, "kind") == k)
                    && query.get("state").is_none_or(|k| text(i, "state") == k)
            });
            let start = if let Some(c) = query.get("cursor") {
                items
                    .iter()
                    .position(|i| text(i, "id") == c)
                    .ok_or_else(|| ApiError::invalid("カーソルが無効です"))?
                    + 1
            } else {
                0
            };
            let limit = query
                .get("limit")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(50)
                .clamp(1, 200);
            let more = items.len() > start + limit;
            let page: Vec<_> = items.into_iter().skip(start).take(limit).collect();
            let cursor = if more {
                page.last().map(|i| text(i, "id").to_string())
            } else {
                None
            };
            Ok(json!({"items":page,"next_cursor":cursor}))
        }
        ("GET", col, id, "")
            if !id.is_empty()
                && [
                    "items",
                    "relations",
                    "records",
                    "metrics",
                    "observations",
                    "views",
                    "changesets",
                ]
                .contains(&col) =>
        {
            get(db, w, col, id)
        }
        ("POST", "items", "", "") => {
            only(
                body,
                &[
                    "title",
                    "kind",
                    "description",
                    "state",
                    "start_date",
                    "due_date",
                    "scheduled_date",
                    "scheduled_time",
                    "fields",
                    "parent_id",
                ],
            )?;
            let stamp = now();
            let item = Item {
                id: new_id("item"),
                workspace_id: w.into(),
                title: title(body, "title", 200)?,
                kind: body["kind"].as_str().unwrap_or("outcome").into(),
                description: text(body, "description").into(),
                state: body["state"].as_str().unwrap_or("active").into(),
                version: 1,
                created_at: stamp.clone(),
                updated_at: stamp,
                archived_at: None,
                start_date: serde_json::from_value(body["start_date"].clone())?,
                due_date: serde_json::from_value(body["due_date"].clone())?,
                scheduled_date: serde_json::from_value(body["scheduled_date"].clone())?,
                scheduled_time: serde_json::from_value(body["scheduled_time"].clone())?,
                fields: if body["fields"].is_null() {
                    ItemFields::default()
                } else {
                    serde_json::from_value(body["fields"].clone())?
                },
            };
            validate_item(db, &item)?;
            if let Some(reference) = &item.fields.field_reference {
                if let Some(existing) = list::<Item>(db, w, "items")?.into_iter().find(|i| {
                    i.fields.field_reference.as_ref().is_some_and(|r| {
                        r.tenant_id == reference.tenant_id
                            && r.external_id == reference.external_id
                            && r.platform_id == reference.platform_id
                    })
                }) {
                    return value(existing);
                }
            }
            put(db, w, "items", &item.id, &item)?;
            if let Some(assignee) = &item.fields.assignee_id {
                create_notification(
                    db,
                    &item,
                    assignee,
                    "assignment",
                    &item.version.to_string(),
                    format!("「{}」の担当になりました", item.title),
                )?;
            }
            if let Some(parent) = body["parent_id"].as_str() {
                let r = Relation {
                    id: new_id("rel"),
                    workspace_id: w.into(),
                    source_id: item.id.clone(),
                    target_id: parent.into(),
                    relation_type: "part_of".into(),
                    rationale: String::new(),
                    version: 1,
                };
                validate_relation(db, &r)?;
                put(db, w, "relations", &r.id, &r)?;
            }
            value(item)
        }
        ("PATCH", "items", id, "") if !id.is_empty() => {
            only(
                body,
                &[
                    "expected_version",
                    "title",
                    "description",
                    "state",
                    "start_date",
                    "due_date",
                    "scheduled_date",
                    "scheduled_time",
                    "fields",
                    "archived_at",
                ],
            )?;
            let old: Item = get(db, w, "items", id)?;
            version(body, old.version)?;
            let mut v = value(&old)?;
            for (key, val) in body.as_object().unwrap() {
                if key == "expected_version" {
                    continue;
                }
                if key == "fields" {
                    for (k, x) in val
                        .as_object()
                        .ok_or_else(|| ApiError::invalid("fieldsの形式を確認してください"))?
                    {
                        v["fields"][k] = x.clone();
                    }
                } else {
                    v[key] = val.clone();
                }
            }
            let mut item: Item = serde_json::from_value(v)?;
            if old.kind == "action" && item.state != old.state && item.state == "done" {
                return Err(ApiError::invalid(
                    "行動完了にはcomplete操作を使用してください",
                ));
            }
            if item.archived_at.is_some() {
                item.archived_at = Some(now());
            }
            if item.fields.self_assessment != old.fields.self_assessment {
                item.fields.assessed_at = Some(now());
            }
            item.title = item.title.trim().into();
            item.version += 1;
            item.updated_at = now();
            validate_item(db, &item)?;
            if old.fields.recurrence != item.fields.recurrence {
                /* Historical definitions stay in immutable records. */
                let rec = json!({"id":new_id("record"),"workspace_id":w,"item_ids":[id],"record_type":"recurrence_change","body":serde_json::to_string(&old.fields.recurrence)?,"happened_at":now(),"created_at":now(),"author":actor.id});
                put(db, w, "records", text(&rec, "id"), &rec)?;
            }
            if old.fields.assignee_id != item.fields.assignee_id {
                let record = json!({"id":new_id("record"),"workspace_id":w,"item_ids":[id],"record_type":"assignment_change","body":json!({"from":old.fields.assignee_id,"to":item.fields.assignee_id}).to_string(),"happened_at":now(),"created_at":now(),"author":actor.id});
                put(db, w, "records", text(&record, "id"), &record)?;
                if let Some(assignee) = &item.fields.assignee_id {
                    create_notification(
                        db,
                        &item,
                        assignee,
                        "assignment",
                        &item.version.to_string(),
                        format!("「{}」の担当になりました", item.title),
                    )?;
                }
            }
            if old.start_date != item.start_date || old.due_date != item.due_date {
                let record = json!({"id":new_id("record"),"workspace_id":w,"item_ids":[id],"record_type":"schedule_change","body":json!({"start_date":{"from":old.start_date,"to":item.start_date},"due_date":{"from":old.due_date,"to":item.due_date}}).to_string(),"happened_at":now(),"created_at":now(),"author":actor.id});
                put(db, w, "records", text(&record, "id"), &record)?;
                if let Some(assignee) = &item.fields.assignee_id {
                    create_notification(
                        db,
                        &item,
                        assignee,
                        "due_date",
                        item.due_date.as_deref().unwrap_or("none"),
                        format!("「{}」の期限が更新されました", item.title),
                    )?;
                }
            }
            put(db, w, "items", id, &item)?;
            value(item)
        }
        ("PATCH", "notifications", id, "read") if !id.is_empty() => {
            only(body, &["read"])?;
            let mut notification: Notification = get(db, w, "notifications", id)?;
            if notification.recipient != actor.id {
                return Err(ApiError::missing());
            }
            notification.read_at = if body["read"].as_bool() == Some(false) {
                None
            } else {
                Some(now())
            };
            put(db, w, "notifications", id, &notification)?;
            value(notification)
        }
        ("POST", "actions", id, "complete" | "reopen" | "skip") => {
            only(
                body,
                &["expected_version", "local_date", "completed_at", "note"],
            )?;
            let mut item: Item = get(db, w, "items", id)?;
            version(body, item.version)?;
            if item.kind != "action"
                || item.archived_at.is_some()
                || item.state == "paused"
                || (item.state == "abandoned" && suffix != "reopen")
            {
                return Err(ApiError::invalid("この行動は記録できる状態ではありません"));
            }
            let d = body["local_date"]
                .as_str()
                .ok_or_else(|| ApiError::invalid("実施するローカル日付が必要です"))?;
            date(&Some(d.into()))?;
            if let Some(r) = &item.fields.recurrence {
                let day = NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap();
                if item.start_date.as_ref().is_some_and(|s| s.as_str() > d)
                    || item.due_date.as_ref().is_some_and(|e| e.as_str() < d)
                    || (r.mode == "fixed_schedule"
                        && !r
                            .weekdays
                            .contains(&(day.weekday().num_days_from_monday() as u8)))
                {
                    return Err(ApiError::invalid("この日は習慣の予定対象外です"));
                }
            }
            let occurrence = format!("{id}:{d}");
            let records: Vec<Record> = list(db, w, "records")?;
            let prior = records
                .iter()
                .rev()
                .find(|r| r.occurrence_key.as_deref() == Some(&occurrence));
            let kind = match suffix {
                "complete" => "completion",
                "skip" => "skip",
                _ => "reopen",
            };
            if prior.is_some_and(|r| r.record_type == kind) {
                return Ok(json!({"item":item,"record":prior,"outcome_updated":false}));
            }
            if item.fields.recurrence.is_none() {
                item.state = if suffix == "complete" {
                    "done"
                } else if suffix == "skip" {
                    "abandoned"
                } else {
                    "active"
                }
                .into();
            }
            item.version += 1;
            item.updated_at = now();
            let happened = body["completed_at"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(now);
            timestamp(&happened)?;
            let rec = Record {
                id: new_id("record"),
                workspace_id: w.into(),
                item_ids: vec![id.into()],
                record_type: kind.into(),
                body: text(body, "note").into(),
                happened_at: happened,
                created_at: now(),
                author: actor.id.clone(),
                decision: None,
                occurrence_key: Some(occurrence),
                supersedes_id: prior.map(|r| r.id.clone()),
            };
            put(db, w, "items", id, &item)?;
            put(db, w, "records", &rec.id, &rec)?;
            if suffix == "complete" {
                if let Some(assignee) = item.fields.assignee_id.clone() {
                    create_notification(
                        db,
                        &item,
                        &assignee,
                        "completion",
                        rec.occurrence_key.as_deref().unwrap_or(&rec.id),
                        format!("「{}」が完了しました", item.title),
                    )?;
                }
            }
            Ok(json!({"item":item,"record":rec,"outcome_updated":false}))
        }
        ("POST", "relations", "", "") => {
            only(body, &["source_id", "target_id", "type", "rationale"])?;
            let r = Relation {
                id: new_id("rel"),
                workspace_id: w.into(),
                source_id: title(body, "source_id", 200)?,
                target_id: title(body, "target_id", 200)?,
                relation_type: title(body, "type", 40)?,
                rationale: text(body, "rationale").into(),
                version: 1,
            };
            validate_relation(db, &r)?;
            put(db, w, col, &r.id, &r)?;
            value(r)
        }
        ("DELETE", "relations", id, "") => {
            let r: Relation = get(db, w, col, id)?;
            version(body, r.version)?;
            remove(db, w, col, id)?;
            Ok(json!({"id":id,"deleted":true}))
        }
        ("POST", "records", "", "") => {
            only(
                body,
                &[
                    "item_ids",
                    "record_type",
                    "body",
                    "happened_at",
                    "decision",
                    "supersedes_id",
                ],
            )?;
            let ids: Vec<String> =
                serde_json::from_value(body.get("item_ids").cloned().unwrap_or(json!([])))?;
            for id in &ids {
                let _: Item = get(db, w, "items", id)?;
            }
            let kind = body["record_type"].as_str().unwrap_or("note");
            if !["note", "review", "learning", "checkin"].contains(&kind) {
                return Err(ApiError::invalid("この記録の種類は使用できません"));
            }
            let supersedes: Option<String> = serde_json::from_value(body["supersedes_id"].clone())?;
            if let Some(id) = &supersedes {
                let old: Record = get(db, w, col, id)?;
                if old.record_type != kind {
                    return Err(ApiError::invalid("同じ種類の記録だけ訂正できます"));
                }
            }
            let happened = body["happened_at"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(now);
            timestamp(&happened)?;
            let r = Record {
                id: new_id("record"),
                workspace_id: w.into(),
                item_ids: ids,
                record_type: kind.into(),
                body: title(body, "body", 20000)?,
                happened_at: happened,
                created_at: now(),
                author: actor.id.clone(),
                decision: serde_json::from_value(body["decision"].clone())?,
                occurrence_key: None,
                supersedes_id: supersedes,
            };
            put(db, w, col, &r.id, &r)?;
            value(r)
        }
        ("POST", "metrics", "", "") => {
            only(
                body,
                &[
                    "item_id",
                    "name",
                    "unit",
                    "baseline",
                    "target",
                    "direction",
                    "period_start",
                    "period_end",
                ],
            )?;
            let m = Metric {
                id: new_id("metric"),
                workspace_id: w.into(),
                item_id: title(body, "item_id", 200)?,
                name: title(body, "name", 200)?,
                unit: title(body, "unit", 40)?,
                baseline: body["baseline"]
                    .as_f64()
                    .ok_or_else(|| ApiError::invalid("基準値が必要です"))?,
                target: body["target"]
                    .as_f64()
                    .ok_or_else(|| ApiError::invalid("目標値が必要です"))?,
                direction: title(body, "direction", 20)?,
                period_start: serde_json::from_value(body["period_start"].clone())?,
                period_end: serde_json::from_value(body["period_end"].clone())?,
                version: 1,
            };
            let _: Item = get(db, w, "items", &m.item_id)?;
            validate_metric(&m)?;
            put(db, w, col, &m.id, &m)?;
            value(m)
        }
        ("POST", "observations", "", "") => {
            only(
                body,
                &[
                    "metric_id",
                    "value",
                    "unit",
                    "source",
                    "observed_at",
                    "supersedes_id",
                ],
            )?;
            let metric: Metric = get(db, w, "metrics", text(body, "metric_id"))?;
            if text(body, "unit") != metric.unit {
                return Err(ApiError::new(
                    422,
                    "INVALID_UNIT",
                    "指標の単位と一致しません",
                ));
            }
            let observed = body["observed_at"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(now);
            timestamp(&observed)?;
            let supersedes: Option<String> = serde_json::from_value(body["supersedes_id"].clone())?;
            if let Some(id) = &supersedes {
                let old: Observation = get(db, w, col, id)?;
                if old.metric_id != metric.id {
                    return Err(ApiError::invalid("同じ指標の観測だけ訂正できます"));
                }
                if list::<Observation>(db, w, col)?
                    .iter()
                    .any(|o| o.supersedes_id.as_ref() == Some(id))
                {
                    return Err(ApiError::new(
                        409,
                        "VERSION_CONFLICT",
                        "この観測はすでに訂正されています",
                    ));
                }
            }
            let o = Observation {
                id: new_id("obs"),
                workspace_id: w.into(),
                metric_id: metric.id,
                value: body["value"]
                    .as_f64()
                    .ok_or_else(|| ApiError::invalid("観測値を入力してください"))?,
                unit: metric.unit,
                source: title(body, "source", 500)?,
                observed_at: observed,
                created_at: now(),
                supersedes_id: supersedes,
            };
            put(db, w, col, &o.id, &o)?;
            value(o)
        }
        ("POST", "views", "", "") => {
            only(body, &["name", "type", "filters"])?;
            if !["list", "map", "timeline", "okr", "today"].contains(&text(body, "type")) {
                return Err(ApiError::invalid("不明なビューです"));
            }
            let v = json!({"id":new_id("view"),"workspace_id":w,"name":title(body,"name",200)?,"type":body["type"],"filters":body.get("filters").cloned().unwrap_or(json!({})),"version":1});
            put(db, w, col, text(&v, "id"), &v)?;
            Ok(v)
        }
        ("POST", "views", id, "query") => {
            let view: Value = get(db, w, "views", id)?;
            let filters: HashMap<String, String> = serde_json::from_value(view["filters"].clone())?;
            dispatch(
                db,
                actor,
                "GET",
                &format!("/v1/workspaces/{w}/items"),
                &filters,
                &Value::Null,
            )
        }
        ("POST", "templates", id, "apply") => apply_template(db, actor, w, id, body),
        ("POST", "onboarding", "complete", "") => complete_onboarding(db, actor, w, body),
        ("POST", "changesets", "preview", "") => preview(db, actor, w, body),
        ("POST", "changesets", id, "approve") if !actor.agent => {
            let mut c: Value = get(db, w, col, id)?;
            validate_preview(db, w, &c)?;
            if c["status"] != "pending" {
                return Err(ApiError::new(
                    409,
                    "VERSION_CONFLICT",
                    "この変更案は承認待ちではありません",
                ));
            }
            c["approved_by"] = json!(actor.id);
            c["status"] = json!("approved");
            c["approved_hash"] = c["hash"].clone();
            put(db, w, col, id, &c)?;
            Ok(c)
        }
        ("POST", "changesets", id, "apply") => {
            let mut c: Value = get(db, w, col, id)?;
            validate_preview(db, w, &c)?;
            if c["status"] != "approved"
                || c["approved_hash"] != c["hash"]
                || c["approved_by"] != actor.id
            {
                return Err(ApiError::new(
                    403,
                    "APPROVAL_REQUIRED",
                    "画面での差分確認と承認が必要です",
                ));
            }
            let ops: Vec<Operation> = serde_json::from_value(c["operations"].clone())?;
            let human = Actor {
                id: actor.id.clone(),
                agent: false,
            };
            let mut output = vec![];
            for op in ops {
                output.push(dispatch(
                    db,
                    &human,
                    &op.method,
                    &op.path,
                    &HashMap::new(),
                    &op.body,
                )?);
            }
            c["status"] = json!("applied");
            c["applied_at"] = json!(now());
            put(db, w, col, id, &c)?;
            Ok(json!({"changeset":c,"results":output}))
        }
        ("POST", "exports", "", "") if !actor.agent => {
            only(body, &[])?;
            let mut backup = json!({"schema_version":1,"exported_at":now(),"workspace_id":w});
            for col in [
                "items",
                "relations",
                "records",
                "metrics",
                "observations",
                "views",
            ] {
                backup[col] = value(list::<Value>(db, w, col)?)?;
            }
            Ok(backup)
        }
        ("POST", "imports", "", "") if !actor.agent => import(db, w, body),
        _ => Err(ApiError::missing()),
    }
}

fn complete_onboarding(db: &Connection, actor: &Actor, w: &str, body: &Value) -> Result<Value> {
    only(
        body,
        &[
            "title",
            "purpose",
            "due_date",
            "initiative_title",
            "action_title",
            "metric",
        ],
    )?;
    let title = title(body, "title", 200)?;
    let purpose = text(body, "purpose").trim();
    if purpose.chars().count() > 10_000 {
        return Err(ApiError::invalid(
            "達成したいことは10000文字以内で入力してください",
        ));
    }
    let due_date: Option<String> = serde_json::from_value(body["due_date"].clone())?;
    date(&due_date)?;
    if list::<Item>(db, w, "items")?
        .iter()
        .any(|item| item.archived_at.is_none())
    {
        return Err(ApiError::new(
            409,
            "ONBOARDING_CONFLICT",
            "別の操作で項目が作成されました。入力を保持したまま最新の内容を確認してください",
        ));
    }

    let base = format!("/v1/workspaces/{w}");
    let mut goal = dispatch(
        db,
        actor,
        "POST",
        &format!("{base}/items"),
        &HashMap::new(),
        &json!({
            "title": title,
            "description": purpose,
            "kind": "outcome",
            "due_date": due_date,
            "fields": { "icon": "target" }
        }),
    )?;

    let initiative_title = text(body, "initiative_title").trim();
    let action_title = text(body, "action_title").trim();
    let mut initiative = Value::Null;
    let mut action = Value::Null;
    if !initiative_title.is_empty() {
        if initiative_title.chars().count() > 200 {
            return Err(ApiError::invalid("取り組みは200文字以内で入力してください"));
        }
        initiative = dispatch(
            db,
            actor,
            "POST",
            &format!("{base}/items"),
            &HashMap::new(),
            &json!({"title":initiative_title,"kind":"initiative","parent_id":goal["id"],"fields":{"icon":"flag"}}),
        )?;
    }
    if !action_title.is_empty() {
        if action_title.chars().count() > 200 {
            return Err(ApiError::invalid("次の一歩は200文字以内で入力してください"));
        }
        let parent = initiative["id"].as_str().or_else(|| goal["id"].as_str());
        action = dispatch(
            db,
            actor,
            "POST",
            &format!("{base}/items"),
            &HashMap::new(),
            &json!({"title":action_title,"kind":"action","parent_id":parent}),
        )?;
        goal = dispatch(
            db,
            actor,
            "PATCH",
            &format!("{base}/items/{}", text(&goal, "id")),
            &HashMap::new(),
            &json!({"expected_version":goal["version"],"fields":{"next_action_id":action["id"]}}),
        )?;
    }

    let mut metric = Value::Null;
    if !body["metric"].is_null() {
        let candidate = &body["metric"];
        only(
            candidate,
            &["name", "unit", "baseline", "target", "direction"],
        )?;
        let mut metric_body = candidate.clone();
        metric_body["item_id"] = goal["id"].clone();
        metric = dispatch(
            db,
            actor,
            "POST",
            &format!("{base}/metrics"),
            &HashMap::new(),
            &metric_body,
        )?;
    }
    Ok(json!({"goal":goal,"initiative":initiative,"action":action,"metric":metric}))
}
fn validate_metric(m: &Metric) -> Result<()> {
    date(&m.period_start)?;
    date(&m.period_end)?;
    if !["increase", "decrease", "threshold"].contains(&m.direction.as_str())
        || (m.direction == "increase" && m.target <= m.baseline)
        || (m.direction == "decrease" && m.target >= m.baseline)
        || m.period_start
            .as_ref()
            .zip(m.period_end.as_ref())
            .is_some_and(|(s, e)| s > e)
    {
        return Err(ApiError::invalid(
            "評価方向・基準値・目標値・期間を確認してください",
        ));
    }
    Ok(())
}
fn workspace_version(db: &Connection, w: &str) -> Result<String> {
    let mut parts = String::new();
    for col in ["items", "relations", "metrics", "views"] {
        parts.push_str(&serde_json::to_string(&list::<Value>(db, w, col)?)?);
    }
    let mut q =
        db.prepare("SELECT actor,role FROM memberships WHERE workspace_id=?1 ORDER BY actor")?;
    for row in q.query_map([w], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        parts.push_str(&format!("{:?}", row?));
    }
    Ok(format!("{:x}", Sha256::digest(parts)))
}
fn preview(db: &Connection, actor: &Actor, w: &str, b: &Value) -> Result<Value> {
    only(b, &["operations", "title"])?;
    let ops: Vec<Operation> = serde_json::from_value(b["operations"].clone())?;
    if ops.is_empty() || ops.len() > 100 {
        return Err(ApiError::invalid("変更は1〜100操作にしてください"));
    }
    for op in &ops {
        let p: Vec<_> = op.path.trim_matches('/').split('/').collect();
        if p.len() < 4
            || p[0] != "v1"
            || p[1] != "workspaces"
            || p[2] != w
            || ![
                "items",
                "relations",
                "records",
                "metrics",
                "observations",
                "actions",
                "templates",
                "views",
            ]
            .contains(&p[3])
            || !["POST", "PATCH", "DELETE"].contains(&op.method.as_str())
        {
            return Err(ApiError::invalid(
                "同じワークスペースの計画操作だけ提案できます",
            ));
        }
    }
    // Validate the complete batch without changing the live plan.
    db.execute_batch("SAVEPOINT preview_validation")?;
    let human = Actor {
        id: actor.id.clone(),
        agent: false,
    };
    let validation = (|| {
        for op in &ops {
            dispatch(db, &human, &op.method, &op.path, &HashMap::new(), &op.body)?;
        }
        Ok::<(), ApiError>(())
    })();
    db.execute_batch("ROLLBACK TO preview_validation; RELEASE preview_validation")?;
    validation?;
    let c = json!({"id":new_id("change"),"workspace_id":w,"title":b["title"].as_str().unwrap_or("計画の変更案"),"operations":ops,"status":"pending","actor":actor.id,"hash":fingerprint("PREVIEW",w,&b["operations"]),"base_version":workspace_version(db,w)?,"created_at":now(),"expires_at":(Utc::now()+chrono::Duration::minutes(30)).to_rfc3339()});
    put(db, w, "changesets", text(&c, "id"), &c)?;
    Ok(c)
}
fn validate_preview(db: &Connection, w: &str, c: &Value) -> Result<()> {
    if text(c, "expires_at") < now().as_str()
        || text(c, "base_version") != workspace_version(db, w)?
    {
        return Err(ApiError::new(
            409,
            "VERSION_CONFLICT",
            "計画または権限が変わったか、有効期限が切れました。再プレビューしてください",
        ));
    }
    Ok(())
}
pub fn templates() -> Value {
    json!([
        {"id":"free","title":"自由形式","version":1,"preview":["目標を1つ作成。数値・期限・親は任意。"]},
        {"id":"okr","title":"OKR","version":1,"preview":["目標を1つ作成","成果指標を自分で設定できるOKRビューを追加"]},
        {"id":"project","title":"プロジェクト","version":1,"preview":["目標を1つ作成","計画を整理する取り組みと、最初の行動を追加"]},
        {"id":"learning","title":"学習計画","version":1,"preview":["目標を1つ作成","練習の取り組みと、週3回の習慣を追加"]},
        {"id":"habit","title":"習慣づくり","version":1,"preview":["目標を1つ作成","週3回取り組む習慣を追加。完了は各日ごとに記録。"]}
    ])
}
fn apply_template(db: &Connection, a: &Actor, w: &str, id: &str, b: &Value) -> Result<Value> {
    only(b, &["title", "description", "start_date", "due_date"])?;
    if !["free", "okr", "project", "learning", "habit"].contains(&id) {
        return Err(ApiError::missing());
    }
    let mut b = b.clone();
    b["fields"] = json!({"template":id,"template_version":1,"icon":match id{"learning"=>"graduation","project"=>"folder","habit"=>"repeat",_=>"target"}});
    let base = format!("/v1/workspaces/{w}");
    let mut root = dispatch(db, a, "POST", &format!("{base}/items"), &HashMap::new(), &b)?;
    if ["project", "learning", "habit"].contains(&id) {
        let child = dispatch(
            db,
            a,
            "POST",
            &format!("{base}/items"),
            &HashMap::new(),
            &json!({"title":if id=="project"{"計画を整理する"}else{"少しずつ練習する"},"kind":"initiative","fields":{"icon":"flag"}}),
        )?;
        dispatch(
            db,
            a,
            "POST",
            &format!("{base}/relations"),
            &HashMap::new(),
            &json!({"source_id":child["id"],"target_id":root["id"],"type":"part_of"}),
        )?;
        let fields = if id == "project" {
            json!({})
        } else {
            json!({"recurrence":{"mode":"period_quota","times_per_week":3,"timezone":"Asia/Tokyo","weekdays":[]}})
        };
        let action = dispatch(
            db,
            a,
            "POST",
            &format!("{base}/items"),
            &HashMap::new(),
            &json!({"kind":"action","title":if id=="project"{"最初の一歩を書き出す"}else{"今日の練習をする"},"fields":fields}),
        )?;
        dispatch(
            db,
            a,
            "POST",
            &format!("{base}/relations"),
            &HashMap::new(),
            &json!({"source_id":action["id"],"target_id":child["id"],"type":"part_of"}),
        )?;
        root = dispatch(
            db,
            a,
            "PATCH",
            &format!("{base}/items/{}", text(&root, "id")),
            &HashMap::new(),
            &json!({"expected_version":1,"fields":{"next_action_id":action["id"]}}),
        )?;
    }
    if id == "okr" {
        dispatch(
            db,
            a,
            "POST",
            &format!("{base}/views"),
            &HashMap::new(),
            &json!({"name":"OKR","type":"okr","filters":{"kind":"outcome"}}),
        )?;
    }
    Ok(root)
}
fn import(db: &Connection, w: &str, b: &Value) -> Result<Value> {
    only(
        b,
        &[
            "schema_version",
            "exported_at",
            "workspace_id",
            "items",
            "relations",
            "records",
            "metrics",
            "observations",
            "views",
        ],
    )?;
    if b["schema_version"] != 1 {
        return Err(ApiError::invalid("対応していないバックアップ形式です"));
    }
    let cols = [
        "items",
        "relations",
        "records",
        "metrics",
        "observations",
        "views",
    ];
    let mut count = 0;
    for col in cols {
        let docs = b[col]
            .as_array()
            .ok_or_else(|| ApiError::invalid("バックアップに必要な一覧がありません"))?;
        if docs.len() > 10000 {
            return Err(ApiError::invalid("一度に取り込める件数を超えています"));
        }
        for doc in docs {
            let id = title(doc, "id", 200)?;
            if text(doc, "workspace_id") != text(b, "workspace_id") {
                return Err(ApiError::invalid(
                    "バックアップ内のワークスペースが不整合です",
                ));
            }
            let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM documents WHERE workspace_id=?1 AND collection=?2 AND id=?3)",params![w,col,id],|r|r.get(0))?;
            if exists {
                return Err(ApiError::new(
                    409,
                    "IMPORT_CONFLICT",
                    "同じIDのデータがあります。既存データは変更していません",
                ));
            }
            let mut doc = doc.clone();
            doc["workspace_id"] = json!(w);
            put(db, w, col, &id, &doc)?;
            count += 1;
        }
    }
    // Validate after insertion so references can resolve independent of file order; outer transaction rolls all back on failure.
    for i in list::<Item>(db, w, "items")? {
        validate_item(db, &i)?;
    }
    for r in list::<Relation>(db, w, "relations")? {
        validate_relation(db, &r)?;
    }
    for m in list::<Metric>(db, w, "metrics")? {
        let _: Item = get(db, w, "items", &m.item_id)?;
        validate_metric(&m)?;
    }
    for r in list::<Record>(db, w, "records")? {
        for id in r.item_ids {
            let _: Item = get(db, w, "items", &id)?;
        }
        timestamp(&r.happened_at)?;
    }
    for o in list::<Observation>(db, w, "observations")? {
        let m: Metric = get(db, w, "metrics", &o.metric_id)?;
        if o.unit != m.unit {
            return Err(ApiError::invalid("観測の単位が一致しません"));
        }
        timestamp(&o.observed_at)?;
        if let Some(id) = o.supersedes_id {
            let _: Observation = get(db, w, "observations", &id)?;
        }
    }
    Ok(json!({"imported":count,"workspace_id":w}))
}
