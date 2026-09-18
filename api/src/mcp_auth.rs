//! PathBase's MCP endpoint as an OAuth 2.1 protected resource.
//!
//! Three separate questions have to be answered before an AI client may act,
//! and the code keeps them separate on purpose:
//!
//! 1. **Who is calling?** A Bearer access token issued by the Tachyon-managed
//!    Cognito user pool, verified here against the pool's JWKS. PathBase does
//!    not run its own identity provider, and it never hands a browser session
//!    cookie or a shared owner token to an MCP client.
//! 2. **Did the person agree to this connection?** A token proves identity, not
//!    consent. Consent is a row in `mcp_connections`: created on first contact
//!    with no scopes, granted only by an explicit action in PathBase, and
//!    revocable. It lives in the shared database, so a disconnect applies to
//!    every execution environment at once.
//! 3. **May this actor touch this workspace?** Unchanged: the business service
//!    re-checks workspace membership on every operation, so naming another
//!    workspace in the arguments grants nothing.
//!
//! A granted scope still does not approve a change. Every write an MCP client
//! makes is a proposal; a person approves the specific changeset in PathBase.
use crate::db::{Db, Tx};
use crate::model::{ApiError, Result};
use crate::params;
use crate::service::{new_id, now, Actor};
use serde::Serialize;
use serde_json::{json, Value};

/// Scopes an MCP connection can hold.
///
/// They describe what the AI client may *attempt*. None of them approves a
/// change: `Apply` only allows applying a changeset a person already approved.
pub const SCOPE_READ: &str = "pathbase.read";
pub const SCOPE_PROPOSE: &str = "pathbase.propose";
pub const SCOPE_APPLY: &str = "pathbase.apply";
pub const GRANTABLE_SCOPES: [&str; 3] = [SCOPE_READ, SCOPE_PROPOSE, SCOPE_APPLY];

/// Which scope each MCP tool needs.
pub fn required_scope(tool: &str) -> &'static str {
    match tool {
        "pathbase_apply_changes" => SCOPE_APPLY,
        "pathbase_preview_changes"
        | "pathbase_propose_plan"
        | "pathbase_reject_change"
        | "pathbase_complete_action"
        | "pathbase_record_checkin"
        | "pathbase_record_observation" => SCOPE_PROPOSE,
        _ => SCOPE_READ,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Connection {
    pub id: String,
    pub actor: String,
    pub client_id: String,
    pub client_name: String,
    pub scopes: Vec<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: String,
    pub version: i64,
}

impl Connection {
    pub fn allows(&self, scope: &str) -> bool {
        self.status == "active" && self.scopes.iter().any(|granted| granted == scope)
    }
}

fn row_to_connection(row: &crate::db::Row) -> Result<Connection> {
    Ok(Connection {
        id: row.text(0)?,
        actor: row.text(1)?,
        client_id: row.text(2)?,
        client_name: row.text(3)?,
        scopes: row.text(4)?.split_whitespace().map(str::to_owned).collect(),
        status: row.text(5)?,
        created_at: row.text(6)?,
        updated_at: row.text(7)?,
        last_used_at: row.text(8)?,
        version: row.int(9)?,
    })
}

const SELECT: &str = "SELECT id,actor,client_id,client_name,scopes,status,created_at,updated_at,\
                      last_used_at,version FROM mcp_connections";

pub async fn list_connections(tx: &mut Tx, actor: &str) -> Result<Vec<Connection>> {
    let rows = tx
        .fetch_all(
            &format!(
                "{SELECT} WHERE actor=? ORDER BY created_at DESC{}",
                tx.lock_reads()
            ),
            &params![actor],
        )
        .await?;
    rows.iter().map(row_to_connection).collect()
}

pub async fn get_connection(tx: &mut Tx, actor: &str, id: &str) -> Result<Connection> {
    let row = tx
        .fetch_optional(
            &format!("{SELECT} WHERE id=? AND actor=?{}", tx.lock_reads()),
            &params![id, actor],
        )
        .await?
        .ok_or_else(ApiError::missing)?;
    row_to_connection(&row)
}

async fn find_for_client(tx: &mut Tx, actor: &str, client_id: &str) -> Result<Option<Connection>> {
    let row = tx
        .fetch_optional(
            &format!("{SELECT} WHERE actor=? AND client_id=?{}", tx.lock_reads()),
            &params![actor, client_id],
        )
        .await?;
    row.as_ref().map(row_to_connection).transpose()
}

/// Records that this client asked, without granting anything.
///
/// The first contact from a new AI client creates a `pending` row with no
/// scopes. Nothing becomes readable until a person approves it in PathBase.
pub async fn ensure_pending(
    db: &Db,
    actor: &str,
    client_id: &str,
    client_name: &str,
) -> Result<Connection> {
    let mut tx = db.begin_write().await?;
    if let Some(existing) = find_for_client(&mut tx, actor, client_id).await? {
        tx.execute(
            "UPDATE mcp_connections SET last_used_at=? WHERE id=?",
            &params![now(), &existing.id],
        )
        .await?;
        tx.commit().await?;
        return Ok(existing);
    }
    let connection = Connection {
        id: new_id("mcpconn"),
        actor: actor.into(),
        client_id: client_id.into(),
        client_name: client_name.chars().take(120).collect(),
        scopes: vec![],
        status: "pending".into(),
        created_at: now(),
        updated_at: now(),
        last_used_at: now(),
        version: 1,
    };
    let sql = tx.dialect().insert_ignore(
        "mcp_connections",
        &[
            "id",
            "actor",
            "client_id",
            "client_name",
            "scopes",
            "status",
            "created_at",
            "updated_at",
            "last_used_at",
            "version",
        ],
    );
    tx.execute(
        &sql,
        &params![
            &connection.id,
            &connection.actor,
            &connection.client_id,
            &connection.client_name,
            "",
            &connection.status,
            &connection.created_at,
            &connection.updated_at,
            &connection.last_used_at,
            1i64
        ],
    )
    .await?;
    // Another execution environment may have inserted it first.
    let stored = find_for_client(&mut tx, actor, client_id)
        .await?
        .unwrap_or(connection);
    tx.commit().await?;
    Ok(stored)
}

/// Grants the scopes a person selected. Never widens beyond the grantable set.
pub async fn approve(
    tx: &mut Tx,
    actor: &str,
    id: &str,
    scopes: &[String],
    expected_version: i64,
) -> Result<Connection> {
    let current = get_connection(tx, actor, id).await?;
    if current.version != expected_version {
        let mut error = ApiError::new(
            409,
            "VERSION_CONFLICT",
            "この接続は別の操作で更新されています",
        );
        error.details =
            json!({"expected_version":expected_version,"current_version":current.version});
        return Err(error);
    }
    if current.status == "revoked" {
        return Err(ApiError::new(
            409,
            "CONNECTION_REVOKED",
            "この接続は解除済みです。AI側から接続し直してください",
        ));
    }
    let mut granted: Vec<String> = Vec::new();
    for scope in scopes {
        if !GRANTABLE_SCOPES.contains(&scope.as_str()) {
            return Err(ApiError::invalid(&format!("許可できない権限です: {scope}")));
        }
        if !granted.contains(scope) {
            granted.push(scope.clone());
        }
    }
    if granted.is_empty() {
        return Err(ApiError::invalid("権限を1つ以上選択してください"));
    }
    tx.execute(
        "UPDATE mcp_connections SET scopes=?,status='active',updated_at=?,version=version+1 \
         WHERE id=? AND actor=?",
        &params![granted.join(" "), now(), id, actor],
    )
    .await?;
    get_connection(tx, actor, id).await
}

/// Grants scopes as part of the OAuth consent screen.
///
/// Separate from `approve` on purpose. `approve` is the settings screen acting
/// on a connection that already exists, so it takes the version it was shown
/// and refuses a connection the person has disconnected — silently
/// reactivating one there would be a surprise.
///
/// This is the other case: the person is *now*, on Basepath's own origin, with
/// their own session, authorizing this client. Re-authorizing something they
/// disconnected earlier is exactly what they asked for, and there is no
/// version on screen to check because the screen is the authorization request.
pub async fn grant_from_consent(
    tx: &mut Tx,
    actor: &str,
    id: &str,
    scopes: &[String],
) -> Result<Connection> {
    get_connection(tx, actor, id).await?;
    let mut granted: Vec<String> = Vec::new();
    for scope in scopes {
        if !GRANTABLE_SCOPES.contains(&scope.as_str()) {
            return Err(ApiError::invalid(&format!("許可できない権限です: {scope}")));
        }
        if !granted.contains(scope) {
            granted.push(scope.clone());
        }
    }
    if granted.is_empty() {
        return Err(ApiError::invalid("権限を1つ以上選択してください"));
    }
    tx.execute(
        "UPDATE mcp_connections SET scopes=?,status='active',updated_at=?,version=version+1 \
         WHERE id=? AND actor=?",
        &params![granted.join(" "), now(), id, actor],
    )
    .await?;
    get_connection(tx, actor, id).await
}

/// Disconnects. A still-valid access token stops working immediately.
pub async fn revoke(tx: &mut Tx, actor: &str, id: &str) -> Result<Connection> {
    get_connection(tx, actor, id).await?;
    tx.execute(
        "UPDATE mcp_connections SET scopes='',status='revoked',updated_at=?,version=version+1 \
         WHERE id=? AND actor=?",
        &params![now(), id, actor],
    )
    .await?;
    // "Disconnected" has to mean the tokens are dead now, not when they would
    // have expired. They are in the same transaction as the status change, so
    // there is no window where one is true and the other is not.
    crate::oauth::revoke_connection(tx, id).await?;
    get_connection(tx, actor, id).await
}

/// Configuration for the protected resource.
#[derive(Debug, Clone)]
pub struct ResourceConfig {
    /// Canonical URL of the MCP endpoint, which is also the resource
    /// identifier clients request a token for (RFC 8707).
    pub resource: String,
    /// The authorization server for this resource, which is Basepath itself.
    ///
    /// Not the Cognito user pool: its discovery document advertises no PKCE
    /// method and it offers no way for a host that mints a callback per
    /// connection to register. `api/src/oauth.rs` carries the full reasoning.
    /// People still authenticate against the pool; what Basepath issues is the
    /// delegation to an AI client.
    pub issuer: String,
    /// Where the Rust API is reachable from outside.
    ///
    /// In a deployment the Cloudflare Worker serves the browser app and
    /// forwards `/api/*` to this process, so the API's public base is
    /// `{public_url}/api`. Running the API directly — a test, or the
    /// single-origin container — it is the origin itself. The OAuth metadata
    /// publishes absolute endpoint URLs, so this cannot be guessed.
    pub api_base: String,
    /// Where a person manages or disconnects a connection after granting it.
    pub consent_url: String,
}

impl ResourceConfig {
    /// Reads the MCP resource configuration.
    ///
    /// Returns `None` when `PATHBASE_MCP_ENABLED` is not set, which is how a
    /// deployment declares that it does not expose a hosted MCP endpoint.
    pub fn from_env() -> Result<Option<Self>> {
        let enabled = std::env::var("PATHBASE_MCP_ENABLED")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .is_some_and(|value| ["1", "true", "yes", "on"].contains(&value.as_str()));
        if !enabled {
            return Ok(None);
        }
        let public_url = std::env::var("PATHBASE_PUBLIC_URL")
            .ok()
            .map(|value| value.trim_end_matches('/').to_owned())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ApiError::new(
                    500,
                    "AUTH_CONFIGURATION",
                    "PATHBASE_PUBLIC_URLを設定してください",
                )
            })?;
        crate::auth::validate_url(&public_url)?;
        let resource = std::env::var("PATHBASE_MCP_RESOURCE")
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("{public_url}/api/mcp"));
        crate::auth::validate_url(&resource)?;
        let api_base = std::env::var("PATHBASE_API_BASE_URL")
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("{public_url}/api"));
        crate::auth::validate_url(&api_base)?;
        Ok(Some(Self {
            resource,
            issuer: public_url.clone(),
            api_base,
            consent_url: format!("{public_url}/settings/connections"),
        }))
    }

    /// RFC 9728 protected resource metadata.
    pub fn metadata(&self) -> Value {
        json!({
            "resource": self.resource,
            "authorization_servers": [self.issuer],
            "scopes_supported": GRANTABLE_SCOPES,
            "bearer_methods_supported": ["header"],
            "resource_documentation": format!("{}/docs/mcp", self.resource.trim_end_matches("/mcp")),
        })
    }

    /// The path RFC 9728 expects the metadata at, derived from the resource
    /// URL's path so a client that follows the spec finds it.
    pub fn metadata_path(&self) -> String {
        let path = url::Url::parse(&self.resource)
            .map(|url| url.path().trim_end_matches('/').to_owned())
            .unwrap_or_default();
        format!("/.well-known/oauth-protected-resource{path}")
    }

    pub fn metadata_url(&self) -> String {
        match url::Url::parse(&self.resource) {
            Ok(url) => format!(
                "{}://{}{}",
                url.scheme(),
                url.host_str().unwrap_or_default(),
                self.metadata_path()
            ),
            Err(_) => self.metadata_path(),
        }
    }

    /// `WWW-Authenticate` value pointing a client at the metadata, as the MCP
    /// authorization specification requires.
    pub fn challenge(&self, error: Option<(&str, &str)>) -> String {
        let mut value = format!(
            "Bearer realm=\"pathbase\", resource_metadata=\"{}\"",
            self.metadata_url()
        );
        if let Some((code, description)) = error {
            value.push_str(&format!(
                ", error=\"{code}\", error_description=\"{description}\""
            ));
        }
        value
    }
}

/// Who is calling, and what they are allowed to attempt.
#[derive(Debug, Clone)]
pub struct McpIdentity {
    pub actor: Actor,
    pub connection: Connection,
}

impl McpIdentity {
    pub fn require(&self, scope: &str) -> Result<()> {
        if self.connection.allows(scope) {
            return Ok(());
        }
        Err(insufficient_scope(scope, &self.connection))
    }
}

pub fn insufficient_scope(scope: &str, connection: &Connection) -> ApiError {
    if connection.status == "revoked" {
        return ApiError::new(
            403,
            "CONNECTION_REVOKED",
            "この接続は解除されています。PathBaseで接続し直してください",
        );
    }
    if connection.status == "pending" {
        return ApiError::new(
            403,
            "CONNECTION_APPROVAL_REQUIRED",
            "PathBaseの接続設定でこのAIクライアントを許可してください",
        );
    }
    ApiError::new(
        403,
        "INSUFFICIENT_SCOPE",
        &format!("この接続には {scope} の権限がありません"),
    )
}
