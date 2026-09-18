//! What a deployment has to prove before it may serve traffic.
//!
//! The Cloud App readiness proof points at `/health/ready`, so these rules are
//! what stops a candidate whose migration did not run — or that was pointed at
//! another deployment's database — from becoming the active deployment.
use pathbase_api::{
    db::{connect_from_env, expected_schema_version, Db},
    service::Service,
    HttpState,
};
use serde_json::Value;
use tower::ServiceExt;

async fn readiness(service: Service) -> (u16, Value) {
    let app = pathbase_api::router(HttpState {
        service,
        token: "x".repeat(32),
        auth: None,
        field: None,
    });
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/health/ready")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

// Environment variables are process-wide, so everything that depends on
// PATHBASE_DB_ENVIRONMENT or DATABASE_URL runs in one sequential test rather
// than racing sibling tests in this binary.
#[tokio::test]
async fn readiness_and_environment_claims_gate_the_deployment() {
    for key in ["PATHBASE_DATABASE_URL", "DATABASE_URL"] {
        std::env::remove_var(key);
    }
    std::env::remove_var("PATHBASE_DB_ENVIRONMENT");

    // --- a migrated database reports ready -------------------------------
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("ready.sqlite3").to_string_lossy())
        .await
        .unwrap();

    let (status, body) = readiness(service).await;
    assert_eq!(status, 200);
    assert_eq!(body["status"], "ready");
    assert_eq!(body["schema"], "current");
    assert_eq!(body["reason"], "ok");
    assert_eq!(body["schema_version"], expected_schema_version());
    assert_eq!(body["storage"], "sqlite");
    // The readiness body is what the Cloud App manifest matches on.
    assert!(
        serde_json::to_string(&body)
            .unwrap()
            .contains(r#""schema":"current""#),
        "readinessProof expectedBody must keep matching"
    );

    // --- an unmigrated database is not ready ------------------------------
    let path = dir
        .path()
        .join("empty.sqlite3")
        .to_string_lossy()
        .into_owned();
    // Open without migrating: this is the shape of a candidate whose
    // migration failed.
    let db = Db::connect(&path).await.unwrap();
    let service = Service::new(db);

    let (status, body) = readiness(service).await;
    assert_eq!(status, 503);
    assert_eq!(body["status"], "unavailable");
    assert_eq!(body["reason"], "database_unreachable");

    // --- an unlabelled claim is adopted, not refused ----------------------
    // A deployment can start before its manifest overlay is applied. That is
    // a missing label, not a mix-up, so the next start adopts it instead of
    // taking the deployment down.
    let adopted = dir
        .path()
        .join("adopted.sqlite3")
        .to_string_lossy()
        .into_owned();
    std::env::remove_var("PATHBASE_DB_ENVIRONMENT");
    Service::open(&adopted).await.unwrap();
    std::env::set_var("PATHBASE_DB_ENVIRONMENT", "production");
    let service = Service::open(&adopted).await.unwrap();
    let (status, body) = readiness(service).await;
    assert_eq!(status, 200);
    assert_eq!(body["database_environment"], "production");

    // --- a database claimed by another deployment is refused ---------------
    let path = dir
        .path()
        .join("claimed.sqlite3")
        .to_string_lossy()
        .into_owned();

    // A preview deployment claims the database.
    std::env::set_var("PATHBASE_DB_ENVIRONMENT", "preview");
    Service::open(&path).await.unwrap();

    // A production build pointed at the same DSN must refuse rather than
    // migrate or serve it.
    std::env::set_var("PATHBASE_DB_ENVIRONMENT", "production");
    let error = Service::open(&path).await.unwrap_err();
    assert_eq!(error.code, "DATABASE_ENVIRONMENT_MISMATCH");
    assert!(error.message.contains("preview"), "{}", error.message);

    // Even if it somehow got a service, readiness refuses to serve.
    let db = Db::connect(&path).await.unwrap();
    let (status, body) = readiness(Service::new(db)).await;
    assert_eq!(status, 503);
    assert_eq!(body["reason"], "environment_mismatch");
    assert_eq!(body["environment"], "production");
    assert_eq!(body["database_environment"], "preview");

    // Returning to the owning deployment works again.
    std::env::set_var("PATHBASE_DB_ENVIRONMENT", "preview");
    let service = Service::open(&path).await.unwrap();
    let (status, _) = readiness(service).await;
    assert_eq!(status, 200);
    std::env::remove_var("PATHBASE_DB_ENVIRONMENT");

    // --- production startup stops when no database is configured ----------
    // This is the error the Lambda exits with, which makes the candidate fail
    // its readiness proof and leaves the previous deployment serving.
    let error = connect_from_env(false).await.unwrap_err();
    assert_eq!(error.code, "DATABASE_NOT_CONFIGURED");
}
