use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
};
use tauri::Manager;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiError {
    code: &'static str,
    message: String,
}

impl ApiError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_state",
            message: message.into(),
        }
    }

    fn storage(error: impl std::fmt::Display) -> Self {
        Self {
            code: "storage_error",
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Goal {
    id: String,
    title: String,
    subtitle: String,
    scope: String,
    icon: String,
    purpose: String,
    progress: u8,
    next: String,
    memo: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Task {
    id: String,
    title: String,
    scope: String,
    time: String,
    done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Initiative {
    id: String,
    goal_id: String,
    title: String,
    icon: String,
    progress: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppState {
    goals: Vec<Goal>,
    tasks: Vec<Task>,
    initiatives: Vec<Initiative>,
    next_done: HashMap<String, bool>,
    reflection: String,
    saved_reflection: String,
    activity: Vec<String>,
    selected_id: String,
}

fn validate_state(state: &AppState) -> Result<(), ApiError> {
    if state.goals.is_empty() {
        return Err(ApiError::invalid("目標は1件以上必要です"));
    }

    let mut ids = HashSet::new();
    for (kind, id, title, progress) in state
        .goals
        .iter()
        .map(|item| ("目標", &item.id, &item.title, item.progress))
        .chain(
            state
                .initiatives
                .iter()
                .map(|item| ("取り組み", &item.id, &item.title, item.progress)),
        )
    {
        if id.trim().is_empty() || title.trim().is_empty() {
            return Err(ApiError::invalid(format!("{kind}のIDとタイトルは必須です")));
        }
        if progress > 100 {
            return Err(ApiError::invalid(format!(
                "{kind}の進捗は0〜100で指定してください"
            )));
        }
        if !ids.insert(format!("{kind}:{id}")) {
            return Err(ApiError::invalid(format!(
                "{kind}のIDが重複しています: {id}"
            )));
        }
    }

    for task in &state.tasks {
        if task.id.trim().is_empty() || task.title.trim().is_empty() {
            return Err(ApiError::invalid("行動のIDとタイトルは必須です"));
        }
        if !ids.insert(format!("行動:{}", task.id)) {
            return Err(ApiError::invalid(format!(
                "行動のIDが重複しています: {}",
                task.id
            )));
        }
    }

    let goal_ids: HashSet<_> = state.goals.iter().map(|goal| goal.id.as_str()).collect();
    if !goal_ids.contains(state.selected_id.as_str()) {
        return Err(ApiError::invalid("選択中の目標が存在しません"));
    }
    if let Some(item) = state
        .initiatives
        .iter()
        .find(|item| !goal_ids.contains(item.goal_id.as_str()))
    {
        return Err(ApiError::invalid(format!(
            "取り組み {} の参照先目標が存在しません",
            item.id
        )));
    }

    Ok(())
}

fn state_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, ApiError> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("pathbase-state.json"))
        .map_err(ApiError::storage)
}

#[tauri::command]
fn load_state(app: tauri::AppHandle) -> Result<Option<AppState>, ApiError> {
    let path = state_path(&app)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(ApiError::storage)?;
    let state: AppState = serde_json::from_slice(&bytes).map_err(ApiError::storage)?;
    validate_state(&state)?;
    Ok(Some(state))
}

#[tauri::command]
fn save_state(app: tauri::AppHandle, state: AppState) -> Result<AppState, ApiError> {
    validate_state(&state)?;
    let path = state_path(&app)?;
    let parent = path
        .parent()
        .ok_or_else(|| ApiError::storage("保存先ディレクトリを取得できません"))?;
    fs::create_dir_all(parent).map_err(ApiError::storage)?;

    let temporary = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(&state).map_err(ApiError::storage)?;
    fs::write(&temporary, json).map_err(ApiError::storage)?;
    fs::rename(&temporary, &path).map_err(ApiError::storage)?;
    Ok(state)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![load_state, save_state])
        .run(tauri::generate_context!())
        .expect("error while running PathBase");
}
