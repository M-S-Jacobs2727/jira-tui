use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::jira::client::JiraClient;
use crate::jira::models::{SearchPage, Sprint};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    Priority,
    Status,
    Key,
    Assignee,
    Created,
    Updated,
    Summary,
    StoryPoints,
}

impl SortField {
    pub const ALL: [SortField; 8] = [
        SortField::Priority,
        SortField::Status,
        SortField::Key,
        SortField::Assignee,
        SortField::Created,
        SortField::Updated,
        SortField::Summary,
        SortField::StoryPoints,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Priority => "Priority",
            Self::Status => "Status",
            Self::Key => "Key",
            Self::Assignee => "Assignee",
            Self::Created => "Created",
            Self::Updated => "Updated",
            Self::Summary => "Summary",
            Self::StoryPoints => "Story points",
        }
    }

    pub fn jql_name(self, story_points_field: Option<&str>) -> String {
        match self {
            Self::Priority => "priority".into(),
            Self::Status => "status".into(),
            Self::Key => "key".into(),
            Self::Assignee => "assignee".into(),
            Self::Created => "created".into(),
            Self::Updated => "updated".into(),
            Self::Summary => "summary".into(),
            Self::StoryPoints => story_points_field.unwrap_or("cf[10016]").to_string(),
        }
    }
}

impl Default for SortField {
    fn default() -> Self {
        Self::Priority
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortDir {
    #[default]
    Desc,
    Asc,
}

impl SortDir {
    pub fn as_jql(self) -> &'static str {
        match self {
            Self::Desc => "DESC",
            Self::Asc => "ASC",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Desc => "desc",
            Self::Asc => "asc",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Desc => Self::Asc,
            Self::Asc => Self::Desc,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AssigneeFilter {
    #[default]
    Any,
    Me,
    Unassigned,
    Account(String),
}

impl AssigneeFilter {
    #[allow(dead_code)]
    pub fn as_config_string(&self) -> String {
        match self {
            Self::Any => "any".into(),
            Self::Me => "me".into(),
            Self::Unassigned => "unassigned".into(),
            Self::Account(id) => format!("account:{id}"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum SprintRef {
    Id(i64),
    Backlog,
}

#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub jql: String,
    pub fields: Vec<String>,
    pub max_results: u32,
    pub next_page_token: Option<String>,
    pub fields_by_keys: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SearchBuilder {
    project: Option<String>,
    sprint: Option<SprintRef>,
    text: Option<String>,
    statuses: Vec<String>,
    assignee: AssigneeFilter,
    issue_types: Vec<String>,
    sort_field: SortField,
    sort_dir: SortDir,
    fields: Vec<String>,
    max_results: u32,
    next_page_token: Option<String>,
    story_points_field: Option<String>,
    extra_clauses: Vec<String>,
    fields_by_keys: bool,
}

impl SearchBuilder {
    pub fn new() -> Self {
        Self {
            max_results: 50,
            sort_field: SortField::Priority,
            sort_dir: SortDir::Desc,
            fields: default_fields(None),
            fields_by_keys: true,
            ..Self::default()
        }
    }

    pub fn project(mut self, key: impl Into<String>) -> Self {
        self.project = Some(key.into());
        self
    }

    pub fn sprint(mut self, sprint: SprintRef) -> Self {
        self.sprint = Some(sprint);
        self
    }

    pub fn clause(mut self, clause: impl Into<String>) -> Self {
        let clause = clause.into();
        if !clause.is_empty() {
            self.extra_clauses.push(clause);
        }
        self
    }

    pub fn text(mut self, text: impl Into<String>) -> Self {
        let text = text.into();
        if !text.is_empty() {
            self.text = Some(text);
        }
        self
    }

    pub fn status<I, S>(mut self, statuses: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.statuses = statuses.into_iter().map(Into::into).collect();
        self
    }

    pub fn assignee(mut self, assignee: AssigneeFilter) -> Self {
        self.assignee = assignee;
        self
    }

    pub fn issue_type<I, S>(mut self, types: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.issue_types = types.into_iter().map(Into::into).collect();
        self
    }

    pub fn order_by(mut self, field: SortField, dir: SortDir) -> Self {
        self.sort_field = field;
        self.sort_dir = dir;
        self
    }

    #[allow(dead_code)]
    pub fn fields<I, S>(mut self, fields: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.fields = fields.into_iter().map(Into::into).collect();
        self
    }

    pub fn fields_by_keys(mut self, yes: bool) -> Self {
        self.fields_by_keys = yes;
        self
    }

    pub fn story_points_field(mut self, field: Option<String>) -> Self {
        self.story_points_field = field;
        if self.fields.is_empty() {
            self.fields = default_fields(self.story_points_field.as_deref());
        }
        self
    }

    #[allow(dead_code)]
    pub fn max_results(mut self, max: u32) -> Self {
        self.max_results = max;
        self
    }

    pub fn next_page(mut self, token: impl Into<String>) -> Self {
        let token = token.into();
        if !token.is_empty() {
            self.next_page_token = Some(token);
        }
        self
    }

    pub fn build(self) -> SearchRequest {
        let mut clauses = Vec::new();
        if let Some(project) = &self.project {
            clauses.push(format!("project = {}", quote(project)));
        }
        match &self.sprint {
            Some(SprintRef::Id(id)) => clauses.push(format!("sprint = {id}")),
            Some(SprintRef::Backlog) => clauses.push("sprint is EMPTY".into()),
            None => {}
        }
        if let Some(text) = &self.text {
            if looks_like_issue_key(text) {
                clauses.push(format!("key = {}", quote(&text.to_uppercase())));
            } else {
                clauses.push(format!("summary ~ {}", quote(text)));
            }
        }
        if !self.statuses.is_empty() {
            let list = self
                .statuses
                .iter()
                .map(|s| quote(s))
                .collect::<Vec<_>>()
                .join(", ");
            clauses.push(format!("status in ({list})"));
        }
        match &self.assignee {
            AssigneeFilter::Any => {}
            AssigneeFilter::Me => clauses.push("assignee = currentUser()".into()),
            AssigneeFilter::Unassigned => clauses.push("assignee is EMPTY".into()),
            AssigneeFilter::Account(id) => clauses.push(format!("assignee = {}", quote(id))),
        }
        if !self.issue_types.is_empty() {
            let list = self
                .issue_types
                .iter()
                .map(|s| quote(s))
                .collect::<Vec<_>>()
                .join(", ");
            clauses.push(format!("issuetype in ({list})"));
        }
        clauses.extend(self.extra_clauses);

        let mut jql = if clauses.is_empty() {
            "order by created DESC".to_string()
        } else {
            clauses.join(" AND ")
        };
        if !jql.to_ascii_lowercase().contains("order by") {
            let field = self.sort_field.jql_name(self.story_points_field.as_deref());
            jql.push_str(&format!(" ORDER BY {field} {}", self.sort_dir.as_jql()));
        }

        let fields = if self.fields.is_empty() {
            default_fields(self.story_points_field.as_deref())
        } else {
            let mut fields = self.fields;
            if let Some(sp) = &self.story_points_field {
                if !fields.iter().any(|f| f == sp || f == "story_points") {
                    fields.push(sp.clone());
                }
            }
            fields
        };

        SearchRequest {
            jql,
            fields,
            max_results: self.max_results,
            next_page_token: self.next_page_token,
            fields_by_keys: self.fields_by_keys,
        }
    }
}

pub struct SearchFacade<'a> {
    client: &'a JiraClient,
}

impl<'a> SearchFacade<'a> {
    pub fn new(client: &'a JiraClient) -> Self {
        Self { client }
    }

    pub async fn search(&self, request: SearchRequest) -> Result<SearchPage> {
        let mut body = serde_json::json!({
            "jql": request.jql,
            "maxResults": request.max_results,
            "fields": request.fields,
        });
        if let Some(token) = request.next_page_token {
            body["nextPageToken"] = serde_json::Value::String(token);
        }
        if request.fields_by_keys {
            body["fieldsByKeys"] = serde_json::Value::Bool(true);
        }
        let value = self.client.post_json("api/3/search/jql", &body).await?;
        let issues = value
            .get("issues")
            .and_then(serde_json::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|issue| {
                        JiraClient::parse_issue(issue, self.client.story_points_field())
                    })
                    .collect()
            })
            .unwrap_or_default();
        let next_page_token = value
            .get("nextPageToken")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        Ok(SearchPage {
            issues,
            next_page_token,
        })
    }

    pub async fn discover_sprints(&self, project_key: Option<&str>) -> Result<Vec<Sprint>> {
        let sprint_field = self.sprint_field_id().await.ok().flatten();
        let mut fields = vec!["summary".into(), "sprint".into()];
        if let Some(id) = &sprint_field {
            if !fields.iter().any(|f| f == id) {
                fields.push(id.clone());
            }
        }
        let mut builder = SearchBuilder::new()
            .clause("(sprint in openSprints() OR sprint in futureSprints())")
            .fields(fields)
            .fields_by_keys(true)
            .max_results(100)
            .order_by(SortField::Created, SortDir::Desc);
        if let Some(project) = project_key {
            builder = builder.project(project);
        }
        let page = self.search(builder.build()).await?;
        let mut sprints = Vec::new();
        for issue in &page.issues {
            collect_sprints(&issue.raw, &mut sprints);
        }
        sprints.sort_by(|a, b| {
            sprint_rank(&a.state)
                .cmp(&sprint_rank(&b.state))
                .then_with(|| a.id.cmp(&b.id))
        });
        sprints.dedup_by_key(|s| s.id);
        tracing::info!(
            issues = page.issues.len(),
            sprints = sprints.len(),
            field = sprint_field.as_deref().unwrap_or("sprint"),
            "discovered sprints via JQL"
        );
        Ok(sprints)
    }

    pub async fn sprint_field_id(&self) -> Result<Option<String>> {
        let value = self.client.get_json("api/3/field").await?;
        Ok(parse_sprint_field_id(&value))
    }

    pub async fn story_points_field_id(&self) -> Result<Option<String>> {
        let value = self.client.get_json("api/3/field").await?;
        Ok(parse_story_points_field_id(&value))
    }
}

pub fn sprints_from_issue(raw: &serde_json::Value) -> Vec<Sprint> {
    let mut sprints = Vec::new();
    collect_sprints(raw, &mut sprints);
    sprints
}

fn collect_sprints(issue: &serde_json::Value, out: &mut Vec<Sprint>) {
    let Some(fields) = issue.get("fields") else {
        return;
    };
    if let Some(value) = fields.get("sprint") {
        push_sprint_value(value, out);
    }
    if let Some(obj) = fields.as_object() {
        for (key, value) in obj {
            if key.starts_with("customfield_") {
                push_sprint_value(value, out);
            }
        }
    }
}

fn push_sprint_value(value: &serde_json::Value, out: &mut Vec<Sprint>) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                push_sprint_value(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            let Some(id) = map.get("id").and_then(as_i64) else {
                return;
            };
            let name = map
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Sprint")
                .to_string();
            let state = map
                .get("state")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            push_sprint(out, Sprint { id, name, state });
        }
        serde_json::Value::String(s) => {
            if let Some(sprint) = parse_sprint_string(s) {
                push_sprint(out, sprint);
            }
        }
        serde_json::Value::Number(n) => {
            if let Some(id) = n.as_i64().or_else(|| n.as_u64().map(|n| n as i64)) {
                push_sprint(
                    out,
                    Sprint {
                        id,
                        name: format!("Sprint {id}"),
                        state: String::new(),
                    },
                );
            }
        }
        _ => {}
    }
}

fn push_sprint(out: &mut Vec<Sprint>, sprint: Sprint) {
    if sprint.state == "closed" || sprint.state == "complete" {
        return;
    }
    if !out.iter().any(|s| s.id == sprint.id) {
        out.push(sprint);
    }
}

fn as_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().map(|n| n as i64))
        .or_else(|| value.as_str()?.parse().ok())
}

fn parse_sprint_string(s: &str) -> Option<Sprint> {
    let id = s
        .split("id=")
        .nth(1)?
        .split([',', ']'])
        .next()?
        .parse()
        .ok()?;
    let name = s
        .split("name=")
        .nth(1)
        .and_then(|rest| rest.split([',', ']']).next())
        .filter(|name| !name.is_empty())
        .unwrap_or("Sprint")
        .to_string();
    let state = s
        .split("state=")
        .nth(1)
        .and_then(|rest| rest.split([',', ']']).next())
        .unwrap_or("")
        .to_ascii_lowercase();
    Some(Sprint { id, name, state })
}

fn parse_sprint_field_id(value: &serde_json::Value) -> Option<String> {
    value.as_array()?.iter().find_map(|field| {
        let id = field.get("id")?.as_str()?;
        let name = field
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let schema = field
            .pointer("/schema/custom")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if schema.contains("gh-sprint") || name.eq_ignore_ascii_case("Sprint") {
            Some(id.to_string())
        } else {
            None
        }
    })
}

fn parse_story_points_field_id(value: &serde_json::Value) -> Option<String> {
    let mut classic = None;
    let mut next_gen = None;
    for field in value.as_array()? {
        let Some(id) = field.get("id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let name = field
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let schema = field
            .pointer("/schema/custom")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if schema.contains("jsw-story-points") || schema.contains("story-points") {
            return Some(id.to_string());
        }
        if name.eq_ignore_ascii_case("Story Points") {
            classic = Some(id.to_string());
        } else if name.eq_ignore_ascii_case("Story point estimate") {
            next_gen = Some(id.to_string());
        }
    }
    classic.or(next_gen)
}

fn sprint_rank(state: &str) -> u8 {
    match state {
        "active" => 0,
        "future" => 1,
        _ => 2,
    }
}

pub fn default_fields(story_points_field: Option<&str>) -> Vec<String> {
    let mut fields = vec![
        "summary".into(),
        "issuetype".into(),
        "priority".into(),
        "status".into(),
        "assignee".into(),
        "comment".into(),
        "description".into(),
        "sprint".into(),
    ];
    if let Some(field) = story_points_field {
        fields.push(field.to_string());
    }
    fields
}

pub fn quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

pub fn looks_like_issue_key(value: &str) -> bool {
    let value = value.trim();
    let mut parts = value.split('-');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(prefix), Some(num), None) => {
            !prefix.is_empty()
                && prefix.chars().all(|c| c.is_ascii_alphabetic())
                && !num.is_empty()
                && num.chars().all(|c| c.is_ascii_digit())
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_sprint_and_filters() {
        let req = SearchBuilder::new()
            .project("ABC")
            .sprint(SprintRef::Id(42))
            .status(["In Progress"])
            .assignee(AssigneeFilter::Me)
            .issue_type(["Bug"])
            .text("checkout")
            .order_by(SortField::Priority, SortDir::Desc)
            .build();
        assert!(req.jql.contains("project = \"ABC\""));
        assert!(req.jql.contains("sprint = 42"));
        assert!(req.jql.contains("status in (\"In Progress\")"));
        assert!(req.jql.contains("assignee = currentUser()"));
        assert!(req.jql.contains("issuetype in (\"Bug\")"));
        assert!(req.jql.contains("summary ~ \"checkout\""));
        assert!(req.jql.contains("ORDER BY priority DESC"));
    }

    #[test]
    fn backlog_and_issue_key_search() {
        let req = SearchBuilder::new()
            .project("ABC")
            .sprint(SprintRef::Backlog)
            .text("abc-12")
            .order_by(SortField::Key, SortDir::Asc)
            .build();
        assert!(req.jql.contains("sprint is EMPTY"));
        assert!(req.jql.contains("key = \"ABC-12\""));
        assert!(req.jql.contains("ORDER BY key ASC"));
    }

    #[test]
    fn quotes_special_characters() {
        assert_eq!(quote(r#"a"b\c"#), r#""a\"b\\c""#);
    }

    #[test]
    fn detects_issue_keys() {
        assert!(looks_like_issue_key("ABC-123"));
        assert!(looks_like_issue_key("abc-1"));
        assert!(!looks_like_issue_key("not a key"));
        assert!(!looks_like_issue_key("ABC-"));
    }

    #[test]
    fn finds_sprint_custom_field() {
        let fields = serde_json::json!([
            { "id": "summary", "name": "Summary" },
            {
                "id": "customfield_10020",
                "name": "Sprint",
                "schema": { "custom": "com.pyxis.greenhopper.jira:gh-sprint" }
            }
        ]);
        assert_eq!(
            parse_sprint_field_id(&fields).as_deref(),
            Some("customfield_10020")
        );
    }

    #[test]
    fn parses_greenhopper_sprint_string() {
        let raw = "com.atlassian.greenhopper.service.sprint.Sprint@abc[id=55,rapidViewId=3,state=ACTIVE,name=Sprint 12,startDate=2026-01-01]";
        let sprint = parse_sprint_string(raw).expect("sprint");
        assert_eq!(sprint.id, 55);
        assert_eq!(sprint.name, "Sprint 12");
        assert_eq!(sprint.state, "active");
    }

    #[test]
    fn collects_sprint_from_custom_field() {
        let issue = serde_json::json!({
            "fields": {
                "customfield_10020": [{
                    "id": "88",
                    "name": "Active Sprint",
                    "state": "active"
                }]
            }
        });
        let mut sprints = Vec::new();
        collect_sprints(&issue, &mut sprints);
        assert_eq!(sprints.len(), 1);
        assert_eq!(sprints[0].id, 88);
        assert_eq!(sprints[0].name, "Active Sprint");
    }

    #[test]
    fn finds_story_point_estimate_field() {
        let fields = serde_json::json!([
            { "id": "customfield_10016", "name": "Story Points" },
            {
                "id": "customfield_10031",
                "name": "Story point estimate",
                "schema": { "custom": "com.atlassian.jira.plugin.system.customfieldtypes:float" }
            }
        ]);
        assert_eq!(
            parse_story_points_field_id(&fields).as_deref(),
            Some("customfield_10016")
        );
    }

    #[test]
    fn prefers_greenhopper_story_points_schema() {
        let fields = serde_json::json!([
            { "id": "customfield_10016", "name": "Business value" },
            {
                "id": "customfield_10028",
                "name": "Story Points",
                "schema": { "custom": "com.pyxis.greenhopper.jira:jsw-story-points" }
            }
        ]);
        assert_eq!(
            parse_story_points_field_id(&fields).as_deref(),
            Some("customfield_10028")
        );
    }
}
