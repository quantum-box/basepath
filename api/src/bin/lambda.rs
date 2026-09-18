use lambda_http::{run, Error};
use pathbase_api::{service::Service, HttpState};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let local_preview = std::env::var("PATHBASE_MODE").as_deref() == Ok("local-preview");
    // Production requires DATABASE_URL. There is no SQLite fallback: /tmp is
    // local to one execution environment and would silently fork the data.
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
    let app = pathbase_api::router(HttpState {
        service,
        token,
        auth,
        field,
    });

    run(app).await
}
