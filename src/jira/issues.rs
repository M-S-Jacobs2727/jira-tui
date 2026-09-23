use serde_json::{Value, json};

use crate::error::Result;
use crate::jira::adf::text_to_adf;
use crate::jira::client::JiraClient;
use crate::jira::models::{CreateMeta, Issue, IssueType, Priority, Transition, User};
use crate::jira::search::SearchFacade;

pub struct IssueFacade<'a> {
    client: &'a JiraClient,
}

#[derive(Debug, Clone, Default)]
pub struct IssueDraft {
    pub issue_type: String,
    pub summary: String,
    pub description: String,
    pub priority: Option<String>,
    pub assignee_account_id: Option<String>,
    pub story_points: Option<f64>,
    pub sprint_id: Option<i64>,
}

impl<'a> IssueFacade<'a> {
    pub fn new(client: &'a JiraClient) -> Self {
        Self { client }
    }

    pub async fn get(&self, issue_id_or_key: &str) -> Result<Issue> {
        let value = self
            .client
            .get_json(&format!(
                "api/3/issue/{issue_id_or_key}?expand=renderedFields,names"
            ))
            .await?;
        JiraClient::parse_issue(&value, self.client.story_points_field())
            .ok_or_else(|| crate::error::Error::message("could not parse issue response"))
    }

    pub async fn create_meta(&self, project_key: &str) -> Result<CreateMeta> {
        let types_value = self
            .client
            .get_json(&format!("api/3/issue/createmeta/{project_key}/issuetypes"))
            .await?;
        let issue_types = types_value
            .get("values")
            .or_else(|| types_value.get("issueTypes"))
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(IssueType {
                            id: v.get("id")?.as_str()?.to_string(),
                            name: v.get("name")?.as_str()?.to_string(),
                            subtask: v.get("subtask").and_then(Value::as_bool).unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let priorities = match self.client.get_json("api/3/priority").await {
            Ok(value) => value
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| {
                            Some(Priority {
                                id: v.get("id")?.as_str()?.to_string(),
                                name: v.get("name")?.as_str()?.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };

        Ok(CreateMeta {
            issue_types,
            priorities,
        })
    }

    pub async fn create(&self, project_key: &str, draft: &IssueDraft) -> Result<String> {
        let mut fields = json!({
            "project": { "key": project_key },
            "issuetype": { "name": draft.issue_type },
            "summary": draft.summary,
        });
        if !draft.description.is_empty() {
            fields["description"] = text_to_adf(&draft.description);
        }
        if let Some(priority) = &draft.priority {
            if !priority.is_empty() {
                fields["priority"] = json!({ "name": priority });
            }
        }
        if let Some(account_id) = &draft.assignee_account_id {
            if !account_id.is_empty() {
                fields["assignee"] = json!({ "accountId": account_id });
            }
        }
        if let (Some(field), Some(points)) = (self.client.story_points_field(), draft.story_points)
        {
            fields[field] = json!(points);
        }
        if let Some(sprint_id) = draft.sprint_id {
            let field = SearchFacade::new(self.client).sprint_field_id().await?;
            let Some(field) = field else {
                return Err(crate::error::Error::message(
                    "could not find the Jira Sprint field to assign the issue",
                ));
            };
            fields[field] = json!(sprint_id);
        }
        let value = self
            .client
            .post_json("api/3/issue", &json!({ "fields": fields }))
            .await?;
        Ok(value
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    pub async fn update(&self, issue_id_or_key: &str, draft: &IssueDraft) -> Result<()> {
        let mut fields = json!({
            "summary": draft.summary,
        });
        fields["description"] = text_to_adf(&draft.description);
        if let Some(priority) = &draft.priority {
            if !priority.is_empty() {
                fields["priority"] = json!({ "name": priority });
            }
        }
        if let (Some(field), Some(points)) = (self.client.story_points_field(), draft.story_points)
        {
            fields[field] = json!(points);
        }
        fields["assignee"] = match &draft.assignee_account_id {
            Some(id) if !id.is_empty() => json!({ "accountId": id }),
            _ => json!({ "accountId": Value::Null }),
        };
        if let Some(field) = SearchFacade::new(self.client).sprint_field_id().await? {
            fields[field] = match draft.sprint_id {
                Some(id) => json!(id),
                None => Value::Null,
            };
        }
        self.client
            .put_json(
                &format!("api/3/issue/{issue_id_or_key}"),
                &json!({ "fields": fields }),
            )
            .await?;
        Ok(())
    }

    pub async fn delete(&self, issue_id_or_key: &str) -> Result<()> {
        self.client
            .delete(&format!("api/3/issue/{issue_id_or_key}"))
            .await?;
        Ok(())
    }

    pub async fn assign(&self, issue_id_or_key: &str, account_id: Option<&str>) -> Result<()> {
        let body = match account_id {
            Some(id) => json!({ "accountId": id }),
            None => json!({ "accountId": Value::Null }),
        };
        self.client
            .put_json(&format!("api/3/issue/{issue_id_or_key}/assignee"), &body)
            .await?;
        Ok(())
    }

    pub async fn set_sprint(&self, issue_id_or_key: &str, sprint_id: Option<i64>) -> Result<()> {
        let Some(field) = SearchFacade::new(self.client).sprint_field_id().await? else {
            return Err(crate::error::Error::message(
                "could not find the Jira Sprint field to move the issue",
            ));
        };
        let mut fields = json!({});
        fields[field] = match sprint_id {
            Some(id) => json!(id),
            None => Value::Null,
        };
        self.client
            .put_json(
                &format!("api/3/issue/{issue_id_or_key}"),
                &json!({ "fields": fields }),
            )
            .await?;
        Ok(())
    }

    pub async fn transitions(&self, issue_id_or_key: &str) -> Result<Vec<Transition>> {
        let value = self
            .client
            .get_json(&format!("api/3/issue/{issue_id_or_key}/transitions"))
            .await?;
        Ok(value
            .get("transitions")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        let fields = v.get("fields").and_then(Value::as_object);
                        let required_fields = fields
                            .map(|obj| {
                                obj.iter()
                                    .filter(|(_, meta)| {
                                        meta.get("required")
                                            .and_then(Value::as_bool)
                                            .unwrap_or(false)
                                    })
                                    .map(|(name, _)| name.clone())
                                    .collect()
                            })
                            .unwrap_or_default();
                        Some(Transition {
                            id: v.get("id")?.as_str()?.to_string(),
                            name: v.get("name")?.as_str()?.to_string(),
                            to_status: v
                                .get("to")
                                .and_then(|to| to.get("name"))
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            required_fields,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub async fn transition(&self, issue_id_or_key: &str, transition_id: &str) -> Result<()> {
        self.client
            .post_json(
                &format!("api/3/issue/{issue_id_or_key}/transitions"),
                &json!({ "transition": { "id": transition_id } }),
            )
            .await?;
        Ok(())
    }

    pub async fn assignable_users(&self, project_key: &str, query: &str) -> Result<Vec<User>> {
        self.assignable_users_limited(project_key, query, 20).await
    }

    pub async fn assignable_users_limited(
        &self,
        project_key: &str,
        query: &str,
        max_results: u32,
    ) -> Result<Vec<User>> {
        let encoded_query = encode_component(query);
        let path = if query.is_empty() {
            format!("api/3/user/assignable/search?project={project_key}&maxResults={max_results}")
        } else {
            format!(
                "api/3/user/assignable/search?project={project_key}&query={encoded_query}&maxResults={max_results}"
            )
        };
        let value = self.client.get_json(&path).await?;
        Ok(parse_users(&value))
    }

    pub async fn myself(&self) -> Result<User> {
        let value = self.client.get_json("api/3/myself").await?;
        let account_id = value
            .get("accountId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if account_id.is_empty() {
            return Err(crate::error::Error::message(
                "could not read the current user",
            ));
        }
        Ok(User {
            account_id,
            display_name: value
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or("you")
                .to_string(),
        })
    }

    /// Returns `(statuses, issue_types)` for the project.
    pub async fn project_filter_options(
        &self,
        project_key: &str,
    ) -> Result<(Vec<String>, Vec<String>)> {
        let value = self
            .client
            .get_json(&format!("api/3/project/{project_key}/statuses"))
            .await?;
        Ok(parse_project_statuses(&value))
    }
}

fn parse_users(value: &Value) -> Vec<User> {
    let mut users = value
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    Some(User {
                        account_id: v.get("accountId")?.as_str()?.to_string(),
                        display_name: v
                            .get("displayName")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    users.sort_by(|a, b| a.account_id.cmp(&b.account_id));
    users.dedup_by_key(|user| user.account_id.clone());
    users.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
            .then_with(|| a.account_id.cmp(&b.account_id))
    });
    users
}

/// Returns `(statuses, issue_types)`.
fn parse_project_statuses(value: &Value) -> (Vec<String>, Vec<String>) {
    let mut types = Vec::new();
    let mut statuses = Vec::new();
    let Some(issue_types) = value.as_array() else {
        return (statuses, types);
    };
    for issue_type in issue_types {
        if let Some(name) = issue_type.get("name").and_then(Value::as_str) {
            push_unique(&mut types, name);
        }
        if let Some(list) = issue_type.get("statuses").and_then(Value::as_array) {
            for status in list {
                if let Some(name) = status.get("name").and_then(Value::as_str) {
                    push_unique(&mut statuses, name);
                }
            }
        }
    }
    sort_names(&mut types);
    sort_names(&mut statuses);
    (statuses, types)
}

fn push_unique(items: &mut Vec<String>, name: &str) {
    let name = name.trim();
    if !name.is_empty() && !items.iter().any(|item| item == name) {
        items.push(name.to_string());
    }
}

fn sort_names(items: &mut [String]) {
    items.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
}

fn encode_component(value: &str) -> String {
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
