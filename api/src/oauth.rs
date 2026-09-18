//! Basepath as the authorization server for its own MCP endpoint.
//!
//! # Why this exists
//!
//! PathBase does not run an identity provider: people sign in through the
//! Tachyon-managed Cognito user pool, and that does not change here. But an AI
//! host cannot be pointed at that pool as its *authorization server*. Its
//! discovery document advertises no `code_challenge_methods_supported`, has no
//! `registration_endpoint`, and its redirect URIs are fixed when the app is
//! deployed — while a host mints a fresh callback per connection. A client that
//! follows the MCP authorization specification therefore cannot complete a
//! single step of the flow against it.
//!
//! So Basepath issues the tokens for its own resource. Authentication is still
//! Cognito's: the consent screen requires the person's ordinary Basepath
//! session, so no token is granted to anyone who has not signed in. What
//! Basepath adds is the part that was always its own anyway — *which AI client
//! may act for this person, and with what scopes*. That is the delegation row
//! in `mcp_connections`, and now the OAuth grant and the delegation are the
//! same act rather than two screens the person has to find separately.
//!
//! # What a token here is, and is not
//!
//! An access token issued here says: this client, acting for this person, may
//! attempt the operations these scopes cover, against this one resource. It
//! does not approve any change. Every write an MCP client makes is still a
//! proposal that the person approves in Basepath, on Basepath's own origin,
//! with their own session.
//!
//! # Secrets
//!
//! Only the SHA-256 of each code and token is stored. Reading the database
//! yields nothing a client could present.
use crate::db::{Db, Tx};
use crate::mcp_auth::{self, ResourceConfig, GRANTABLE_SCOPES};
use crate::model::{ApiError, Result};
use crate::params;
use crate::service::{new_id, now, Actor, Service};
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// How long a validated authorization request waits for the person to decide.
const REQUEST_TTL_SECONDS: i64 = 15 * 60;
/// How long an authorization code is usable. Single use; the short life is a
/// second bound, not the protection.
const CODE_TTL_SECONDS: i64 = 5 * 60;
const ACCESS_TTL_SECONDS: i64 = 60 * 60;
const REFRESH_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;

/// Marks a Basepath-issued MCP access token, so a token for something else is
/// rejected before it is looked up.
const ACCESS_PREFIX: &str = "pbmcp_at_";
const REFRESH_PREFIX: &str = "pbmcp_rt_";
const CODE_PREFIX: &str = "pbmcp_ac_";
const REQUEST_PREFIX: &str = "pbmcp_rq_";

/// A random secret with at least 240 bits of entropy.
///
/// Built from UUIDv4 bytes because that is the CSPRNG this crate already
/// depends on; two of them are concatenated because one is 122 bits, which is
/// fine for an identifier and thin for a bearer token.
fn secret(prefix: &str) -> String {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    format!(
        "{prefix}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn expiry(seconds: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::seconds(seconds)).to_rfc3339()
}

/// An OAuth error, which has its own wire shape (RFC 6749 §5.2) and must not
/// be dressed up as PathBase's ordinary API error.
#[derive(Debug, Clone)]
pub struct OAuthError {
    pub status: u16,
    pub code: &'static str,
    pub description: String,
}

impl OAuthError {
    fn new(status: u16, code: &'static str, description: impl Into<String>) -> Self {
        Self {
            status,
            code,
            description: description.into(),
        }
    }
    pub fn invalid_request(description: impl Into<String>) -> Self {
        Self::new(400, "invalid_request", description)
    }
    pub fn invalid_grant(description: impl Into<String>) -> Self {
        Self::new(400, "invalid_grant", description)
    }
    pub fn invalid_client(description: impl Into<String>) -> Self {
        Self::new(401, "invalid_client", description)
    }
    pub fn body(&self) -> Value {
        json!({"error": self.code, "error_description": self.description})
    }
}

impl From<ApiError> for OAuthError {
    fn from(error: ApiError) -> Self {
        Self::new(
            if error.status >= 500 { 500 } else { 400 },
            "server_error",
            error.message,
        )
    }
}

type OAuthResult<T> = std::result::Result<T, OAuthError>;

/// RFC 8414 authorization server metadata.
///
/// `code_challenge_methods_supported` is the field whose absence makes an
/// authorization server unusable for MCP, and `registration_endpoint` is what
/// lets a host that mints a callback per connection register at all. Both are
/// the reason this document exists rather than pointing at the user pool.
pub fn metadata(resource: &ResourceConfig) -> Value {
    let base = resource.issuer.trim_end_matches('/');
    let api = resource.api_base.trim_end_matches('/');
    json!({
        "issuer": base,
        // The person's screen is on the app origin; everything a client calls
        // without a browser is on the API's.
        "authorization_endpoint": format!("{base}/oauth/authorize"),
        "token_endpoint": format!("{api}/oauth/token"),
        "registration_endpoint": format!("{api}/oauth/register"),
        "revocation_endpoint": format!("{api}/oauth/revoke"),
        "scopes_supported": GRANTABLE_SCOPES,
        "response_types_supported": ["code"],
        "response_modes_supported": ["query"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        // Public clients only. Basepath issues no client secret, so a stolen
        // registration is not a credential.
        "token_endpoint_auth_methods_supported": ["none"],
        "code_challenge_methods_supported": ["S256"],
        // RFC 9207: the client can tell which authorization server answered.
        "authorization_response_iss_parameter_supported": true,
        "service_documentation": format!("{api}/docs/mcp"),
    })
}

/// A client registered through RFC 7591 dynamic client registration.
#[derive(Debug, Clone)]
pub struct Client {
    pub id: String,
    pub name: String,
    pub redirect_uris: Vec<String>,
}

fn row_to_client(row: &crate::db::Row) -> Result<Client> {
    Ok(Client {
        id: row.text(0)?,
        name: row.text(1)?,
        redirect_uris: row
            .text(2)?
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect(),
    })
}

async fn find_client(tx: &mut Tx, id: &str) -> Result<Option<Client>> {
    let row = tx
        .fetch_optional(
            &format!(
                "SELECT id,name,redirect_uris FROM mcp_clients WHERE id=?{}",
                tx.lock_reads()
            ),
            &params![id],
        )
        .await?;
    row.as_ref().map(row_to_client).transpose()
}

/// A redirect URI a code may be delivered to.
///
/// HTTPS only, except for loopback, which is how a locally-run client receives
/// a code. A fragment is refused because the authorization response appends
/// its own query and a fragment would swallow it.
fn valid_redirect_uri(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    if url.fragment().is_some() {
        return false;
    }
    match url.scheme() {
        "https" => url.host_str().is_some(),
        "http" => matches!(
            url.host_str(),
            Some("127.0.0.1") | Some("localhost") | Some("[::1]")
        ),
        _ => false,
    }
}

/// RFC 7591 dynamic client registration.
///
/// Deliberately unauthenticated, as the specification intends: registering
/// grants nothing at all. No person has consented, no scope is held, and the
/// resulting client id can do nothing until someone signs in to Basepath and
/// approves it. The limits below exist to keep the table small, not to make a
/// trust decision.
pub async fn register(db: &Db, body: &Value) -> OAuthResult<Value> {
    let method = body["token_endpoint_auth_method"]
        .as_str()
        .unwrap_or("none");
    if method != "none" {
        return Err(OAuthError::new(
            400,
            "invalid_client_metadata",
            "Basepath issues public clients only; use token_endpoint_auth_method=none with PKCE",
        ));
    }
    let uris: Vec<String> = body["redirect_uris"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if uris.is_empty() || uris.len() > 8 {
        return Err(OAuthError::new(
            400,
            "invalid_redirect_uri",
            "between 1 and 8 redirect_uris are required",
        ));
    }
    for uri in &uris {
        if uri.len() > 500 || !valid_redirect_uri(uri) {
            return Err(OAuthError::new(
                400,
                "invalid_redirect_uri",
                format!("{uri} is not an absolute https (or loopback http) URI without a fragment"),
            ));
        }
    }
    for grant in body["grant_types"].as_array().unwrap_or(&vec![]) {
        let grant = grant.as_str().unwrap_or_default();
        if !["authorization_code", "refresh_token"].contains(&grant) {
            return Err(OAuthError::new(
                400,
                "invalid_client_metadata",
                format!("unsupported grant_type {grant}"),
            ));
        }
    }
    let name: String = body["client_name"]
        .as_str()
        .unwrap_or("MCP client")
        .chars()
        .take(120)
        .collect();
    let client = Client {
        id: new_id("mcpclient"),
        name,
        redirect_uris: uris,
    };
    let mut tx = db.begin_write().await.map_err(OAuthError::from)?;
    tx.execute(
        "INSERT INTO mcp_clients(id,name,redirect_uris,created_at) VALUES(?,?,?,?)",
        &params![
            &client.id,
            &client.name,
            client.redirect_uris.join("\n"),
            now()
        ],
    )
    .await
    .map_err(OAuthError::from)?;
    tx.commit().await.map_err(OAuthError::from)?;
    Ok(json!({
        "client_id": client.id,
        "client_id_issued_at": chrono::Utc::now().timestamp(),
        "client_name": client.name,
        "redirect_uris": client.redirect_uris,
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
        "scope": GRANTABLE_SCOPES.join(" "),
    }))
}

/// A validated authorization request, waiting for the person's decision.
#[derive(Debug, Clone)]
pub struct PendingRequest {
    pub id: String,
    pub client_id: String,
    pub client_name: String,
    pub scopes: Vec<String>,
    pub resource: String,
    pub redirect_uri: String,
    pub state: String,
}

impl PendingRequest {
    pub fn describe(&self, handle: &str) -> Value {
        json!({
            "request_id": handle,
            "client_name": self.client_name,
            "scopes": self.scopes,
            "resource": self.resource,
            "redirect_uri": self.redirect_uri,
        })
    }
}

/// Builds the redirect back to the client, always carrying `iss` (RFC 9207).
fn redirect_with(
    issuer: &str,
    redirect_uri: &str,
    state: &str,
    params: &[(&str, &str)],
) -> Result<String> {
    let mut url = url::Url::parse(redirect_uri)
        .map_err(|_| ApiError::invalid("redirect_uriを解釈できません"))?;
    {
        let mut query = url.query_pairs_mut();
        for (key, value) in params {
            query.append_pair(key, value);
        }
        if !state.is_empty() {
            query.append_pair("state", state);
        }
        query.append_pair("iss", issuer);
    }
    Ok(url.to_string())
}

/// The outcome of validating an incoming authorization request.
pub enum Authorization {
    /// Ask the person. The handle is the only way to act on this request.
    Ask(Box<PendingRequest>, String),
    /// Refused in a way the specification says to report to the client.
    Redirect(String),
}

/// Validates `GET /oauth/authorize` without asking anyone anything yet.
///
/// An unknown `client_id` or an unregistered `redirect_uri` must *not* be
/// redirected to: doing so would turn this endpoint into an open redirector
/// and would report the error to whoever supplied the URI. Those fail here.
/// Everything else is reported to the registered redirect, as the
/// specification requires.
pub async fn begin_authorization(
    db: &Db,
    resource: &ResourceConfig,
    issuer: &str,
    query: &std::collections::HashMap<String, String>,
) -> Result<Authorization> {
    let get = |key: &str| {
        query
            .get(key)
            .map(String::as_str)
            .unwrap_or_default()
            .trim()
    };
    let client_id = get("client_id");
    let redirect_uri = get("redirect_uri");
    let mut tx = db.begin_read().await?;
    let client = find_client(&mut tx, client_id).await?.ok_or_else(|| {
        ApiError::new(
            400,
            "INVALID_CLIENT",
            "このクライアントは登録されていません",
        )
    })?;
    // The redirect must be one the client registered. A code is delivered
    // there, so an unregistered value is an attempt to have it delivered
    // somewhere else.
    if !client.redirect_uris.iter().any(|uri| uri == redirect_uri) {
        return Err(ApiError::new(
            400,
            "INVALID_REDIRECT_URI",
            "このredirect_uriは登録されていません",
        ));
    }
    let state = get("state").chars().take(500).collect::<String>();
    let refuse = |code: &str, description: &str| -> Result<Authorization> {
        Ok(Authorization::Redirect(redirect_with(
            issuer,
            redirect_uri,
            &state,
            &[("error", code), ("error_description", description)],
        )?))
    };
    if get("response_type") != "code" {
        return refuse(
            "unsupported_response_type",
            "only response_type=code is supported",
        );
    }
    if get("code_challenge_method") != "S256" {
        return refuse("invalid_request", "code_challenge_method=S256 is required");
    }
    let challenge = get("code_challenge");
    if challenge.len() < 43 || challenge.len() > 128 {
        return refuse("invalid_request", "a PKCE code_challenge is required");
    }
    // RFC 8707. A token is bound to one resource; a request naming another
    // one is not a request this server can answer.
    let requested_resource = get("resource");
    if !requested_resource.is_empty()
        && requested_resource.trim_end_matches('/') != resource.resource.trim_end_matches('/')
    {
        return refuse(
            "invalid_target",
            "this authorization server issues tokens for its own MCP endpoint only",
        );
    }
    let mut scopes: Vec<String> = Vec::new();
    for scope in get("scope").split_whitespace() {
        if !GRANTABLE_SCOPES.contains(&scope) {
            return refuse(
                "invalid_scope",
                "requested scope is not offered by this resource",
            );
        }
        if !scopes.iter().any(|held| held == scope) {
            scopes.push(scope.to_owned());
        }
    }
    if scopes.is_empty() {
        // A client that asks for nothing is offered the read scope, which is
        // the least it can do anything with. The person still chooses.
        scopes.push(mcp_auth::SCOPE_READ.to_owned());
    }

    let handle = secret(REQUEST_PREFIX);
    let pending = PendingRequest {
        id: new_id("mcpreq"),
        client_id: client.id.clone(),
        client_name: client.name.clone(),
        scopes,
        resource: resource.resource.clone(),
        redirect_uri: redirect_uri.to_owned(),
        state,
    };
    drop(tx);
    let mut tx = db.begin_write().await?;
    insert_grant(
        &mut tx,
        &pending.id,
        "request",
        &digest(&handle),
        &pending.client_id,
        "",
        "",
        &pending.scopes.join(" "),
        &pending.resource,
        &pending.redirect_uri,
        challenge,
        &pending.state,
        "",
        &expiry(REQUEST_TTL_SECONDS),
    )
    .await?;
    tx.commit().await?;
    Ok(Authorization::Ask(Box::new(pending), handle))
}

#[allow(clippy::too_many_arguments)]
async fn insert_grant(
    tx: &mut Tx,
    id: &str,
    kind: &str,
    secret_hash: &str,
    client_id: &str,
    actor: &str,
    connection_id: &str,
    scopes: &str,
    resource: &str,
    redirect_uri: &str,
    code_challenge: &str,
    state_value: &str,
    chain: &str,
    expires_at: &str,
) -> Result<()> {
    tx.execute(
        "INSERT INTO mcp_grants(id,kind,secret_hash,client_id,actor,connection_id,scopes,resource,\
         redirect_uri,code_challenge,state_value,chain,expires_at,created_at,consumed_at) \
         VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,'')",
        &params![
            id,
            kind,
            secret_hash,
            client_id,
            actor,
            connection_id,
            scopes,
            resource,
            redirect_uri,
            code_challenge,
            state_value,
            chain,
            expires_at,
            now()
        ],
    )
    .await?;
    Ok(())
}

struct Grant {
    id: String,
    client_id: String,
    actor: String,
    connection_id: String,
    scopes: String,
    resource: String,
    redirect_uri: String,
    code_challenge: String,
    state_value: String,
    chain: String,
    expires_at: String,
    consumed_at: String,
}

const GRANT_COLUMNS: &str = "SELECT id,client_id,actor,connection_id,scopes,resource,\
                             redirect_uri,code_challenge,state_value,chain,expires_at,consumed_at \
                             FROM mcp_grants";

fn row_to_grant(row: &crate::db::Row) -> Result<Grant> {
    Ok(Grant {
        id: row.text(0)?,
        client_id: row.text(1)?,
        actor: row.text(2)?,
        connection_id: row.text(3)?,
        scopes: row.text(4)?,
        resource: row.text(5)?,
        redirect_uri: row.text(6)?,
        code_challenge: row.text(7)?,
        state_value: row.text(8)?,
        chain: row.text(9)?,
        expires_at: row.text(10)?,
        consumed_at: row.text(11)?,
    })
}

async fn find_grant(tx: &mut Tx, presented: &str, kind: &str) -> Result<Option<Grant>> {
    let row = tx
        .fetch_optional(
            &format!(
                "{GRANT_COLUMNS} WHERE secret_hash=? AND kind=?{}",
                tx.lock_reads()
            ),
            &params![digest(presented), kind],
        )
        .await?;
    row.as_ref().map(row_to_grant).transpose()
}

async fn consume(tx: &mut Tx, id: &str) -> Result<()> {
    tx.execute(
        "UPDATE mcp_grants SET consumed_at=? WHERE id=? AND consumed_at=''",
        &params![now(), id],
    )
    .await?;
    Ok(())
}

/// Kills every live artifact descended from one authorization code.
///
/// Used when a single-use secret is presented twice: the specification's
/// answer to a replayed code or a rotated-out refresh token is not to refuse
/// that one request, it is to assume the family is compromised.
async fn revoke_chain(tx: &mut Tx, chain: &str) -> Result<()> {
    if chain.is_empty() {
        return Ok(());
    }
    tx.execute(
        "UPDATE mcp_grants SET consumed_at=? WHERE chain=? AND consumed_at=''",
        &params![now(), chain],
    )
    .await?;
    Ok(())
}

/// Every token belonging to a connection stops working.
///
/// Called when a person disconnects an AI client, so "disconnect" means the
/// tokens are dead now, not when they happen to expire.
pub async fn revoke_connection(tx: &mut Tx, connection_id: &str) -> Result<()> {
    tx.execute(
        "UPDATE mcp_grants SET consumed_at=? WHERE connection_id=? AND consumed_at=''",
        &params![now(), connection_id],
    )
    .await?;
    Ok(())
}

/// The person's decision on a pending authorization request.
///
/// This runs with their ordinary Basepath session, on Basepath's own origin.
/// That is what makes it consent: the same click made from inside an AI host
/// would reach this server indistinguishably from the model's own calls.
pub async fn decide(
    service: &Service,
    actor: &Actor,
    issuer: &str,
    handle: &str,
    granted: &[String],
    allow: bool,
) -> Result<Value> {
    // Read first, so the slow parts (workspace provisioning, the delegation
    // row) happen outside the transaction that finally consumes the request.
    let mut tx = service.db.begin_read().await?;
    let pending = live_request(&mut tx, handle).await?;
    let client_name = find_client(&mut tx, &pending.client_id)
        .await?
        .map(|client| client.name)
        .unwrap_or_else(|| "MCP client".to_owned());
    drop(tx);

    if !allow {
        let mut tx = service.db.begin_write().await?;
        let pending = live_request(&mut tx, handle).await?;
        consume(&mut tx, &pending.id).await?;
        tx.commit().await?;
        return Ok(json!({"redirect_to": redirect_with(
            issuer,
            &pending.redirect_uri,
            &pending.state_value,
            &[("error", "access_denied"), ("error_description", "the person declined this connection")],
        )?}));
    }

    // Never wider than what was asked for, and never outside what exists.
    let requested: Vec<&str> = pending.scopes.split_whitespace().collect();
    let mut scopes: Vec<String> = Vec::new();
    for scope in granted {
        if !requested.contains(&scope.as_str()) {
            return Err(ApiError::invalid("要求されていない権限は付与できません"));
        }
        if !scopes.contains(scope) {
            scopes.push(scope.clone());
        }
    }
    if scopes.is_empty() {
        return Err(ApiError::invalid("権限を1つ以上選択してください"));
    }

    // The person's own workspace exists whether they arrive through a browser
    // or an AI client, and the delegation row may already be there from an
    // earlier connection.
    service.provision_personal(actor).await?;
    let existing =
        mcp_auth::ensure_pending(&service.db, &actor.id, &pending.client_id, &client_name).await?;

    // Everything that must not happen twice happens here, in one transaction,
    // against a request that is re-read under the same lock.
    let mut tx = service.db.begin_write().await?;
    let pending = live_request(&mut tx, handle).await?;
    consume(&mut tx, &pending.id).await?;
    let connection =
        mcp_auth::grant_from_consent(&mut tx, &actor.id, &existing.id, &scopes).await?;
    let code = secret(CODE_PREFIX);
    insert_grant(
        &mut tx,
        &new_id("mcpcode"),
        "code",
        &digest(&code),
        &pending.client_id,
        &actor.id,
        &connection.id,
        &scopes.join(" "),
        &pending.resource,
        &pending.redirect_uri,
        &pending.code_challenge,
        "",
        &new_id("mcpchain"),
        &expiry(CODE_TTL_SECONDS),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"redirect_to": redirect_with(
        issuer,
        &pending.redirect_uri,
        &pending.state_value,
        &[("code", code.as_str())],
    )?}))
}

/// The pending authorization request behind this handle, if it is still one.
///
/// Re-read inside whichever transaction is about to act on it, so two
/// simultaneous decisions cannot both produce a code.
async fn live_request(tx: &mut Tx, handle: &str) -> Result<Grant> {
    let pending = find_grant(tx, handle, "request").await?.ok_or_else(|| {
        ApiError::new(410, "AUTHORIZATION_EXPIRED", "この認可リクエストは無効です")
    })?;
    if !pending.consumed_at.is_empty() || pending.expires_at < now() {
        return Err(ApiError::new(
            410,
            "AUTHORIZATION_EXPIRED",
            "この認可リクエストは期限切れです。AI側から接続し直してください",
        ));
    }
    Ok(pending)
}

fn verify_pkce(verifier: &str, challenge: &str) -> bool {
    if verifier.len() < 43 || verifier.len() > 128 {
        return false;
    }
    let computed = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    // Constant time is not the concern here — the challenge is public — but
    // the comparison must be exact.
    computed == challenge
}

async fn issue_pair(
    tx: &mut Tx,
    grant: &Grant,
    scopes: &str,
    chain: &str,
) -> Result<(String, String)> {
    let access = secret(ACCESS_PREFIX);
    let refresh = secret(REFRESH_PREFIX);
    insert_grant(
        tx,
        &new_id("mcpaccess"),
        "access",
        &digest(&access),
        &grant.client_id,
        &grant.actor,
        &grant.connection_id,
        scopes,
        &grant.resource,
        "",
        "",
        "",
        chain,
        &expiry(ACCESS_TTL_SECONDS),
    )
    .await?;
    insert_grant(
        tx,
        &new_id("mcprefresh"),
        "refresh",
        &digest(&refresh),
        &grant.client_id,
        &grant.actor,
        &grant.connection_id,
        scopes,
        &grant.resource,
        "",
        "",
        "",
        chain,
        &expiry(REFRESH_TTL_SECONDS),
    )
    .await?;
    Ok((access, refresh))
}

/// `POST /api/oauth/token`.
pub async fn token(db: &Db, resource: &ResourceConfig, form: &Value) -> OAuthResult<Value> {
    let get = |key: &str| form[key].as_str().unwrap_or_default().trim();
    let requested_resource = get("resource");
    if !requested_resource.is_empty()
        && requested_resource.trim_end_matches('/') != resource.resource.trim_end_matches('/')
    {
        return Err(OAuthError::new(
            400,
            "invalid_target",
            "this authorization server issues tokens for its own MCP endpoint only",
        ));
    }
    match get("grant_type") {
        "authorization_code" => exchange_code(db, form).await,
        "refresh_token" => refresh(db, form).await,
        other => Err(OAuthError::new(
            400,
            "unsupported_grant_type",
            format!("unsupported grant_type {other}"),
        )),
    }
}

async fn exchange_code(db: &Db, form: &Value) -> OAuthResult<Value> {
    let get = |key: &str| form[key].as_str().unwrap_or_default().trim();
    let code = get("code");
    if code.is_empty() {
        return Err(OAuthError::invalid_request("code is required"));
    }
    let mut tx = db.begin_write().await.map_err(OAuthError::from)?;
    let grant = find_grant(&mut tx, code, "code")
        .await
        .map_err(OAuthError::from)?
        .ok_or_else(|| OAuthError::invalid_grant("the authorization code is not valid"))?;
    // A code is single use. Presenting a used one is treated as a compromised
    // family, not as one bad request.
    if !grant.consumed_at.is_empty() {
        revoke_chain(&mut tx, &grant.chain)
            .await
            .map_err(OAuthError::from)?;
        tx.commit().await.map_err(OAuthError::from)?;
        return Err(OAuthError::invalid_grant(
            "this authorization code was already used; the grant has been revoked",
        ));
    }
    if grant.expires_at < now() {
        return Err(OAuthError::invalid_grant(
            "the authorization code has expired",
        ));
    }
    if grant.client_id != get("client_id") {
        return Err(OAuthError::invalid_client(
            "the authorization code was issued to another client",
        ));
    }
    if grant.redirect_uri != get("redirect_uri") {
        return Err(OAuthError::invalid_grant(
            "redirect_uri does not match the authorization request",
        ));
    }
    if !verify_pkce(get("code_verifier"), &grant.code_challenge) {
        return Err(OAuthError::invalid_grant(
            "the PKCE code_verifier does not match",
        ));
    }
    consume(&mut tx, &grant.id)
        .await
        .map_err(OAuthError::from)?;
    let scopes = grant.scopes.clone();
    let chain = grant.chain.clone();
    let (access, refresh) = issue_pair(&mut tx, &grant, &scopes, &chain)
        .await
        .map_err(OAuthError::from)?;
    tx.commit().await.map_err(OAuthError::from)?;
    Ok(json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": ACCESS_TTL_SECONDS,
        "refresh_token": refresh,
        "scope": scopes,
    }))
}

async fn refresh(db: &Db, form: &Value) -> OAuthResult<Value> {
    let get = |key: &str| form[key].as_str().unwrap_or_default().trim();
    let presented = get("refresh_token");
    if presented.is_empty() {
        return Err(OAuthError::invalid_request("refresh_token is required"));
    }
    let mut tx = db.begin_write().await.map_err(OAuthError::from)?;
    let grant = find_grant(&mut tx, presented, "refresh")
        .await
        .map_err(OAuthError::from)?
        .ok_or_else(|| OAuthError::invalid_grant("the refresh token is not valid"))?;
    // Rotation: a token that was already exchanged must never work again, and
    // its reappearance means the family is no longer trustworthy.
    if !grant.consumed_at.is_empty() {
        revoke_chain(&mut tx, &grant.chain)
            .await
            .map_err(OAuthError::from)?;
        tx.commit().await.map_err(OAuthError::from)?;
        return Err(OAuthError::invalid_grant(
            "this refresh token was already used; the grant has been revoked",
        ));
    }
    if grant.expires_at < now() {
        return Err(OAuthError::invalid_grant("the refresh token has expired"));
    }
    if grant.client_id != get("client_id") {
        return Err(OAuthError::invalid_client(
            "the refresh token was issued to another client",
        ));
    }
    // The delegation can be revoked between refreshes, and that answer wins.
    let connection = connection_of(&mut tx, &grant)
        .await
        .map_err(OAuthError::from)?;
    if connection.status != "active" {
        return Err(OAuthError::invalid_grant(
            "the person has disconnected this client in Basepath",
        ));
    }
    // Never wider than what the person currently grants.
    let scopes: Vec<String> = grant
        .scopes
        .split_whitespace()
        .filter(|scope| connection.scopes.iter().any(|held| held == scope))
        .map(str::to_owned)
        .collect();
    if scopes.is_empty() {
        return Err(OAuthError::invalid_grant(
            "the person no longer grants any of these scopes",
        ));
    }
    consume(&mut tx, &grant.id)
        .await
        .map_err(OAuthError::from)?;
    let joined = scopes.join(" ");
    let chain = grant.chain.clone();
    let (access, rotated) = issue_pair(&mut tx, &grant, &joined, &chain)
        .await
        .map_err(OAuthError::from)?;
    tx.commit().await.map_err(OAuthError::from)?;
    Ok(json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": ACCESS_TTL_SECONDS,
        "refresh_token": rotated,
        "scope": joined,
    }))
}

async fn connection_of(tx: &mut Tx, grant: &Grant) -> Result<mcp_auth::Connection> {
    mcp_auth::get_connection(tx, &grant.actor, &grant.connection_id).await
}

/// RFC 7009 token revocation. Always answers 200, as the specification says:
/// a client learning whether a token existed is not useful to it.
pub async fn revoke_token(db: &Db, form: &Value) -> Result<()> {
    let presented = form["token"].as_str().unwrap_or_default().trim();
    if presented.is_empty() {
        return Ok(());
    }
    let kind = if presented.starts_with(REFRESH_PREFIX) {
        "refresh"
    } else {
        "access"
    };
    let mut tx = db.begin_write().await?;
    if let Some(grant) = find_grant(&mut tx, presented, kind).await? {
        if kind == "refresh" {
            revoke_chain(&mut tx, &grant.chain).await?;
        } else {
            consume(&mut tx, &grant.id).await?;
        }
    }
    tx.commit().await
}

/// Who a Basepath-issued MCP access token belongs to.
///
/// Four separate things are checked, and each can fail on its own: the token
/// exists and is live, it was issued for *this* resource, the delegation is
/// still active, and the scopes are the ones the person currently grants —
/// not the ones they granted when the token was minted.
pub async fn verify_access_token(
    db: &Db,
    resource: &ResourceConfig,
    presented: &str,
) -> Result<(Actor, mcp_auth::Connection)> {
    if !presented.starts_with(ACCESS_PREFIX) {
        return Err(ApiError::new(
            401,
            "INVALID_TOKEN",
            "アクセストークンを確認できません",
        ));
    }
    let mut tx = db.begin_write().await?;
    let grant = find_grant(&mut tx, presented, "access")
        .await?
        .ok_or_else(|| ApiError::new(401, "INVALID_TOKEN", "アクセストークンを確認できません"))?;
    if !grant.consumed_at.is_empty() || grant.expires_at < now() {
        return Err(ApiError::new(
            401,
            "INVALID_TOKEN",
            "アクセストークンの有効期限が切れています",
        ));
    }
    if grant.resource.trim_end_matches('/') != resource.resource.trim_end_matches('/') {
        return Err(ApiError::new(
            401,
            "INVALID_AUDIENCE",
            "このMCPエンドポイント向けのトークンではありません",
        ));
    }
    let mut connection = connection_of(&mut tx, &grant).await?;
    // The token carries what it was issued for; the connection carries what
    // the person allows now. The narrower of the two is what applies.
    connection.scopes.retain(|scope| {
        grant
            .scopes
            .split_whitespace()
            .any(|issued| issued == scope)
    });
    tx.execute(
        "UPDATE mcp_connections SET last_used_at=? WHERE id=?",
        &params![now(), &connection.id],
    )
    .await?;
    tx.commit().await?;
    Ok((
        Actor {
            id: grant.actor.clone(),
            agent: true,
            connection: Some(connection.id.clone()),
        },
        connection,
    ))
}

/// Clears artifacts that can no longer be used.
///
/// Consumed and expired rows are kept briefly so a replay is still detectable;
/// past that they are only weight.
pub async fn purge_expired(db: &Db) -> Result<u64> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(45)).to_rfc3339();
    let mut tx = db.begin_write().await?;
    let removed = tx
        .execute(
            "DELETE FROM mcp_grants WHERE expires_at<? AND created_at<?",
            &params![now(), cutoff],
        )
        .await?;
    tx.commit().await?;
    Ok(removed)
}
