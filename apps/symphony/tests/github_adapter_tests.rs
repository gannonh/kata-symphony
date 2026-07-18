use mockito::{Matcher, Server, ServerGuard};
use serde_json::json;
use symphony::domain::{ApiKey, GithubProjectOwnerType, TrackerConfig, KATA_PHASE_NAMES};
use symphony::error::SymphonyError;
use symphony::github::adapter::{GithubAdapter, StateMode};
use symphony::github::client::GithubClient;
use symphony::linear::adapter::TrackerAdapter;

fn test_config(assignee: Option<&str>) -> TrackerConfig {
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
        assignee: assignee.map(|value| value.to_string()),
        active_states: vec!["Todo".to_string(), "In Progress".to_string()],
        terminal_states: vec!["Done".to_string(), "Cancelled".to_string()],
        exclude_labels: vec![],
    }
}

fn test_adapter(server: &ServerGuard, assignee: Option<&str>) -> GithubAdapter {
    let client = GithubClient::with_base_url(
        "test-token",
        "kata-sh",
        "kata-mono",
        "symphony",
        server.url(),
    );
    GithubAdapter::new(client, test_config(assignee))
}

fn test_projects_adapter(
    server: &ServerGuard,
    assignee: Option<&str>,
    project_number: u64,
) -> GithubAdapter {
    let mut config = test_config(assignee);
    config.github_project_owner_type = Some(GithubProjectOwnerType::Org);
    config.github_project_number = Some(project_number);

    let client = GithubClient::with_base_url(
        "test-token",
        "kata-sh",
        "kata-mono",
        "symphony",
        server.url(),
    );

    GithubAdapter::new(client, config)
}

fn projects_v2_status_field_payload() -> serde_json::Value {
    json!({
        "data": {
            "user": {
                "projectV2": {
                    "id": "project_42",
                    "field": {
                        "id": "status_field",
                        "options": [
                            { "id": "opt_todo", "name": "Todo" },
                            { "id": "opt_in_progress", "name": "In Progress" },
                            { "id": "opt_done", "name": "Done" }
                        ]
                    }
                }
            },
            "organization": null
        }
    })
}

fn projects_v2_items_payload(items: &[serde_json::Value]) -> serde_json::Value {
    json!({
        "data": {
            "node": {
                "items": {
                    "nodes": items,
                    "pageInfo": {
                        "hasNextPage": false,
                        "endCursor": null
                    }
                }
            }
        }
    })
}

fn project_item_node(
    item_id: &str,
    issue_number: u64,
    option_id: &str,
    status_name: &str,
) -> serde_json::Value {
    project_item_node_with_fields(item_id, issue_number, option_id, status_name, None, None)
}

fn project_item_node_with_fields(
    item_id: &str,
    issue_number: u64,
    option_id: &str,
    status_name: &str,
    kata_id: Option<&str>,
    blocked_by: Option<&str>,
) -> serde_json::Value {
    let blocked_by_nodes = blocked_by_issue_numbers(blocked_by.unwrap_or_default())
        .into_iter()
        .map(|number| json!({ "number": number }))
        .collect::<Vec<_>>();

    json!({
        "id": item_id,
        "content": {
            "number": issue_number,
            "repository": {
                "name": "kata-mono",
                "owner": { "login": "kata-sh" }
            },
            "blockedBy": { "nodes": blocked_by_nodes }
        },
        "status": {
            "name": status_name,
            "optionId": option_id
        },
        "kataId": kata_id.map(|text| json!({ "text": text }))
    })
}

fn blocked_by_issue_numbers(value: &str) -> Vec<u64> {
    let after_hash = value
        .split('#')
        .skip(1)
        .filter_map(|segment| {
            let digits = segment
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>();
            digits.parse::<u64>().ok()
        })
        .collect::<Vec<_>>();
    if !after_hash.is_empty() {
        return after_hash;
    }

    value
        .split(|ch: char| !ch.is_ascii_digit())
        .filter_map(|segment| segment.parse::<u64>().ok())
        .collect()
}

fn project_item_node_without_dependency_fields(
    item_id: &str,
    issue_number: u64,
    option_id: &str,
    status_name: &str,
    kata_id: Option<&str>,
) -> serde_json::Value {
    json!({
        "id": item_id,
        "content": {
            "number": issue_number,
            "repository": {
                "name": "kata-mono",
                "owner": { "login": "kata-sh" }
            }
        },
        "status": {
            "name": status_name,
            "optionId": option_id
        },
        "kataId": kata_id.map(|text| json!({ "text": text }))
    })
}

async fn run_tracker_contract(
    adapter: &GithubAdapter,
    expected_state: &str,
    transition_state: &str,
) {
    let candidates = adapter
        .fetch_candidate_issues()
        .await
        .expect("fetch_candidate_issues should succeed");
    assert!(!candidates.is_empty(), "contract expects candidate issues");

    let by_state = adapter
        .fetch_issues_by_states(&[expected_state.to_string()])
        .await
        .expect("fetch_issues_by_states should succeed");
    assert!(!by_state.is_empty(), "contract expects issues_by_states");

    let issue_id = candidates[0].id.clone();
    let issue_states = adapter
        .fetch_issue_states_by_ids(std::slice::from_ref(&issue_id))
        .await
        .expect("fetch_issue_states_by_ids should succeed");
    assert!(!issue_states.is_empty(), "contract expects issue_states");

    adapter
        .create_comment(&issue_id, "contract comment")
        .await
        .expect("create_comment should succeed");

    adapter
        .update_issue_state(&issue_id, transition_state)
        .await
        .expect("update_issue_state should succeed");
}

fn issue_json(
    number: u64,
    labels: &[&str],
    user_login: &str,
    assignees: &[&str],
    html_url: Option<&str>,
) -> serde_json::Value {
    issue_json_with_metadata(
        number,
        &format!("Issue {number}"),
        Some(&format!("Body {number}")),
        labels,
        user_login,
        assignees,
        html_url,
    )
}

fn issue_json_with_metadata(
    number: u64,
    title: &str,
    body: Option<&str>,
    labels: &[&str],
    user_login: &str,
    assignees: &[&str],
    html_url: Option<&str>,
) -> serde_json::Value {
    let first_assignee = assignees.first().copied().unwrap_or(user_login);
    let parent_issue_url = body
        .and_then(parent_issue_number_from_body)
        .map(|parent_number| {
            format!("https://api.github.com/repos/kata-sh/kata-mono/issues/{parent_number}")
        });

    json!({
        "number": number,
        "title": title,
        "body": body,
        "state": "open",
        "user": { "login": user_login },
        "assignee": { "login": first_assignee },
        "assignees": assignees.iter().map(|login| json!({ "login": login })).collect::<Vec<_>>(),
        "labels": labels.iter().map(|name| json!({ "name": name, "color": "ffffff", "description": null })).collect::<Vec<_>>(),
        "created_at": "2026-03-29T10:00:00Z",
        "updated_at": "2026-03-29T10:30:00Z",
        "html_url": html_url,
        "parent_issue_url": parent_issue_url,
        "sub_issues_summary": { "total": 0, "completed": 0, "percent_completed": 0 }
    })
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

fn issue_state_json(mut issue: serde_json::Value, state: &str) -> serde_json::Value {
    issue["state"] = json!(state);
    issue
}

fn phase_to_label(phase: &str) -> String {
    phase
        .to_ascii_lowercase()
        .replace('_', "-")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

fn projects_v2_option_name_for_phase(phase: &str) -> String {
    match phase {
        "Todo" => "to_do".to_string(),
        "In Progress" => "in-progress".to_string(),
        "Agent Review" => "agent_review".to_string(),
        "Human Review" => "human review".to_string(),
        _ => phase_to_label(phase),
    }
}

fn pull_request_json(mut issue: serde_json::Value) -> serde_json::Value {
    issue["pull_request"] = json!({
        "url": "https://api.github.com/repos/kata-sh/kata-mono/pulls/123"
    });
    issue
}

#[tokio::test]
async fn test_fetch_candidate_issues_returns_matching_issues() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([
                issue_json(1, &["symphony:todo"], "alice", &["alice"], None),
                issue_json(2, &["symphony:in-progress"], "bob", &["bob"], None),
                issue_json(3, &["bug"], "carol", &["carol"], None)
            ])
            .to_string(),
        )
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("fetch_candidate_issues should succeed");

    mock.assert_async().await;
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].identifier, "#1");
    assert_eq!(issues[0].state, "Todo");
    assert_eq!(issues[1].identifier, "#2");
    assert_eq!(issues[1].state, "In Progress");
}

#[tokio::test]
async fn test_fetch_candidate_issues_accepts_trailing_colon_label_prefix() {
    let mut server = Server::new_async().await;

    let mut config = test_config(None);
    config.label_prefix = Some("symphony:".to_string());
    let client = GithubClient::with_base_url(
        "test-token",
        "kata-sh",
        "kata-mono",
        "symphony",
        server.url(),
    );
    let adapter = GithubAdapter::new(client, config);

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([
                issue_json(1, &["symphony:todo"], "alice", &["alice"], None),
                issue_json(2, &["symphony:in-progress"], "bob", &["bob"], None)
            ])
            .to_string(),
        )
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("fetch_candidate_issues should succeed with trailing-colon label prefix");

    mock.assert_async().await;
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].state, "Todo");
    assert_eq!(issues[1].state, "In Progress");
}

#[tokio::test]
async fn test_fetch_candidate_issues_skips_pull_requests() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let pull_request =
        pull_request_json(issue_json(4, &["symphony:todo"], "alice", &["alice"], None));

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!([pull_request]).to_string())
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("fetch_candidate_issues should succeed");

    mock.assert_async().await;
    assert!(issues.is_empty(), "pull requests must be ignored");
}

#[tokio::test]
async fn test_fetch_candidate_issues_applies_assignee_filter() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, Some("alice"));

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([
                issue_json(1, &["symphony:todo"], "alice", &["alice"], None),
                issue_json(2, &["symphony:todo"], "bob", &["bob"], None),
                issue_json(3, &["symphony:in-progress"], "eve", &["alice"], None)
            ])
            .to_string(),
        )
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("fetch_candidate_issues should succeed");

    mock.assert_async().await;
    assert_eq!(
        issues.len(),
        2,
        "only issues assigned to alice should remain"
    );
    assert_eq!(issues[0].identifier, "#1");
    assert_eq!(issues[1].identifier, "#3");
}

#[tokio::test]
async fn test_fetch_issues_by_states_filters_by_state_labels() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([
                issue_json(1, &["symphony:todo"], "alice", &["alice"], None),
                issue_json(2, &["symphony:done"], "bob", &["bob"], None),
                issue_json(3, &["symphony:in-progress"], "carol", &["carol"], None)
            ])
            .to_string(),
        )
        .create_async()
        .await;

    let issues = adapter
        .fetch_issues_by_states(&["Done".to_string()])
        .await
        .expect("fetch_issues_by_states should succeed");

    mock.assert_async().await;
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].identifier, "#2");
    assert_eq!(issues[0].state, "Done");
}

#[tokio::test]
async fn test_fetch_issues_by_states_marks_assignment_with_assignee_filter() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, Some("alice"));

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([
                issue_json(8, &["symphony:todo"], "alice", &["alice"], None),
                issue_json(9, &["symphony:todo"], "bob", &["bob"], None)
            ])
            .to_string(),
        )
        .create_async()
        .await;

    let issues = adapter
        .fetch_issues_by_states(&["Todo".to_string()])
        .await
        .expect("fetch_issues_by_states should succeed");

    mock.assert_async().await;
    assert_eq!(issues.len(), 2);
    assert!(issues[0].assigned_to_worker);
    assert!(!issues[1].assigned_to_worker);
}

#[tokio::test]
async fn test_fetch_issue_states_by_ids_returns_individual_issues() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let m1 = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/10")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(10, &["symphony:todo"], "alice", &["alice"], None).to_string())
        .create_async()
        .await;

    let m2 = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/11")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(11, &["symphony:done"], "bob", &["bob"], None).to_string())
        .create_async()
        .await;

    let issues = adapter
        .fetch_issue_states_by_ids(&["10".to_string(), "11".to_string()])
        .await
        .expect("fetch_issue_states_by_ids should succeed");

    m1.assert_async().await;
    m2.assert_async().await;
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].identifier, "#10");
    assert_eq!(issues[1].identifier, "#11");
}

#[tokio::test]
async fn test_fetch_issue_states_by_ids_marks_assignment_with_assignee_filter() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, Some("alice"));

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/12")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(12, &["symphony:todo"], "bob", &["bob"], None).to_string())
        .create_async()
        .await;

    let issues = adapter
        .fetch_issue_states_by_ids(&["12".to_string()])
        .await
        .expect("fetch_issue_states_by_ids should succeed");

    mock.assert_async().await;
    assert_eq!(issues.len(), 1);
    assert!(!issues[0].assigned_to_worker);
}

#[tokio::test]
async fn test_fetch_issue_states_by_ids_skips_pull_requests() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let pull_request = pull_request_json(issue_json(
        13,
        &["symphony:todo"],
        "alice",
        &["alice"],
        None,
    ));

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/13")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(pull_request.to_string())
        .create_async()
        .await;

    let issues = adapter
        .fetch_issue_states_by_ids(&["13".to_string()])
        .await
        .expect("fetch_issue_states_by_ids should succeed");

    mock.assert_async().await;
    assert!(issues.is_empty(), "pull requests must be ignored");
}

#[tokio::test]
async fn test_create_comment_delegates_to_client() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let mock = server
        .mock("POST", "/repos/kata-sh/kata-mono/issues/7/comments")
        .match_body(Matcher::PartialJson(json!({ "body": "hello" })))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create_async()
        .await;

    adapter
        .create_comment("7", "hello")
        .await
        .expect("create_comment should succeed");

    mock.assert_async().await;
}

#[tokio::test]
async fn test_create_comment_preserves_structured_markdown_body() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let structured_body = "## Symphony Execution Summary\n\n**Issue:** [S01]#7\n**Status:** Done\n**Turns:** 3\n**Tokens:** 1200\n**Duration:** 2m 1s\n**Worker:** local";

    let mock = server
        .mock("POST", "/repos/kata-sh/kata-mono/issues/7/comments")
        .match_body(Matcher::PartialJson(json!({ "body": structured_body })))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create_async()
        .await;

    adapter
        .create_comment("7", structured_body)
        .await
        .expect("create_comment should preserve structured markdown");

    mock.assert_async().await;
}

#[tokio::test]
async fn test_update_issue_state_swaps_labels() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let get_mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/7")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(7, &["symphony:todo", "bug"], "alice", &["alice"], None).to_string())
        .create_async()
        .await;

    let remove_mock = server
        .mock(
            "DELETE",
            "/repos/kata-sh/kata-mono/issues/7/labels/symphony:todo",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create_async()
        .await;

    let add_mock = server
        .mock("POST", "/repos/kata-sh/kata-mono/issues/7/labels")
        .match_body(Matcher::PartialJson(
            json!({ "labels": ["symphony:in-progress"] }),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .create_async()
        .await;

    adapter
        .update_issue_state("7", "In Progress")
        .await
        .expect("update_issue_state should succeed");

    get_mock.assert_async().await;
    remove_mock.assert_async().await;
    add_mock.assert_async().await;
}

#[tokio::test]
async fn test_update_issue_state_preserves_prefixed_non_state_labels() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let get_mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/71")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            issue_json(
                71,
                &["symphony:todo", "symphony:slice", "bug"],
                "alice",
                &["alice"],
                None,
            )
            .to_string(),
        )
        .create_async()
        .await;

    let remove_todo = server
        .mock(
            "DELETE",
            "/repos/kata-sh/kata-mono/issues/71/labels/symphony:todo",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create_async()
        .await;

    let remove_slice = server
        .mock(
            "DELETE",
            "/repos/kata-sh/kata-mono/issues/71/labels/symphony:slice",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .expect(0)
        .create_async()
        .await;

    let add_in_progress = server
        .mock("POST", "/repos/kata-sh/kata-mono/issues/71/labels")
        .match_body(Matcher::PartialJson(
            json!({ "labels": ["symphony:in-progress"] }),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .create_async()
        .await;

    adapter
        .update_issue_state("71", "In Progress")
        .await
        .expect("update_issue_state should preserve non-state prefixed labels");

    get_mock.assert_async().await;
    remove_todo.assert_async().await;
    remove_slice.assert_async().await;
    add_in_progress.assert_async().await;
}

#[tokio::test]
async fn test_update_issue_state_removes_all_existing_state_labels() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let get_mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/17")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            issue_json(
                17,
                &["symphony:todo", "symphony:in-progress", "bug"],
                "alice",
                &["alice"],
                None,
            )
            .to_string(),
        )
        .create_async()
        .await;

    let remove_todo = server
        .mock(
            "DELETE",
            "/repos/kata-sh/kata-mono/issues/17/labels/symphony:todo",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create_async()
        .await;

    let remove_in_progress = server
        .mock(
            "DELETE",
            "/repos/kata-sh/kata-mono/issues/17/labels/symphony:in-progress",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create_async()
        .await;

    let add_done = server
        .mock("POST", "/repos/kata-sh/kata-mono/issues/17/labels")
        .match_body(Matcher::PartialJson(json!({ "labels": ["symphony:done"] })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .create_async()
        .await;

    adapter
        .update_issue_state("17", "Done")
        .await
        .expect("update_issue_state should succeed");

    get_mock.assert_async().await;
    remove_todo.assert_async().await;
    remove_in_progress.assert_async().await;
    add_done.assert_async().await;
}

#[tokio::test]
async fn test_update_issue_state_handles_missing_old_label() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let get_mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/8")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(8, &["bug"], "alice", &["alice"], None).to_string())
        .create_async()
        .await;

    let add_mock = server
        .mock("POST", "/repos/kata-sh/kata-mono/issues/8/labels")
        .match_body(Matcher::PartialJson(json!({ "labels": ["symphony:done"] })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .create_async()
        .await;

    adapter
        .update_issue_state("8", "Done")
        .await
        .expect("update_issue_state should succeed");

    get_mock.assert_async().await;
    add_mock.assert_async().await;
}

#[tokio::test]
async fn test_kata_phase_vocabulary_round_trip_in_label_mode() {
    for phase in KATA_PHASE_NAMES {
        let mut server = Server::new_async().await;
        let adapter = test_adapter(&server, None);

        let initial_issue = issue_json(77, &["symphony:todo"], "alice", &["alice"], None);
        let round_trip_label = format!("symphony:{}", phase_to_label(phase));
        let round_trip_issue =
            issue_json(77, &[round_trip_label.as_str()], "alice", &["alice"], None);

        let get_for_update = server
            .mock("GET", "/repos/kata-sh/kata-mono/issues/77")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(initial_issue.to_string())
            .expect(1)
            .create_async()
            .await;

        let remove_old = if phase != "Todo" {
            Some(
                server
                    .mock(
                        "DELETE",
                        "/repos/kata-sh/kata-mono/issues/77/labels/symphony:todo",
                    )
                    .with_status(200)
                    .with_header("content-type", "application/json")
                    .with_body("{}")
                    .expect(1)
                    .create_async()
                    .await,
            )
        } else {
            None
        };

        let add_new = if phase != "Todo" {
            Some(
                server
                    .mock("POST", "/repos/kata-sh/kata-mono/issues/77/labels")
                    .match_body(Matcher::PartialJson(json!({
                        "labels": [round_trip_label]
                    })))
                    .with_status(200)
                    .with_header("content-type", "application/json")
                    .with_body("[]")
                    .expect(1)
                    .create_async()
                    .await,
            )
        } else {
            None
        };

        let get_for_fetch = server
            .mock("GET", "/repos/kata-sh/kata-mono/issues/77")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(round_trip_issue.to_string())
            .expect(1)
            .create_async()
            .await;

        adapter
            .update_issue_state("77", phase)
            .await
            .expect("update_issue_state should support canonical kata phases");

        let issues = adapter
            .fetch_issue_states_by_ids(&["77".to_string()])
            .await
            .expect("fetch_issue_states_by_ids should succeed");

        get_for_update.assert_async().await;
        if let Some(mock) = remove_old {
            mock.assert_async().await;
        }
        if let Some(mock) = add_new {
            mock.assert_async().await;
        }
        get_for_fetch.assert_async().await;

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].state, phase);
    }
}

#[tokio::test]
async fn test_kata_phase_vocabulary_round_trip_in_projects_v2_mode() {
    for phase in KATA_PHASE_NAMES {
        let option_name = projects_v2_option_name_for_phase(phase);
        let mut server = Server::new_async().await;
        let adapter = test_projects_adapter(&server, None, 42);

        let fields_mock = server
            .mock("POST", "/graphql")
            .match_body(Matcher::Regex("projectV2\\(number".to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                json!({
                    "data": {
                        "user": {
                            "projectV2": {
                                "id": "project_42",
                                "field": {
                                    "id": "status_field",
                                    "options": [
                                        { "id": "opt_target", "name": option_name }
                                    ]
                                }
                            }
                        },
                        "organization": null
                    }
                })
                .to_string(),
            )
            .expect(1)
            .create_async()
            .await;

        let items_mock = server
            .mock("POST", "/graphql")
            .match_body(Matcher::Regex("fieldValueByName".to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                projects_v2_items_payload(&[project_item_node(
                    "item_7",
                    7,
                    "opt_target",
                    &option_name,
                )])
                .to_string(),
            )
            .expect(2)
            .create_async()
            .await;

        let mutation_mock = server
            .mock("POST", "/graphql")
            .match_body(Matcher::AllOf(vec![
                Matcher::Regex("updateProjectV2ItemFieldValue".to_string()),
                Matcher::PartialJson(json!({
                    "variables": {
                        "projectId": "project_42",
                        "itemId": "item_7",
                        "fieldId": "status_field",
                        "singleSelectOptionId": "opt_target"
                    }
                })),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                json!({
                    "data": {
                        "updateProjectV2ItemFieldValue": {
                            "projectV2Item": { "id": "item_7" }
                        }
                    }
                })
                .to_string(),
            )
            .expect(1)
            .create_async()
            .await;

        let issue_mock = server
            .mock("GET", "/repos/kata-sh/kata-mono/issues/7")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(issue_json(7, &["symphony:todo"], "alice", &["alice"], None).to_string())
            .expect(1)
            .create_async()
            .await;

        adapter
            .update_issue_state("7", phase)
            .await
            .expect("Projects v2 update should accept canonical kata phase names");

        let states = adapter
            .fetch_issue_states_by_ids(&["7".to_string()])
            .await
            .expect("Projects v2 state fetch should succeed");

        fields_mock.assert_async().await;
        items_mock.assert_async().await;
        mutation_mock.assert_async().await;
        issue_mock.assert_async().await;

        assert_eq!(states.len(), 1);
        assert_eq!(states[0].state, phase);
    }
}

#[tokio::test]
async fn test_issue_to_domain_enriches_kata_identifier_and_parent_reference() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let kata_issue = issue_json_with_metadata(
        42,
        "[S01] Build feature",
        Some("Context\n\n**Parent:** #10\n\nMore context"),
        &["symphony:todo", "kata:task"],
        "alice",
        &["alice"],
        Some("https://github.com/kata-sh/kata-mono/issues/42"),
    );

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!([kata_issue]).to_string())
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("fetch_candidate_issues should succeed");

    mock.assert_async().await;
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].identifier, "[S01]#42");
    assert_eq!(issues[0].parent_identifier.as_deref(), Some("#10"));
    assert_eq!(issues[0].state, "Todo");
    assert_eq!(
        issues[0].url.as_deref(),
        Some("https://github.com/kata-sh/kata-mono/issues/42")
    );
    assert!(issues[0].assigned_to_worker);
    assert!(issues[0].labels.contains(&"kata:task".to_string()));
}

#[tokio::test]
async fn test_issue_to_domain_does_not_infer_parent_from_body_without_parent_issue_url() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let issue_without_parent_metadata = json!({
        "number": 98,
        "title": "[T01] Child task",
        "body": "**Parent:** #10",
        "state": "open",
        "user": { "login": "alice" },
        "assignee": { "login": "alice" },
        "assignees": [{ "login": "alice" }],
        "labels": [{ "name": "symphony:todo", "color": "ffffff", "description": null }],
        "created_at": "2026-03-29T10:00:00Z",
        "updated_at": "2026-03-29T10:30:00Z",
        "html_url": "https://github.com/kata-sh/kata-mono/issues/98",
        "parent_issue_url": null,
        "sub_issues_summary": { "total": 0, "completed": 0, "percent_completed": 0 }
    });

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!([issue_without_parent_metadata]).to_string())
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("fetch_candidate_issues should succeed");

    mock.assert_async().await;
    assert_eq!(issues.len(), 1);
    assert!(issues[0].parent_identifier.is_none());
}

#[tokio::test]
async fn test_issue_to_domain_maps_sub_issue_summary_total_to_children_count() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let mut slice_issue = issue_json_with_metadata(
        99,
        "[S01] Slice with tasks",
        Some("Slice body"),
        &["symphony:todo", "kata:slice"],
        "alice",
        &["alice"],
        Some("https://github.com/kata-sh/kata-mono/issues/99"),
    );
    slice_issue["sub_issues_summary"] =
        json!({ "total": 7, "completed": 2, "percent_completed": 28 });

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!([slice_issue]).to_string())
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("fetch_candidate_issues should succeed");

    mock.assert_async().await;
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].identifier, "[S01]#99");
    assert_eq!(issues[0].children_count, 7);
}

#[tokio::test]
async fn test_issue_to_domain_parses_slice_task_and_milestone_kata_prefixes() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let issues_payload = json!([
        issue_json_with_metadata(
            50,
            "[S01] Slice",
            Some("Body"),
            &["symphony:todo"],
            "alice",
            &["alice"],
            None
        ),
        issue_json_with_metadata(
            51,
            "[T1] Task",
            Some("Body"),
            &["symphony:todo"],
            "alice",
            &["alice"],
            None
        ),
        issue_json_with_metadata(
            52,
            "[M001] Milestone",
            Some("Body"),
            &["symphony:todo"],
            "alice",
            &["alice"],
            None
        )
    ]);

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issues_payload.to_string())
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("fetch_candidate_issues should succeed");

    mock.assert_async().await;
    assert_eq!(issues.len(), 3);
    assert_eq!(issues[0].identifier, "[S01]#50");
    assert_eq!(issues[1].identifier, "[T1]#51");
    assert_eq!(issues[2].identifier, "[M001]#52");
}

#[tokio::test]
async fn test_issue_to_domain_preserves_hash_identifier_for_non_kata_title() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let regular_issue = issue_json_with_metadata(
        43,
        "Regular ticket title",
        Some("Part of: #10"),
        &["symphony:todo"],
        "alice",
        &["alice"],
        None,
    );

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!([regular_issue]).to_string())
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("fetch_candidate_issues should succeed");

    mock.assert_async().await;
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].identifier, "#43");
    assert_eq!(issues[0].parent_identifier.as_deref(), Some("#10"));
}

#[tokio::test]
async fn test_issue_to_domain_maps_identifier_format() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([issue_json(
                123,
                &["symphony:todo"],
                "alice",
                &["alice"],
                None
            )])
            .to_string(),
        )
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("fetch_candidate_issues should succeed");

    mock.assert_async().await;
    assert_eq!(issues[0].id, "123");
    assert_eq!(issues[0].identifier, "#123");
}

#[tokio::test]
async fn test_issue_to_domain_maps_github_url() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([issue_json(
                124,
                &["symphony:todo"],
                "alice",
                &["alice"],
                Some("https://github.com/kata-sh/kata-mono/issues/124")
            )])
            .to_string(),
        )
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("fetch_candidate_issues should succeed");

    mock.assert_async().await;
    assert_eq!(
        issues[0].url.as_deref(),
        Some("https://github.com/kata-sh/kata-mono/issues/124")
    );
}

#[tokio::test]
async fn test_issue_to_domain_extracts_state_from_label() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([issue_json(
                200,
                &["symphony:in-progress"],
                "alice",
                &["alice"],
                None
            )])
            .to_string(),
        )
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("fetch_candidate_issues should succeed");

    mock.assert_async().await;
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].state, "In Progress");
}

#[tokio::test]
async fn test_mode_detection_selects_projects_v2_when_project_number_set() {
    let server = Server::new_async().await;
    let adapter = test_projects_adapter(&server, None, 42);

    assert!(matches!(
        adapter.state_mode(),
        StateMode::ProjectsV2 { project_number, .. } if *project_number == 42
    ));
}

#[tokio::test]
async fn test_mode_detection_selects_labels_when_project_number_absent() {
    let server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    assert!(matches!(adapter.state_mode(), StateMode::Labels));
}

#[tokio::test]
async fn test_projects_v2_fetch_candidate_issues_queries_by_status() {
    let mut server = Server::new_async().await;
    let adapter = test_projects_adapter(&server, None, 42);

    let fields_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(projects_v2_status_field_payload().to_string())
        .expect(1)
        .create_async()
        .await;

    let items_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            projects_v2_items_payload(&[
                project_item_node("item_101", 101, "opt_todo", "Todo"),
                project_item_node("item_102", 102, "opt_in_progress", "In Progress"),
                project_item_node("item_103", 103, "opt_done", "Done"),
            ])
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let issue_101 = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/101")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(101, &["symphony:done"], "alice", &["alice"], None).to_string())
        .create_async()
        .await;

    let issue_102 = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/102")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(102, &["symphony:todo"], "bob", &["bob"], None).to_string())
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("projects v2 fetch_candidate_issues should succeed");

    fields_mock.assert_async().await;
    items_mock.assert_async().await;
    issue_101.assert_async().await;
    issue_102.assert_async().await;

    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].identifier, "#101");
    assert_eq!(issues[0].state, "Todo");
    assert_eq!(issues[1].identifier, "#102");
    assert_eq!(issues[1].state, "In Progress");
}

#[tokio::test]
async fn test_projects_v2_candidate_maps_resolved_blocker_status() {
    let mut server = Server::new_async().await;
    let adapter = test_projects_adapter(&server, None, 42);

    let fields_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(projects_v2_status_field_payload().to_string())
        .expect(1)
        .create_async()
        .await;

    let items_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            projects_v2_items_payload(&[
                project_item_node_with_fields(
                    "item_401",
                    401,
                    "opt_todo",
                    "Todo",
                    Some("S002"),
                    Some("[S001]#400"),
                ),
                project_item_node_with_fields(
                    "item_400",
                    400,
                    "opt_agent_review",
                    "Agent Review",
                    Some("S001"),
                    None,
                ),
            ])
            .to_string(),
        )
        .expect(2)
        .create_async()
        .await;

    let issue_mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/401")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(401, &["symphony:todo"], "alice", &["alice"], None).to_string())
        .expect(1)
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("projects v2 fetch_candidate_issues should succeed");

    fields_mock.assert_async().await;
    items_mock.assert_async().await;
    issue_mock.assert_async().await;

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].blocked_by.len(), 1);
    assert_eq!(issues[0].blocked_by[0].id.as_deref(), Some("400"));
    assert_eq!(
        issues[0].blocked_by[0].identifier.as_deref(),
        Some("S001#400")
    );
    assert_eq!(
        issues[0].blocked_by[0].state.as_deref(),
        Some("Agent Review")
    );
}

#[tokio::test]
async fn test_projects_v2_candidate_maps_terminal_blocker_status() {
    let mut server = Server::new_async().await;
    let adapter = test_projects_adapter(&server, None, 42);

    let fields_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(projects_v2_status_field_payload().to_string())
        .expect(1)
        .create_async()
        .await;

    let items_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            projects_v2_items_payload(&[
                project_item_node_with_fields(
                    "item_411",
                    411,
                    "opt_todo",
                    "Todo",
                    Some("S003"),
                    Some("#410"),
                ),
                project_item_node_with_fields(
                    "item_410",
                    410,
                    "opt_done",
                    "Done",
                    Some("S001"),
                    None,
                ),
            ])
            .to_string(),
        )
        .expect(2)
        .create_async()
        .await;

    let issue_mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/411")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(411, &["symphony:todo"], "alice", &["alice"], None).to_string())
        .expect(1)
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("projects v2 fetch_candidate_issues should succeed");

    fields_mock.assert_async().await;
    items_mock.assert_async().await;
    issue_mock.assert_async().await;

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].blocked_by.len(), 1);
    assert_eq!(issues[0].blocked_by[0].id.as_deref(), Some("410"));
    assert_eq!(
        issues[0].blocked_by[0].identifier.as_deref(),
        Some("S001#410")
    );
    assert_eq!(issues[0].blocked_by[0].state.as_deref(), Some("Done"));
}

#[tokio::test]
async fn test_projects_v2_candidate_preserves_unknown_blocker_without_issue_fetch() {
    let mut server = Server::new_async().await;
    let adapter = test_projects_adapter(&server, None, 42);

    let fields_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(projects_v2_status_field_payload().to_string())
        .expect(1)
        .create_async()
        .await;

    let items_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            projects_v2_items_payload(&[project_item_node_with_fields(
                "item_421",
                421,
                "opt_todo",
                "Todo",
                Some("S004"),
                Some("#999"),
            )])
            .to_string(),
        )
        .expect(2)
        .create_async()
        .await;

    let issue_mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/421")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(421, &["symphony:todo"], "alice", &["alice"], None).to_string())
        .expect(1)
        .create_async()
        .await;

    let unknown_issue_mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/999")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(999, &["symphony:todo"], "alice", &["alice"], None).to_string())
        .expect(0)
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("projects v2 fetch_candidate_issues should succeed");

    fields_mock.assert_async().await;
    items_mock.assert_async().await;
    issue_mock.assert_async().await;
    unknown_issue_mock.assert_async().await;

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].blocked_by.len(), 1);
    assert_eq!(issues[0].blocked_by[0].id.as_deref(), Some("999"));
    assert_eq!(issues[0].blocked_by[0].identifier.as_deref(), Some("#999"));
    assert_eq!(issues[0].blocked_by[0].state, None);
}

#[tokio::test]
async fn test_projects_v2_candidate_empty_and_absent_blocked_by_yields_empty_blocker_list() {
    let mut server = Server::new_async().await;
    let adapter = test_projects_adapter(&server, None, 42);

    let fields_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(projects_v2_status_field_payload().to_string())
        .expect(1)
        .create_async()
        .await;

    let items_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            projects_v2_items_payload(&[
                project_item_node_with_fields(
                    "item_431",
                    431,
                    "opt_todo",
                    "Todo",
                    Some("S005"),
                    Some("   "),
                ),
                project_item_node("item_432", 432, "opt_in_progress", "In Progress"),
                project_item_node_without_dependency_fields(
                    "item_433",
                    433,
                    "opt_todo",
                    "Todo",
                    Some("S006"),
                ),
            ])
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let issue_431 = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/431")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(431, &["symphony:todo"], "alice", &["alice"], None).to_string())
        .expect(1)
        .create_async()
        .await;

    let issue_432 = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/432")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(432, &["symphony:todo"], "alice", &["alice"], None).to_string())
        .expect(1)
        .create_async()
        .await;

    let issue_433 = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/433")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(433, &["symphony:todo"], "alice", &["alice"], None).to_string())
        .expect(1)
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("projects v2 fetch_candidate_issues should succeed");

    fields_mock.assert_async().await;
    items_mock.assert_async().await;
    issue_431.assert_async().await;
    issue_432.assert_async().await;
    issue_433.assert_async().await;

    assert_eq!(issues.len(), 3);
    assert!(issues.iter().all(|issue| issue.blocked_by.is_empty()));
}

#[tokio::test]
async fn test_projects_v2_fetch_candidate_issues_skips_closed_issues() {
    let mut server = Server::new_async().await;
    let adapter = test_projects_adapter(&server, None, 42);

    let fields_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(projects_v2_status_field_payload().to_string())
        .expect(1)
        .create_async()
        .await;

    let items_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            projects_v2_items_payload(&[
                project_item_node("item_301", 301, "opt_todo", "Todo"),
                project_item_node("item_302", 302, "opt_in_progress", "In Progress"),
            ])
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let issue_open = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/301")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(301, &["symphony:todo"], "alice", &["alice"], None).to_string())
        .create_async()
        .await;

    let issue_closed = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/302")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            issue_state_json(
                issue_json(302, &["symphony:todo"], "bob", &["bob"], None),
                "closed",
            )
            .to_string(),
        )
        .create_async()
        .await;

    let issues = adapter
        .fetch_candidate_issues()
        .await
        .expect("projects v2 fetch_candidate_issues should succeed");

    fields_mock.assert_async().await;
    items_mock.assert_async().await;
    issue_open.assert_async().await;
    issue_closed.assert_async().await;

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].identifier, "#301");
}

#[tokio::test]
async fn test_projects_v2_fetch_issues_by_states() {
    let mut server = Server::new_async().await;
    let adapter = test_projects_adapter(&server, None, 42);

    let fields_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(projects_v2_status_field_payload().to_string())
        .expect(1)
        .create_async()
        .await;

    let items_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            projects_v2_items_payload(&[
                project_item_node("item_201", 201, "opt_todo", "Todo"),
                project_item_node("item_202", 202, "opt_done", "Done"),
            ])
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let issue_202 = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/202")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(202, &["symphony:todo"], "alice", &["alice"], None).to_string())
        .create_async()
        .await;

    let issues = adapter
        .fetch_issues_by_states(&["Done".to_string()])
        .await
        .expect("projects v2 fetch_issues_by_states should succeed");

    fields_mock.assert_async().await;
    items_mock.assert_async().await;
    issue_202.assert_async().await;

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].identifier, "#202");
    assert_eq!(issues[0].state, "Done");
}

#[tokio::test]
async fn test_projects_v2_update_issue_state_mutates_status_field() {
    let mut server = Server::new_async().await;
    let adapter = test_projects_adapter(&server, None, 42);

    let fields_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(projects_v2_status_field_payload().to_string())
        .expect(1)
        .create_async()
        .await;

    let items_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            projects_v2_items_payload(&[project_item_node("item_7", 7, "opt_todo", "Todo")])
                .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let mutation_mock = server
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
        .with_body(
            json!({
                "data": {
                    "updateProjectV2ItemFieldValue": {
                        "projectV2Item": { "id": "item_7" }
                    }
                }
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    adapter
        .update_issue_state("7", "Done")
        .await
        .expect("projects v2 update_issue_state should succeed");

    fields_mock.assert_async().await;
    items_mock.assert_async().await;
    mutation_mock.assert_async().await;
}

#[tokio::test]
async fn test_projects_v2_update_issue_state_unknown_status_errors() {
    let mut server = Server::new_async().await;
    let adapter = test_projects_adapter(&server, None, 42);

    let fields_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(projects_v2_status_field_payload().to_string())
        .expect(1)
        .create_async()
        .await;

    let err = adapter
        .update_issue_state("7", "Blocked")
        .await
        .expect_err("unknown status should fail");

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
async fn test_projects_v2_update_issue_state_issue_not_on_board_errors() {
    let mut server = Server::new_async().await;
    let adapter = test_projects_adapter(&server, None, 42);

    let fields_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(projects_v2_status_field_payload().to_string())
        .expect(1)
        .create_async()
        .await;

    let items_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            projects_v2_items_payload(&[project_item_node("item_99", 99, "opt_todo", "Todo")])
                .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let err = adapter
        .update_issue_state("7", "Done")
        .await
        .expect_err("missing project item should fail");

    fields_mock.assert_async().await;
    items_mock.assert_async().await;
    match err {
        SymphonyError::GithubProjectsV2Error(message) => {
            assert!(message.contains("issue #7 is not on project board #42"));
        }
        other => panic!("expected GithubProjectsV2Error, got {other:?}"),
    }
}

#[tokio::test]
async fn test_projects_v2_fetch_issue_states_by_ids_reads_board_status() {
    let mut server = Server::new_async().await;
    let adapter = test_projects_adapter(&server, None, 42);

    let fields_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(projects_v2_status_field_payload().to_string())
        .expect(1)
        .create_async()
        .await;

    let items_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            projects_v2_items_payload(&[project_item_node(
                "item_55",
                55,
                "opt_in_progress",
                "In Progress",
            )])
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let issue_mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/55")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(55, &["symphony:todo"], "alice", &["alice"], None).to_string())
        .expect(1)
        .create_async()
        .await;

    let issues = adapter
        .fetch_issue_states_by_ids(&["55".to_string()])
        .await
        .expect("projects v2 fetch_issue_states_by_ids should succeed");

    fields_mock.assert_async().await;
    items_mock.assert_async().await;
    issue_mock.assert_async().await;

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].identifier, "#55");
    assert_eq!(issues[0].state, "In Progress");
}

#[tokio::test]
async fn test_contract_matrix_label_mode_all_methods() {
    let mut server = Server::new_async().await;
    let adapter = test_adapter(&server, None);

    let issues_list_mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([issue_json(7, &["symphony:todo"], "alice", &["alice"], None)]).to_string(),
        )
        .expect(2)
        .create_async()
        .await;

    let issue_by_id_mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/7")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([issue_json(7, &["symphony:todo"], "alice", &["alice"], None)])[0].to_string(),
        )
        .expect(2)
        .create_async()
        .await;

    let comment_mock = server
        .mock("POST", "/repos/kata-sh/kata-mono/issues/7/comments")
        .match_body(Matcher::PartialJson(json!({ "body": "contract comment" })))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .expect(1)
        .create_async()
        .await;

    let remove_mock = server
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

    let add_mock = server
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

    run_tracker_contract(&adapter, "Todo", "In Progress").await;

    issues_list_mock.assert_async().await;
    issue_by_id_mock.assert_async().await;
    comment_mock.assert_async().await;
    remove_mock.assert_async().await;
    add_mock.assert_async().await;
}

#[tokio::test]
async fn test_contract_matrix_projects_v2_mode_all_methods() {
    let mut server = Server::new_async().await;
    let adapter = test_projects_adapter(&server, None, 42);

    let fields_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(projects_v2_status_field_payload().to_string())
        .expect(1)
        .create_async()
        .await;

    let items_mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("fieldValueByName".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            projects_v2_items_payload(&[project_item_node("item_7", 7, "opt_todo", "Todo")])
                .to_string(),
        )
        .expect(4)
        .create_async()
        .await;

    let issue_mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/7")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(7, &["symphony:done"], "alice", &["alice"], None).to_string())
        .expect(3)
        .create_async()
        .await;

    let comment_mock = server
        .mock("POST", "/repos/kata-sh/kata-mono/issues/7/comments")
        .match_body(Matcher::PartialJson(json!({ "body": "contract comment" })))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .expect(1)
        .create_async()
        .await;

    let mutation_mock = server
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
        .with_body(
            json!({
                "data": {
                    "updateProjectV2ItemFieldValue": {
                        "projectV2Item": { "id": "item_7" }
                    }
                }
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    run_tracker_contract(&adapter, "Todo", "Done").await;

    fields_mock.assert_async().await;
    items_mock.assert_async().await;
    issue_mock.assert_async().await;
    comment_mock.assert_async().await;
    mutation_mock.assert_async().await;
}
