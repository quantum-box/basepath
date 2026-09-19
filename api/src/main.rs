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
    let local_preview = std::env::var("PATHBASE_MODE").as_deref() == Ok("local-preview");
    // `--migrate` applies the schema and exits, for an operator or a
    // deployment gate that wants migration separated from serving.
    if args.iter().any(|arg| arg == "--migrate") {
        let db = pathbase_api::db::connect_from_env(local_preview)
            .await
            .map_err(|e| e.message)?;
        db.migrate().await.map_err(|e| e.message)?;
        let status = db.schema_status().await.map_err(|e| e.message)?;
        println!("{}", serde_json::to_string_pretty(&status)?);
        db.close().await;
        if !status.is_ready() {
            std::process::exit(2);
        }
        return Ok(());
    }
    // Data migration and inventory report. Both read `DATABASE_URL` (or the
    // local-preview SQLite path) as the target.
    if let Some(index) = args.iter().position(|arg| arg == "--migrate-from") {
        let source = args
            .get(index + 1)
            .ok_or("--migrate-from needs a source database URL or SQLite path")?;
        let source = pathbase_api::db::Db::connect(source)
            .await
            .map_err(|e| e.message)?;
        let target = pathbase_api::db::connect_from_env(local_preview)
            .await
            .map_err(|e| e.message)?;
        target.migrate().await.map_err(|e| e.message)?;
        let report = pathbase_api::migrate::migrate_data(
            &source,
            &target,
            args.iter().any(|arg| arg == "--dry-run"),
        )
        .await
        .map_err(|e| e.message)?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        source.close().await;
        target.close().await;
        if !report.succeeded() {
            std::process::exit(2);
        }
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--inventory") {
        let db = pathbase_api::db::connect_from_env(local_preview)
            .await
            .map_err(|e| e.message)?;
        let report = serde_json::json!({
            "inventory": pathbase_api::migrate::inventory(&db).await.map_err(|e| e.message)?,
            "integrity": pathbase_api::migrate::validate_integrity(&db).await.map_err(|e| e.message)?,
        });
        println!("{}", serde_json::to_string_pretty(&report)?);
        db.close().await;
        let passed = report["integrity"]
            .as_array()
            .is_some_and(|checks| checks.iter().all(|check| check["passed"] == true));
        if !passed {
            std::process::exit(2);
        }
        return Ok(());
    }
    let service = Service::open_from_env(local_preview)
        .await
        .map_err(|e| e.message)?;
    if local_preview {
        service
            .initialize(std::env::var("PATHBASE_SEED_DEMO").as_deref() == Ok("1"))
            .await
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
            pathbase_api::auth::TachyonAuth::for_runtime_from_env(
                pathbase_api::auth::AuthConfig::from_env().map_err(|e| e.message)?,
            )
            .map_err(|e| e.message)?
            // Sessions live in the shared database, which is what lets them be
            // renewed and — the part a cookie could never do — ended.
            .with_database(service.db.clone()),
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
    let remote_mcp = pathbase_api::remote_mcp_router(service.clone()).map_err(|e| e.message)?;
    let api = pathbase_api::router_with_mcp(
        HttpState {
            service,
            token,
            auth,
            field,
        },
        remote_mcp,
    );
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
