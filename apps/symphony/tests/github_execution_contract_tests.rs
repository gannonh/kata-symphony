use mockito::{Matcher, Server, ServerGuard};
use serde_json::json;
use symphony::domain::{ApiKey, GithubProjectOwnerType, TrackerConfig};
use symphony::error::SymphonyError;
use symphony::github::adapter::GithubAdapter;
use symphony::github::client::GithubClient;
use symphony::linear::adapter::TrackerAdapter;

fn base_config() -> TrackerConfig {
    TrackerConfig {
        kind: Some("github".to_string()),
        endpoint: "https://api.github.com".to_string(),
        api_key: Some(ApiKey::new("test-token")),
        project_slug: None,
        workspace_slug: None,
        repo_owner: Some("kata-sh".to_string()),
        repo_name: Some("kata-mono".to_string()),
        github_project_owner_type: Some(GithubProjectOwnerType::Org),
        github_project_number: None,
        label_prefix: Some("symphony".to_string()),
        assignee: None,
        active_states: vec![
            "Todo".to_string(),
            "In Progress".to_string(),
            "Agent Review".to_string(),
            "Human Review".to_string(),
            "Merging".to_string(),
        ],
        terminal_states: vec!["Done".to_string(), "Canceled".to_string()],
        exclude_labels: vec![],
    }
}

fn label_adapter(server: &ServerGuard) -> GithubAdapter {
    let client = GithubClient::with_base_url(
        "test-token",
        "kata-sh",
        "kata-mono",
        "symphony",
        server.url(),
    );
    GithubAdapter::new(client, base_config())
}

fn projects_adapter(server: &ServerGuard) -> GithubAdapter {
    let mut config = base_config();
    config.github_project_owner_type = Some(GithubProjectOwnerType::Org);
    config.github_project_number = Some(42);
    config.active_states = vec!["Todo".to_string(), "In Progress".to_string()];

    let client = GithubClient::with_base_url(
        "test-token",
        "kata-sh",
        "kata-mono",
        "symphony",
        server.url(),
    );
    GithubAdapter::new(client, config)
}

fn issue_json(number: u64, title: &str, body: Option<&str>, labels: &[&str]) -> serde_json::Value {
    let parent_issue_url = body
        .and_then(parent_issue_number_from_body)
        .map(|parent_number| {
            format!("https://api.github.com/repos/kata-sh/kata-mono/issues/{parent_number}")
        });

    issue_json_with_parent_url(number, title, body, labels, parent_issue_url)
}

fn parent_issue_number_from_body(body: &str) -> Option<u64> {
    let marker_index = body.find('#')?;
    let digits: String = body[marker_index + 1..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();

    if digits.is_empty() {
        return None;
    }

    digits.parse::<u64>().ok()
}

fn issue_json_with_parent_url(
    number: u64,
    title: &str,
    body: Option<&str>,
    labels: &[&str],
    parent_issue_url: Option<String>,
) -> serde_json::Value {
    json!({
        "number": number,
        "title": title,
        "body": body,
        "state": "open",
        "user": { "login": "alice" },
        "assignee": { "login": "alice" },
        "assignees": [{ "login": "alice" }],
        "labels": labels
            .iter()
            .map(|name| json!({ "name": name, "color": "ffffff", "description": null }))
            .collect::<Vec<_>>(),
        "created_at": "2026-03-29T10:00:00Z",
        "updated_at": "2026-03-29T10:30:00Z",
        "html_url": format!("https://github.com/kata-sh/kata-mono/issues/{number}"),
        "parent_issue_url": parent_issue_url,
        "sub_issues_summary": { "total": 0, "completed": 0, "percent_completed": 0 }
    })
}

fn projects_field_payload(options: &[(&str, &str)]) -> serde_json::Value {
    json!({
        "data": {
            "user": {
                "projectV2": {
                    "id": "project_42",
                    "field": {
                        "id": "status_field",
                        "options": options
                            .iter()
                            .map(|(id, name)| json!({ "id": id, "name": name }))
                            .collect::<Vec<_>>()
                    }
                }
            },
            "organization": null
        }
    })
}

fn project_items_payload(status_name: &str, option_id: &str) -> serde_json::Value {
    json!({
        "data": {
            "node": {
                "items": {
                    "nodes": [
                        {
                            "id": "item_7",
                            "content": {
                                "number": 7,
                                "repository": {
                                    "name": "kata-mono",
                                    "owner": { "login": "kata-sh" }
                                }
                            },
                            "status": {
                                "name": status_name,
                                "optionId": option_id
                            }
                        }
                    ],
                    "pageInfo": {
                        "hasNextPage": false,
                        "endCursor": null
                    }
                }
            }
        }
    })
}

#[tokio::test]
async fn test_label_mode_execution_lifecycle_contract_accepts_trailing_colon_prefix() {
    let mut server = Server::new_async().await;
    let mut config = base_config();
    config.label_prefix = Some("symphony:".to_string());

    let client = GithubClient::with_base_url(
        "test-token",
        "kata-sh",
        "kata-mono",
        "symphony",
        server.url(),
    );
    let adapter = GithubAdapter::new(client, config);

    let list_mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([issue_json(
                7,
                "[S01] Build feature",
                Some("body"),
                &["symphony:todo", "kata:slice"],
            )])
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let candidates = adapter
        .fetch_candidate_issues()
        .await
        .expect("candidate dispatch fetch should succeed with trailing-colon label prefix");

    list_mock.assert_async().await;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].state, "Todo");
}

#[tokio::test]
async fn test_label_mode_execution_lifecycle_contract() {
    let mut server = Server::new_async().await;
    let adapter = label_adapter(&server);

    let todo_issue = issue_json(
        7,
        "[S01] Build feature",
        Some("**Parent:** #10\n\nimplements feature"),
        &["symphony:todo", "kata:slice"],
    );
    let in_progress_issue = issue_json(
        7,
        "[S01] Build feature",
        Some("**Parent:** #10\n\nimplements feature"),
        &["symphony:in-progress", "kata:slice"],
    );
    let done_issue = issue_json(
        7,
        "[S01] Build feature",
        Some("**Parent:** #10\n\nimplements feature"),
        &["symphony:done", "kata:slice"],
    );

    let list_mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!([todo_issue.clone()]).to_string())
        .expect(1)
        .create_async()
        .await;

    let get_for_in_progress_update = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/7")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(todo_issue.to_string())
        .expect(1)
        .create_async()
        .await;

    let remove_todo = server
        .mock(
            "DELETE",
            "/repos/kata-sh/kata-mono/issues/7/labels/symphony:todo",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .expect(1)
        .create_async()
        .await;

    let add_in_progress = server
        .mock("POST", "/repos/kata-sh/kata-mono/issues/7/labels")
        .match_body(Matcher::PartialJson(
            json!({ "labels": ["symphony:in-progress"] }),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .expect(1)
        .create_async()
        .await;

    let get_for_in_progress_refresh = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/7")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(in_progress_issue.clone().to_string())
        .expect(1)
        .create_async()
        .await;

    let get_for_done_update = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/7")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(in_progress_issue.to_string())
        .expect(1)
        .create_async()
        .await;

    let remove_in_progress = server
        .mock(
            "DELETE",
            "/repos/kata-sh/kata-mono/issues/7/labels/symphony:in-progress",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .expect(1)
        .create_async()
        .await;

    let add_done = server
        .mock("POST", "/repos/kata-sh/kata-mono/issues/7/labels")
        .match_body(Matcher::PartialJson(json!({ "labels": ["symphony:done"] })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .expect(1)
        .create_async()
        .await;

    let get_for_done_refresh = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/7")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(done_issue.to_string())
        .expect(1)
        .create_async()
        .await;

    let candidates = adapter
        .fetch_candidate_issues()
        .await
        .expect("candidate dispatch fetch should succeed");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].identifier, "[S01]#7");
    assert_eq!(candidates[0].state, "Todo");
    assert_eq!(candidates[0].parent_identifier.as_deref(), Some("#10"));

    adapter
        .update_issue_state("7", "In Progress")
        .await
        .expect("state transition to In Progress should succeed");

    let in_progress_state = adapter
        .fetch_issue_states_by_ids(&["7".to_string()])
        .await
        .expect("refresh after In Progress transition should succeed");
    assert_eq!(in_progress_state.len(), 1);
    assert_eq!(in_progress_state[0].identifier, "[S01]#7");
    assert_eq!(in_progress_state[0].state, "In Progress");
    assert_eq!(
        in_progress_state[0].parent_identifier.as_deref(),
        Some("#10")
    );

    adapter
        .update_issue_state("7", "Done")
        .await
        .expect("state transition to Done should succeed");

    let done_state = adapter
        .fetch_issue_states_by_ids(&["7".to_string()])
        .await
        .expect("refresh after Done transition should succeed");
    assert_eq!(done_state.len(), 1);
    assert_eq!(done_state[0].identifier, "[S01]#7");
    assert_eq!(done_state[0].state, "Done");
    assert_eq!(done_state[0].parent_identifier.as_deref(), Some("#10"));
    assert!(done_state[0].assigned_to_worker);

    list_mock.assert_async().await;
    get_for_in_progress_update.assert_async().await;
    remove_todo.assert_async().await;
    add_in_progress.assert_async().await;
    get_for_in_progress_refresh.assert_async().await;
    get_for_done_update.assert_async().await;
    remove_in_progress.assert_async().await;
    add_done.assert_async().await;
    get_for_done_refresh.assert_async().await;
}

#[tokio::test]
async fn test_projects_v2_execution_lifecycle_contract() {
    let mut server = Server::new_async().await;
    let adapter = projects_adapter(&server);

    let fields_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            projects_field_payload(&[
                ("opt_todo", "Todo"),
                ("opt_in_progress", "In Progress"),
                ("opt_done", "Done"),
            ])
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let items_candidate = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(project_items_payload("Todo", "opt_todo").to_string())
        .expect(1)
        .create_async()
        .await;

    let issue_for_candidate = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/7")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            issue_json(
                7,
                "[S01] Build feature",
                Some("Part of: #10"),
                &["symphony:todo", "kata:slice"],
            )
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let items_update_in_progress = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(project_items_payload("Todo", "opt_todo").to_string())
        .expect(1)
        .create_async()
        .await;

    let mutation_in_progress = server
        .mock("POST", "/graphql")
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex("updateProjectV2ItemFieldValue".to_string()),
            Matcher::PartialJson(json!({
                "variables": {
                    "projectId": "project_42",
                    "itemId": "item_7",
                    "fieldId": "status_field",
                    "singleSelectOptionId": "opt_in_progress"
                }
            })),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "data": { "updateProjectV2ItemFieldValue": { "projectV2Item": { "id": "item_7" }}}}).to_string())
        .expect(1)
        .create_async()
        .await;

    let items_refresh_in_progress = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(project_items_payload("In Progress", "opt_in_progress").to_string())
        .expect(1)
        .create_async()
        .await;

    let issue_for_refresh_in_progress = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/7")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            issue_json(
                7,
                "[S01] Build feature",
                Some("Part of: #10"),
                &["symphony:todo", "kata:slice"],
            )
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let items_update_done = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(project_items_payload("In Progress", "opt_in_progress").to_string())
        .expect(1)
        .create_async()
        .await;

    let mutation_done = server
        .mock("POST", "/graphql")
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex("updateProjectV2ItemFieldValue".to_string()),
            Matcher::PartialJson(json!({
                "variables": {
                    "projectId": "project_42",
                    "itemId": "item_7",
                    "fieldId": "status_field",
                    "singleSelectOptionId": "opt_done"
                }
            })),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "data": { "updateProjectV2ItemFieldValue": { "projectV2Item": { "id": "item_7" }}}}).to_string())
        .expect(1)
        .create_async()
        .await;

    let items_refresh_done = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(project_items_payload("Done", "opt_done").to_string())
        .expect(1)
        .create_async()
        .await;

    let issue_for_refresh_done = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/7")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            issue_json(
                7,
                "[S01] Build feature",
                Some("Part of: #10"),
                &["symphony:todo", "kata:slice"],
            )
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let candidates = adapter
        .fetch_candidate_issues()
        .await
        .expect("projects v2 candidate fetch should succeed");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].identifier, "[S01]#7");
    assert_eq!(candidates[0].state, "Todo");

    adapter
        .update_issue_state("7", "In Progress")
        .await
        .expect("projects v2 transition to In Progress should succeed");

    let in_progress_state = adapter
        .fetch_issue_states_by_ids(&["7".to_string()])
        .await
        .expect("projects v2 refresh for In Progress should succeed");
    assert_eq!(in_progress_state.len(), 1);
    assert_eq!(in_progress_state[0].identifier, "[S01]#7");
    assert_eq!(in_progress_state[0].state, "In Progress");
    assert_eq!(
        in_progress_state[0].parent_identifier.as_deref(),
        Some("#10")
    );

    adapter
        .update_issue_state("7", "Done")
        .await
        .expect("projects v2 transition to Done should succeed");

    let done_state = adapter
        .fetch_issue_states_by_ids(&["7".to_string()])
        .await
        .expect("projects v2 refresh for Done should succeed");
    assert_eq!(done_state.len(), 1);
    assert_eq!(done_state[0].identifier, "[S01]#7");
    assert_eq!(done_state[0].state, "Done");
    assert_eq!(done_state[0].parent_identifier.as_deref(), Some("#10"));

    fields_mock.assert_async().await;
    items_candidate.assert_async().await;
    issue_for_candidate.assert_async().await;
    items_update_in_progress.assert_async().await;
    mutation_in_progress.assert_async().await;
    items_refresh_in_progress.assert_async().await;
    issue_for_refresh_in_progress.assert_async().await;
    items_update_done.assert_async().await;
    mutation_done.assert_async().await;
    items_refresh_done.assert_async().await;
    issue_for_refresh_done.assert_async().await;
}

#[tokio::test]
async fn test_projects_v2_unknown_status_reports_actionable_error() {
    let mut server = Server::new_async().await;
    let adapter = projects_adapter(&server);

    let fields_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            projects_field_payload(&[
                ("opt_todo", "Todo"),
                ("opt_in_progress", "In Progress"),
                ("opt_done", "Done"),
            ])
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let err = adapter
        .update_issue_state("7", "Blocked")
        .await
        .expect_err("unknown status should fail in projects v2 mode");

    fields_mock.assert_async().await;
    match err {
        SymphonyError::GithubProjectsV2Error(message) => {
            assert!(message.contains("status option 'Blocked' not found"));
            assert!(message.contains("Todo"));
            assert!(message.contains("In Progress"));
            assert!(message.contains("Done"));
        }
        other => panic!("expected GithubProjectsV2Error, got {other:?}"),
    }
}

#[tokio::test]
async fn test_refresh_deleted_issue_and_comment_on_closed_issue_do_not_panic() {
    let mut server = Server::new_async().await;
    let adapter = label_adapter(&server);

    let missing_issue = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/404")
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(json!({ "message": "Not Found" }).to_string())
        .expect(1)
        .create_async()
        .await;

    let states = adapter
        .fetch_issue_states_by_ids(&["404".to_string()])
        .await
        .expect("deleted issue refresh should skip gracefully");
    assert!(states.is_empty());

    let closed_comment = server
        .mock("POST", "/repos/kata-sh/kata-mono/issues/404/comments")
        .with_status(403)
        .with_header("content-type", "application/json")
        .with_body(json!({ "message": "Issue is closed" }).to_string())
        .expect(1)
        .create_async()
        .await;

    let comment_result = adapter
        .create_comment("404", "## Symphony Execution Summary")
        .await;
    assert!(
        comment_result.is_err(),
        "closed issue comments should return an error, not panic"
    );

    missing_issue.assert_async().await;
    closed_comment.assert_async().await;
}

#[tokio::test]
async fn test_label_and_projects_modes_emit_identical_issue_contract_shape() {
    let mut label_server = Server::new_async().await;
    let label_adapter = label_adapter(&label_server);

    let issue_payload = issue_json(
        7,
        "[S01] Build feature",
        Some("Parent: #10"),
        &["symphony:todo", "kata:slice"],
    );

    let label_list = label_server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!([issue_payload.clone()]).to_string())
        .expect(1)
        .create_async()
        .await;

    let mut projects_server = Server::new_async().await;
    let projects_adapter = projects_adapter(&projects_server);

    let fields_mock = projects_server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            projects_field_payload(&[("opt_todo", "Todo"), ("opt_in_progress", "In Progress")])
                .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let items_mock = projects_server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(project_items_payload("Todo", "opt_todo").to_string())
        .expect(1)
        .create_async()
        .await;

    let issue_mock = projects_server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/7")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_payload.to_string())
        .expect(1)
        .create_async()
        .await;

    let label_issue = label_adapter
        .fetch_candidate_issues()
        .await
        .expect("label mode candidate fetch should succeed")
        .into_iter()
        .next()
        .expect("label mode should return one issue");

    let projects_issue = projects_adapter
        .fetch_candidate_issues()
        .await
        .expect("projects mode candidate fetch should succeed")
        .into_iter()
        .next()
        .expect("projects mode should return one issue");

    label_list.assert_async().await;
    fields_mock.assert_async().await;
    items_mock.assert_async().await;
    issue_mock.assert_async().await;

    let label_shape = (
        label_issue.description.is_some(),
        label_issue.url.is_some(),
        label_issue.parent_identifier.is_some(),
        !label_issue.labels.is_empty(),
        label_issue.assignee_id.is_some(),
    );
    let projects_shape = (
        projects_issue.description.is_some(),
        projects_issue.url.is_some(),
        projects_issue.parent_identifier.is_some(),
        !projects_issue.labels.is_empty(),
        projects_issue.assignee_id.is_some(),
    );

    assert_eq!(label_shape, projects_shape);
    assert_eq!(label_issue.identifier, projects_issue.identifier);
    assert_eq!(label_issue.state, projects_issue.state);
    assert_eq!(
        label_issue.parent_identifier,
        projects_issue.parent_identifier
    );
    assert_eq!(label_issue.url, projects_issue.url);
}
