use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client, Method, StatusCode};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::auth::oauth::OAuthClient;
use crate::auth::store::{StoredTokens, TokenStore};
use crate::error::{Error, JiraApiError, Result};
use crate::jira::adf::adf_to_text;
use crate::jira::models::{Comment, Issue, User};

const DEFAULT_HTTP_BASE: &str = "https://api.atlassian.com";

#[derive(Clone)]
pub struct JiraClient {
    http: Client,
    tokens: Arc<Mutex<StoredTokens>>,
    refresh_lock: Arc<Mutex<()>>,
    cloud_id: String,
    story_points_field: Option<String>,
    http_base: String,
}

impl JiraClient {
    pub fn new(
        cloud_id: String,
        tokens: StoredTokens,
        story_points_field: Option<String>,
    ) -> Result<Self> {
        Self::with_http_base(cloud_id, tokens, story_points_field, DEFAULT_HTTP_BASE)
    }

    pub fn with_http_base(
        cloud_id: String,
        tokens: StoredTokens,
        story_points_field: Option<String>,
        http_base: impl Into<String>,
    ) -> Result<Self> {
        Ok(Self {
            http: Client::builder().timeout(Duration::from_secs(45)).build()?,
            tokens: Arc::new(Mutex::new(tokens)),
            refresh_lock: Arc::new(Mutex::new(())),
            cloud_id,
            story_points_field,
            http_base: http_base.into().trim_end_matches('/').to_string(),
        })
    }

    #[allow(dead_code)]
    pub fn cloud_id(&self) -> &str {
        &self.cloud_id
    }

    pub fn story_points_field(&self) -> Option<&str> {
        self.story_points_field.as_deref()
    }

    pub fn set_story_points_field(&mut self, field: Option<String>) {
        self.story_points_field = field;
    }

    pub fn api_url(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        format!("{}/ex/jira/{}/rest/{path}", self.http_base, self.cloud_id)
    }

    pub async fn get_json(&self, path: &str) -> Result<Value> {
        self.request(Method::GET, path, None).await
    }

    #[allow(dead_code)]
    pub async fn get_json_query(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.request_query(Method::GET, path, query, None).await
    }

    pub async fn post_json(&self, path: &str, body: &Value) -> Result<Value> {
        self.request(Method::POST, path, Some(body)).await
    }

    pub async fn put_json(&self, path: &str, body: &Value) -> Result<Value> {
        self.request(Method::PUT, path, Some(body)).await
    }

    pub async fn delete(&self, path: &str) -> Result<Value> {
        self.request(Method::DELETE, path, None).await
    }

    async fn request(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
        self.request_query(method, path, &[], body).await
    }

    async fn request_query(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<Value> {
        let token = self.ensure_access_token().await?;
        let mut attempt = self
            .http
            .request(method.clone(), self.api_url(path))
            .bearer_auth(&token)
            .header("Accept", "application/json");
        for (k, v) in query {
            attempt = attempt.query(&[(k, v)]);
        }
        if let Some(body) = body {
            attempt = attempt.json(body);
        }
        let response = match attempt.send().await {
            Ok(response) => response,
            Err(err) => {
                tracing::warn!(%method, path, error = %err, "jira request failed");
                return Err(err.into());
            }
        };
        tracing::info!(%method, path, status = %response.status(), "jira request");
        if response.status() == StatusCode::UNAUTHORIZED {
            let token = self.refresh_tokens().await?;
            let mut retry = self
                .http
                .request(method.clone(), self.api_url(path))
                .bearer_auth(&token)
                .header("Accept", "application/json");
            for (k, v) in query {
                retry = retry.query(&[(k, v)]);
            }
            if let Some(body) = body {
                retry = retry.json(body);
            }
            let retry_response = match retry.send().await {
                Ok(response) => response,
                Err(err) => {
                    tracing::warn!(%method, path, error = %err, "jira request failed");
                    return Err(err.into());
                }
            };
            tracing::info!(%method, path, status = %retry_response.status(), "jira request");
            return read_json_or_error(retry_response).await;
        }
        read_json_or_error(response).await
    }

    async fn ensure_access_token(&self) -> Result<String> {
        {
            let tokens = self.tokens.lock().await;
            if !tokens.is_access_expired() {
                return Ok(tokens.access_token.clone());
            }
        }
        self.refresh_tokens().await
    }

    async fn refresh_tokens(&self) -> Result<String> {
        let _guard = self.refresh_lock.lock().await;
        {
            let tokens = self.tokens.lock().await;
            if !tokens.is_access_expired() {
                return Ok(tokens.access_token.clone());
            }
            if !tokens.has_refresh() {
                return Err(Error::auth("session expired; log in again"));
            }
        }

        let refresh = {
            let tokens = self.tokens.lock().await;
            tokens.refresh_token.clone()
        };
        let oauth = OAuthClient::new()?;
        let refreshed = oauth.refresh(&refresh).await?;
        {
            let mut tokens = self.tokens.lock().await;
            *tokens = refreshed.clone();
        }
        TokenStore::save(&refreshed)?;
        Ok(refreshed.access_token)
    }

    pub fn parse_issue(value: &Value, story_points_field: Option<&str>) -> Option<Issue> {
        let key = value.get("key")?.as_str()?.to_string();
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let fields = value.get("fields").cloned().unwrap_or(Value::Null);
        let summary = fields
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let issue_type = fields
            .get("issuetype")
            .and_then(|v| v.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let is_subtask = fields
            .get("issuetype")
            .and_then(|v| v.get("subtask"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let priority = fields
            .get("priority")
            .and_then(|v| v.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let status = fields
            .get("status")
            .and_then(|v| v.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let assignee = fields.get("assignee").and_then(|v| {
            if v.is_null() {
                None
            } else {
                Some(User {
                    account_id: v.get("accountId")?.as_str()?.to_string(),
                    display_name: v
                        .get("displayName")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                })
            }
        });
        let story_points = story_points_field.and_then(|field| fields.get(field).and_then(as_f64));
        let rendered = value
            .get("renderedFields")
            .and_then(|r| r.get("description"))
            .and_then(Value::as_str)
            .map(strip_html);
        let description_text = rendered.unwrap_or_else(|| {
            fields
                .get("description")
                .map(adf_to_text)
                .unwrap_or_default()
        });
        let comments = parse_comments(&fields);
        let parent = fields.get("parent").and_then(|v| {
            if v.is_null() {
                return None;
            }
            let key = v.get("key")?.as_str()?.to_string();
            let summary = v
                .get("fields")
                .and_then(|f| f.get("summary"))
                .and_then(Value::as_str)
                .or_else(|| v.get("summary").and_then(Value::as_str))
                .unwrap_or_default()
                .to_string();
            Some(crate::jira::models::ParentRef { key, summary })
        });
        Some(Issue {
            id,
            key,
            summary,
            issue_type,
            is_subtask,
            priority,
            status,
            assignee,
            story_points,
            description_text,
            comments,
            parent,
            raw: value.clone(),
        })
    }
}

async fn read_json_or_error(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if status == StatusCode::NO_CONTENT || text.trim().is_empty() {
        if status.is_success() {
            return Ok(Value::Null);
        }
        let err = JiraApiError {
            status: status.as_u16(),
            messages: vec![format!("empty error response ({status})")],
            field_errors: Vec::new(),
        };
        tracing::warn!(status = status.as_u16(), message = %err, "jira request failed");
        return Err(err.into());
    }
    let value: Value = serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text.clone()));
    if status.is_success() {
        return Ok(value);
    }
    let err = jira_error(status.as_u16(), &value, &text);
    tracing::warn!(status = status.as_u16(), message = %err, "jira request failed");
    Err(err.into())
}

fn jira_error(status: u16, value: &Value, raw: &str) -> JiraApiError {
    let mut messages = value
        .get("errorMessages")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let field_errors: Vec<(String, String)> = value
        .get("errors")
        .and_then(Value::as_object)
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|msg| (k.clone(), msg.to_string())))
                .collect()
        })
        .unwrap_or_default();
    if messages.is_empty() {
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            messages.push(message.to_string());
        }
    }
    if messages.is_empty() && field_errors.is_empty() {
        messages.push(raw.chars().take(300).collect());
    }
    if status == 401
        && messages
            .iter()
            .any(|m| m.to_ascii_lowercase().contains("scope"))
    {
        messages.push(
            "enable classic scopes read:jira-work, write:jira-work, and read:jira-user on the OAuth app, then :logout and log in again".into(),
        );
    }
    JiraApiError {
        status,
        messages,
        field_errors,
    }
}

fn parse_comments(fields: &Value) -> Vec<Comment> {
    let comments = fields
        .get("comment")
        .and_then(|c| c.get("comments"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    comments
        .iter()
        .map(|c| Comment {
            author: c
                .get("author")
                .and_then(|a| a.get("displayName"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            created: c
                .get("created")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            body: c
                .get("renderedBody")
                .and_then(Value::as_str)
                .map(strip_html)
                .unwrap_or_else(|| c.get("body").map(adf_to_text).unwrap_or_default()),
        })
        .collect()
}

fn as_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|n| n as f64))
        .or_else(|| value.as_u64().map(|n| n as f64))
        .or_else(|| value.as_str()?.parse().ok())
        .or_else(|| value.get("value").and_then(as_f64))
}

fn strip_html(input: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            '\t' if !in_tag => out.push_str("    "),
            ch if !in_tag && (ch == '\n' || !ch.is_control()) => out.push(ch),
            _ => {}
        }
    }
    html_unescape(&out)
}

fn html_unescape(input: &str) -> String {
    input
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}
