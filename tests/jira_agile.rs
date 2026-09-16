mod common;

use common::{JiraMock, fixture};
use jira_tui::jira::agile::AgileFacade;

#[tokio::test]
async fn boards_parse_project_location() {
    let jira = JiraMock::start().await;
    jira.mock_json(
        "GET",
        "agile/1.0/board?maxResults=50",
        &fixture("boards.json"),
    )
    .await;

    let boards = AgileFacade::new(&jira.client).boards(None).await.unwrap();
    insta::assert_debug_snapshot!(boards);
    insta::assert_json_snapshot!(jira.request_snapshots().await);
}

#[tokio::test]
async fn boards_filter_by_project_key() {
    let jira = JiraMock::start().await;
    jira.mock_json(
        "GET",
        "agile/1.0/board?projectKeyOrId=DEMO&maxResults=50",
        &fixture("boards.json"),
    )
    .await;

    let boards = AgileFacade::new(&jira.client)
        .boards(Some("DEMO"))
        .await
        .unwrap();
    insta::assert_debug_snapshot!(boards);
    insta::assert_json_snapshot!(jira.request_snapshots().await);
}

#[tokio::test]
async fn open_sprints_sort_active_before_future() {
    let jira = JiraMock::start().await;
    jira.mock_json(
        "GET",
        "agile/1.0/board/1/sprint?state=active,future&maxResults=50",
        &fixture("sprints.json"),
    )
    .await;

    let sprints = AgileFacade::new(&jira.client)
        .open_sprints(1)
        .await
        .unwrap();
    insta::assert_debug_snapshot!(sprints);
    insta::assert_json_snapshot!(jira.request_snapshots().await);
}

#[tokio::test]
async fn projects_reuse_board_model() {
    let jira = JiraMock::start().await;
    jira.mock_json(
        "GET",
        "api/3/project/search?maxResults=50",
        &fixture("projects.json"),
    )
    .await;

    let projects = AgileFacade::new(&jira.client).projects().await.unwrap();
    insta::assert_debug_snapshot!(projects);
}

#[tokio::test]
async fn story_points_field_reads_board_configuration() {
    let jira = JiraMock::start().await;
    jira.mock_json(
        "GET",
        "agile/1.0/board/1/configuration",
        &fixture("board_configuration.json"),
    )
    .await;

    let field = AgileFacade::new(&jira.client)
        .story_points_field(1)
        .await
        .unwrap();
    insta::assert_debug_snapshot!(field);
}
