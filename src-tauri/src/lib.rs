use pathbase_api::{
    model::ApiError,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use std::collections::HashMap;
use tauri::Manager;

#[tauri::command]
async fn pathbase_request(
    service: tauri::State<'_, Service>,
    method: String,
    path: String,
    body: Value,
    key: String,
) -> Result<Value, ApiError> {
    if path == "/auth/status" {
        return Ok(json!({"mode":"local-preview","configured":false,"field_configured":false}));
    }
    // This bridge is only installed for the explicit local development window.
    // Authenticated desktop deployments navigate to the same Tachyon-protected web app.
    let url = url::Url::parse(&format!("http://localhost{path}"))
        .map_err(|_| ApiError::invalid("Invalid API path"))?;
    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let path = url.path().to_owned();
    service
        .inner()
        .handle(&Actor::local(), &method, &path, &query, body, Some(&key))
        .await
}
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if let Ok(web_url) = std::env::var("PATHBASE_WEB_URL") {
                pathbase_api::auth::validate_url(&web_url).map_err(|e| e.message)?;
                let window = app
                    .get_webview_window("main")
                    .ok_or("Main window unavailable")?;
                window.navigate(web_url.parse()?)?;
            } else if cfg!(debug_assertions) {
                let path = app.path().app_data_dir()?.join("preview.sqlite3");
                let service = tauri::async_runtime::block_on(async {
                    // The desktop debug window is an explicit local preview; it
                    // never shares the production database.
                    let service = Service::open(&path.to_string_lossy()).await?;
                    service.initialize(true).await?;
                    Ok::<_, ApiError>(service)
                })
                .map_err(|e| e.message)?;
                app.manage(service);
            } else {
                return Err(
                    "PATHBASE_WEB_URL must point to the Tachyon-authenticated PathBase deployment"
                        .into(),
                );
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![pathbase_request])
        .run(tauri::generate_context!())
        .expect("error while running PathBase");
}
