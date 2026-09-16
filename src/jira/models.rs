use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct Issue {
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "displayName", default)]
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct Comment {
    pub author: String,
    pub created: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct Sprint {
    pub id: i64,
    pub name: String,
    pub state: String,
}

#[derive(Debug, Clone)]
pub struct Board {
    pub id: i64,
    pub name: String,
    pub board_type: String,
    pub project_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IssueType {
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
    #[allow(dead_code)]
    pub subtask: bool,
}

#[derive(Debug, Clone)]
pub struct Priority {
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Transition {
    pub id: String,
    pub name: String,
    pub to_status: String,
    pub required_fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SearchPage {
    pub issues: Vec<Issue>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateMeta {
    pub issue_types: Vec<IssueType>,
    pub priorities: Vec<Priority>,
}
