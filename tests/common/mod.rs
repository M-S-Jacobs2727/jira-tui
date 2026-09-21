#![allow(dead_code)]

use std::fs;
use std::path::Path;

use jira_tui::auth::store::StoredTokens;
use jira_tui::jira::client::JiraClient;
use jira_tui::jira::models::{Comment, Issue, SearchPage, User};
use serde_json::Value;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

pub const CLOUD_ID: &str = "test-cloud";
pub const STORY_POINTS_FIELD: &str = "customfield_10016";

#[derive(Debug)]
pub struct IssueSnapshot {
    pub id: String,
    pub key: String,
    pub summary: String,
    pub issue_type: String,
    pub priority: Option<String>,
    pub status: String,
    pub assignee: Option<User>,
    pub story_points: Option<f64>,
    pub description_text: String,
    pub comments: Vec<Comment>,
}

#[derive(Debug)]
pub struct SearchPageSnapshot {
    pub issues: Vec<IssueSnapshot>,
    pub next_page_token: Option<String>,
}

pub fn issue_snapshot(issue: &Issue) -> IssueSnapshot {
    IssueSnapshot {
        id: issue.id.clone(),
        key: issue.key.clone(),
        summary: issue.summary.clone(),
        issue_type: issue.issue_type.clone(),
        priority: issue.priority.clone(),
        status: issue.status.clone(),
        assignee: issue.assignee.clone(),
        story_points: issue.story_points,
        description_text: issue.description_text.clone(),
        comments: issue.comments.clone(),
    }
}

pub fn search_page_snapshot(page: &SearchPage) -> SearchPageSnapshot {
    SearchPageSnapshot {
        issues: page.issues.iter().map(issue_snapshot).collect(),
        next_page_token: page.next_page_token.clone(),
    }
}

pub fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("read fixture {}: {err}", path.display()))
}

pub struct JiraMock {
    pub server: MockServer,
    pub client: JiraClient,
}

impl JiraMock {
    pub async fn start() -> Self {
        let server = MockServer::start().await;
        let tokens = StoredTokens {
            access_token: "test-access-token".into(),
            expires_at: u64::MAX,
            ..StoredTokens::default()
        };
        let client = JiraClient::with_http_base(
            CLOUD_ID.into(),
            tokens,
            Some(STORY_POINTS_FIELD.into()),
            server.uri(),
        )
        .expect("jira client");
        Self { server, client }
    }

    pub fn rest_path(&self, rest_path: &str) -> String {
        format!(
            "/ex/jira/{CLOUD_ID}/rest/{}",
            rest_path.trim_start_matches('/')
        )
    }

    pub async fn mock_json(&self, http_method: &str, rest_path_and_query: &str, body: &str) {
        self.mock_response(http_method, rest_path_and_query, 200, Some(body))
            .await;
    }

    pub async fn mock_status(&self, http_method: &str, rest_path_and_query: &str, status: u16) {
        self.mock_response(http_method, rest_path_and_query, status, None)
            .await;
    }

    pub async fn mock_error(
        &self,
        http_method: &str,
        rest_path_and_query: &str,
        status: u16,
        body: &str,
    ) {
        self.mock_response(http_method, rest_path_and_query, status, Some(body))
            .await;
    }

    async fn mock_response(
        &self,
        http_method: &str,
        rest_path_and_query: &str,
        status: u16,
        body: Option<&str>,
    ) {
        let (path_part, query) = split_path_query(rest_path_and_query);
        let full_path = self.rest_path(path_part);
        let mut builder = Mock::given(method(http_method)).and(path(full_path));
        for (key, value) in query {
            builder = builder.and(query_param(key, value));
        }
        let mut template = ResponseTemplate::new(status);
        if let Some(body) = body {
            template = template.set_body_raw(body.to_string(), "application/json");
        }
        builder.respond_with(template).mount(&self.server).await;
    }

    pub async fn received(&self) -> Vec<Request> {
        self.server.received_requests().await.unwrap_or_default()
    }

    pub async fn request_snapshots(&self) -> Vec<Value> {
        self.received().await.iter().map(request_snapshot).collect()
    }

    pub async fn json_bodies_for(&self, http_method: &str, rest_path: &str) -> Vec<Value> {
        let expected = self.rest_path(rest_path);
        self.received()
            .await
            .into_iter()
            .filter(|req| {
                req.method.as_str().eq_ignore_ascii_case(http_method) && req.url.path() == expected
            })
            .map(|req| serde_json::from_slice(&req.body).unwrap_or(Value::Null))
            .collect()
    }
}

pub fn request_snapshot(req: &Request) -> Value {
    let body = if req.body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&req.body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&req.body).into_owned()))
    };
    serde_json::json!({
        "method": req.method.to_string(),
        "path": req.url.path(),
        "query": query_pairs(req),
        "body": body,
    })
}

fn query_pairs(req: &Request) -> Value {
    let mut pairs: Vec<(String, String)> = req
        .url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    Value::Array(
        pairs
            .into_iter()
            .map(|(k, v)| serde_json::json!({ "k": k, "v": v }))
            .collect(),
    )
}

fn split_path_query(rest_path_and_query: &str) -> (&str, Vec<(&str, &str)>) {
    match rest_path_and_query.split_once('?') {
        Some((path_part, query)) => {
            let params = query
                .split('&')
                .filter(|part| !part.is_empty())
                .filter_map(|part| part.split_once('='))
                .collect();
            (path_part, params)
        }
        None => (rest_path_and_query, Vec::new()),
    }
}
