use crate::error::Result;
use crate::jira::client::JiraClient;
use crate::jira::models::{SearchPage, Sprint};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    /// Board backlog order (`ORDER BY Rank ASC`).
    Default,
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
    pub const ALL: [SortField; 9] = [
        SortField::Default,
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
            Self::Default => "Default",
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

    /// Whether direction can be toggled for this field.
    pub fn has_direction(self) -> bool {
        !matches!(self, Self::Default)
    }

    pub fn jql_name(self, story_points_field: Option<&str>) -> Option<String> {
        match self {
            Self::Default => Some("Rank".into()),
            Self::Priority => Some("priority".into()),
            Self::Status => Some("status".into()),
            Self::Key => Some("key".into()),
            Self::Assignee => Some("assignee".into()),
            Self::Created => Some("created".into()),
            Self::Updated => Some("updated".into()),
            Self::Summary => Some("summary".into()),
            Self::StoryPoints => Some(story_points_field.unwrap_or("cf[10016]").to_string()),
        }
    }
}

impl Default for SortField {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

/// Selected assignees. An empty filter matches everyone.
///
/// `me` is a temporary flag until the current account id is known;
/// [`AssigneeFilter::resolve_me`] folds it into `accounts` so the same person
/// is not stored twice.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AssigneeFilter {
    pub unassigned: bool,
    pub accounts: Vec<String>,
    pub me: bool,
}

impl AssigneeFilter {
    pub fn is_empty(&self) -> bool {
        !self.unassigned && !self.me && self.accounts.iter().all(|id| id.trim().is_empty())
    }

    pub fn dedup_accounts(&mut self) {
        let mut seen = Vec::new();
        for id in self.accounts.drain(..) {
            let id = id.trim().to_string();
            if !id.is_empty() && !seen.iter().any(|existing: &String| existing == &id) {
                seen.push(id);
            }
        }
        self.accounts = seen;
    }

    /// Replace a `me` flag with the current account id, dropping a duplicate.
    pub fn resolve_me(&mut self, self_id: &str) {
        let self_id = self_id.trim();
        if self_id.is_empty() {
            self.dedup_accounts();
            return;
        }
        if self.me && !self.accounts.iter().any(|id| id == self_id) {
            self.accounts.push(self_id.to_string());
        }
        if self.accounts.iter().any(|id| id == self_id) {
            self.me = false;
        }
        self.dedup_accounts();
    }

    pub fn toggle_account(&mut self, account_id: &str) {
        let account_id = account_id.trim();
        if account_id.is_empty() {
            return;
        }
        if let Some(idx) = self.accounts.iter().position(|id| id == account_id) {
            self.accounts.remove(idx);
        } else {
            self.accounts.push(account_id.to_string());
        }
        self.me = false;
        self.dedup_accounts();
    }

    /// JQL fragment, or `None` when the filter should not constrain assignee.
    /// A resolved current user is an account id, never `currentUser()` as well.
    pub fn jql_clause(&self, self_id: Option<&str>) -> Option<String> {
        let mut filter = self.clone();
        if let Some(id) = self_id {
            filter.resolve_me(id);
        } else {
            filter.dedup_accounts();
        }
        if filter.is_empty() {
            return None;
        }

        let mut people = Vec::new();
        if filter.me {
            people.push("currentUser()".to_string());
        }
        for id in &filter.accounts {
            let quoted = quote(id);
            if !people.iter().any(|existing| existing == &quoted) {
                people.push(quoted);
            }
        }

        let mut parts = Vec::new();
        match people.len() {
            0 => {}
            1 => parts.push(format!("assignee = {}", people[0])),
            _ => parts.push(format!("assignee in ({})", people.join(", "))),
        }
        if filter.unassigned {
            parts.push("assignee is EMPTY".into());
        }
        match parts.len() {
            0 => None,
            1 => Some(parts.remove(0)),
            _ => Some(format!("({})", parts.join(" OR "))),
        }
    }

    pub fn footer_label<'a, F>(&self, self_id: Option<&str>, mut display_name: F) -> String
    where
        F: FnMut(&str) -> Option<&'a str>,
    {
        let mut filter = self.clone();
        if let Some(id) = self_id {
            filter.resolve_me(id);
        }
        if filter.is_empty() {
            return String::new();
        }
        let mut parts = Vec::new();
        if filter.unassigned {
            parts.push("unassigned".to_string());
        }
        if filter.me {
            parts.push("you".to_string());
        }
        let self_id = self_id.unwrap_or("");
        for id in &filter.accounts {
            if id == self_id && !self_id.is_empty() {
                parts.push("you".to_string());
            } else if let Some(name) = display_name(id) {
                parts.push(name.to_string());
            } else {
                parts.push(id.clone());
            }
        }
        parts.join(", ")
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
            sort_field: SortField::Default,
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
        if let Some(clause) = self.assignee.jql_clause(None) {
            clauses.push(clause);
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

        let sort_dir = if self.sort_field.has_direction() {
            self.sort_dir
        } else {
            SortDir::Asc
        };
        let mut jql = if clauses.is_empty() {
            match self.sort_field.jql_name(self.story_points_field.as_deref()) {
                Some(field) => format!("order by {field} {}", sort_dir.as_jql()),
                None => String::new(),
            }
        } else {
            clauses.join(" AND ")
        };
        if let Some(field) = self
            .sort_field
            .jql_name(self.story_points_field.as_deref())
        {
            if !jql.is_empty() && !jql.to_ascii_lowercase().contains("order by") {
                jql.push_str(&format!(" ORDER BY {field} {}", sort_dir.as_jql()));
            }
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
        tracing::info!(jql = %request.jql, "jira search");
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
        "parent".into(),
    ];
    if let Some(field) = story_points_field {
        fields.push(field.to_string());
    }
    fields
}

/// What kinds of issues may be selected as a parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentSearchKind {
    /// Stories / tasks / bugs sit under epics.
    Epic,
    /// Sub-tasks sit under non-epic, non-subtask issues.
    NonEpicNonSubtask,
}

impl ParentSearchKind {
    pub fn jql_clause(self) -> &'static str {
        match self {
            Self::Epic => "issuetype = Epic",
            Self::NonEpicNonSubtask => {
                "issuetype != Epic AND issuetype not in subTaskIssueTypes()"
            }
        }
    }
}

/// JQL clause matching children of `parent_key`.
pub fn children_clause(parent_key: &str) -> String {
    format!("parent = {}", quote(parent_key))
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
            .assignee(AssigneeFilter {
                me: true,
                ..AssigneeFilter::default()
            })
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
    fn default_sort_orders_by_rank_asc() {
        let req = SearchBuilder::new()
            .project("ABC")
            .sprint(SprintRef::Backlog)
            .order_by(SortField::Default, SortDir::Desc)
            .build();
        assert!(req.jql.contains("project = \"ABC\""));
        assert!(req.jql.contains("sprint is EMPTY"));
        assert!(req.jql.contains("ORDER BY Rank ASC"));
    }

    #[test]
    fn backlog_hides_epics_and_subtasks_via_clause() {
        let req = SearchBuilder::new()
            .project("ABC")
            .sprint(SprintRef::Backlog)
            .clause("issuetype != Epic AND issuetype not in subTaskIssueTypes()")
            .order_by(SortField::Default, SortDir::Asc)
            .build();
        assert!(req.jql.contains("issuetype != Epic"));
        assert!(req.jql.contains("issuetype not in subTaskIssueTypes()"));
    }

    #[test]
    fn parent_search_kinds_and_children_clause() {
        assert_eq!(ParentSearchKind::Epic.jql_clause(), "issuetype = Epic");
        assert_eq!(
            ParentSearchKind::NonEpicNonSubtask.jql_clause(),
            "issuetype != Epic AND issuetype not in subTaskIssueTypes()"
        );
        assert_eq!(children_clause("ABC-1"), "parent = \"ABC-1\"");

        let epic_parents = SearchBuilder::new()
            .project("ABC")
            .clause(ParentSearchKind::Epic.jql_clause())
            .text("checkout")
            .build();
        assert!(epic_parents.jql.contains("issuetype = Epic"));
        assert!(epic_parents.jql.contains("summary ~ \"checkout\""));

        let kids = SearchBuilder::new()
            .project("ABC")
            .clause(children_clause("ABC-9"))
            .build();
        assert!(kids.jql.contains("parent = \"ABC-9\""));
        assert!(default_fields(None).iter().any(|f| f == "parent"));
    }

    #[test]
    fn footer_label_prefers_display_names() {
        let filter = AssigneeFilter {
            accounts: vec!["abc".into(), "xyz".into()],
            unassigned: true,
            ..AssigneeFilter::default()
        };
        let label = filter.footer_label(Some("abc"), |id| match id {
            "abc" => Some("Alice"),
            "xyz" => Some("Bob"),
            _ => None,
        });
        assert_eq!(label, "unassigned, you, Bob");
    }

    #[test]
    fn assignee_jql_skips_empty_and_does_not_double_count_me() {
        assert_eq!(AssigneeFilter::default().jql_clause(None), None);

        let me = AssigneeFilter {
            me: true,
            ..AssigneeFilter::default()
        };
        assert_eq!(
            me.jql_clause(None).as_deref(),
            Some("assignee = currentUser()")
        );
        assert_eq!(
            me.jql_clause(Some("abc")).as_deref(),
            Some("assignee = \"abc\"")
        );

        let mut both = AssigneeFilter {
            me: true,
            accounts: vec!["abc".into(), "other".into(), "abc".into()],
            unassigned: true,
        };
        both.resolve_me("abc");
        assert!(!both.me);
        assert_eq!(both.accounts, vec!["abc".to_string(), "other".to_string()]);
        assert_eq!(
            both.jql_clause(None).as_deref(),
            Some("(assignee in (\"abc\", \"other\") OR assignee is EMPTY)")
        );
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
