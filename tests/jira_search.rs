mod common;

use common::{JiraMock, fixture, search_page_snapshot};
use jira_tui::jira::search::{SearchBuilder, SearchFacade, SortDir, SortField, SprintRef};

#[tokio::test]
async fn search_returns_issues_and_next_page_token() {
    let jira = JiraMock::start().await;
    jira.mock_json("POST", "api/3/search/jql", &fixture("search_jql.json"))
        .await;

    let request = SearchBuilder::new()
        .project("DEMO")
        .sprint(SprintRef::Id(12))
        .status(["In Progress"])
        .issue_type(["Bug"])
        .text("checkout")
        .story_points_field(Some(common::STORY_POINTS_FIELD.into()))
        .order_by(SortField::Priority, SortDir::Desc)
        .next_page("page-1")
        .build();
    let page = SearchFacade::new(&jira.client)
        .search(request)
        .await
        .unwrap();

    insta::assert_debug_snapshot!(search_page_snapshot(&page));
    insta::assert_json_snapshot!(jira.json_bodies_for("POST", "api/3/search/jql").await);
}

#[tokio::test]
async fn discovers_sprint_and_story_points_field_ids() {
    let jira = JiraMock::start().await;
    jira.mock_json("GET", "api/3/field", &fixture("fields.json"))
        .await;
    let search = SearchFacade::new(&jira.client);

    insta::assert_debug_snapshot!("sprint_field_id", search.sprint_field_id().await.unwrap());
    insta::assert_debug_snapshot!(
        "story_points_field_id",
        search.story_points_field_id().await.unwrap()
    );
    insta::assert_json_snapshot!("field_discovery_requests", jira.request_snapshots().await);
}

#[tokio::test]
async fn discover_sprints_collects_open_sprints_and_ignores_closed() {
    let jira = JiraMock::start().await;
    jira.mock_json("GET", "api/3/field", &fixture("fields.json"))
        .await;
    jira.mock_json(
        "POST",
        "api/3/search/jql",
        &fixture("discover_sprints.json"),
    )
    .await;

    let sprints = SearchFacade::new(&jira.client)
        .discover_sprints(Some("DEMO"))
        .await
        .unwrap();

    insta::assert_debug_snapshot!(sprints);
    insta::assert_json_snapshot!(jira.json_bodies_for("POST", "api/3/search/jql").await);
}
