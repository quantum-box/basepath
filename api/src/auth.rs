//! Tachyon authN only. Tenant and policy lookup deliberately happen on protected routes.
use crate::{
    model::{ApiError, Result},
    service::Actor,
};
use aes_gcm::{
    aead::{rand_core::RngCore, Aead, KeyInit, OsRng, Payload},
    Aes256Gcm, Nonce,
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
    /// Secretless Cognito App Client used for the human sign-in form. ADR-0036
    /// makes Cognito the only issuer of human access tokens, because Field and
    /// the other downstream APIs verify Cognito's issuer and JWKS and reject a
    /// token minted by Tachyon's own OAuth2 authorization server.
    pub cognito_client_id: Option<String>,
    /// Cognito user pool issuer, e.g.
    /// `https://cognito-idp.<region>.amazonaws.com/<pool-id>`.
    pub cognito_issuer: Option<String>,
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
        let optional = |k: &str| {
            std::env::var(k)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let client_id = required("TACHYON_OIDC_CLIENT_ID")?;
        let cognito_issuer = optional("PATHBASE_COGNITO_ISSUER");
        let config = Self {
            issuer: required("TACHYON_OIDC_ISSUER")?,
            client_secret: std::env::var("TACHYON_OIDC_CLIENT_SECRET")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            redirect_uri: required("TACHYON_OIDC_REDIRECT_URI")?,
            public_url: required("PATHBASE_PUBLIC_URL")?,
            tachyon_api_url: required("TACHYON_API_URL")?,
            // An `OAuth2Client` with `useTachyonUserPool` is registered as a
            // Cognito App Client, so `TACHYON_OIDC_CLIENT_ID` is already the
            // user pool client id. Only a deployment that pins a different App
            // Client needs to set this explicitly.
            cognito_client_id: optional("PATHBASE_COGNITO_CLIENT_ID")
                .or_else(|| cognito_issuer.as_ref().map(|_| client_id.clone())),
            cognito_issuer,
            client_id,
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
        match (&self.cognito_client_id, &self.cognito_issuer) {
            (Some(client_id), Some(issuer)) => {
                if client_id.len() > 128
                    || !client_id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    return Err(ApiError::invalid("Invalid Cognito client id"));
                }
                validate_url(issuer)?;
                let url = Url::parse(issuer).unwrap();
                // Loopback issuers are the test/preview mock; only a real
                // deployment is pinned to Cognito's hostname.
                let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
                let host_is_cognito = loopback
                    || url
                        .host_str()
                        .is_some_and(|host| host.starts_with("cognito-idp."));
                if !host_is_cognito || url.path().trim_matches('/').is_empty() {
                    return Err(ApiError::invalid(
                        "PATHBASE_COGNITO_ISSUER must be https://cognito-idp.<region>.amazonaws.com/<user-pool-id>",
                    ));
                }
            }
            (None, None) => {}
            _ => {
                return Err(ApiError::invalid(
                    "Set PATHBASE_COGNITO_CLIENT_ID and PATHBASE_COGNITO_ISSUER together",
                ))
            }
        }
        Ok(())
    }
    /// Cognito's AWS JSON endpoint, derived from the user pool issuer so the
    /// region is never configured twice.
    fn cognito_idp_endpoint(&self) -> Option<String> {
        let issuer = self.cognito_issuer.as_ref()?;
        let url = Url::parse(issuer).ok()?;
        Some(format!("{}/", url.origin().ascii_serialization()))
    }
    fn cognito_jwks_uri(&self) -> Option<String> {
        let issuer = self.cognito_issuer.as_ref()?;
        Some(format!(
            "{}/.well-known/jwks.json",
            issuer.trim_end_matches('/')
        ))
    }
    pub fn cognito_configured(&self) -> bool {
        self.cognito_client_id.is_some() && self.cognito_issuer.is_some()
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
    expires_at: i64,
    session_expires_at: i64,
    /// Never leaves the server. Carried here only so a tenant change can
    /// rewrite the stored envelope without losing it.
    refresh_token: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct SessionEnvelope {
    access_token: String,
    selected_tenant: Option<String>,
    expires_at: i64,
    session_expires_at: i64,
    /// Kept only in the stored envelope, never in the cookie.
    ///
    /// A rotating refresh token in a stateless cookie is a cookie that, once
    /// stolen, renews itself forever with nothing able to stop it. Here it
    /// sits behind a row that signing out deletes.
    #[serde(default)]
    refresh_token: Option<String>,
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
    session_keys: Arc<Vec<[u8; 32]>>,
    /// Where sessions live, when there is somewhere to put them.
    ///
    /// Without it the session is the cookie and lasts exactly as long as one
    /// access token, because a refresh token has nowhere safe to go. With it
    /// the cookie is an opaque id and the session can be renewed — and ended.
    db: Option<crate::db::Db>,
}

/// How long a session may live, however many access tokens it goes through.
///
/// Renewal has to stop somewhere. A session that renews indefinitely is one
/// that never ends, and "sign out everywhere" then means "wait".
const SESSION_LIFETIME: i64 = 12 * 3600;

/// How close to expiry an access token is refreshed.
///
/// Wide enough that a request in flight does not race the expiry, narrow
/// enough that a token is not replaced on every request.
const REFRESH_WINDOW: i64 = 120;
#[derive(Deserialize)]
struct Claims {
    sub: String,
    nonce: Option<String>,
}
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    id_token: Option<String>,
    expires_in: Option<i64>,
    #[serde(default)]
    refresh_token: Option<String>,
}
/// Claims Tachyon's own Cognito verifier checks, mirrored here so PathBase
/// rejects a token before it ever reaches Tachyon or Field.
#[derive(Deserialize)]
struct CognitoAccessClaims {
    token_use: Option<String>,
    client_id: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    exp: Option<i64>,
}

/// What a protected resource learns from a verified access token.
#[derive(Clone, Debug)]
pub struct VerifiedAccessToken {
    /// The OAuth client the token was issued to. This is the audience: a
    /// token minted for one client must not be accepted by another.
    pub client_id: String,
    /// Scopes the authorization server itself put in the token, if any.
    pub scopes: Vec<String>,
    pub expires_at: Option<i64>,
}
#[derive(Deserialize)]
struct InitiateAuthResponse {
    #[serde(rename = "AuthenticationResult")]
    authentication_result: Option<AuthenticationResult>,
    #[serde(rename = "ChallengeName")]
    challenge_name: Option<String>,
}
#[derive(Deserialize)]
struct AuthenticationResult {
    #[serde(rename = "AccessToken")]
    access_token: Option<String>,
    #[serde(rename = "ExpiresIn")]
    expires_in: Option<i64>,
    /// Cognito returns this when the app client allows refresh; without it a
    /// session can only ever last as long as one access token.
    #[serde(rename = "RefreshToken")]
    refresh_token: Option<String>,
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
        let mut key = [0_u8; 32];
        OsRng.fill_bytes(&mut key);
        Self::for_runtime_with_session_keys(config, vec![key])
    }
    pub fn for_runtime_with_session_keys(
        config: AuthConfig,
        session_keys: Vec<[u8; 32]>,
    ) -> Result<Self> {
        config.validate()?;
        if session_keys.is_empty() {
            return Err(ApiError::new(
                500,
                "AUTH_CONFIGURATION",
                "session key is required",
            ));
        }
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
            session_keys: Arc::new(session_keys),
            db: None,
        })
    }
    pub fn for_runtime_from_env(config: AuthConfig) -> Result<Self> {
        Self::for_runtime_with_session_keys(config, session_keys_from_env()?)
    }
    /// Gives sessions somewhere to live, which is what makes renewal possible.
    pub fn with_database(mut self, db: crate::db::Db) -> Self {
        self.db = Some(db);
        self
    }
}

pub fn session_keys_from_env() -> Result<Vec<[u8; 32]>> {
    let raw = std::env::var("PATHBASE_SESSION_KEYS").map_err(|_| {
        ApiError::new(
            500,
            "AUTH_CONFIGURATION",
            "PATHBASE_SESSION_KEYSを設定してください",
        )
    })?;
    raw.split(',')
        .map(|value| {
            let bytes = URL_SAFE_NO_PAD.decode(value.trim()).map_err(|_| {
                ApiError::new(
                    500,
                    "AUTH_CONFIGURATION",
                    "PATHBASE_SESSION_KEYSの形式が不正です",
                )
            })?;
            bytes.try_into().map_err(|_| {
                ApiError::new(500, "AUTH_CONFIGURATION", "session key must be 32 bytes")
            })
        })
        .collect::<Result<Vec<[u8; 32]>>>()
}

impl TachyonAuth {
    pub async fn new(config: AuthConfig) -> Result<Self> {
        let mut auth = Self::for_runtime(config)?;
        auth.discover().await?;
        Ok(auth)
    }
    pub async fn new_with_session_keys(
        config: AuthConfig,
        session_keys: Vec<[u8; 32]>,
    ) -> Result<Self> {
        let mut auth = Self::for_runtime_with_session_keys(config, session_keys)?;
        auth.discover().await?;
        Ok(auth)
    }
    async fn discover(&mut self) -> Result<()> {
        let r = self
            .client
            .get(format!(
                "{}/.well-known/openid-configuration",
                self.config.issuer.trim_end_matches('/')
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
        if discovery.issuer != self.config.issuer {
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
        self.discovery = discovery;
        Ok(())
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
    fn seal_session(&self, session: &SessionEnvelope) -> Result<String> {
        let cipher = Aes256Gcm::new_from_slice(&self.session_keys[0]).map_err(|_| unavailable())?;
        let mut nonce = [0_u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let plaintext = serde_json::to_vec(session)?;
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext.as_ref(),
                    aad: self.session_aad().as_bytes(),
                },
            )
            .map_err(|_| unavailable())?;
        let mut sealed = nonce.to_vec();
        sealed.extend(ciphertext);
        Ok(URL_SAFE_NO_PAD.encode(sealed))
    }
    fn seal_cookie_session(&self, session: &SessionEnvelope) -> Result<String> {
        let encoded = self.seal_session(session)?;
        if encoded.len() > 3800 {
            return Err(ApiError::new(
                503,
                "SESSION_TOO_LARGE",
                "認証セッションを保存できません",
            ));
        }
        Ok(encoded)
    }
    fn open_session(&self, value: &str) -> Result<SessionEnvelope> {
        let sealed = URL_SAFE_NO_PAD.decode(value).map_err(|_| unauthorized())?;
        if sealed.len() <= 12 {
            return Err(unauthorized());
        }
        let (nonce, ciphertext) = sealed.split_at(12);
        for key in self.session_keys.iter() {
            let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| unavailable())?;
            if let Ok(plaintext) = cipher.decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: self.session_aad().as_bytes(),
                },
            ) {
                return serde_json::from_slice(&plaintext).map_err(|_| unauthorized());
            }
        }
        Err(unauthorized())
    }
    fn session_aad(&self) -> String {
        format!(
            "pathbase-session-v1\0{}\0{}",
            self.config.client_id, self.config.public_url
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
    /// Sign in against the Cognito user pool with the secretless App Client.
    ///
    /// ADR-0036 makes Cognito the only issuer of human access tokens. Field
    /// delegates to Tachyon's `/auth/v1beta/verify`, which requires the token's
    /// `iss` to equal the Cognito user pool issuer and its signature to come
    /// from that pool's JWKS, so a Tachyon-issued token can never be accepted.
    /// The Hosted UI is not used: the password is posted to this API by the
    /// PathBase form, forwarded once to Cognito, and never stored or logged.
    async fn cognito_direct_login(&self, username: &str, password: &str) -> Result<String> {
        let client_id = self
            .config
            .cognito_client_id
            .as_ref()
            .ok_or_else(|| ApiError::new(500, "AUTH_CONFIGURATION", "Cognito未設定です"))?;
        let endpoint = self
            .config
            .cognito_idp_endpoint()
            .ok_or_else(|| ApiError::new(500, "AUTH_CONFIGURATION", "Cognito未設定です"))?;
        let response = self
            .client
            .post(endpoint)
            // The AWS JSON protocol is content-typed `x-amz-json-1.1`, so the
            // body is serialized here instead of through reqwest's `json`,
            // which would overwrite the header.
            .header("content-type", "application/x-amz-json-1.1")
            .header(
                "x-amz-target",
                "AWSCognitoIdentityProviderService.InitiateAuth",
            )
            .body(
                json!({
                    "AuthFlow": "USER_PASSWORD_AUTH",
                    "ClientId": client_id,
                    "AuthParameters": {"USERNAME": username, "PASSWORD": password},
                })
                .to_string(),
            )
            .send()
            .await
            .map_err(|_| unavailable())?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            // Cognito reports the failure kind in the body, not the status.
            let kind = response
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|body| {
                    body.get("__type")
                        .and_then(|value| value.as_str())
                        .map(|value| value.rsplit('#').next().unwrap_or(value).to_string())
                })
                .unwrap_or_default();
            return Err(match kind.as_str() {
                "TooManyRequestsException" | "LimitExceededException" => ApiError::new(
                    429,
                    "RATE_LIMITED",
                    "ログイン試行が多すぎます。時間をおいて再試行してください",
                ),
                "PasswordResetRequiredException" => ApiError::new(
                    409,
                    "NEW_PASSWORD_REQUIRED",
                    "Tachyonで新しいパスワードを設定してから再試行してください",
                ),
                _ if status >= 500 => unavailable(),
                _ => unauthorized(),
            });
        }
        let body: InitiateAuthResponse = response.json().await.map_err(|_| unavailable())?;
        if let Some(challenge) = body.challenge_name.as_deref() {
            return Err(match challenge {
                "NEW_PASSWORD_REQUIRED" => ApiError::new(
                    409,
                    "NEW_PASSWORD_REQUIRED",
                    "Tachyonで新しいパスワードを設定してから再試行してください",
                ),
                // MFA and the other challenges need a second round trip that the
                // current sign-in form does not collect.
                _ => ApiError::new(
                    409,
                    "AUTH_CHALLENGE_REQUIRED",
                    "この認証方式にはTachyonでの追加操作が必要です",
                ),
            });
        }
        let result = body.authentication_result.ok_or_else(unauthorized)?;
        let access_token = result.access_token.ok_or_else(unauthorized)?;
        self.validate_cognito_access_token(&access_token).await?;
        self.establish_cognito_session(access_token, result.expires_in, result.refresh_token)
            .await
    }
    /// Reject anything Tachyon's verifier would reject, before it is stored in a
    /// session cookie: pool issuer, pool JWKS signature, expiry, `token_use` and
    /// the App Client this deployment is pinned to.
    async fn validate_cognito_access_token(&self, token: &str) -> Result<()> {
        let verified = self.inspect_access_token(token).await?;
        if Some(verified.client_id.as_str()) != self.config.cognito_client_id.as_deref() {
            return Err(unauthorized());
        }
        Ok(())
    }

    /// Verifies an access token's signature, issuer, expiry and `token_use`
    /// without pinning it to the web sign-in client.
    ///
    /// The caller decides which OAuth client it will accept, so the MCP
    /// endpoint can require its own client and refuse a browser token.
    pub async fn inspect_access_token(&self, token: &str) -> Result<VerifiedAccessToken> {
        let issuer = self
            .config
            .cognito_issuer
            .as_ref()
            .ok_or_else(|| ApiError::new(500, "AUTH_CONFIGURATION", "Cognito未設定です"))?;
        let jwks_uri = self
            .config
            .cognito_jwks_uri()
            .ok_or_else(|| ApiError::new(500, "AUTH_CONFIGURATION", "Cognito未設定です"))?;
        let header = decode_header(token).map_err(|_| unauthorized())?;
        if header.alg != Algorithm::RS256 {
            return Err(unauthorized());
        }
        let jwks: JwkSet = self
            .client
            .get(jwks_uri)
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
        // Cognito access tokens carry `client_id`, not `aud`; it is checked below.
        validation.validate_aud = false;
        validation.set_issuer(&[issuer]);
        validation.set_required_spec_claims(&["exp", "iss", "sub"]);
        let claims = decode::<CognitoAccessClaims>(token, &key, &validation)
            .map_err(|_| unauthorized())?
            .claims;
        if claims.token_use.as_deref() != Some("access") {
            return Err(unauthorized());
        }
        Ok(VerifiedAccessToken {
            client_id: claims.client_id.ok_or_else(unauthorized)?,
            scopes: claims
                .scope
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
            expires_at: claims.exp,
        })
    }
    async fn establish_cognito_session(
        &self,
        access_token: String,
        expires_in: Option<i64>,
        refresh_token: Option<String>,
    ) -> Result<String> {
        // Canonical identity and tenant memberships still come from Tachyon,
        // which accepts a Cognito access token on the same bearer scheme.
        let identity = self.verify_identity(&access_token).await?;
        let now = Utc::now().timestamp();
        let expires_at = now + expires_in.unwrap_or(3600).clamp(60, 8 * 3600);
        // How long the session may live, as opposed to how long this
        // particular access token lasts. Without a refresh token they are the
        // same thing, which is the old behaviour: about an hour.
        let session_expires_at = if refresh_token.is_some() {
            now + SESSION_LIFETIME
        } else {
            expires_at
        };
        let envelope = SessionEnvelope {
            access_token,
            selected_tenant: None,
            expires_at,
            session_expires_at,
            refresh_token,
        };
        self.store_session(&identity.id, envelope).await
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
        if self.config.cognito_configured() {
            return self.cognito_direct_login(username, password).await;
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
        self.verify_identity(&token.access_token).await?;
        // No Field tenant lookup, membership resolution or RBAC in authentication.
        let now = Utc::now().timestamp();
        let expires_at = now + token.expires_in.unwrap_or(3600).min(8 * 3600);
        let identity = self.verify_identity(&token.access_token).await?;
        let session_expires_at = if token.refresh_token.is_some() {
            now + SESSION_LIFETIME
        } else {
            expires_at
        };
        let envelope = SessionEnvelope {
            access_token: token.access_token,
            selected_tenant: None,
            expires_at,
            session_expires_at,
            refresh_token: token.refresh_token,
        };
        self.store_session(&identity.id, envelope).await
    }
    /// Cookie values that name a row rather than carry one.
    const STORED_PREFIX: &'static str = "s1.";

    /// Writes the session where it can be renewed and, more importantly, ended.
    ///
    /// Falls back to the sealed cookie when there is no database: without
    /// somewhere to keep a refresh token there is nothing to renew, so the
    /// session is the token's lifetime and behaves exactly as it used to.
    async fn store_session(&self, actor: &str, envelope: SessionEnvelope) -> Result<String> {
        let now = Utc::now().timestamp();
        let max_age = envelope.session_expires_at - now;
        let Some(db) = &self.db else {
            let mut envelope = envelope;
            // A refresh token would have to ride in the cookie, and a cookie
            // that renews itself forever is the thing this avoids.
            envelope.refresh_token = None;
            let value = self.seal_cookie_session(&envelope)?;
            return Ok(self.cookie_header("pathbase_session", &value, max_age));
        };
        let id = random();
        let sealed = self.seal_session(&envelope)?;
        let mut tx = db.begin_write().await?;
        tx.execute(
            "INSERT INTO sessions(id,actor,envelope,expires_at,created_at,last_used_at) \
             VALUES(?,?,?,?,?,?)",
            &crate::params![
                &id,
                actor,
                &sealed,
                &envelope.session_expires_at.to_string(),
                &now.to_string(),
                &now.to_string()
            ],
        )
        .await?;
        tx.commit().await?;
        Ok(self.cookie_header(
            "pathbase_session",
            &format!("{}{id}", Self::STORED_PREFIX),
            max_age,
        ))
    }

    /// Reads the envelope a cookie refers to, from wherever it lives.
    async fn load_session(&self, value: &str) -> Result<(Option<String>, SessionEnvelope)> {
        let Some(id) = value.strip_prefix(Self::STORED_PREFIX) else {
            // A cookie from before sessions were stored, or a deployment with
            // no database. It carries the envelope itself.
            return Ok((None, self.open_session(value)?));
        };
        let db = self.db.as_ref().ok_or_else(unauthorized)?;
        let mut tx = db.begin_read().await?;
        let row = tx
            .fetch_optional(
                &format!(
                    "SELECT envelope FROM sessions WHERE id=?{}",
                    tx.lock_reads()
                ),
                &crate::params![id],
            )
            .await?;
        // No row is the whole point: signing out deleted it, and this cookie
        // now refers to nothing. Not "expired" — gone.
        let sealed = row.ok_or_else(unauthorized)?.text(0)?;
        Ok((Some(id.to_owned()), self.open_session(&sealed)?))
    }

    /// Exchanges the refresh token for a new access token.
    ///
    /// Returns `None` when there is nothing to refresh with, which is not an
    /// error: it simply means this session ends when its token does.
    async fn refresh_access_token(
        &self,
        envelope: &SessionEnvelope,
    ) -> Result<Option<SessionEnvelope>> {
        let (Some(refresh), Some(client_id)) = (
            envelope.refresh_token.as_deref(),
            self.config.cognito_client_id.as_deref(),
        ) else {
            return Ok(None);
        };
        let response = self
            .client
            .post(
                self.config
                    .cognito_idp_endpoint()
                    .ok_or_else(unauthorized)?,
            )
            .header("content-type", "application/x-amz-json-1.1")
            .header(
                "x-amz-target",
                "AWSCognitoIdentityProviderService.InitiateAuth",
            )
            .body(
                json!({
                    "AuthFlow": "REFRESH_TOKEN_AUTH",
                    "ClientId": client_id,
                    "AuthParameters": {"REFRESH_TOKEN": refresh},
                })
                .to_string(),
            )
            .send()
            .await
            .map_err(|_| unavailable())?;
        if !response.status().is_success() {
            // The upstream has ended the session — a password change, a
            // revoked token, a disabled account. Ending it here too is the
            // correct answer, not a retry.
            return Ok(None);
        }
        let body: InitiateAuthResponse = response.json().await.map_err(|_| unavailable())?;
        let Some(result) = body.authentication_result else {
            return Ok(None);
        };
        let Some(access_token) = result.access_token else {
            return Ok(None);
        };
        self.validate_cognito_access_token(&access_token).await?;
        let now = Utc::now().timestamp();
        Ok(Some(SessionEnvelope {
            access_token,
            selected_tenant: envelope.selected_tenant.clone(),
            expires_at: now + result.expires_in.unwrap_or(3600).clamp(60, 8 * 3600),
            // The session's own end does not move. Renewing the token is not
            // renewing the session.
            session_expires_at: envelope.session_expires_at,
            // Cognito returns a new refresh token only when it rotates them.
            refresh_token: result
                .refresh_token
                .or_else(|| envelope.refresh_token.clone()),
        }))
    }

    /// Ends a session, everywhere, at once.
    ///
    /// This is what a stateless cookie could not do. Deleting the row means
    /// the next request carrying that cookie finds nothing — in whichever
    /// execution environment it lands.
    async fn delete_session(&self, id: &str) -> Result<()> {
        let Some(db) = &self.db else { return Ok(()) };
        let mut tx = db.begin_write().await?;
        tx.execute("DELETE FROM sessions WHERE id=?", &crate::params![id])
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Replaces the stored envelope in place, keeping the same cookie.
    async fn update_session(&self, id: &str, envelope: &SessionEnvelope) -> Result<()> {
        let Some(db) = &self.db else { return Ok(()) };
        let sealed = self.seal_session(envelope)?;
        let mut tx = db.begin_write().await?;
        tx.execute(
            "UPDATE sessions SET envelope=?,last_used_at=? WHERE id=?",
            &crate::params![&sealed, &Utc::now().timestamp().to_string(), id],
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn session(&self, headers: &HeaderMap) -> Result<Session> {
        let value = cookie(headers, "pathbase_session").ok_or_else(unauthorized)?;
        let (id, mut envelope) = self.load_session(&value).await?;
        let now = Utc::now().timestamp();
        // The session's own end is final. Renewal moves the token, not this.
        if envelope.session_expires_at <= now {
            if let Some(id) = &id {
                self.delete_session(id).await?;
            }
            return Err(unauthorized());
        }
        // The token is spent or nearly so. Renew it rather than ending the
        // person's work, which is what used to happen roughly every hour.
        if envelope.expires_at <= now + REFRESH_WINDOW {
            match (&id, self.refresh_access_token(&envelope).await?) {
                (Some(id), Some(renewed)) => {
                    self.update_session(id, &renewed).await?;
                    envelope = renewed;
                }
                // Nothing to renew with, or the upstream refused. Either way
                // this session is over; say so rather than retrying.
                _ => {
                    if let Some(id) = &id {
                        self.delete_session(id).await?;
                    }
                    return Err(unauthorized());
                }
            }
        }
        // Token revocation / authN changes are checked at the protected boundary.
        let identity = self.verify_identity(&envelope.access_token).await?;
        let mut selected_tenant = envelope.selected_tenant;
        if selected_tenant
            .as_ref()
            .is_some_and(|selected| !identity.tenants.iter().any(|tenant| &tenant.id == selected))
        {
            selected_tenant = None;
        }
        Ok(Session {
            identity,
            access_token: envelope.access_token,
            selected_tenant,
            expires_at: envelope.expires_at,
            session_expires_at: envelope.session_expires_at,
            refresh_token: envelope.refresh_token,
        })
    }
    pub async fn select_tenant(
        &self,
        headers: &HeaderMap,
        tenant_id: &str,
    ) -> Result<(TachyonTenant, String)> {
        let session = self.session(headers).await?;
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
        let envelope = SessionEnvelope {
            access_token: session.access_token,
            selected_tenant: Some(tenant.id.clone()),
            expires_at: session.expires_at,
            session_expires_at: session.session_expires_at,
            refresh_token: session.refresh_token,
        };
        let max_age = envelope.session_expires_at - Utc::now().timestamp();
        // A stored session keeps its cookie: the id has not changed, only what
        // it refers to. Re-issuing the cookie here would also be correct, but
        // it would quietly rotate the id on every tenant switch.
        let value = match self
            .load_session(&cookie(headers, "pathbase_session").unwrap_or_default())
            .await?
        {
            (Some(id), _) => {
                self.update_session(&id, &envelope).await?;
                format!("{}{id}", Self::STORED_PREFIX)
            }
            (None, _) => self.seal_cookie_session(&envelope)?,
        };
        Ok((
            tenant,
            self.cookie_header("pathbase_session", &value, max_age),
        ))
    }
    /// Signing out, which now actually ends something.
    ///
    /// Clearing the cookie only ever stopped *this* browser from presenting
    /// it. The row is what the session is, so deleting it is what ends it —
    /// and a copy of the cookie taken elsewhere stops working too.
    pub async fn logout(&self, headers: &HeaderMap) -> Result<String> {
        if let Some(value) = cookie(headers, "pathbase_session") {
            if let Some(id) = value.strip_prefix(Self::STORED_PREFIX) {
                self.delete_session(id).await?;
            }
        }
        Ok(self.cookie_header("pathbase_session", "", 0))
    }
    /// Who this session is, and which tenant it is acting in.
    ///
    /// The tenant comes from the session's own stored selection, revalidated
    /// against Tachyon membership in [`Self::session`] — never from the
    /// request. A session that has not selected one yet gets an empty tenant,
    /// which reaches no workspace; the `TENANT_SELECTION_REQUIRED` gate turns
    /// that into a clear answer before any workspace route runs.
    pub fn actor(session: &Session) -> Actor {
        Actor {
            id: session.identity.id.clone(),
            tenant: session.selected_tenant.clone().unwrap_or_default(),
            agent: false,
            connection: None,
        }
    }
}
