use std::time::Duration;

use oauth2::{CsrfToken, PkceCodeChallenge};
use reqwest::Client;
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::timeout;
use url::Url;

use crate::auth::store::StoredTokens;
use crate::error::{Error, Result};

pub const REDIRECT_URI: &str = "http://127.0.0.1:8787/callback";
pub const CALLBACK_ADDR: &str = "127.0.0.1:8787";
pub const DEFAULT_TOKEN_SERVICE: &str = "http://127.0.0.1:8788";

/// Classic Jira Cloud scopes only. Requesting Jira Software granular scopes
/// (`*:jira-software`) causes Atlassian to return 401 "scope does not match"
/// unless those exact scopes were also enabled on the OAuth app.
pub const SCOPES: &[&str] = &["read:jira-work", "write:jira-work", "offline_access"];

const AUTHORIZE_URL: &str = "https://auth.atlassian.com/authorize";
const RESOURCES_URL: &str = "https://api.atlassian.com/oauth/token/accessible-resources";

#[derive(Debug, Clone, Deserialize)]
pub struct AccessibleResource {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub scopes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenServiceConfig {
    pub client_id: String,
    pub redirect_uri: String,
}

#[derive(Debug, Clone)]
pub struct AuthorizeRequest {
    pub url: String,
    pub state: String,
    pub pkce_verifier: String,
}

pub fn token_service_url() -> String {
    std::env::var("JIRA_TUI_TOKEN_SERVICE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_TOKEN_SERVICE.to_string())
}

pub struct OAuthClient {
    http: Client,
    token_service: String,
}

impl OAuthClient {
    pub fn new() -> Result<Self> {
        Self::with_base(token_service_url())
    }

    pub fn with_base(token_service: impl Into<String>) -> Result<Self> {
        Ok(Self {
            http: Client::builder().timeout(Duration::from_secs(30)).build()?,
            token_service: token_service.into().trim_end_matches('/').to_string(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.token_service, path.trim_start_matches('/'))
    }

    pub async fn fetch_config(&self) -> Result<TokenServiceConfig> {
        let response = self.http.get(self.url("/v1/config")).send().await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(Error::auth(format!(
                "token service config failed ({status}): {body}"
            )));
        }
        let config: TokenServiceConfig = serde_json::from_str(&body)?;
        if config.client_id.trim().is_empty() {
            return Err(Error::auth("token service did not return a client id"));
        }
        if config.redirect_uri != REDIRECT_URI {
            return Err(Error::auth(format!(
                "token service redirect_uri {} does not match {REDIRECT_URI}",
                config.redirect_uri
            )));
        }
        Ok(config)
    }

    pub async fn exchange_code(&self, code: &str, code_verifier: &str) -> Result<StoredTokens> {
        let response = self
            .http
            .post(self.url("/v1/oauth/exchange"))
            .json(&serde_json::json!({
                "code": code,
                "code_verifier": code_verifier,
            }))
            .send()
            .await?;
        parse_token_response(response).await
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<StoredTokens> {
        let response = self
            .http
            .post(self.url("/v1/oauth/refresh"))
            .json(&serde_json::json!({
                "refresh_token": refresh_token,
            }))
            .send()
            .await?;
        parse_token_response(response).await
    }

    pub async fn revoke(&self, token: &str) -> Result<()> {
        let _ = self
            .http
            .post(self.url("/v1/oauth/revoke"))
            .json(&serde_json::json!({ "token": token }))
            .send()
            .await;
        Ok(())
    }

    pub async fn accessible_resources(
        &self,
        access_token: &str,
    ) -> Result<Vec<AccessibleResource>> {
        let response = self
            .http
            .get(RESOURCES_URL)
            .bearer_auth(access_token)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(Error::auth(format!(
                "failed to list accessible resources ({})",
                response.status()
            )));
        }
        Ok(response.json().await?)
    }
}

pub fn authorize_url(client_id: &str) -> Result<AuthorizeRequest> {
    let csrf = CsrfToken::new_random();
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let mut url = Url::parse(AUTHORIZE_URL)?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("audience", "api.atlassian.com");
        pairs.append_pair("client_id", client_id);
        pairs.append_pair("scope", &SCOPES.join(" "));
        pairs.append_pair("redirect_uri", REDIRECT_URI);
        pairs.append_pair("state", csrf.secret());
        pairs.append_pair("response_type", "code");
        pairs.append_pair("prompt", "consent");
        pairs.append_pair("code_challenge", pkce_challenge.as_str());
        pairs.append_pair("code_challenge_method", pkce_challenge.method().as_ref());
    }
    Ok(AuthorizeRequest {
        url: url.to_string(),
        state: csrf.secret().clone(),
        pkce_verifier: pkce_verifier.secret().clone(),
    })
}

pub async fn listen_for_callback(expected_state: &str) -> Result<String> {
    let listener = TcpListener::bind(CALLBACK_ADDR).await.map_err(|e| {
        Error::auth(format!(
            "cannot listen on {CALLBACK_ADDR}: {e}. Register {REDIRECT_URI} and free the port."
        ))
    })?;

    let (mut socket, _) = timeout(Duration::from_secs(180), listener.accept())
        .await
        .map_err(|_| Error::auth("timed out waiting for the OAuth callback"))??;

    let mut buf = vec![0u8; 4096];
    let n = socket.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let first_line = request.lines().next().unwrap_or_default();
    let path = first_line.split_whitespace().nth(1).unwrap_or("/");
    let parsed = Url::parse(&format!("http://127.0.0.1{path}"))?;

    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            "error_description" => {
                if error.is_some() {
                    error = Some(format!("{}: {value}", error.unwrap_or_default()));
                }
            }
            _ => {}
        }
    }

    let body = if code.is_some() && state.as_deref() == Some(expected_state) {
        "<html><body><h1>jira-tui</h1><p>Authorization complete. You can close this tab.</p></body></html>"
    } else {
        "<html><body><h1>jira-tui</h1><p>Authorization failed. Return to the terminal.</p></body></html>"
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;

    if let Some(error) = error {
        return Err(Error::auth(error));
    }
    if state.as_deref() != Some(expected_state) {
        return Err(Error::auth("OAuth state mismatch; try logging in again"));
    }
    code.ok_or_else(|| Error::auth("authorization callback did not include a code"))
}

async fn parse_token_response(response: reqwest::Response) -> Result<StoredTokens> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(Error::auth(format!(
            "token service rejected the request ({status}): {body}"
        )));
    }
    let parsed: TokenResponse = serde_json::from_str(&body)?;
    let mut tokens = StoredTokens::default();
    tokens.apply_token_response(parsed.access_token, parsed.refresh_token, parsed.expires_in);
    if tokens.refresh_token.is_empty() {
        return Err(Error::auth(
            "no refresh token returned; add the offline_access scope and try again",
        ));
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::{REDIRECT_URI, authorize_url};

    #[test]
    fn authorize_url_includes_pkce() {
        let request = authorize_url("client-id").unwrap();
        let parsed = url::Url::parse(&request.url).unwrap();
        let pairs: Vec<(String, String)> = parsed
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert!(
            pairs
                .iter()
                .any(|(k, v)| k == "redirect_uri" && v == REDIRECT_URI)
        );
        assert!(pairs.iter().any(|(k, _)| k == "code_challenge"));
        assert!(
            pairs
                .iter()
                .any(|(k, v)| k == "code_challenge_method" && v == "S256")
        );
        assert!(!request.pkce_verifier.is_empty());
        assert!(!request.state.is_empty());
    }
}
