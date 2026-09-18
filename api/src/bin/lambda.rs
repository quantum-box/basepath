use lambda_http::{run, Error};
use pathbase_api::{service::Service, HttpState};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let local_preview = std::env::var("PATHBASE_MODE").as_deref() == Ok("local-preview");
    // Production requires DATABASE_URL. There is no SQLite fallback: /tmp is
    // local to one execution environment and would silently fork the data.
    //
    // Opening the service also applies migrations, under a database-wide
    // advisory lock so simultaneous cold starts converge instead of racing.
    // This runs inside the app's own network, which is the only place the
    // PrivateLink-only managed TiDB is reachable from. A failure here means
    // the candidate never answers its readiness proof, so Tachyon keeps the
    // previously deployed version serving.
    let service = Service::open_from_env(local_preview)
        .await
        .map_err(|error| error.message)?;
    if local_preview {
        service
            .initialize(std::env::var("PATHBASE_SEED_DEMO").as_deref() == Ok("1"))
            .await
            .map_err(|error| error.message)?;
    }
    let auth = if local_preview {
        None
    } else {
        Some(std::sync::Arc::new(
            pathbase_api::auth::TachyonAuth::for_runtime_from_env(
                pathbase_api::auth::AuthConfig::from_env().map_err(|error| error.message)?,
            )
            .map_err(|error| error.message)?,
        ))
    };
    let field = pathbase_api::field::FieldClient::from_env().map_err(|error| error.message)?;
    let token = std::env::var("PATHBASE_API_TOKEN").unwrap_or_default();
    let remote_mcp =
        pathbase_api::remote_mcp_router(service.clone()).map_err(|error| error.message)?;
    let app = pathbase_api::router_with_mcp(
        HttpState {
            service,
            token,
            auth,
            field,
        },
        remote_mcp,
    );

    run(app).await
}
