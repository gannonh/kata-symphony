use std::time::{Duration, Instant};

use chrono::Utc;
use mockito::{Matcher, Server, ServerGuard};
use serde_json::json;
use symphony::error::SymphonyError;
use symphony::github::client::{GithubClient, GithubPullRequestReviewComment};

fn test_client(server: &ServerGuard) -> GithubClient {
    GithubClient::with_base_url(
        "test-token",
        "kata-sh",
        "kata-mono",
        "symphony",
        server.url(),
    )
}

fn issue_json(number: u64, label: &str) -> serde_json::Value {
    json!({
        "number": number,
        "title": format!("Issue {number}"),
        "body": "body",
        "state": "open",
        "user": { "login": "octocat" },
        "assignee": { "login": "octocat" },
        "assignees": [{ "login": "octocat" }],
        "labels": [{ "name": label, "color": "ffffff", "description": null }],
        "created_at": "2026-03-29T10:00:00Z",
        "updated_at": "2026-03-29T10:30:00Z",
        "html_url": format!("https://github.com/kata-sh/kata-mono/issues/{number}")
    })
}

#[tokio::test]
async fn test_list_issues_returns_parsed_issues() {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "open".into()),
            Matcher::UrlEncoded("labels".into(), "symphony:todo".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .match_header("authorization", "Bearer test-token")
        .match_header("accept", "application/vnd.github+json")
        .match_header("user-agent", "symphony")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!([issue_json(123, "symphony:todo")]).to_string())
        .create_async()
        .await;

    let issues = client
        .list_issues("open", &["symphony:todo".to_string()])
        .await
        .expect("list_issues should succeed");

    mock.assert_async().await;
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].number, 123);
    assert_eq!(issues[0].title, "Issue 123");
    assert_eq!(issues[0].labels[0].name, "symphony:todo");
}

#[tokio::test]
async fn test_get_issue_returns_single_issue() {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/42")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(issue_json(42, "symphony:todo").to_string())
        .create_async()
        .await;

    let issue = client
        .get_issue(42)
        .await
        .expect("get_issue should succeed");

    mock.assert_async().await;
    assert_eq!(issue.number, 42);
    assert_eq!(issue.title, "Issue 42");
}

#[tokio::test]
async fn test_create_comment_posts_body() {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let mock = server
        .mock("POST", "/repos/kata-sh/kata-mono/issues/42/comments")
        .match_header("content-type", Matcher::Regex("application/json".into()))
        .match_body(Matcher::PartialJson(
            json!({ "body": "hello from symphony" }),
        ))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create_async()
        .await;

    client
        .create_comment(42, "hello from symphony")
        .await
        .expect("create_comment should succeed");

    mock.assert_async().await;
}

#[tokio::test]
async fn test_add_label_sends_correct_payload() {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let mock = server
        .mock("POST", "/repos/kata-sh/kata-mono/issues/42/labels")
        .match_body(Matcher::PartialJson(
            json!({ "labels": ["symphony:in-progress"] }),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("[]")
        .create_async()
        .await;

    client
        .add_label(42, "symphony:in-progress")
        .await
        .expect("add_label should succeed");

    mock.assert_async().await;
}

#[tokio::test]
async fn test_remove_label_sends_delete() {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let mock = server
        .mock(
            "DELETE",
            "/repos/kata-sh/kata-mono/issues/42/labels/in%20progress%2Fnow",
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create_async()
        .await;

    client
        .remove_label(42, "in progress/now")
        .await
        .expect("remove_label should succeed");

    mock.assert_async().await;
}

#[tokio::test]
async fn test_list_labels_returns_labels() {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/labels")
        .match_query(Matcher::UrlEncoded("per_page".into(), "100".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([
                { "name": "symphony:todo", "color": "ffffff", "description": "Todo state" },
                { "name": "symphony:done", "color": "000000", "description": null }
            ])
            .to_string(),
        )
        .create_async()
        .await;

    let labels = client
        .list_labels()
        .await
        .expect("list_labels should succeed");

    mock.assert_async().await;
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[0].name, "symphony:todo");
    assert_eq!(labels[1].name, "symphony:done");
}

#[tokio::test]
async fn test_rate_limit_headers_parsed_and_stored() {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let reset_ts = (Utc::now().timestamp() + 300).to_string();
    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/1")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header("x-ratelimit-remaining", "9")
        .with_header("x-ratelimit-limit", "100")
        .with_header("x-ratelimit-reset", &reset_ts)
        .with_body(issue_json(1, "symphony:todo").to_string())
        .create_async()
        .await;

    client.get_issue(1).await.expect("request should succeed");
    mock.assert_async().await;

    let state = client.rate_limit_state().await;
    assert_eq!(state.remaining, 9);
    assert_eq!(state.limit, 100);
    assert_eq!(state.reset.timestamp(), reset_ts.parse::<i64>().unwrap());
}

#[tokio::test]
async fn test_rate_limit_warning_logged_at_10_percent() {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let reset_ts = (Utc::now().timestamp() + 300).to_string();
    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/2")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header("x-ratelimit-remaining", "10")
        .with_header("x-ratelimit-limit", "100")
        .with_header("x-ratelimit-reset", &reset_ts)
        .with_body(issue_json(2, "symphony:todo").to_string())
        .create_async()
        .await;

    client.get_issue(2).await.expect("request should succeed");
    mock.assert_async().await;

    let state = client.rate_limit_state().await;
    assert_eq!(state.remaining, 10);
    assert_eq!(state.limit, 100);
}

#[tokio::test]
async fn test_rate_limit_exhausted_delays_request() {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let reset_epoch = Utc::now().timestamp() + 5;
    let reset_ts = reset_epoch.to_string();
    let first = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/10")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header("x-ratelimit-remaining", "0")
        .with_header("x-ratelimit-limit", "100")
        .with_header("x-ratelimit-reset", &reset_ts)
        .with_body(issue_json(10, "symphony:todo").to_string())
        .create_async()
        .await;

    let second = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/11")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_header("x-ratelimit-remaining", "99")
        .with_header("x-ratelimit-limit", "100")
        .with_header("x-ratelimit-reset", &reset_ts)
        .with_body(issue_json(11, "symphony:todo").to_string())
        .create_async()
        .await;

    client
        .get_issue(10)
        .await
        .expect("first request should succeed");
    first.assert_async().await;

    let started_wall = Utc::now().timestamp();
    let started = Instant::now();
    client
        .get_issue(11)
        .await
        .expect("second request should succeed after delay");
    second.assert_async().await;

    let expected_delay_secs = (reset_epoch - started_wall).max(0) as u64;
    let minimum_expected_secs = expected_delay_secs.saturating_sub(1);

    assert!(
        started.elapsed() >= Duration::from_secs(minimum_expected_secs),
        "rate-limit exhaustion should delay subsequent request close to reset window"
    );
}

#[tokio::test]
async fn test_401_returns_auth_error() {
    assert_error_status(401, "Unauthorized").await;
}

#[tokio::test]
async fn test_403_returns_rate_limit_error() {
    assert_error_status(403, "API rate limit exceeded for user").await;
}

#[tokio::test]
async fn test_404_returns_not_found_error() {
    assert_error_status(404, "Not Found").await;
}

async fn assert_error_status(status: usize, message: &str) {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let repeated_body = message.repeat(40);
    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/issues/404")
        .with_status(status)
        .with_header("content-type", "application/json")
        .with_body(repeated_body)
        .create_async()
        .await;

    let err = client
        .get_issue(404)
        .await
        .expect_err("request should fail with mocked error status");

    mock.assert_async().await;
    match err {
        SymphonyError::GithubApiStatus {
            status: actual,
            message,
        } => {
            assert_eq!(actual, status as u16);
            assert!(message.len() <= 200, "error message should be truncated");
            assert!(!message.is_empty(), "error message should include preview");
        }
        other => panic!("expected GithubApiStatus error, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_list_pull_requests_filters_head_base_state() {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/pulls")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("state".into(), "all".into()),
            Matcher::UrlEncoded("head".into(), "kata-sh:symphony/42".into()),
            Matcher::UrlEncoded("base".into(), "main".into()),
            Matcher::UrlEncoded("per_page".into(), "100".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([{
                "number": 9,
                "html_url": "https://github.com/kata-sh/kata-mono/pull/9",
                "draft": true,
                "state": "open",
                "title": "impl",
                "head": { "ref": "symphony/42", "sha": "abc123" },
                "base": { "ref": "main", "sha": "def456" },
                "user": { "login": "bot" },
                "body": "<!-- symphony:implementation-pr:intent -->"
            }])
            .to_string(),
        )
        .create_async()
        .await;

    let prs = client
        .list_pull_requests("all", Some("kata-sh:symphony/42"), Some("main"), 2)
        .await
        .expect("list_pull_requests");
    mock.assert_async().await;
    assert_eq!(prs.len(), 1);
    assert_eq!(prs[0].number, 9);
    assert!(prs[0].draft);
    assert_eq!(prs[0].head.ref_name, "symphony/42");
    assert_eq!(prs[0].head.sha, "abc123");
}

#[tokio::test]
async fn test_list_pull_request_reviews_returns_author_marker_and_commit() {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let mock = server
        .mock("GET", "/repos/kata-sh/kata-mono/pulls/11/reviews")
        .match_query(Matcher::UrlEncoded("per_page".into(), "100".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!([{
                "id": 73,
                "user": { "login": "symphony-bot" },
                "body": "<!-- symphony:review:intent-7 -->\\nSummary",
                "commit_id": "head-sha",
                "state": "COMMENTED",
                "html_url": "https://github.com/kata-sh/kata-mono/pull/11#pullrequestreview-73",
                "submitted_at": "2026-07-30T10:00:00Z"
            }])
            .to_string(),
        )
        .create_async()
        .await;

    let reviews = client
        .list_pull_request_reviews(11, 2)
        .await
        .expect("list_pull_request_reviews");
    mock.assert_async().await;
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].id, 73);
    assert_eq!(reviews[0].user.as_ref().unwrap().login, "symphony-bot");
    assert!(reviews[0]
        .body
        .as_deref()
        .unwrap()
        .contains("<!-- symphony:review:intent-7 -->"));
    assert_eq!(reviews[0].commit_id.as_deref(), Some("head-sha"));
}

#[tokio::test]
async fn test_probe_pull_request_review_permission_accepts_unprocessable_entity() {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let mock = server
        .mock("POST", "/repos/kata-sh/kata-mono/pulls/11/reviews")
        .match_body(Matcher::PartialJson(json!({ "event": "INVALID" })))
        .with_status(422)
        .with_body("invalid review event")
        .create_async()
        .await;

    client
        .probe_pull_request_review_permission(11)
        .await
        .expect("HTTP 422 proves review endpoint authorization");
    mock.assert_async().await;
}

#[tokio::test]
async fn test_probe_pull_request_review_permission_rejects_forbidden() {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let mock = server
        .mock("POST", "/repos/kata-sh/kata-mono/pulls/11/reviews")
        .with_status(403)
        .with_body("forbidden")
        .create_async()
        .await;

    let error = client
        .probe_pull_request_review_permission(11)
        .await
        .expect_err("HTTP 403 must fail the permission probe");
    mock.assert_async().await;
    assert!(matches!(
        error,
        SymphonyError::GithubApiStatus { status: 403, .. }
    ));
}

#[tokio::test]
async fn test_probe_pull_request_review_permission_rejects_unexpected_success() {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let mock = server
        .mock("POST", "/repos/kata-sh/kata-mono/pulls/11/reviews")
        .with_status(201)
        .with_body("created")
        .create_async()
        .await;

    let error = client
        .probe_pull_request_review_permission(11)
        .await
        .expect_err("successful review creation must fail the invalid-event probe");
    mock.assert_async().await;
    assert!(matches!(
        error,
        SymphonyError::GithubApiStatus { status: 201, .. }
    ));
}

#[tokio::test]
async fn test_create_pull_request_review_serializes_multiline_anchors_and_omits_single_line_fields()
{
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let mock = server
        .mock("POST", "/repos/kata-sh/kata-mono/pulls/11/reviews")
        .match_header("content-type", Matcher::Regex("application/json".into()))
        .match_body(Matcher::Json(json!({
            "commit_id": "head-sha",
            "body": "<!-- symphony:review:intent-7 -->\\nSummary",
            "event": "COMMENT",
            "comments": [
                {
                    "path": "src/lib.rs",
                    "line": 27,
                    "side": "RIGHT",
                    "start_line": 24,
                    "start_side": "RIGHT",
                    "body": "Handle this error before publishing."
                },
                {
                    "path": "src/main.rs",
                    "line": 10,
                    "side": "RIGHT",
                    "body": "Handle this error before publishing."
                }
            ]
        })))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "id": 73,
                "user": { "login": "symphony-bot" },
                "body": "<!-- symphony:review:intent-7 -->\\nSummary",
                "commit_id": "head-sha",
                "state": "COMMENTED",
                "html_url": "https://github.com/kata-sh/kata-mono/pull/11#pullrequestreview-73"
            })
            .to_string(),
        )
        .create_async()
        .await;

    let review = client
        .create_pull_request_review(
            11,
            "head-sha",
            "<!-- symphony:review:intent-7 -->\\nSummary",
            &[
                GithubPullRequestReviewComment {
                    path: "src/lib.rs".to_string(),
                    line: 27,
                    side: "RIGHT".to_string(),
                    start_line: Some(24),
                    start_side: Some("RIGHT".to_string()),
                    body: "Handle this error before publishing.".to_string(),
                },
                GithubPullRequestReviewComment {
                    path: "src/main.rs".to_string(),
                    line: 10,
                    side: "RIGHT".to_string(),
                    start_line: None,
                    start_side: None,
                    body: "Handle this error before publishing.".to_string(),
                },
            ],
        )
        .await
        .expect("create_pull_request_review");
    mock.assert_async().await;
    assert_eq!(review.id, 73);
    assert_eq!(review.commit_id.as_deref(), Some("head-sha"));
    assert_eq!(review.user.as_ref().unwrap().login, "symphony-bot");
    assert_eq!(
        review.html_url.as_deref(),
        Some("https://github.com/kata-sh/kata-mono/pull/11#pullrequestreview-73")
    );
}

#[tokio::test]
async fn test_create_pull_request_posts_draft() {
    let mut server = Server::new_async().await;
    let client = test_client(&server);

    let mock = server
        .mock("POST", "/repos/kata-sh/kata-mono/pulls")
        .match_header("content-type", Matcher::Regex("application/json".into()))
        .match_body(Matcher::PartialJson(json!({
            "title": "Symphony implementation",
            "body": "Closes #1",
            "head": "symphony/1",
            "base": "main",
            "draft": true
        })))
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "number": 11,
                "html_url": "https://github.com/kata-sh/kata-mono/pull/11",
                "draft": true,
                "state": "open",
                "title": "Symphony implementation",
                "head": { "ref": "symphony/1", "sha": "aaa" },
                "base": { "ref": "main", "sha": "bbb" },
                "body": "Closes #1"
            })
            .to_string(),
        )
        .create_async()
        .await;

    let pr = client
        .create_pull_request(
            "Symphony implementation",
            "Closes #1",
            "symphony/1",
            "main",
            true,
        )
        .await
        .expect("create_pull_request");
    mock.assert_async().await;
    assert_eq!(pr.number, 11);
    assert!(pr.draft);
}
