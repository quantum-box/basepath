use axum::Router;
use pathbase_api::{service::Service, HttpState};
use rmcp::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--preflight") {
        let report = pathbase_api::preflight::run_from_env().await;
        println!("{}", serde_json::to_string_pretty(&report)?);
        if !report.succeeded() {
            std::process::exit(2);
        }
        return Ok(());
    }
    let path = std::env::var("PATHBASE_DB").unwrap_or_else(|_| "data/pathbase.sqlite3".into());
    let service = Service::open(std::path::Path::new(&path)).map_err(|e| e.message)?;
    let local_preview = std::env::var("PATHBASE_MODE").as_deref() == Ok("local-preview");
    if local_preview {
        service
            .initialize(std::env::var("PATHBASE_SEED_DEMO").as_deref() == Ok("1"))
            .map_err(|e| e.message)?;
    }
    if args.iter().any(|arg| arg == "--mcp-stdio") {
        if !local_preview {
            return Err("Local stdio MCP requires PATHBASE_MODE=local-preview. Hosted MCP OAuth is not configured.".into());
        }
        let server = pathbase_api::mcp::Mcp::new(service)
            .serve(rmcp::transport::stdio())
            .await?;
        server.waiting().await?;
        return Ok(());
    }
    let auth = if local_preview {
        None
    } else {
        Some(std::sync::Arc::new(
            pathbase_api::auth::TachyonAuth::new(
                pathbase_api::auth::AuthConfig::from_env().map_err(|e| e.message)?,
            )
            .await
            .map_err(|e| e.message)?,
        ))
    };
    let field = pathbase_api::field::FieldClient::from_env().map_err(|e| e.message)?;
    let token = std::env::var("PATHBASE_API_TOKEN").unwrap_or_default();
    if local_preview && token.len() < 32 {
        return Err("PATHBASE_API_TOKEN must contain at least 32 characters".into());
    }
    let cloud_port = std::env::var("PORT").ok();
    let port: u16 = cloud_port
        .clone()
        .or_else(|| std::env::var("PATHBASE_API_PORT").ok())
        .unwrap_or_else(|| "1431".into())
        .parse()?;
    let host = if cloud_port.is_some() {
        std::net::Ipv4Addr::UNSPECIFIED
    } else {
        std::net::Ipv4Addr::LOCALHOST
    };
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    let api = pathbase_api::router(HttpState {
        service,
        token,
        auth,
        field,
    });
    let app = if let Ok(web_root) = std::env::var("PATHBASE_WEB_ROOT") {
        let index = std::path::Path::new(&web_root).join("index.html");
        Router::new()
            .nest("/api", api)
            .fallback_service(ServeDir::new(web_root).fallback(ServeFile::new(index)))
    } else {
        api
    };
    eprintln!("PathBase listening on http://{host}:{port}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
