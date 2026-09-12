use crate::{model::*, service::now, storage::*};
use chrono::{Datelike, Utc};
use rusqlite::{params, Connection};
use serde_json::json;
pub fn seed(db: &Connection, demo: bool) -> Result<()> {
    for (id, name, scope) in [
        ("personal", "個人", "個人"),
        ("team", "チーム（ローカル）", "チーム"),
        ("organization", "組織（ローカル）", "組織"),
    ] {
        let ws = Workspace {
            id: id.into(),
            name: name.into(),
            scope: scope.into(),
            timezone: "Asia/Tokyo".into(),
            role: "owner".into(),
            local: true,
            version: 1,
        };
        db.execute(
            "INSERT INTO workspaces VALUES(?1,?2)",
            params![id, serde_json::to_string(&ws)?],
        )?;
        db.execute(
            "INSERT INTO memberships VALUES(?1,'local-owner','owner')",
            [id],
        )?;
    }
    if !demo {
        return Ok(());
    }
    let today = Utc::now()
        .with_timezone(&chrono_tz::Asia::Tokyo)
        .date_naive();
    let date = today.to_string();
    let start = today.with_day(1).unwrap();
    let raw: serde_json::Value = serde_json::from_str(include_str!("seed.json"))?;
    for goal in raw["goals"].as_array().unwrap() {
        let id = goal["id"].as_str().unwrap();
        let w = scope(goal["scope"].as_str().unwrap());
        let next = format!("next-{id}");
        let action = json!({"id":next,"workspace_id":w,"kind":"action","title":goal["next"],"description":"","state":"active","version":1,"created_at":now(),"updated_at":now(),"scheduled_date":date,"fields":{}});
        put(db, w, "items", &next, &action)?;
        let item = json!({"id":id,"workspace_id":w,"kind":"outcome","title":goal["title"],"description":goal["purpose"],"state":"active","version":1,"created_at":now(),"updated_at":now(),"start_date":start.to_string(),"due_date":(today+chrono::Duration::days(50)).to_string(),"fields":{"icon":goal["icon"],"subtitle":goal["subtitle"],"memo":goal["memo"],"next_action_id":next,"self_assessment":goal["progress"],"assessed_at":now()}});
        put(db, w, "items", id, &item)?;
        let r = json!({"id":format!("rel-{next}"),"workspace_id":w,"source_id":next,"target_id":id,"type":"part_of","rationale":"","version":1});
        put(db, w, "relations", r["id"].as_str().unwrap(), &r)?;
    }
    for item in raw["initiatives"].as_array().unwrap() {
        let id = item["id"].as_str().unwrap();
        let parent = item["goalId"].as_str().unwrap();
        let w = match parent {
            "english" => "personal",
            "event" => "team",
            _ => "organization",
        };
        let v = json!({"id":id,"workspace_id":w,"kind":"initiative","title":item["title"],"description":"","state":"active","version":1,"created_at":now(),"updated_at":now(),"fields":{"icon":item["icon"],"self_assessment":item["progress"],"assessed_at":now()}});
        put(db, w, "items", id, &v)?;
        let r = json!({"id":format!("rel-{id}"),"workspace_id":w,"source_id":id,"target_id":parent,"type":"part_of","rationale":"","version":1});
        put(db, w, "relations", r["id"].as_str().unwrap(), &r)?;
    }
    for item in raw["tasks"].as_array().unwrap() {
        let id = item["id"].as_str().unwrap();
        let w = scope(item["scope"].as_str().unwrap());
        let v = json!({"id":id,"workspace_id":w,"kind":"action","title":item["title"],"description":"","state":if item["done"]==true{"done"}else{"active"},"version":1,"created_at":now(),"updated_at":now(),"scheduled_date":date,"scheduled_time":item["time"],"fields":{}});
        put(db, w, "items", id, &v)?;
    }
    for (i, s) in raw["learnings"].as_array().unwrap().iter().enumerate() {
        let id = format!("learning-{i}");
        let r = json!({"id":id,"workspace_id":"personal","item_ids":[],"record_type":"learning","body":s,"happened_at":now(),"created_at":now(),"author":"local-owner"});
        put(db, "personal", "records", &id, &r)?;
    }
    Ok(())
}
fn scope(s: &str) -> &str {
    match s {
        "個人" => "personal",
        "チーム" => "team",
        _ => "organization",
    }
}
