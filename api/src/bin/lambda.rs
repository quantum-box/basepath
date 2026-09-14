use lambda_http::{run, Error};
use pathbase_api::{service::Service, HttpState};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let path = std::env::var("PATHBASE_DB").unwrap_or_else(|_| "/tmp/pathbase.sqlite3".into());
    let service = Service::open(std::path::Path::new(&path)).map_err(|error| error.message)?;
    let local_preview = std::env::var("PATHBASE_MODE").as_deref() == Ok("local-preview");
    if local_preview {
        service
            .initialize(std::env::var("PATHBASE_SEED_DEMO").as_deref() == Ok("1"))
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
