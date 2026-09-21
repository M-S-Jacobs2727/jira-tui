use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use token_service::{AppState, Settings, router};
use tower::ServiceExt;
use wiremock::matchers::{body_partial_json, body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn settings(token_url: String, revoke_url: String) -> Settings {
    Settings {
        client_id: "test-client".into(),
        client_secret: "test-secret".into(),
        redirect_uri: "http://127.0.0.1:8787/callback".into(),
        bind_addr: "127.0.0.1:0".into(),
        token_url,
        revoke_url,
    }
}

fn app(settings: &Settings) -> axum::Router {
    router(AppState::from_settings(settings).expect("http client"))
}

async fn send(app: axum::Router, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let response = app.oneshot(request).await.expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, body.to_vec())
}

#[tokio::test]
async fn health_and_config() {
    let settings = settings(
        "http://127.0.0.1:1/oauth/token".into(),
        "http://127.0.0.1:1/oauth/revoke".into(),
    );
    let (status, body) = send(
        app(&settings),
        Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, b"ok");

    let (status, body) = send(
        app(&settings),
        Request::builder()
            .uri("/v1/config")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["client_id"], "test-client");
    assert_eq!(parsed["redirect_uri"], "http://127.0.0.1:8787/callback");
}

#[tokio::test]
async fn exchange_requires_verifier() {
    let settings = settings(
        "http://127.0.0.1:1/oauth/token".into(),
        "http://127.0.0.1:1/oauth/revoke".into(),
    );
    let (status, body) = send(
        app(&settings),
        Request::builder()
            .method("POST")
            .uri("/v1/oauth/exchange")
            .header("content-type", "application/json")
            .body(Body::from(json!({"code": "abc"}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(String::from_utf8_lossy(&body), "code_verifier is required");
}

#[tokio::test]
async fn exchange_proxies_to_atlassian() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_partial_json(json!({
            "grant_type": "authorization_code",
            "client_id": "test-client",
            "client_secret": "test-secret",
            "code": "the-code",
            "redirect_uri": "http://127.0.0.1:8787/callback",
            "code_verifier": "the-verifier"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access",
            "refresh_token": "refresh",
            "expires_in": 3600
        })))
        .mount(&server)
        .await;

    let settings = settings(
        format!("{}/oauth/token", server.uri()),
        format!("{}/oauth/revoke", server.uri()),
    );
    let (status, body) = send(
        app(&settings),
        Request::builder()
            .method("POST")
            .uri("/v1/oauth/exchange")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"code": "the-code", "code_verifier": "the-verifier"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["access_token"], "access");
    assert_eq!(parsed["refresh_token"], "refresh");
}

#[tokio::test]
async fn refresh_proxies_to_atlassian() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_partial_json(json!({
            "grant_type": "refresh_token",
            "client_id": "test-client",
            "client_secret": "test-secret",
            "refresh_token": "old-refresh"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "new-access",
            "refresh_token": "new-refresh",
            "expires_in": 3600
        })))
        .mount(&server)
        .await;

    let settings = settings(
        format!("{}/oauth/token", server.uri()),
        format!("{}/oauth/revoke", server.uri()),
    );
    let (status, body) = send(
        app(&settings),
        Request::builder()
            .method("POST")
            .uri("/v1/oauth/refresh")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"refresh_token": "old-refresh"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["access_token"], "new-access");
    assert_eq!(parsed["refresh_token"], "new-refresh");
}

#[tokio::test]
async fn revoke_proxies_form_to_atlassian() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/revoke"))
        .and(body_string_contains("token=to-revoke"))
        .and(body_string_contains("client_id=test-client"))
        .and(body_string_contains("client_secret=test-secret"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let settings = settings(
        format!("{}/oauth/token", server.uri()),
        format!("{}/oauth/revoke", server.uri()),
    );
    let (status, _) = send(
        app(&settings),
        Request::builder()
            .method("POST")
            .uri("/v1/oauth/revoke")
            .header("content-type", "application/json")
            .body(Body::from(json!({"token": "to-revoke"}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}
