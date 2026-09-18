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
    let remote_mcp = match std::env::var("PATHBASE_MCP_TOKEN") {
        Ok(mcp_token) => {
            if mcp_token.len() < 32 {
                return Err("PATHBASE_MCP_TOKEN must contain at least 32 characters".into());
            }
            let actor_id = std::env::var("PATHBASE_MCP_ACTOR_ID")
                .map_err(|_| "PATHBASE_MCP_ACTOR_ID is required when remote MCP is enabled")?;
            if actor_id.trim().is_empty() {
                return Err("PATHBASE_MCP_ACTOR_ID must not be empty".into());
            }
            let allowed_hosts = std::env::var("PATHBASE_MCP_ALLOWED_HOSTS")
                .unwrap_or_else(|_| "localhost,127.0.0.1,::1".into())
                .split(',')
                .map(str::trim)
                .filter(|host| !host.is_empty())
                .map(String::from)
                .collect::<Vec<_>>();
            if allowed_hosts.is_empty() {
                return Err("PATHBASE_MCP_ALLOWED_HOSTS must contain at least one host".into());
            }
            Some(pathbase_api::mcp::remote_router(
                service.clone(),
                actor_id,
                mcp_token,
                allowed_hosts,
            ))
        }
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
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
