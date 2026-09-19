//! Basepath as the authorization server for its own MCP endpoint.
//!
//! Written from the attacker's side, because this is where a bearer token for
//! someone's plan comes from. Every test below is a way to try to obtain one
//! without the person, or to keep one after they took it away.
use pathbase_api::{
    mcp_auth::ResourceConfig,
    model::Result,
    oauth, params,
    service::{Actor, Service},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

const RESOURCE: &str = "https://basepath.example/api/mcp";
const ISSUER: &str = "https://basepath.example";

fn resource() -> ResourceConfig {
    ResourceConfig {
        resource: RESOURCE.into(),
        issuer: ISSUER.into(),
        api_base: format!("{ISSUER}/api"),
        consent_url: format!("{ISSUER}/settings/connections"),
    }
}

async fn setup() -> (tempfile::TempDir, Service) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(&dir.path().join("oauth.sqlite3").to_string_lossy())
        .await
        .unwrap();
    service.initialize(false).await.unwrap();
    (dir, service)
}

/// A PKCE pair. The verifier is what only the real client knows.
fn pkce(seed: &str) -> (String, String) {
    use base64::Engine;
    let verifier = format!("{seed}-{seed}-{seed}-{seed}-verifier-0123456789");
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

async fn register(service: &Service, redirect: &str) -> String {
    oauth::register(
        &service.db,
        &json!({
            "client_name": "Test AI host",
            "redirect_uris": [redirect],
            "token_endpoint_auth_method": "none",
        }),
    )
    .await
    .unwrap()["client_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn authorization_query(
    client_id: &str,
    redirect: &str,
    challenge: &str,
    scope: &str,
) -> HashMap<String, String> {
    let mut query = HashMap::new();
    for (key, value) in [
        ("client_id", client_id),
        ("redirect_uri", redirect),
        ("response_type", "code"),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("scope", scope),
        ("state", "state-value"),
        ("resource", RESOURCE),
    ] {
        query.insert(key.to_string(), value.to_string());
    }
    query
}

fn person(id: &str) -> Actor {
    Actor {
        id: id.into(),
        tenant: pathbase_api::service::LOCAL_TENANT.into(),
        agent: false,
        connection: None,
    }
}

fn code_in(redirect_to: &str) -> String {
    url::Url::parse(redirect_to)
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default()
}

/// The whole flow, as a host performs it. Returns the access token.
async fn connect(service: &Service, actor: &str, scopes: &[&str]) -> (String, String, String) {
    let redirect = "https://chatgpt.example/connector/oauth/abc";
    let client_id = register(service, redirect).await;
    let (verifier, challenge) = pkce("connect");
    let asked = oauth::begin_authorization(
        &service.db,
        &resource(),
        ISSUER,
        &authorization_query(&client_id, redirect, &challenge, &scopes.join(" ")),
    )
    .await
    .unwrap();
    let oauth::Authorization::Ask(_, handle) = asked else {
        panic!("the request should have reached the person");
    };
    let decided = oauth::decide(
        service,
        &person(actor),
        ISSUER,
        &handle,
        &scopes.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        true,
    )
    .await
    .unwrap();
    let code = code_in(decided["redirect_to"].as_str().unwrap());
    let tokens = oauth::token(
        &service.db,
        &resource(),
        &json!({
            "grant_type": "authorization_code",
            "code": code,
            "code_verifier": verifier,
            "redirect_uri": redirect,
            "client_id": client_id,
        }),
    )
    .await
    .unwrap();
    (
        tokens["access_token"].as_str().unwrap().to_owned(),
        tokens["refresh_token"].as_str().unwrap().to_owned(),
        client_id,
    )
}

#[test]
fn the_metadata_says_what_a_host_needs_to_decide_it_can_connect() {
    let metadata = oauth::metadata(&resource());
    assert_eq!(metadata["issuer"], ISSUER);
    // The two fields whose absence makes the Cognito pool unusable here.
    assert_eq!(metadata["code_challenge_methods_supported"][0], "S256");
    assert_eq!(
        metadata["registration_endpoint"],
        format!("{ISSUER}/api/oauth/register")
    );
    // Public clients only: there is no secret to leak in a plugin package.
    assert_eq!(metadata["token_endpoint_auth_methods_supported"][0], "none");
    assert_eq!(
        metadata["authorization_response_iss_parameter_supported"],
        true
    );
    assert_eq!(
        metadata["authorization_endpoint"],
        format!("{ISSUER}/oauth/authorize")
    );
}

#[tokio::test]
async fn registration_grants_nothing_and_refuses_what_it_cannot_deliver_to() {
    let (_dir, service) = setup().await;

    // A registration with no usable redirect is not a client.
    for uris in [
        json!([]),
        json!(["http://plaintext.example/cb"]),
        json!(["https://host.example/cb#fragment"]),
        json!(["not-a-url"]),
    ] {
        assert!(
            oauth::register(
                &service.db,
                &json!({"client_name":"x","redirect_uris":uris}),
            )
            .await
            .is_err(),
            "{uris} should be refused"
        );
    }
    // Loopback is allowed, because that is how a locally run client receives
    // its code.
    assert!(oauth::register(
        &service.db,
        &json!({"client_name":"local","redirect_uris":["http://127.0.0.1:8123/cb"]}),
    )
    .await
    .is_ok());
    // Basepath issues no client secret, so a confidential client cannot exist.
    assert!(oauth::register(
        &service.db,
        &json!({"client_name":"x","redirect_uris":["https://host.example/cb"],
                "token_endpoint_auth_method":"client_secret_basic"}),
    )
    .await
    .is_err());

    let registered = oauth::register(
        &service.db,
        &json!({"client_name":"Host","redirect_uris":["https://host.example/cb"]}),
    )
    .await
    .unwrap();
    assert!(registered["client_id"]
        .as_str()
        .unwrap()
        .starts_with("mcpclient_"));
    assert_eq!(registered["token_endpoint_auth_method"], "none");
    // Nothing was granted: registering is not consenting.
    let mut tx = service.db.begin_read().await.unwrap();
    let rows = tx
        .fetch_all("SELECT id FROM mcp_connections", &[])
        .await
        .unwrap();
    assert!(
        rows.is_empty(),
        "no delegation exists until a person says so"
    );
}

#[tokio::test]
async fn an_authorization_request_is_refused_before_it_can_redirect_anywhere_unregistered() {
    let (_dir, service) = setup().await;
    let redirect = "https://host.example/cb";
    let client_id = register(&service, redirect).await;
    let (_, challenge) = pkce("open-redirect");

    // An unknown client, and a redirect the client never registered, both
    // fail here rather than being reported to the supplied URI. Redirecting
    // would make this endpoint an open redirector.
    let mut query = authorization_query("mcpclient_unknown", redirect, &challenge, "pathbase.read");
    assert!(
        oauth::begin_authorization(&service.db, &resource(), ISSUER, &query)
            .await
            .is_err()
    );
    query = authorization_query(
        &client_id,
        "https://evil.example/steal",
        &challenge,
        "pathbase.read",
    );
    assert!(
        oauth::begin_authorization(&service.db, &resource(), ISSUER, &query)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_request_without_s256_or_for_another_resource_is_reported_to_the_client() {
    let (_dir, service) = setup().await;
    let redirect = "https://host.example/cb";
    let client_id = register(&service, redirect).await;
    let (_, challenge) = pkce("refusals");

    async fn refusal(service: &Service, query: HashMap<String, String>) -> String {
        match oauth::begin_authorization(&service.db, &resource(), ISSUER, &query)
            .await
            .unwrap()
        {
            oauth::Authorization::Redirect(url) => url,
            oauth::Authorization::Ask(..) => panic!("this request should not reach a person"),
        }
    }

    // No PKCE: a code intercepted on the way back would be usable.
    let mut query = authorization_query(&client_id, redirect, &challenge, "pathbase.read");
    query.insert("code_challenge_method".into(), "plain".into());
    let url = refusal(&service, query).await;
    assert!(url.contains("error=invalid_request"), "{url}");
    // The client can tell which server answered (RFC 9207).
    assert!(url.contains("iss=https%3A%2F%2Fbasepath.example"), "{url}");
    assert!(url.contains("state=state-value"), "{url}");

    // A token for somebody else's resource is not something to ask about.
    let mut query = authorization_query(&client_id, redirect, &challenge, "pathbase.read");
    query.insert("resource".into(), "https://elsewhere.example/mcp".into());
    assert!(refusal(&service, query)
        .await
        .contains("error=invalid_target"));

    // A scope this resource does not offer.
    let query = authorization_query(&client_id, redirect, &challenge, "pathbase.admin");
    assert!(refusal(&service, query)
        .await
        .contains("error=invalid_scope"));
}

#[tokio::test]
async fn the_person_decides_and_can_narrow_what_was_asked_for() {
    let (_dir, service) = setup().await;
    let redirect = "https://host.example/cb";
    let client_id = register(&service, redirect).await;
    let (verifier, challenge) = pkce("narrow");
    let query = authorization_query(
        &client_id,
        redirect,
        &challenge,
        "pathbase.read pathbase.propose pathbase.apply",
    );
    let oauth::Authorization::Ask(pending, handle) =
        oauth::begin_authorization(&service.db, &resource(), ISSUER, &query)
            .await
            .unwrap()
    else {
        panic!("this request should reach the person");
    };
    assert_eq!(pending.client_name, "Test AI host");
    assert_eq!(pending.scopes.len(), 3);

    // Granting more than was asked for is not a thing the screen can do.
    assert!(oauth::decide(
        &service,
        &person("us_alice"),
        ISSUER,
        &handle,
        &["pathbase.read".into(), "pathbase.admin".into()],
        true,
    )
    .await
    .is_err());

    let decided = oauth::decide(
        &service,
        &person("us_alice"),
        ISSUER,
        &handle,
        &["pathbase.read".into()],
        true,
    )
    .await
    .unwrap();
    let code = code_in(decided["redirect_to"].as_str().unwrap());
    assert!(!code.is_empty());

    // The same request cannot produce a second code.
    assert!(oauth::decide(
        &service,
        &person("us_alice"),
        ISSUER,
        &handle,
        &["pathbase.read".into()],
        true,
    )
    .await
    .is_err());

    let tokens = oauth::token(
        &service.db,
        &resource(),
        &json!({"grant_type":"authorization_code","code":code,"code_verifier":verifier,
                "redirect_uri":redirect,"client_id":client_id}),
    )
    .await
    .unwrap();
    // The token carries only what the person allowed, not what was requested.
    assert_eq!(tokens["scope"], "pathbase.read");
    assert_eq!(tokens["token_type"], "Bearer");

    let (actor, connection) = oauth::verify_access_token(
        &service.db,
        &resource(),
        tokens["access_token"].as_str().unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(actor.id, "us_alice");
    // Arriving through an AI client never stops being an agent.
    assert!(actor.agent);
    assert_eq!(connection.scopes, vec!["pathbase.read".to_string()]);
}

#[tokio::test]
async fn declining_tells_the_client_and_grants_nothing() {
    let (_dir, service) = setup().await;
    let redirect = "https://host.example/cb";
    let client_id = register(&service, redirect).await;
    let (_, challenge) = pkce("decline");
    let oauth::Authorization::Ask(_, handle) = oauth::begin_authorization(
        &service.db,
        &resource(),
        ISSUER,
        &authorization_query(&client_id, redirect, &challenge, "pathbase.read"),
    )
    .await
    .unwrap() else {
        panic!("this request should reach the person");
    };
    let declined = oauth::decide(&service, &person("us_alice"), ISSUER, &handle, &[], false)
        .await
        .unwrap();
    let url = declined["redirect_to"].as_str().unwrap();
    assert!(url.contains("error=access_denied"), "{url}");
    assert!(url.contains("state=state-value"), "{url}");

    let mut tx = service.db.begin_read().await.unwrap();
    let rows = tx
        .fetch_all("SELECT status FROM mcp_connections", &[])
        .await
        .unwrap();
    assert!(rows.is_empty(), "declining leaves no delegation behind");
}

#[tokio::test]
async fn a_code_is_bound_to_its_verifier_its_client_and_its_redirect() {
    let (_dir, service) = setup().await;
    let redirect = "https://host.example/cb";
    let client_id = register(&service, redirect).await;
    let other = register(&service, redirect).await;
    let (verifier, challenge) = pkce("binding");
    let oauth::Authorization::Ask(_, handle) = oauth::begin_authorization(
        &service.db,
        &resource(),
        ISSUER,
        &authorization_query(&client_id, redirect, &challenge, "pathbase.read"),
    )
    .await
    .unwrap() else {
        panic!("this request should reach the person");
    };
    let decided = oauth::decide(
        &service,
        &person("us_alice"),
        ISSUER,
        &handle,
        &["pathbase.read".into()],
        true,
    )
    .await
    .unwrap();
    let code = code_in(decided["redirect_to"].as_str().unwrap());

    let exchange = |body: Value| {
        let db = service.db.clone();
        async move { oauth::token(&db, &resource(), &body).await }
    };
    // Without the verifier, an intercepted code is not enough.
    assert!(
        exchange(json!({"grant_type":"authorization_code","code":code,
        "code_verifier":"wrong-verifier-wrong-verifier-wrong-verifier","redirect_uri":redirect,
        "client_id":client_id}))
        .await
        .is_err()
    );
    // Another registered client cannot redeem it either.
    assert!(
        exchange(json!({"grant_type":"authorization_code","code":code,
        "code_verifier":verifier,"redirect_uri":redirect,"client_id":other}))
        .await
        .is_err()
    );
    // Nor can it be redirected somewhere else.
    assert!(
        exchange(json!({"grant_type":"authorization_code","code":code,
        "code_verifier":verifier,"redirect_uri":"https://evil.example/cb",
        "client_id":client_id}))
        .await
        .is_err()
    );
    // The real exchange still works: none of the above consumed the code.
    let tokens = exchange(json!({"grant_type":"authorization_code","code":code,
        "code_verifier":verifier,"redirect_uri":redirect,"client_id":client_id}))
    .await
    .unwrap();
    let access = tokens["access_token"].as_str().unwrap().to_owned();

    // Replaying the code is treated as a compromised grant, not a bad request:
    // the token it already produced stops working too.
    assert!(
        exchange(json!({"grant_type":"authorization_code","code":code,
        "code_verifier":verifier,"redirect_uri":redirect,"client_id":client_id}))
        .await
        .is_err()
    );
    assert!(
        oauth::verify_access_token(&service.db, &resource(), &access)
            .await
            .is_err(),
        "a replayed code revokes what it had already issued"
    );
}

#[tokio::test]
async fn refresh_rotates_and_a_reused_refresh_token_kills_the_family() {
    let (_dir, service) = setup().await;
    let (access, refresh, client_id) = connect(&service, "us_alice", &["pathbase.read"]).await;

    let rotated = oauth::token(
        &service.db,
        &resource(),
        &json!({"grant_type":"refresh_token","refresh_token":refresh,"client_id":client_id}),
    )
    .await
    .unwrap();
    let second_refresh = rotated["refresh_token"].as_str().unwrap().to_owned();
    assert_ne!(second_refresh, refresh);
    // The access token from before the refresh is still inside its hour.
    assert!(
        oauth::verify_access_token(&service.db, &resource(), &access)
            .await
            .is_ok()
    );

    // Presenting the rotated-out token means someone else has a copy.
    assert!(oauth::token(
        &service.db,
        &resource(),
        &json!({"grant_type":"refresh_token","refresh_token":refresh,"client_id":client_id}),
    )
    .await
    .is_err());
    assert!(
        oauth::token(
            &service.db,
            &resource(),
            &json!({"grant_type":"refresh_token","refresh_token":second_refresh,
                    "client_id":client_id}),
        )
        .await
        .is_err(),
        "the whole family is revoked, not only the replayed token"
    );
    assert!(
        oauth::verify_access_token(&service.db, &resource(), &access)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_token_is_for_one_resource_and_dies_with_the_delegation() {
    let (_dir, service) = setup().await;
    let (access, refresh, client_id) =
        connect(&service, "us_alice", &["pathbase.read", "pathbase.propose"]).await;

    // Another deployment's MCP endpoint — a preview, say — is another resource.
    let elsewhere = ResourceConfig {
        resource: "https://basepath.example/api/mcp/preview".into(),
        issuer: ISSUER.into(),
        api_base: format!("{ISSUER}/api"),
        consent_url: format!("{ISSUER}/settings/connections"),
    };
    assert!(
        oauth::verify_access_token(&service.db, &elsewhere, &access)
            .await
            .is_err(),
        "a production token must not work against the preview resource"
    );
    // And a token this server never issued is not a token.
    assert!(
        oauth::verify_access_token(&service.db, &resource(), "Bearer-looking-but-not-ours")
            .await
            .is_err()
    );

    // The person disconnects in Basepath.
    let (_, connection) = oauth::verify_access_token(&service.db, &resource(), &access)
        .await
        .unwrap();
    service
        .handle(
            &person("us_alice"),
            "POST",
            &format!("/v1/mcp/connections/{}/revoke", connection.id),
            &HashMap::new(),
            json!({}),
            Some("revoke-oauth"),
        )
        .await
        .unwrap();

    // The still-unexpired access token stops working immediately, and the
    // refresh token cannot mint a replacement.
    assert!(
        oauth::verify_access_token(&service.db, &resource(), &access)
            .await
            .is_err(),
        "disconnect must mean now, not at expiry"
    );
    assert!(oauth::token(
        &service.db,
        &resource(),
        &json!({"grant_type":"refresh_token","refresh_token":refresh,"client_id":client_id}),
    )
    .await
    .is_err());
}

#[tokio::test]
async fn narrowing_scopes_in_settings_narrows_a_token_already_issued() {
    let (_dir, service) = setup().await;
    let (access, _, _) = connect(
        &service,
        "us_alice",
        &["pathbase.read", "pathbase.propose", "pathbase.apply"],
    )
    .await;
    let (_, connection) = oauth::verify_access_token(&service.db, &resource(), &access)
        .await
        .unwrap();
    assert_eq!(connection.scopes.len(), 3);

    service
        .handle(
            &person("us_alice"),
            "POST",
            &format!("/v1/mcp/connections/{}/approve", connection.id),
            &HashMap::new(),
            json!({"scopes":["pathbase.read"],"expected_version":connection.version}),
            Some("narrow-oauth"),
        )
        .await
        .unwrap();

    let (_, narrowed) = oauth::verify_access_token(&service.db, &resource(), &access)
        .await
        .unwrap();
    // What the person allows now wins over what the token was minted with.
    assert_eq!(narrowed.scopes, vec!["pathbase.read".to_string()]);
}

#[tokio::test]
async fn an_expired_access_token_is_refused_and_can_be_replaced_by_refreshing() {
    let (_dir, service) = setup().await;
    let (access, refresh, client_id) = connect(&service, "us_alice", &["pathbase.read"]).await;
    age_out(&service, &access).await.unwrap();

    assert!(
        oauth::verify_access_token(&service.db, &resource(), &access)
            .await
            .is_err(),
        "an expired token is not a token"
    );
    let renewed = oauth::token(
        &service.db,
        &resource(),
        &json!({"grant_type":"refresh_token","refresh_token":refresh,"client_id":client_id}),
    )
    .await
    .unwrap();
    assert!(oauth::verify_access_token(
        &service.db,
        &resource(),
        renewed["access_token"].as_str().unwrap()
    )
    .await
    .is_ok());
}

/// Moves one stored token's expiry into the past.
///
/// Reaching into the table is the only way to test expiry without waiting an
/// hour, and it is safe here because the lookup is by the token's own digest.
async fn age_out(service: &Service, token: &str) -> Result<()> {
    let digest = format!("{:x}", Sha256::digest(token.as_bytes()));
    let mut tx = service.db.begin_write().await?;
    let changed = tx
        .execute(
            "UPDATE mcp_grants SET expires_at=? WHERE secret_hash=?",
            &params!["2000-01-01T00:00:00Z", digest],
        )
        .await?;
    assert_eq!(changed, 1, "the token should exist before it is aged out");
    tx.commit().await
}

#[tokio::test]
async fn two_people_using_the_same_client_get_separate_delegations() {
    let (_dir, service) = setup().await;
    let redirect = "https://host.example/cb";
    let client_id = register(&service, redirect).await;

    let mut tokens = Vec::new();
    for actor in ["us_alice", "us_bob"] {
        let (verifier, challenge) = pkce(actor);
        let oauth::Authorization::Ask(_, handle) = oauth::begin_authorization(
            &service.db,
            &resource(),
            ISSUER,
            &authorization_query(&client_id, redirect, &challenge, "pathbase.read"),
        )
        .await
        .unwrap() else {
            panic!("this request should reach the person");
        };
        let decided = oauth::decide(
            &service,
            &person(actor),
            ISSUER,
            &handle,
            &["pathbase.read".into()],
            true,
        )
        .await
        .unwrap();
        let code = code_in(decided["redirect_to"].as_str().unwrap());
        let issued = oauth::token(
            &service.db,
            &resource(),
            &json!({"grant_type":"authorization_code","code":code,"code_verifier":verifier,
                    "redirect_uri":redirect,"client_id":client_id}),
        )
        .await
        .unwrap();
        tokens.push(issued["access_token"].as_str().unwrap().to_owned());
    }

    let (alice, _) = oauth::verify_access_token(&service.db, &resource(), &tokens[0])
        .await
        .unwrap();
    let (bob, bob_connection) = oauth::verify_access_token(&service.db, &resource(), &tokens[1])
        .await
        .unwrap();
    assert_eq!(alice.id, "us_alice");
    assert_eq!(bob.id, "us_bob");
    // One registered client, two delegations: nobody shares a fixed actor.
    assert_ne!(alice.connection, bob.connection);
    assert_eq!(bob_connection.actor, "us_bob");
}
