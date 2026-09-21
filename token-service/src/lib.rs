use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub const DEFAULT_REDIRECT_URI: &str = "http://127.0.0.1:8787/callback";
pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8788";
pub const DEFAULT_TOKEN_URL: &str = "https://auth.atlassian.com/oauth/token";
pub const DEFAULT_REVOKE_URL: &str = "https://auth.atlassian.com/oauth/revoke";

#[derive(Debug, Clone)]
pub struct Settings {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub bind_addr: String,
    pub token_url: String,
    pub revoke_url: String,
}

impl Settings {
    pub fn from_env() -> Result<Self, String> {
        Self::from_get(|key| std::env::var(key).ok())
    }

    pub fn from_get(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let required = |key: &str| {
            get(key)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("{key} is required"))
        };
        Ok(Self {
            client_id: required("ATLASSIAN_CLIENT_ID")?,
            client_secret: required("ATLASSIAN_CLIENT_SECRET")?,
            redirect_uri: optional(&get, "OAUTH_REDIRECT_URI", DEFAULT_REDIRECT_URI),
            bind_addr: bind_addr(&get),
            token_url: optional(&get, "ATLASSIAN_TOKEN_URL", DEFAULT_TOKEN_URL),
            revoke_url: optional(&get, "ATLASSIAN_REVOKE_URL", DEFAULT_REVOKE_URL),
        })
    }
}

fn optional(get: &impl Fn(&str) -> Option<String>, key: &str, default: &str) -> String {
    get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn bind_addr(get: &impl Fn(&str) -> Option<String>) -> String {
    if let Some(addr) = get("BIND_ADDR")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return addr;
    }
    if let Some(port) = get("PORT")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return format!("0.0.0.0:{port}");
    }
    DEFAULT_BIND_ADDR.to_string()
}

#[derive(Clone)]
pub struct AppState {
    http: Client,
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    token_url: String,
    revoke_url: String,
}

impl AppState {
    pub fn from_settings(settings: &Settings) -> Result<Self, String> {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|err| err.to_string())?;
        Ok(Self {
            http,
            client_id: settings.client_id.clone(),
            client_secret: settings.client_secret.clone(),
            redirect_uri: settings.redirect_uri.clone(),
            token_url: settings.token_url.clone(),
            revoke_url: settings.revoke_url.clone(),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct ServiceConfig {
    pub client_id: String,
    pub redirect_uri: String,
}

#[derive(Debug, Deserialize)]
pub struct ExchangeRequest {
    pub code: String,
    #[serde(default)]
    pub code_verifier: String,
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Deserialize)]
pub struct RevokeRequest {
    pub token: String,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/config", get(config))
        .route("/v1/oauth/exchange", post(exchange))
        .route("/v1/oauth/refresh", post(refresh))
        .route("/v1/oauth/revoke", post(revoke))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn config(State(state): State<AppState>) -> Json<ServiceConfig> {
    Json(ServiceConfig {
        client_id: state.client_id.clone(),
        redirect_uri: state.redirect_uri.clone(),
    })
}

async fn exchange(State(state): State<AppState>, Json(body): Json<ExchangeRequest>) -> Response {
    if body.code.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "code is required").into_response();
    }
    if body.code_verifier.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "code_verifier is required").into_response();
    }
    let payload = serde_json::json!({
        "grant_type": "authorization_code",
        "client_id": state.client_id,
        "client_secret": state.client_secret,
        "code": body.code,
        "redirect_uri": state.redirect_uri,
        "code_verifier": body.code_verifier,
    });
    proxy_json(&state, &state.token_url, payload).await
}

async fn refresh(State(state): State<AppState>, Json(body): Json<RefreshRequest>) -> Response {
    if body.refresh_token.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "refresh_token is required").into_response();
    }
    let payload = serde_json::json!({
        "grant_type": "refresh_token",
        "client_id": state.client_id,
        "client_secret": state.client_secret,
        "refresh_token": body.refresh_token,
    });
    proxy_json(&state, &state.token_url, payload).await
}

async fn revoke(State(state): State<AppState>, Json(body): Json<RevokeRequest>) -> Response {
    if body.token.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "token is required").into_response();
    }
    let form = serde_urlencoded::to_string([
        ("token", body.token.as_str()),
        ("client_id", state.client_id.as_str()),
        ("client_secret", state.client_secret.as_str()),
    ])
    .unwrap_or_default();
    let response = match state
        .http
        .post(&state.revoke_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, format!("revoke failed: {err}")).into_response();
        }
    };
    upstream_response(response).await
}

async fn proxy_json(state: &AppState, url: &str, payload: serde_json::Value) -> Response {
    let response = match state.http.post(url).json(&payload).send().await {
        Ok(response) => response,
        Err(err) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("token request failed: {err}"),
            )
                .into_response();
        }
    };
    upstream_response(response).await
}

async fn upstream_response(response: reqwest::Response) -> Response {
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/json")
        .to_string();
    let body = response.bytes().await.unwrap_or_default();
    (
        status,
        [(axum::http::header::CONTENT_TYPE, content_type)],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod settings_tests {
    use super::Settings;
    use std::collections::HashMap;

    #[test]
    fn requires_client_credentials() {
        let vars = HashMap::<&str, &str>::new();
        let err =
            Settings::from_get(|key| vars.get(key).map(|value| (*value).to_string())).unwrap_err();
        assert!(err.contains("ATLASSIAN_CLIENT_ID"));
    }

    #[test]
    fn fills_defaults() {
        let mut vars = HashMap::new();
        vars.insert("ATLASSIAN_CLIENT_ID", "id");
        vars.insert("ATLASSIAN_CLIENT_SECRET", "secret");
        let settings =
            Settings::from_get(|key| vars.get(key).map(|value| (*value).to_string())).unwrap();
        assert_eq!(settings.client_id, "id");
        assert_eq!(settings.redirect_uri, super::DEFAULT_REDIRECT_URI);
        assert_eq!(settings.bind_addr, super::DEFAULT_BIND_ADDR);
        assert_eq!(settings.token_url, super::DEFAULT_TOKEN_URL);
    }

    #[test]
    fn port_binds_all_interfaces() {
        let mut vars = HashMap::new();
        vars.insert("ATLASSIAN_CLIENT_ID", "id");
        vars.insert("ATLASSIAN_CLIENT_SECRET", "secret");
        vars.insert("PORT", "8080");
        let settings =
            Settings::from_get(|key| vars.get(key).map(|value| (*value).to_string())).unwrap();
        assert_eq!(settings.bind_addr, "0.0.0.0:8080");
    }

    #[test]
    fn bind_addr_wins_over_port() {
        let mut vars = HashMap::new();
        vars.insert("ATLASSIAN_CLIENT_ID", "id");
        vars.insert("ATLASSIAN_CLIENT_SECRET", "secret");
        vars.insert("PORT", "8080");
        vars.insert("BIND_ADDR", "127.0.0.1:9");
        let settings =
            Settings::from_get(|key| vars.get(key).map(|value| (*value).to_string())).unwrap();
        assert_eq!(settings.bind_addr, "127.0.0.1:9");
    }
}
