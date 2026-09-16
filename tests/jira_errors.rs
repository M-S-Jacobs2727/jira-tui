mod common;

use common::{JiraMock, fixture};
use jira_tui::error::Error;
use jira_tui::jira::issues::IssueFacade;

#[tokio::test]
async fn create_maps_error_messages_and_field_errors() {
    let jira = JiraMock::start().await;
    jira.mock_error("POST", "api/3/issue", 400, &fixture("error_400.json"))
        .await;

    let err = IssueFacade::new(&jira.client)
        .create(
            "DEMO",
            &jira_tui::jira::issues::IssueDraft {
                issue_type: String::new(),
                summary: String::new(),
                ..jira_tui::jira::issues::IssueDraft::default()
            },
        )
        .await
        .unwrap_err();

    match err {
        Error::Jira(api) => {
            insta::assert_debug_snapshot!(api);
            insta::assert_snapshot!(api.to_string());
        }
        other => panic!("expected Jira API error, got {other}"),
    }
}

#[tokio::test]
async fn delete_treats_empty_success_as_ok() {
    let jira = JiraMock::start().await;
    jira.mock_status("DELETE", "api/3/issue/DEMO-1", 204).await;

    IssueFacade::new(&jira.client)
        .delete("DEMO-1")
        .await
        .unwrap();
    insta::assert_json_snapshot!(jira.request_snapshots().await);
}

#[tokio::test]
async fn empty_error_body_is_reported() {
    let jira = JiraMock::start().await;
    jira.mock_status(
        "GET",
        "api/3/issue/DEMO-missing?expand=renderedFields,names",
        404,
    )
    .await;

    let err = IssueFacade::new(&jira.client)
        .get("DEMO-missing")
        .await
        .unwrap_err();
    match err {
        Error::Jira(api) => {
            insta::assert_debug_snapshot!(api);
            insta::assert_snapshot!(api.to_string());
        }
        other => panic!("expected Jira API error, got {other}"),
    }
}
