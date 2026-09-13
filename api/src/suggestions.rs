use crate::{
    model::{ApiError, Item, Record, Result},
    service::{now, text},
    storage::{get, list},
};
use chrono::{Duration, Utc};
use rusqlite::Connection;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn short_id(seed: &str, index: usize) -> String {
    let hash = format!("{:x}", Sha256::digest(format!("{seed}:{index}")));
    format!("suggestion_{}", &hash[..16])
}

fn concise(value: &str, max: usize) -> String {
    let value = value.trim().replace(['\n', '\r'], " ");
    if value.chars().count() <= max {
        value
    } else {
        format!("{}…", value.chars().take(max).collect::<String>())
    }
}

pub(crate) fn preview(db: &Connection, workspace: &str, body: &Value) -> Result<Value> {
    let allowed = ["goal_id", "expected_version"];
    let object = body
        .as_object()
        .ok_or_else(|| ApiError::invalid("JSONオブジェクトを指定してください"))?;
    if let Some(field) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(ApiError::invalid(&format!(
            "変更できないフィールドです: {field}"
        )));
    }

    let goal_id = text(body, "goal_id");
    let goal: Item = get(db, workspace, "items", goal_id)?;
    if !["outcome", "idea", "milestone"].contains(&goal.kind.as_str()) || goal.archived_at.is_some()
    {
        return Err(ApiError::invalid("提案対象の目標を選択してください"));
    }
    let expected = body["expected_version"]
        .as_i64()
        .ok_or_else(|| ApiError::new(428, "VERSION_REQUIRED", "expected_versionが必要です"))?;
    if expected != goal.version {
        return Err(ApiError::new(
            409,
            "VERSION_CONFLICT",
            "目標が更新されています。最新の内容で提案を作り直してください",
        ));
    }

    let latest = list::<Record>(db, workspace, "records")?
        .into_iter()
        .filter(|record| record.item_ids.is_empty() || record.item_ids.contains(&goal.id))
        .max_by(|a, b| a.happened_at.cmp(&b.happened_at));
    let latest_body = latest
        .as_ref()
        .map(|record| concise(&record.body, 180))
        .filter(|body| !body.is_empty());
    let evidence = json!({
        "goal": {"id": goal.id, "title": goal.title, "version": goal.version},
        "deadline": goal.due_date,
        "latest_record": latest.as_ref().map(|record| json!({
            "id": record.id,
            "type": record.record_type,
            "body": latest_body,
            "happened_at": record.happened_at,
        })),
    });
    let seed = format!(
        "{}:{}:{}:{}",
        workspace,
        goal.id,
        goal.version,
        latest
            .as_ref()
            .map(|record| record.id.as_str())
            .unwrap_or("")
    );
    let subject = concise(&goal.title, 70);
    let actions = [
        format!("「{subject}」の次の一歩を3つ書き出し、最小の1つを選ぶ（20分）"),
        format!("「{subject}」に必要な相手・情報を1つ確認する（15分）"),
        format!("「{subject}」を前に進める作業をタイマーで30分だけ試す"),
    ];
    let suggestions: Vec<Value> = actions
        .into_iter()
        .enumerate()
        .map(|(index, action)| {
            let duration = [20, 15, 30][index];
            json!({
                "id": short_id(&seed, index),
                "kind": "action",
                "title": action,
                "duration_minutes": duration,
                "reason": if latest.is_some() {
                    "目標と直近の記録を踏まえ、30分以内に完了できる粒度にしました"
                } else {
                    "目標を具体化し、30分以内に着手できる粒度にしました"
                },
                "evidence": evidence,
            })
        })
        .collect();

    let fact = latest_body
        .map(|body| format!("直近の記録には「{body}」と残っています。"))
        .unwrap_or_else(|| "この目標に関連する直近の記録はまだありません。".into());
    let deadline_fact = goal
        .due_date
        .as_ref()
        .map(|date| format!("期限は{date}です。"))
        .unwrap_or_else(|| "期限は設定されていません。".into());
    let reflection = json!({
        "id": short_id(&seed, 3),
        "kind": "reflection",
        "title": format!("「{subject}」の振り返り案"),
        "fact": [fact, deadline_fact],
        "inference": ["小さな行動へ分けると、次に試すことを決めやすそうです。"],
        "questions": [
            "実際に進んだことは何ですか？",
            "止まった理由について、事実として確認できることは何ですか？",
            "次の30分で試すなら何を選びますか？"
        ],
        "body": format!("事実\n- {fact}\n- {deadline_fact}\n\n推測\n- 小さな行動へ分けると、次に試すことを決めやすそうです。\n\n質問\n- 実際に進んだことは何ですか？\n- 止まった理由について、事実として確認できることは何ですか？\n- 次の30分で試すなら何を選びますか？"),
        "evidence": evidence,
    });

    Ok(json!({
        "id": format!("ai_{}", &format!("{:x}", Sha256::digest(&seed))[..16]),
        "workspace_id": workspace,
        "goal_id": goal.id,
        "goal_version": goal.version,
        "created_at": now(),
        "expires_at": (Utc::now() + Duration::minutes(30)).to_rfc3339(),
        "provider": "safe_local_fallback",
        "provider_notice": "AI接続が未設定または利用できないため、ワークスペース内の記録だけを使った安全な候補を表示しています。",
        "suggestions": suggestions,
        "reflection": reflection,
    }))
}
