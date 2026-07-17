use mockito::{Matcher, Server, ServerGuard};
use serde_json::json;
use symphony::error::SymphonyError;
use symphony::github::client::GithubClient;
use symphony::github::projects_v2::ProjectsV2Client;
use symphony::triage::domain::TriageRoutesConfig;
use symphony::triage::intake::{GithubTriageIntake, TriageIntakePort};

fn github_client(server: &ServerGuard) -> GithubClient {
    GithubClient::with_base_url(
        "test-token",
        "kata-sh",
        "kata-mono",
        "symphony",
        server.url(),
    )
}

fn issue_json(number: u64, labels: &[&str]) -> serde_json::Value {
    json!({
        "number": number,
        "title": format!("Issue {number}"),
        "body": "body",
        "state": "open",
        "user": { "login": "octocat" },
        "assignees": [{ "login": "octocat" }],
        "labels": labels.iter().map(|name| json!({ "name": name })).collect::<Vec<_>>(),
        "milestone": { "number": 1, "title": "M1" },
        "created_at": "2026-07-16T10:00:00Z",
        "updated_at": "2026-07-16T11:00:00Z"
    })
}

fn project_items_page(issue_numbers: &[u64], has_next: bool, cursor: Option<&str>) -> String {
    let nodes: Vec<_> = issue_numbers
        .iter()
        .map(|number| {
            json!({
                "id": format!("item-{number}"),
                "content": {
                    "number": number,
                    "blockedBy": { "nodes": [] }
                },
                "status": { "name": "Todo", "optionId": "opt_todo" },
                "kataId": null
            })
        })
        .collect();

    json!({
        "data": {
            "node": {
                "items": {
                    "nodes": nodes,
                    "pageInfo": {
                        "hasNextPage": has_next,
                        "endCursor": cursor
                    }
                }
            }
        }
    })
    .to_string()
}

#[tokio::test]
async fn list_issues_paginated_fails_when_truncated() {
    let mut server = Server::new_async().await;
    let client = github_client(&server);

    let page1 = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("labels".into(), "needs-triage".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header(
            "link",
            &format!(
                "<{}/repos/kata-sh/kata-mono/issues?page=2>; rel=\"next\"",
                server.url()
            ),
        )
        .with_body(json!([issue_json(1, &["needs-triage"])]).to_string())
        .create_async()
        .await;

    let err = client
        .list_issues_paginated("open", &["needs-triage".to_string()], 1)
        .await
        .expect_err("truncated pagination must fail");

    page1.assert_async().await;
    match err {
        SymphonyError::GithubApiRequest(message) => {
            assert!(message.contains("truncated"));
            assert!(message.contains("max_pages=1"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn list_comments_paginated_returns_user_and_fails_when_truncated() {
    let mut server = Server::new_async().await;
    let client = github_client(&server);

    let page1 = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/7/comments")
        .match_query(Matcher::UrlEncoded("per_page".into(), "100".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header(
            "link",
            &format!(
                "<{}/repos/kata-sh/kata-mono/issues/7/comments?page=2>; rel=\"next\"",
                server.url()
            ),
        )
        .with_body(
            json!([{
                "id": 11,
                "user": { "login": "alice" },
                "body": "hello",
                "created_at": "2026-07-16T10:00:00Z",
                "updated_at": "2026-07-16T10:00:00Z"
            }])
            .to_string(),
        )
        .create_async()
        .await;

    let err = client
        .list_comments_paginated(7, 1)
        .await
        .expect_err("truncated comments must fail");
    page1.assert_async().await;
    assert!(matches!(err, SymphonyError::GithubApiRequest(_)));
}

#[tokio::test]
async fn get_authenticated_user_returns_login() {
    let mut server = Server::new_async().await;
    let client = github_client(&server);
    let mock = server
        .mock("GET", "/user")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "login": "symphony-bot" }).to_string())
        .create_async()
        .await;

    let user = client.get_authenticated_user().await.unwrap();
    mock.assert_async().await;
    assert_eq!(user.login, "symphony-bot");
}

#[tokio::test]
async fn intake_intersects_project_membership_without_active_state() {
    let mut server = Server::new_async().await;
    let github = github_client(&server);
    let projects = ProjectsV2Client::new(github.clone());
    let intake = GithubTriageIntake::new(
        github,
        projects,
        "kata-sh",
        "kata-mono",
        42,
        "github.com",
        TriageRoutesConfig::default().managed_labels(),
    );

    let issues = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("labels".into(), "needs-triage".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([
                issue_json(10, &["needs-triage", "docs"]),
                issue_json(11, &["needs-triage"])
            ])
            .to_string(),
        )
        .create_async()
        .await;

    let fields = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("projectV2\\(number".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "data": {
                    "user": null,
                    "organization": {
                        "projectV2": {
                            "id": "project_42",
                            "field": {
                                "id": "status_field",
                                "options": [{ "id": "opt_todo", "name": "Todo" }]
                            }
                        }
                    }
                }
            })
            .to_string(),
        )
        .create_async()
        .await;

    let items = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("node\\(id: \\$projectId\\)".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(project_items_page(&[10], false, None))
        .create_async()
        .await;

    let comments_10 = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/10/comments")
        .match_query(Matcher::UrlEncoded("per_page".into(), "100".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([{
                "id": 100,
                "user": { "login": "alice" },
                "body": "note",
                "created_at": "2026-07-16T10:05:00Z",
                "updated_at": "2026-07-16T10:05:00Z"
            }])
            .to_string(),
        )
        .create_async()
        .await;

    let comments_11 = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/11/comments")
        .match_query(Matcher::UrlEncoded("per_page".into(), "100".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .create_async()
        .await;

    let results = intake
        .fetch_intake_issues("needs-triage", 10)
        .await
        .expect("intake should succeed");

    issues.assert_async().await;
    fields.assert_async().await;
    items.assert_async().await;
    comments_10.assert_async().await;
    comments_11.assert_async().await;

    assert_eq!(results.len(), 2);
    let on_project = results
        .iter()
        .find(|issue| issue.issue_number == 10)
        .unwrap();
    let off_project = results
        .iter()
        .find(|issue| issue.issue_number == 11)
        .unwrap();
    assert!(on_project.in_project);
    assert!(!off_project.in_project);
    assert_eq!(on_project.non_managed_labels, vec!["docs".to_string()]);
    assert_eq!(on_project.comments.len(), 1);
    assert_eq!(on_project.comments[0].author_login, "alice");
    assert_eq!(on_project.repository, "kata-sh/kata-mono");
}

#[tokio::test]
async fn query_all_items_fails_when_truncated() {
    let mut server = Server::new_async().await;
    let github = github_client(&server);
    let projects = ProjectsV2Client::new(github);

    let mock = server
        .mock("POST", "/graphql")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(project_items_page(&[1], true, Some("cursor-1")))
        .create_async()
        .await;

    let err = projects
        .query_all_items("project_42", 1)
        .await
        .expect_err("truncated project items must fail");
    mock.assert_async().await;
    match err {
        SymphonyError::GithubProjectsV2Error(message) => {
            assert!(message.contains("truncated"));
            assert!(message.contains("max_pages=1"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
