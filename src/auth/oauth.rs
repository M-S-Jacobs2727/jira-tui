use std::time::Duration;

use oauth2::{AuthUrl, ClientId, CsrfToken, RedirectUrl, Scope};
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

/// Classic Jira Cloud scopes only. Requesting Jira Software granular scopes
/// (`*:jira-software`) causes Atlassian to return 401 "scope does not match"
/// unless those exact scopes were also enabled on the OAuth app.
pub const SCOPES: &[&str] = &["read:jira-work", "write:jira-work", "offline_access"];

const AUTHORIZE_URL: &str = "https://auth.atlassian.com/authorize";
const TOKEN_URL: &str = "https://auth.atlassian.com/oauth/token";
const REVOKE_URL: &str = "https://auth.atlassian.com/oauth/revoke";
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

pub struct OAuthClient {
    http: Client,
    client_id: String,
    client_secret: String,
}

impl OAuthClient {
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Result<Self> {
        Ok(Self {
            http: Client::builder().timeout(Duration::from_secs(30)).build()?,
            client_id: client_id.into(),
            client_secret: client_secret.into(),
        })
    }

    #[allow(dead_code)]
    pub fn authorize_url_and_state(&self) -> Result<(String, String)> {
        authorize_url(&self.client_id)
    }

    pub async fn exchange_code(&self, code: &str) -> Result<StoredTokens> {
        let response = self
            .http
            .post(TOKEN_URL)
            .json(&serde_json::json!({
                "grant_type": "authorization_code",
                "client_id": self.client_id,
                "client_secret": self.client_secret,
                "code": code,
                "redirect_uri": REDIRECT_URI,
            }))
            .send()
            .await?;
        parse_token_response(response, &self.client_secret).await
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<StoredTokens> {
        let response = self
            .http
            .post(TOKEN_URL)
            .json(&serde_json::json!({
                "grant_type": "refresh_token",
                "client_id": self.client_id,
                "client_secret": self.client_secret,
                "refresh_token": refresh_token,
            }))
            .send()
            .await?;
        parse_token_response(response, &self.client_secret).await
    }

    pub async fn revoke(&self, token: &str) -> Result<()> {
        let _ = self
            .http
            .post(REVOKE_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!(
                "token={}&client_id={}&client_secret={}",
                urlencoding_token(token),
                urlencoding_token(&self.client_id),
                urlencoding_token(&self.client_secret),
            ))
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

pub fn authorize_url(client_id: &str) -> Result<(String, String)> {
    let auth_url =
        AuthUrl::new(AUTHORIZE_URL.to_string()).map_err(|e| Error::auth(e.to_string()))?;
    let redirect =
        RedirectUrl::new(REDIRECT_URI.to_string()).map_err(|e| Error::auth(e.to_string()))?;
    let _client_id = ClientId::new(client_id.to_string());
    let csrf = CsrfToken::new_random();
    let mut url = Url::parse(auth_url.as_str())?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("audience", "api.atlassian.com");
        pairs.append_pair("client_id", client_id);
        pairs.append_pair("scope", &SCOPES.join(" "));
        pairs.append_pair("redirect_uri", redirect.as_str());
        pairs.append_pair("state", csrf.secret());
        pairs.append_pair("response_type", "code");
        pairs.append_pair("prompt", "consent");
    }
    let _ = Scope::new(SCOPES.join(" "));
    Ok((url.to_string(), csrf.secret().clone()))
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

async fn parse_token_response(
    response: reqwest::Response,
    client_secret: &str,
) -> Result<StoredTokens> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(Error::auth(format!(
            "token endpoint rejected the request ({status}): {body}"
        )));
    }
    let parsed: TokenResponse = serde_json::from_str(&body)?;
    let mut tokens = StoredTokens {
        client_secret: client_secret.to_string(),
        ..StoredTokens::default()
    };
    tokens.apply_token_response(parsed.access_token, parsed.refresh_token, parsed.expires_in);
    if tokens.refresh_token.is_empty() {
        return Err(Error::auth(
            "no refresh token returned; add the offline_access scope and try again",
        ));
    }
    Ok(tokens)
}

fn urlencoding_token(value: &str) -> String {
    let mut encoded = String::new();
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            _ => encoded.push_str(&format!("%{b:02X}")),
        }
    }
    encoded
}
