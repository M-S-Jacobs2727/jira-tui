mod common;

use common::{JiraMock, fixture, issue_snapshot};
use jira_tui::jira::issues::{IssueDraft, IssueFacade};

fn draft() -> IssueDraft {
    IssueDraft {
        issue_type: "Story".into(),
        summary: "New checkout flow".into(),
        description: "Line 1\n\nLine 2".into(),
        priority: Some("High".into()),
        assignee_account_id: Some("acc-1".into()),
        story_points: Some(8.0),
        sprint_id: None,
        include_sprint: true,
        parent_key: None,
    }
}

#[tokio::test]
async fn get_parses_rendered_description_comments_and_story_points() {
    let jira = JiraMock::start().await;
    jira.mock_json(
        "GET",
        "api/3/issue/DEMO-1?expand=renderedFields,names",
        &fixture("issue_get.json"),
    )
    .await;

    let issue = IssueFacade::new(&jira.client).get("DEMO-1").await.unwrap();
    insta::assert_debug_snapshot!(issue_snapshot(&issue));
    insta::assert_json_snapshot!(jira.request_snapshots().await);
}

#[tokio::test]
async fn create_meta_loads_issue_types_and_priorities() {
    let jira = JiraMock::start().await;
    jira.mock_json(
        "GET",
        "api/3/issue/createmeta/DEMO/issuetypes",
        &fixture("createmeta.json"),
    )
    .await;
    jira.mock_json("GET", "api/3/priority", &fixture("priorities.json"))
        .await;

    let meta = IssueFacade::new(&jira.client)
        .create_meta("DEMO")
        .await
        .unwrap();
    insta::assert_debug_snapshot!(meta);
}

#[tokio::test]
async fn create_posts_fields_and_returns_key() {
    let jira = JiraMock::start().await;
    jira.mock_json("POST", "api/3/issue", &fixture("issue_create.json"))
        .await;

    let key = IssueFacade::new(&jira.client)
        .create("DEMO", &draft())
        .await
        .unwrap();
    assert_eq!(key, "DEMO-42");
    insta::assert_json_snapshot!(jira.json_bodies_for("POST", "api/3/issue").await);
}

#[tokio::test]
async fn create_with_parent_sets_parent_key() {
    let jira = JiraMock::start().await;
    jira.mock_json("POST", "api/3/issue", &fixture("issue_create.json"))
        .await;

    let mut draft = draft();
    draft.parent_key = Some("DEMO-100".into());
    let key = IssueFacade::new(&jira.client)
        .create("DEMO", &draft)
        .await
        .unwrap();
    assert_eq!(key, "DEMO-42");
    insta::assert_json_snapshot!(jira.json_bodies_for("POST", "api/3/issue").await);
}

#[tokio::test]
async fn update_clears_parent_when_none() {
    let jira = JiraMock::start().await;
    jira.mock_json("GET", "api/3/field", &fixture("fields.json"))
        .await;
    jira.mock_json("PUT", "api/3/issue/DEMO-1", "null").await;

    let mut draft = draft();
    draft.parent_key = None;
    IssueFacade::new(&jira.client)
        .update("DEMO-1", &draft)
        .await
        .unwrap();
    let bodies = jira.json_bodies_for("PUT", "api/3/issue/DEMO-1").await;
    let parent = bodies[0]["fields"]["parent"].clone();
    assert!(parent.is_null());
}

#[tokio::test]
async fn create_with_sprint_looks_up_sprint_field() {
    let jira = JiraMock::start().await;
    jira.mock_json("GET", "api/3/field", &fixture("fields.json"))
        .await;
    jira.mock_json("POST", "api/3/issue", &fixture("issue_create.json"))
        .await;

    let mut draft = draft();
    draft.sprint_id = Some(12);
    let key = IssueFacade::new(&jira.client)
        .create("DEMO", &draft)
        .await
        .unwrap();
    assert_eq!(key, "DEMO-42");
    insta::assert_json_snapshot!(jira.json_bodies_for("POST", "api/3/issue").await);
}

#[tokio::test]
async fn update_puts_summary_description_and_clears_sprint() {
    let jira = JiraMock::start().await;
    jira.mock_json("GET", "api/3/field", &fixture("fields.json"))
        .await;
    jira.mock_json("PUT", "api/3/issue/DEMO-1", "null").await;

    IssueFacade::new(&jira.client)
        .update("DEMO-1", &draft())
        .await
        .unwrap();
    insta::assert_json_snapshot!(jira.json_bodies_for("PUT", "api/3/issue/DEMO-1").await);
}

#[tokio::test]
async fn delete_succeeds_on_no_content() {
    let jira = JiraMock::start().await;
    jira.mock_status("DELETE", "api/3/issue/DEMO-1", 204).await;

    IssueFacade::new(&jira.client)
        .delete("DEMO-1")
        .await
        .unwrap();
    insta::assert_json_snapshot!(jira.request_snapshots().await);
}

#[tokio::test]
async fn assign_and_unassign_put_account_id() {
    let jira = JiraMock::start().await;
    jira.mock_json("PUT", "api/3/issue/DEMO-1/assignee", "null")
        .await;

    let facade = IssueFacade::new(&jira.client);
    facade.assign("DEMO-1", Some("acc-1")).await.unwrap();
    facade.assign("DEMO-1", None).await.unwrap();
    insta::assert_json_snapshot!(
        jira.json_bodies_for("PUT", "api/3/issue/DEMO-1/assignee")
            .await
    );
}

#[tokio::test]
async fn set_sprint_puts_sprint_field_or_null() {
    let jira = JiraMock::start().await;
    jira.mock_json("GET", "api/3/field", &fixture("fields.json"))
        .await;
    jira.mock_json("PUT", "api/3/issue/DEMO-1", "null").await;

    let facade = IssueFacade::new(&jira.client);
    facade.set_sprint("DEMO-1", Some(12)).await.unwrap();
    facade.set_sprint("DEMO-1", None).await.unwrap();
    insta::assert_json_snapshot!(jira.json_bodies_for("PUT", "api/3/issue/DEMO-1").await);
}

#[tokio::test]
async fn transitions_parse_required_fields() {
    let jira = JiraMock::start().await;
    jira.mock_json(
        "GET",
        "api/3/issue/DEMO-1/transitions",
        &fixture("transitions.json"),
    )
    .await;

    let items = IssueFacade::new(&jira.client)
        .transitions("DEMO-1")
        .await
        .unwrap();
    insta::assert_debug_snapshot!(items);
}

#[tokio::test]
async fn transition_posts_transition_id() {
    let jira = JiraMock::start().await;
    jira.mock_json("POST", "api/3/issue/DEMO-1/transitions", "null")
        .await;

    IssueFacade::new(&jira.client)
        .transition("DEMO-1", "31")
        .await
        .unwrap();
    insta::assert_json_snapshot!(
        jira.json_bodies_for("POST", "api/3/issue/DEMO-1/transitions")
            .await
    );
}

#[tokio::test]
async fn assignable_users_with_and_without_query() {
    let jira = JiraMock::start().await;
    jira.mock_json(
        "GET",
        "api/3/user/assignable/search?project=DEMO&maxResults=20",
        &fixture("assignable_users.json"),
    )
    .await;
    jira.mock_json(
        "GET",
        "api/3/user/assignable/search?project=DEMO&query=Ada&maxResults=20",
        &fixture("assignable_users.json"),
    )
    .await;

    let facade = IssueFacade::new(&jira.client);
    let all = facade.assignable_users("DEMO", "").await.unwrap();
    let filtered = facade.assignable_users("DEMO", "Ada").await.unwrap();
    insta::assert_debug_snapshot!("assignable_users", (all, filtered));
    insta::assert_json_snapshot!("assignable_users_requests", jira.request_snapshots().await);
}
