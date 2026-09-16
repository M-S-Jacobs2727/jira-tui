use serde_json::Value;

use crate::error::Result;
use crate::jira::client::JiraClient;
use crate::jira::models::{Board, Sprint};

pub struct AgileFacade<'a> {
    client: &'a JiraClient,
}

impl<'a> AgileFacade<'a> {
    pub fn new(client: &'a JiraClient) -> Self {
        Self { client }
    }

    pub async fn boards(&self, project_key: Option<&str>) -> Result<Vec<Board>> {
        let path = match project_key {
            Some(key) if !key.is_empty() => {
                format!("agile/1.0/board?projectKeyOrId={key}&maxResults=50")
            }
            _ => "agile/1.0/board?maxResults=50".to_string(),
        };
        let value = self.client.get_json(&path).await?;
        Ok(parse_boards(&value))
    }

    pub async fn open_sprints(&self, board_id: i64) -> Result<Vec<Sprint>> {
        let value = self
            .client
            .get_json(&format!(
                "agile/1.0/board/{board_id}/sprint?state=active,future&maxResults=50"
            ))
            .await?;
        let mut sprints = parse_sprints(&value);
        sprints.sort_by(|a, b| {
            sprint_rank(&a.state)
                .cmp(&sprint_rank(&b.state))
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(sprints)
    }

    pub async fn projects(&self) -> Result<Vec<Board>> {
        let value = self
            .client
            .get_json("api/3/project/search?maxResults=50")
            .await?;
        Ok(value
            .get("values")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(Board {
                            id: as_i64(v.get("id")?)?,
                            name: v.get("name")?.as_str()?.to_string(),
                            board_type: "project".into(),
                            project_key: v.get("key").and_then(Value::as_str).map(str::to_string),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub async fn story_points_field(&self, board_id: i64) -> Result<Option<String>> {
        let value = self
            .client
            .get_json(&format!("agile/1.0/board/{board_id}/configuration"))
            .await?;
        Ok(value
            .pointer("/estimation/field/fieldId")
            .and_then(Value::as_str)
            .map(str::to_string))
    }
}

fn parse_boards(value: &Value) -> Vec<Board> {
    value
        .get("values")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    Some(Board {
                        id: as_i64(v.get("id")?)?,
                        name: v.get("name")?.as_str()?.to_string(),
                        board_type: v
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        project_key: v.get("location").and_then(location_project_key),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_sprints(value: &Value) -> Vec<Sprint> {
    value
        .get("values")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    Some(Sprint {
                        id: as_i64(v.get("id")?)?,
                        name: v.get("name")?.as_str()?.to_string(),
                        state: v
                            .get("state")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn location_project_key(location: &Value) -> Option<String> {
    location
        .get("projectKey")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            location.get("projectId").and_then(|id| {
                id.as_str()
                    .map(str::to_string)
                    .or_else(|| id.as_i64().map(|n| n.to_string()))
                    .or_else(|| id.as_u64().map(|n| n.to_string()))
            })
        })
}

fn as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().map(|n| n as i64))
        .or_else(|| value.as_str()?.parse().ok())
}

fn sprint_rank(state: &str) -> u8 {
    match state {
        "active" => 0,
        "future" => 1,
        _ => 2,
    }
}
