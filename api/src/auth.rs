//! Tachyon authN only. Tenant and policy lookup deliberately happen on protected routes.
use crate::{
    model::{ApiError, Result},
    service::Actor,
};
use axum::http::HeaderMap;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use url::Url;
#[derive(Clone)]
pub struct AuthConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
    pub public_url: String,
    pub tachyon_api_url: String,
}
impl AuthConfig {
    pub fn from_env() -> Result<Self> {
        let required = |k: &str| {
            std::env::var(k)
                .ok()
                .filter(|x| !x.is_empty())
                .ok_or_else(|| {
                    ApiError::new(500, "AUTH_CONFIGURATION", &format!("{k}を設定してください"))
                })
        };
        let config = Self {
            issuer: required("TACHYON_OIDC_ISSUER")?,
            client_id: required("TACHYON_OIDC_CLIENT_ID")?,
            client_secret: std::env::var("TACHYON_OIDC_CLIENT_SECRET")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            redirect_uri: required("TACHYON_OIDC_REDIRECT_URI")?,
            public_url: required("PATHBASE_PUBLIC_URL")?,
            tachyon_api_url: required("TACHYON_API_URL")?,
        };
        config.validate()?;
        Ok(config)
    }
    fn validate(&self) -> Result<()> {
        for u in [
            &self.issuer,
            &self.redirect_uri,
            &self.public_url,
            &self.tachyon_api_url,
        ] {
            validate_url(u)?;
        }
        let redirect = Url::parse(&self.redirect_uri).unwrap();
        let public = Url::parse(&self.public_url).unwrap();
        if redirect.origin() != public.origin() || redirect.path() != "/api/auth/callback" {
            return Err(ApiError::invalid(
                "Tachyon callback must be PATHBASE_PUBLIC_URL/api/auth/callback",
            ));
        }
        Ok(())
    }
}
pub fn validate_url(raw: &str) -> Result<()> {
    let u = Url::parse(raw).map_err(|_| ApiError::invalid("Invalid service URL"))?;
    let loopback = matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if u.username() != ""
        || u.password().is_some()
        || u.fragment().is_some()
        || u.query().is_some()
        || (u.scheme() != "https" && !(u.scheme() == "http" && loopback))
    {
        return Err(ApiError::invalid(
            "Service URLs must use HTTPS (or loopback HTTP for development)",
        ));
    }
    Ok(())
}
#[derive(Clone, Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Identity {
    pub id: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub tenants: Vec<TachyonTenant>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct TachyonTenant {
    pub id: String,
    pub name: String,
}
#[derive(Clone)]
pub struct Session {
    pub identity: Identity,
    pub access_token: String,
    pub selected_tenant: Option<String>,
    refresh_token: Option<String>,
    expires_at: i64,
    session_expires_at: i64,
    refresh_lock: Arc<tokio::sync::Mutex<()>>,
}
#[derive(Clone)]
struct Login {
    verifier: String,
    nonce: String,
    expires_at: i64,
}
#[derive(Clone)]
pub struct TachyonAuth {
    pub config: AuthConfig,
    pub client: Client,
    discovery: Discovery,
    logins: Arc<Mutex<HashMap<String, Login>>>,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
}
#[derive(Deserialize)]
struct Claims {
    sub: String,
    nonce: Option<String>,
}
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    id_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}
#[derive(Deserialize)]
struct PasswordLoginResponse {
    status: String,
    session_token: Option<String>,
}
#[derive(Deserialize)]
struct AuthorizeResponse {
    authorization_code: String,
    state: String,
}
#[derive(Deserialize)]
struct MeResponse {
    user: MeUser,
    tenants: Vec<TachyonTenant>,
}
#[derive(Deserialize)]
struct MeUser {
    id: String,
    name: Option<String>,
    username: Option<String>,
    email: Option<String>,
}
fn unavailable() -> ApiError {
    ApiError::new(
        503,
        "AUTH_UNAVAILABLE",
        "Tachyon認証サービスに接続できません。時間をおいて再試行してください",
    )
}
fn unauthorized() -> ApiError {
    ApiError::new(401, "UNAUTHENTICATED", "Tachyonへのログインが必要です")
}
pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get("cookie")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| {
            s.split(';').find_map(|p| {
                let (k, v) = p.trim().split_once('=')?;
                (k == name).then(|| v.to_owned())
            })
        })
}
fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
impl TachyonAuth {
    /// Build the runtime auth client without making an upstream request. Tachyon's
    /// OAuth2 endpoints are stable API routes; connectivity and discovery are
    /// validated separately by `--preflight`.
    pub fn for_runtime(config: AuthConfig) -> Result<Self> {
        config.validate()?;
        let issuer = config.issuer.trim_end_matches('/');
        let discovery = Discovery {
            issuer: config.issuer.clone(),
            authorization_endpoint: format!("{issuer}/oauth2/authorize"),
            token_endpoint: format!("{issuer}/oauth2/token"),
            jwks_uri: format!("{issuer}/oauth2/jwks"),
        };
        for u in [
            &discovery.authorization_endpoint,
            &discovery.token_endpoint,
            &discovery.jwks_uri,
        ] {
            validate_url(u)?;
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| unavailable())?;
        Ok(Self {
            config,
            client,
            discovery,
            logins: Default::default(),
            sessions: Default::default(),
        })
    }

    pub async fn new(config: AuthConfig) -> Result<Self> {
        let mut auth = Self::for_runtime(config)?;
        let r = auth
            .client
            .get(format!(
                "{}/.well-known/openid-configuration",
                auth.config.issuer.trim_end_matches('/')
            ))
            .send()
            .await
            .map_err(|_| unavailable())?;
        let discovery: Discovery = r
            .error_for_status()
            .map_err(|_| unavailable())?
            .json()
            .await
            .map_err(|_| unavailable())?;
        if discovery.issuer != auth.config.issuer {
            return Err(ApiError::new(
                500,
                "AUTH_CONFIGURATION",
                "OIDC issuer does not match discovery",
            ));
        }
        for u in [
            &discovery.authorization_endpoint,
            &discovery.token_endpoint,
            &discovery.jwks_uri,
        ] {
            validate_url(u)?;
        }
        auth.discovery = discovery;
        Ok(auth)
    }
    pub async fn probe_verification_boundary(&self) -> Result<u16> {
        const INVALID_PROBE_TOKEN: &str = "pathbase-preflight-intentionally-invalid";
        let response = self
            .client
            .post(format!(
                "{}/auth/v1beta/verify",
                self.config.tachyon_api_url.trim_end_matches('/')
            ))
            .bearer_auth(INVALID_PROBE_TOKEN)
            .json(&json!({"token": INVALID_PROBE_TOKEN}))
            .send()
            .await
            .map_err(|_| unavailable())?;
        Ok(response.status().as_u16())
    }
    pub fn cookie_header(&self, name: &str, value: &str, max_age: i64) -> String {
        format!(
            "{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{}",
            if self.config.public_url.starts_with("https:") {
                "; Secure"
            } else {
                ""
            }
        )
    }
    pub fn begin(&self) -> Result<(String, String)> {
        let state = random();
        let verifier = random();
        let nonce = random();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut logins = self.logins.lock().map_err(|_| unavailable())?;
        logins.retain(|_, l| l.expires_at > Utc::now().timestamp());
        if logins.len() > 500 {
            return Err(ApiError::new(
                429,
                "RATE_LIMITED",
                "ログイン要求が多すぎます",
            ));
        }
        logins.insert(
            state.clone(),
            Login {
                verifier,
                nonce: nonce.clone(),
                expires_at: Utc::now().timestamp() + 600,
            },
        );
        let mut url = Url::parse(&self.discovery.authorization_endpoint).unwrap();
        url.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("client_id", &self.config.client_id),
            ("redirect_uri", &self.config.redirect_uri),
            ("scope", "openid profile email"),
            ("state", &state),
            ("nonce", &nonce),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ]);
        Ok((
            url.into(),
            self.cookie_header("pathbase_login", &state, 600),
        ))
    }
    async fn tokens(&self, form: Vec<(&str, String)>) -> Result<TokenResponse> {
        let mut request = self.client.post(&self.discovery.token_endpoint).form(&form);
        if let Some(secret) = &self.config.client_secret {
            request = request.basic_auth(&self.config.client_id, Some(secret));
        }
        let r = request.send().await.map_err(|_| unavailable())?;
        if !r.status().is_success() {
            return Err(if r.status().is_server_error() {
                unavailable()
            } else {
                unauthorized()
            });
        }
        r.json().await.map_err(|_| unavailable())
    }
    pub async fn direct_login(&self, username: &str, password: &str) -> Result<String> {
        let username = username.trim();
        if username.is_empty()
            || username.len() > 320
            || password.is_empty()
            || password.len() > 4096
        {
            return Err(unauthorized());
        }
        let response = self
            .client
            .post(format!(
                "{}/oauth2/login",
                self.config.issuer.trim_end_matches('/')
            ))
            .json(&json!({
                "username": username,
                "password": password,
                "client_id": self.config.client_id,
            }))
            .send()
            .await
            .map_err(|_| unavailable())?;
        if !response.status().is_success() {
            return Err(match response.status().as_u16() {
                429 => ApiError::new(
                    429,
                    "RATE_LIMITED",
                    "ログイン試行が多すぎます。時間をおいて再試行してください",
                ),
                status if status >= 500 => unavailable(),
                _ => unauthorized(),
            });
        }
        let login: PasswordLoginResponse = response.json().await.map_err(|_| unavailable())?;
        if login.status == "new_password_required" {
            return Err(ApiError::new(
                409,
                "NEW_PASSWORD_REQUIRED",
                "Tachyonで新しいパスワードを設定してから再試行してください",
            ));
        }
        let session_token = login.session_token.ok_or_else(unauthorized)?;

        let state = random();
        let verifier = random();
        let nonce = random();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let response = self
            .client
            .post(&self.discovery.authorization_endpoint)
            .bearer_auth(session_token)
            .json(&json!({
                "client_id": self.config.client_id,
                "redirect_uri": self.config.redirect_uri,
                "response_type": "code",
                "scope": "openid profile email",
                "state": state,
                "nonce": nonce,
                "code_challenge": challenge,
                "code_challenge_method": "S256",
            }))
            .send()
            .await
            .map_err(|_| unavailable())?;
        if !response.status().is_success() {
            return Err(match response.status().as_u16() {
                403 => ApiError::new(
                    403,
                    "FORBIDDEN",
                    "このTachyonアカウントはPathBaseを利用できません",
                ),
                429 => ApiError::new(
                    429,
                    "RATE_LIMITED",
                    "ログイン試行が多すぎます。時間をおいて再試行してください",
                ),
                status if status >= 500 => unavailable(),
                _ => unauthorized(),
            });
        }
        let authorization: AuthorizeResponse = response.json().await.map_err(|_| unavailable())?;
        if authorization.state != state || authorization.authorization_code.is_empty() {
            return Err(unauthorized());
        }
        let token = self
            .tokens(vec![
                ("grant_type", "authorization_code".into()),
                ("client_id", self.config.client_id.clone()),
                ("redirect_uri", self.config.redirect_uri.clone()),
                ("code", authorization.authorization_code),
                ("code_verifier", verifier),
            ])
            .await?;
        self.establish_session(token, &nonce).await
    }
    async fn validate_id_token(&self, token: &str, nonce: &str) -> Result<String> {
        let header = decode_header(token).map_err(|_| unauthorized())?;
        if header.alg != Algorithm::RS256 {
            return Err(unauthorized());
        }
        let jwks: JwkSet = self
            .client
            .get(&self.discovery.jwks_uri)
            .send()
            .await
            .map_err(|_| unavailable())?
            .error_for_status()
            .map_err(|_| unavailable())?
            .json()
            .await
            .map_err(|_| unavailable())?;
        let jwk = jwks
            .find(header.kid.as_deref().ok_or_else(unauthorized)?)
            .ok_or_else(unauthorized)?;
        let key = DecodingKey::from_jwk(jwk).map_err(|_| unauthorized())?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[&self.config.client_id]);
        validation.set_issuer(&[&self.config.issuer]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        let claims = decode::<Claims>(token, &key, &validation)
            .map_err(|_| unauthorized())?
            .claims;
        if claims.nonce.as_deref() != Some(nonce) || claims.sub.is_empty() {
            return Err(unauthorized());
        }
        Ok(claims.sub)
    }
    pub async fn verify_identity(&self, access_token: &str) -> Result<Identity> {
        let response = self
            .client
            .get(format!(
                "{}/v1/me",
                self.config.tachyon_api_url.trim_end_matches('/')
            ))
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|_| unavailable())?;
        if !response.status().is_success() {
            return Err(if response.status().is_server_error() {
                unavailable()
            } else {
                unauthorized()
            });
        }
        let me: MeResponse = response.json().await.map_err(|_| unavailable())?;
        if !me.user.id.starts_with("us_")
            || me.user.id.len() > 128
            || me.tenants.iter().any(|tenant| {
                !tenant.id.starts_with("tn_")
                    || tenant.id.len() > 128
                    || tenant.name.trim().is_empty()
            })
        {
            return Err(unauthorized());
        }
        Ok(Identity {
            id: me.user.id,
            name: me.user.name.or(me.user.username),
            email: me.user.email,
            tenants: me.tenants,
        })
    }
    pub async fn callback(&self, headers: &HeaderMap, state: &str, code: &str) -> Result<String> {
        if cookie(headers, "pathbase_login").as_deref() != Some(state) || code.is_empty() {
            return Err(unauthorized());
        }
        let login = self
            .logins
            .lock()
            .map_err(|_| unavailable())?
            .remove(state)
            .ok_or_else(unauthorized)?;
        if login.expires_at < Utc::now().timestamp() {
            return Err(unauthorized());
        }
        let token = self
            .tokens(vec![
                ("grant_type", "authorization_code".into()),
                ("client_id", self.config.client_id.clone()),
                ("redirect_uri", self.config.redirect_uri.clone()),
                ("code", code.into()),
                ("code_verifier", login.verifier),
            ])
            .await?;
        self.establish_session(token, &login.nonce).await
    }
    async fn establish_session(&self, token: TokenResponse, nonce: &str) -> Result<String> {
        self.validate_id_token(token.id_token.as_deref().ok_or_else(unauthorized)?, nonce)
            .await?;
        let identity = self.verify_identity(&token.access_token).await?;
        // No Field tenant lookup, membership resolution or RBAC in authentication.
        let id = random();
        let now = Utc::now().timestamp();
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        sessions.retain(|_, s| s.session_expires_at > now);
        sessions.insert(
            id.clone(),
            Session {
                identity,
                access_token: token.access_token,
                selected_tenant: None,
                refresh_token: token.refresh_token,
                expires_at: now + token.expires_in.unwrap_or(3600).min(86400),
                session_expires_at: now + 8 * 3600,
                refresh_lock: Default::default(),
            },
        );
        Ok(self.cookie_header("pathbase_session", &id, 8 * 3600))
    }
    pub async fn session(&self, headers: &HeaderMap) -> Result<Session> {
        let id = cookie(headers, "pathbase_session").ok_or_else(unauthorized)?;
        let mut session = self
            .sessions
            .lock()
            .map_err(|_| unavailable())?
            .get(&id)
            .cloned()
            .ok_or_else(unauthorized)?;
        if session.session_expires_at < Utc::now().timestamp() {
            self.sessions.lock().map_err(|_| unavailable())?.remove(&id);
            return Err(unauthorized());
        }
        if session.expires_at <= Utc::now().timestamp() + 30 {
            // Serialize refresh for this session, then re-read it: concurrent dashboard
            // requests must not reuse a provider's rotating refresh token.
            let lock = session.refresh_lock.clone();
            let _guard = lock.lock().await;
            session = self
                .sessions
                .lock()
                .map_err(|_| unavailable())?
                .get(&id)
                .cloned()
                .ok_or_else(unauthorized)?;
            if session.expires_at <= Utc::now().timestamp() + 30 {
                let token = self
                    .tokens(vec![
                        ("grant_type", "refresh_token".into()),
                        ("client_id", self.config.client_id.clone()),
                        (
                            "refresh_token",
                            session.refresh_token.clone().ok_or_else(unauthorized)?,
                        ),
                    ])
                    .await?;
                let identity = self.verify_identity(&token.access_token).await?;
                if identity.id != session.identity.id {
                    return Err(unauthorized());
                }
                session.access_token = token.access_token;
                session.refresh_token = token.refresh_token.or(session.refresh_token);
                session.expires_at =
                    Utc::now().timestamp() + token.expires_in.unwrap_or(3600).min(86400);
                let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
                let existing = sessions.get_mut(&id).ok_or_else(unauthorized)?;
                *existing = session.clone();
            }
        }
        // Token revocation / authN changes are checked at the protected boundary.
        let identity = self.verify_identity(&session.access_token).await?;
        if identity.id != session.identity.id {
            return Err(unauthorized());
        }
        if session
            .selected_tenant
            .as_ref()
            .is_some_and(|selected| !identity.tenants.iter().any(|tenant| &tenant.id == selected))
        {
            session.selected_tenant = None;
        }
        session.identity = identity;
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let existing = sessions.get_mut(&id).ok_or_else(unauthorized)?;
        *existing = session.clone();
        Ok(session)
    }
    pub async fn select_tenant(
        &self,
        headers: &HeaderMap,
        tenant_id: &str,
    ) -> Result<TachyonTenant> {
        let mut session = self.session(headers).await?;
        let tenant = session
            .identity
            .tenants
            .iter()
            .find(|tenant| tenant.id == tenant_id)
            .cloned()
            .ok_or_else(|| {
                ApiError::new(
                    403,
                    "TENANT_NOT_ALLOWED",
                    "このTachyonテナントは選択できません",
                )
            })?;
        session.selected_tenant = Some(tenant.id.clone());
        let id = cookie(headers, "pathbase_session").ok_or_else(unauthorized)?;
        let mut sessions = self.sessions.lock().map_err(|_| unavailable())?;
        let existing = sessions.get_mut(&id).ok_or_else(unauthorized)?;
        *existing = session;
        Ok(tenant)
    }
    pub fn logout(&self, headers: &HeaderMap) -> Result<String> {
        if let Some(id) = cookie(headers, "pathbase_session") {
            self.sessions.lock().map_err(|_| unavailable())?.remove(&id);
        }
        Ok(self.cookie_header("pathbase_session", "", 0))
    }
    pub fn actor(session: &Session) -> Actor {
        Actor {
            id: session.identity.id.clone(),
            agent: false,
        }
    }
}
