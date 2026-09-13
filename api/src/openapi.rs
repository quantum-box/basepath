use crate::model::*;
use serde_json::{json, Value};
use utoipa::OpenApi;
#[derive(OpenApi)]
#[openapi(
    info(title = "PathBase API", version = "0.1.0"),
    components(schemas(
        Item,
        ItemFields,
        Recurrence,
        Relation,
        Record,
        Metric,
        Observation,
        WeeklyReview,
        Operation,
        Workspace,
        Invitation,
        ApiError
    ))
)]
struct Contract;
pub fn document() -> Value {
    let mut doc = serde_json::to_value(Contract::openapi()).unwrap();
    doc["info"]["description"]=json!("Rust application commands shared by HTTP, Tauri and local stdio MCP. Tachyon sessions authenticate HTTP requests; local-preview explicitly permits a loopback bearer credential instead. Resource access checks workspace memberships. Writes require Idempotency-Key, retained for the database lifetime. PATCH and action completion require expected_version. Tachyon browser writes also require X-PathBase-Request: 1 and the configured origin. Lists use cursor paging (default 50, max 200). /snapshot is a workspace dashboard projection. See docs/api.md for input contracts and deployment limits.");
    doc["components"]["securitySchemes"] = json!({"localPreviewBearer":{"type":"http","scheme":"bearer","description":"Only in explicit local-preview mode"},"tachyonSession":{"type":"apiKey","in":"cookie","name":"pathbase_session"}});
    doc["security"] = json!([{"tachyonSession":[]},{"localPreviewBearer":[]}]);
    for (path, methods) in [
        ("/v1/me", vec!["get"]),
        ("/v1/workspaces", vec!["get", "post"]),
        ("/v1/workspaces/{w}", vec!["patch"]),
        ("/v1/workspaces/{w}/members", vec!["get"]),
        ("/v1/workspaces/{w}/members/{id}", vec!["patch", "delete"]),
        ("/v1/workspaces/{w}/leave", vec!["post"]),
        ("/v1/workspaces/{w}/invitations", vec!["post"]),
        ("/v1/workspaces/{w}/invitations/{id}", vec!["delete"]),
        ("/v1/invitations", vec!["get"]),
        ("/v1/invitations/{id}/accept", vec!["post"]),
        ("/v1/invitations/{id}/decline", vec!["post"]),
        ("/v1/settings", vec!["get", "patch"]),
        ("/v1/templates", vec!["get"]),
        ("/v1/integrations/field/tenants", vec!["get"]),
        ("/v1/integrations/field/tasks", vec!["get"]),
        ("/v1/integrations/field/metrics", vec!["get"]),
        ("/v1/workspaces/{w}/field/attach-task", vec!["post"]),
        ("/v1/workspaces/{w}/field/record-metric", vec!["post"]),
        ("/v1/workspaces/{w}/snapshot", vec!["get"]),
        ("/v1/workspaces/{w}/weekly-review", vec!["get"]),
        ("/v1/workspaces/{w}/weekly-reviews", vec!["get"]),
        ("/v1/workspaces/{w}/weekly-reviews/draft", vec!["post"]),
        (
            "/v1/workspaces/{w}/weekly-reviews/{id}/finalize",
            vec!["post"],
        ),
        ("/v1/workspaces/{w}/today", vec!["get"]),
        ("/v1/workspaces/{w}/calendar", vec!["get"]),
        ("/v1/workspaces/{w}/graph", vec!["get"]),
        ("/v1/workspaces/{w}/audit", vec!["get"]),
        ("/v1/workspaces/{w}/items", vec!["get", "post"]),
        ("/v1/workspaces/{w}/items/{id}", vec!["get", "patch"]),
        ("/v1/workspaces/{w}/relations", vec!["get", "post"]),
        ("/v1/workspaces/{w}/relations/{id}", vec!["get", "delete"]),
        ("/v1/workspaces/{w}/records", vec!["get", "post"]),
        ("/v1/workspaces/{w}/metrics", vec!["get", "post"]),
        ("/v1/workspaces/{w}/observations", vec!["get", "post"]),
        ("/v1/workspaces/{w}/views", vec!["get", "post"]),
        ("/v1/workspaces/{w}/views/{id}/query", vec!["post"]),
        ("/v1/workspaces/{w}/actions/{id}/complete", vec!["post"]),
        ("/v1/workspaces/{w}/actions/{id}/reopen", vec!["post"]),
        ("/v1/workspaces/{w}/actions/{id}/skip", vec!["post"]),
        ("/v1/workspaces/{w}/templates/{id}/apply", vec!["post"]),
        ("/v1/workspaces/{w}/onboarding/complete", vec!["post"]),
        ("/v1/workspaces/{w}/changesets", vec!["get"]),
        ("/v1/workspaces/{w}/ai/suggestions/preview", vec!["post"]),
        ("/v1/workspaces/{w}/changesets/preview", vec!["post"]),
        ("/v1/workspaces/{w}/changesets/{id}/approve", vec!["post"]),
        ("/v1/workspaces/{w}/changesets/{id}/apply", vec!["post"]),
        ("/v1/workspaces/{w}/exports", vec!["post"]),
        ("/v1/workspaces/{w}/imports", vec!["post"]),
    ] {
        for method in methods {
            let mut params = vec![];
            for name in ["w", "id"] {
                if path.contains(&format!("{{{name}}}")) {
                    params.push(
                        json!({"name":name,"in":"path","required":true,"schema":{"type":"string"}}),
                    );
                }
            }
            if method != "get" {
                params.push(json!({"name":"Idempotency-Key","in":"header","required":true,"schema":{"type":"string","maxLength":200}}));
            }
            let mut operation = json!({"summary":format!("{} {}",method.to_uppercase(),path),"parameters":params,"responses":{"200":{"description":"Success","content":{"application/json":{"schema":{"type":"object"}}}},"default":{"description":"Structured application error","content":{"application/json":{"schema":{"$ref":"#/components/schemas/ApiError"}}}}}});
            if method != "get" {
                operation["requestBody"] = json!({"required":true,"content":{"application/json":{"schema":{"type":"object","description":"See docs/api.md for allowed fields and examples. Unknown fields are rejected."}}}});
            }
            doc["paths"][path][method] = operation;
        }
    }
    doc
}
