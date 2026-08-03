---
type: Plan
title: Symphony Linear Execution And Backend Uat Implementation Plan
description: "Archived implementation plan: Symphony Linear Execution And Backend Uat Implementation Plan."
tags: [archive]
timestamp: 2026-07-15T20:00:00Z
---

# Symphony Linear Execution And Backend UAT Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring Symphony Linear execution to the same issue lifecycle and direct helper surface as the GitHub backend, then add a real-backend UAT skill for the Symphony helper contract.

**Architecture:** Keep `WORKFLOW.md` as Symphony's execution config and keep the orchestrator behind the existing `TrackerAdapter` boundary. Add Linear helper parity by deriving issue, child, comment, document, and follow-up context from the issue itself and its tracker project, with no new milestone or planning metadata dependency.

**Tech Stack:** Rust, Tokio, reqwest, serde, mockito, existing Symphony tracker adapters, Node.js skill runner scripts.

---

## Source Material

- Spec: `docs/superpowers/specs/2026-05-07-symphony-linear-execution-and-backend-uat-design.md`
- Current helper implementation: `apps/symphony/src/main.rs`
- Linear client and adapter: `apps/symphony/src/linear/client.rs`, `apps/symphony/src/linear/adapter.rs`
- GitHub parity reference: `apps/symphony/src/github/adapter.rs`, helper code in `apps/symphony/src/main.rs`
- Prompt contract: `apps/symphony/prompts/system.md`, `apps/symphony/prompts/in-progress.md`, `apps/symphony/prompts/rework.md`, `apps/symphony/prompts/agent-review.md`, `apps/symphony/prompts/merging.md`
- Existing sibling skill: `.agents/skills/kata-backend-uat`

## Scope Boundaries

- Do not add Linear project milestone fields, team fields, or Kata planning metadata to `TrackerConfig`.
- Do not change Kata CLI backend behavior.
- Do not run a full live worker dispatch cycle in the UAT skill.
- Do keep child Linear issues from dispatching independently through the existing `parent_identifier` gate.
- Do support Linear through the same Symphony lifecycle concepts GitHub supports: candidate reads, issue reads, child context, comments, state transitions, follow-up issues, and marker comment documents.
- Do not modify prompts; backends should remain fully abstracted.
- Do treat any required prompt change for Linear support as an abstraction leak. Fix the helper or adapter contract instead.
- Do keep GitHub PR helper operations GitHub-scoped. They are code-host operations, not tracker backend operations, and may still apply when the tracker backend is Linear.

## File Structure

- Create `apps/symphony/src/helper.rs`: testable helper contract, shared input parsing, output envelopes, GitHub and Linear operation routing.
- Modify `apps/symphony/src/main.rs`: remove helper implementation details and delegate the CLI subcommand to `symphony::helper`.
- Modify `apps/symphony/src/lib.rs`: export the helper module.
- Modify `apps/symphony/src/linear/client.rs`: add Linear helper queries and mutations for issue detail, children, comments, marker upsert, document comments, and follow-up creation.
- Modify `apps/symphony/src/linear/adapter.rs`: expose focused helper-facing methods while preserving `TrackerAdapter`.
- Modify `apps/symphony/tests/linear_client_tests.rs`: mock Linear GraphQL coverage for helper-facing methods.
- Create `apps/symphony/tests/linear_helper_tests.rs`: backend-neutral helper contract tests for Linear.
- Modify `apps/symphony/tests/orchestrator_tests.rs`: add Linear-shaped child and blocker dispatch regressions where existing generic tests do not lock the Linear shape.
- Modify `apps/symphony/tests/backend_neutral_worker_contract_tests.rs` and `apps/symphony/tests/workflow_config_tests.rs`: guard prompt/config boundaries and GitHub PR helper scope.
- Create `.agents/skills/symphony-backend-uat`: sibling skill for real backend helper UAT.

## Task 1: Lock The Existing Symphony Scope

**Files:**

- Modify: `docs/superpowers/specs/2026-05-07-symphony-linear-execution-and-backend-uat-design.md`
- Modify: `apps/symphony/tests/workflow_config_tests.rs`
- Modify: `apps/symphony/docs/WORKFLOW-REFERENCE.md`

- [ ] **Step 1: Confirm the spec has no milestone-driven Symphony requirements**

Run:

```bash
rg -n "project_milestone|project milestone|team_key|milestone filtering|Linear team" docs/superpowers/specs/2026-05-07-symphony-linear-execution-and-backend-uat-design.md
```

Expected: no output.

- [ ] **Step 2: Add a config regression test for the existing Linear fields**

Append this test near the Linear config tests in `apps/symphony/tests/workflow_config_tests.rs`:

```rust
#[test]
fn test_linear_lifecycle_config_uses_existing_tracker_fields() {
    let content = r#"---
tracker:
  kind: linear
  api_key: test-key
  endpoint: http://127.0.0.1:4010/graphql
  project_slug: kata-project
  workspace_slug: kata-sh
  active_states:
    - Todo
    - In Progress
  terminal_states:
    - Done
  exclude_labels:
    - kata:task
---
Prompt
"#;

    let def = symphony::workflow::parse_workflow_str(content).expect("workflow parses");
    let config = symphony::config::from_workflow(&def).expect("config loads");

    assert_eq!(config.tracker.kind.as_deref(), Some("linear"));
    assert_eq!(config.tracker.project_slug.as_deref(), Some("kata-project"));
    assert_eq!(config.tracker.workspace_slug.as_deref(), Some("kata-sh"));
    assert_eq!(config.tracker.active_states, vec!["Todo", "In Progress"]);
    assert_eq!(config.tracker.terminal_states, vec!["Done"]);
    assert_eq!(config.tracker.exclude_labels, vec!["kata:task"]);
}
```

- [ ] **Step 3: Run the config regression**

Run:

```bash
cargo test --manifest-path apps/symphony/Cargo.toml --test workflow_config_tests test_linear_lifecycle_config_uses_existing_tracker_fields
```

Expected: PASS.

- [ ] **Step 4: Check workflow reference wording**

Run:

```bash
rg -n "project_milestone|project milestone|team_key|milestone filtering|Linear team" apps/symphony/docs/WORKFLOW-REFERENCE.md
```

Expected: no output. If there is output, remove that wording and keep the reference centered on `tracker.kind`, `tracker.api_key`, `tracker.endpoint`, `tracker.project_slug`, `tracker.workspace_slug`, `tracker.active_states`, `tracker.terminal_states`, `tracker.exclude_labels`, and `tracker.assignee`.

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/specs/2026-05-07-symphony-linear-execution-and-backend-uat-design.md apps/symphony/tests/workflow_config_tests.rs apps/symphony/docs/WORKFLOW-REFERENCE.md
git commit -m "docs(symphony): narrow linear execution scope"
```

## Task 2: Extract The Helper Boundary

**Files:**

- Create: `apps/symphony/src/helper.rs`
- Modify: `apps/symphony/src/main.rs`
- Modify: `apps/symphony/src/lib.rs`
- Test: `apps/symphony/tests/cli_tests.rs`

- [ ] **Step 1: Create the helper module with operation constants and envelopes**

Create `apps/symphony/src/helper.rs` with the helper operations and pure parsing utilities moved from `apps/symphony/src/main.rs`:

```rust
use std::path::Path;

use serde_json::Value;

use crate::config;
use crate::domain::TrackerConfig;
use crate::workflow_store::RuntimeBootstrapDeps;

pub const SHARED_HELPER_OPERATIONS: &[&str] = &[
    "issue.get",
    "issue.list-children",
    "comment.upsert",
    "issue.update-state",
    "issue.create-followup",
    "document.read",
    "document.write",
];

pub const GITHUB_ONLY_HELPER_OPERATIONS: &[&str] = &[
    "pr.inspect-feedback",
    "pr.inspect-checks",
    "pr.land-status",
];

pub fn success_envelope(data: Value) -> Value {
    serde_json::json!({ "ok": true, "data": data })
}

pub fn error_envelope(message: impl Into<String>) -> Value {
    serde_json::json!({
        "ok": false,
        "error": {
            "code": "HELPER_ERROR",
            "message": message.into(),
        },
    })
}

pub fn read_helper_input(input_path: Option<&str>) -> Result<Value, String> {
    let Some(input_path) = input_path else {
        return Ok(serde_json::json!({}));
    };
    let raw = std::fs::read_to_string(input_path)
        .map_err(|err| format!("failed to read helper input {input_path}: {err}"))?;
    serde_json::from_str(&raw)
        .map_err(|err| format!("helper input must be valid JSON object: {err}"))
        .and_then(|value: Value| {
            if value.is_object() {
                Ok(value)
            } else {
                Err("helper input must be a JSON object".to_string())
            }
        })
}

pub fn required_str(input: &Value, field: &str) -> Result<String, String> {
    input
        .get(field)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| format!("helper input field `{field}` must be a non-empty string"))
}

pub fn optional_str(input: &Value, field: &str) -> Option<String> {
    input
        .get(field)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub fn resolve_issue_id_value(
    raw: String,
    field: &str,
    current_id: Option<String>,
    current_identifier: Option<String>,
) -> Result<String, String> {
    if raw == "@current" {
        return current_id.ok_or_else(|| {
            format!("helper input field `{field}` used @current, but SYMPHONY_ISSUE_ID is not set")
        });
    }

    if let (Some(current_id), Some(current_identifier)) = (current_id, current_identifier) {
        if raw == current_identifier {
            return Ok(current_id);
        }
    }

    Ok(raw)
}

pub fn parse_symphony_document_comment(body: &str) -> Option<(String, String)> {
    let rest = body.strip_prefix("<!-- symphony:document:")?;
    let (title, content) = rest.split_once("-->")?;
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    Some((title.to_string(), content.trim_start().to_string()))
}

pub fn symphony_document_marker(title: &str) -> String {
    format!("<!-- symphony:document:{} -->", title.trim())
}
```

Also move the existing GitHub helper functions from `apps/symphony/src/main.rs` into this module. Keep their function bodies unchanged at this step and update helper utility names to the module-local names above.

- [ ] **Step 2: Export the helper module**

Add this line to `apps/symphony/src/lib.rs`:

```rust
pub mod helper;
```

- [ ] **Step 3: Delegate the CLI subcommand from `main.rs`**

Replace the helper implementation in `apps/symphony/src/main.rs` with:

```rust
fn run_helper(workflow_path: &Path, operation: &str, input_path: Option<&str>) -> i32 {
    let input = match symphony::helper::read_helper_input(input_path) {
        Ok(input) => input,
        Err(err) => {
            println!("{}", symphony::helper::error_envelope(err));
            return 1;
        }
    };

    let context = match RuntimeBootstrapDeps::load_startup_context(workflow_path) {
        Ok(context) => context,
        Err(err) => {
            println!("{}", symphony::helper::error_envelope(err.to_string()));
            return 1;
        }
    };

    if let Err(err) = config::validate(&context.effective_config) {
        println!(
            "{}",
            symphony::helper::error_envelope(format!("invalid workflow config: {err}"))
        );
        return 1;
    }

    let tracker = context.effective_config.tracker;
    let runtime = match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle,
        Err(err) => {
            println!(
                "{}",
                symphony::helper::error_envelope(format!("missing tokio runtime for helper: {err}"))
            );
            return 1;
        }
    };

    let result = tokio::task::block_in_place(|| {
        runtime.block_on(async { symphony::helper::run_operation(&tracker, operation, input).await })
    });

    match result {
        Ok(data) => {
            println!("{}", symphony::helper::success_envelope(data));
            0
        }
        Err(err) => {
            println!("{}", symphony::helper::error_envelope(err));
            1
        }
    }
}
```

- [ ] **Step 4: Add module tests for pure helper utilities**

Append to the test module in `apps/symphony/src/helper.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_comment_parser_reads_title_and_content() {
        let parsed = parse_symphony_document_comment(
            "<!-- symphony:document:Context -->\n\n# Context\n\nDetails",
        )
        .expect("document marker parses");

        assert_eq!(parsed.0, "Context");
        assert_eq!(parsed.1, "# Context\n\nDetails");
    }

    #[test]
    fn current_identifier_rewrites_to_current_issue_id() {
        let result = resolve_issue_id_value(
            "KAT-123".to_string(),
            "issueId",
            Some("linear-uuid".to_string()),
            Some("KAT-123".to_string()),
        )
        .expect("identifier resolves");

        assert_eq!(result, "linear-uuid");
    }
}
```

- [ ] **Step 5: Run helper and CLI parsing tests**

Run:

```bash
cargo test --manifest-path apps/symphony/Cargo.toml helper::tests
cargo test --manifest-path apps/symphony/Cargo.toml --test cli_tests test_helper_subcommand_parses_backend_neutral_operation
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/symphony/src/helper.rs apps/symphony/src/main.rs apps/symphony/src/lib.rs apps/symphony/tests/cli_tests.rs
git commit -m "refactor(symphony): extract helper boundary"
```

## Task 3: Add Linear Helper Client Methods

**Files:**

- Modify: `apps/symphony/src/linear/client.rs`
- Modify: `apps/symphony/src/linear/adapter.rs`
- Test: `apps/symphony/tests/linear_client_tests.rs`

- [ ] **Step 1: Add failing tests for Linear helper reads**

Append tests in `apps/symphony/tests/linear_client_tests.rs` that use `mockito` and the existing `test_client` helper:

```rust
#[tokio::test]
async fn test_linear_helper_issue_detail_reads_children_and_comments() {
    let mut server = mockito::Server::new_async().await;
    let client = test_client(&server, None);

    let mock = server
        .mock("POST", "/graphql")
        .match_body(mockito::Matcher::AllOf(vec![
            mockito::Matcher::Regex("SymphonyLinearHelperIssue".to_string()),
            mockito::Matcher::Regex("\"issueId\":\"issue-parent\"".to_string()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "data": {
                    "issue": {
                        "id": "issue-parent",
                        "identifier": "KAT-1",
                        "title": "Parent",
                        "description": "Parent body",
                        "priority": 1,
                        "state": { "name": "Todo" },
                        "branchName": "gannon/kat-1",
                        "url": "https://linear.app/kata/issue/KAT-1/parent",
                        "assignee": { "id": "user-1" },
                        "labels": { "nodes": [{ "name": "kata:slice" }] },
                        "inverseRelations": { "nodes": [] },
                        "children": {
                            "nodes": [{
                                "id": "issue-child",
                                "identifier": "KAT-2",
                                "title": "Child",
                                "description": "Child body",
                                "priority": 2,
                                "state": { "name": "Todo" },
                                "branchName": null,
                                "url": "https://linear.app/kata/issue/KAT-2/child",
                                "assignee": null,
                                "labels": { "nodes": [{ "name": "kata:task" }] },
                                "inverseRelations": { "nodes": [] },
                                "children": { "nodes": [] },
                                "parent": { "identifier": "KAT-1" },
                                "createdAt": "2026-05-07T10:00:00Z",
                                "updatedAt": "2026-05-07T10:10:00Z"
                            }]
                        },
                        "parent": null,
                        "comments": {
                            "nodes": [{
                                "id": "comment-1",
                                "body": "## Agent Workpad\n\nPlan",
                                "url": "https://linear.app/kata/comment/comment-1",
                                "createdAt": "2026-05-07T10:00:00Z",
                                "updatedAt": "2026-05-07T10:05:00Z"
                            }]
                        },
                        "createdAt": "2026-05-07T09:00:00Z",
                        "updatedAt": "2026-05-07T09:30:00Z"
                    }
                }
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let detail = client
        .fetch_helper_issue("issue-parent", true, true)
        .await
        .expect("helper issue detail loads");

    mock.assert_async().await;
    assert_eq!(detail.issue.identifier, "KAT-1");
    assert_eq!(detail.children.len(), 1);
    assert_eq!(detail.children[0].parent_identifier.as_deref(), Some("KAT-1"));
    assert_eq!(detail.comments.len(), 1);
    assert_eq!(detail.comments[0].id, "comment-1");
}
```

- [ ] **Step 2: Add failing tests for marker comment upsert**

Append:

```rust
#[tokio::test]
async fn test_linear_helper_upserts_existing_marker_comment() {
    let mut server = mockito::Server::new_async().await;
    let client = test_client(&server, None);

    let list_mock = server
        .mock("POST", "/graphql")
        .match_body(mockito::Matcher::Regex("SymphonyLinearHelperIssueComments".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "data": {
                    "issue": {
                        "comments": {
                            "nodes": [{
                                "id": "comment-workpad",
                                "body": "## Agent Workpad\n\nOld",
                                "url": "https://linear.app/kata/comment/comment-workpad",
                                "createdAt": "2026-05-07T10:00:00Z",
                                "updatedAt": "2026-05-07T10:00:00Z"
                            }]
                        }
                    }
                }
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let update_mock = server
        .mock("POST", "/graphql")
        .match_body(mockito::Matcher::AllOf(vec![
            mockito::Matcher::Regex("SymphonyLinearUpdateComment".to_string()),
            mockito::Matcher::Regex("\"commentId\":\"comment-workpad\"".to_string()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "data": {
                    "commentUpdate": {
                        "success": true,
                        "comment": {
                            "id": "comment-workpad",
                            "body": "## Agent Workpad\n\nNew",
                            "url": "https://linear.app/kata/comment/comment-workpad",
                            "createdAt": "2026-05-07T10:00:00Z",
                            "updatedAt": "2026-05-07T10:10:00Z"
                        }
                    }
                }
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let comment = client
        .upsert_comment("issue-parent", Some("## Agent Workpad"), "## Agent Workpad\n\nNew")
        .await
        .expect("comment updates");

    list_mock.assert_async().await;
    update_mock.assert_async().await;
    assert_eq!(comment.id, "comment-workpad");
    assert!(comment.body.contains("New"));
}
```

- [ ] **Step 3: Add failing tests for follow-up creation**

Append:

```rust
#[tokio::test]
async fn test_linear_helper_create_followup_derives_project_and_team_from_parent() {
    let mut server = mockito::Server::new_async().await;
    let client = test_client(&server, None);

    let context_mock = server
        .mock("POST", "/graphql")
        .match_body(mockito::Matcher::Regex("SymphonyLinearFollowupContext".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "data": {
                    "issue": {
                        "id": "issue-parent",
                        "team": { "id": "team-1" },
                        "project": { "id": "project-1" }
                    }
                }
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let create_mock = server
        .mock("POST", "/graphql")
        .match_body(mockito::Matcher::AllOf(vec![
            mockito::Matcher::Regex("SymphonyLinearCreateFollowup".to_string()),
            mockito::Matcher::Regex("\"teamId\":\"team-1\"".to_string()),
            mockito::Matcher::Regex("\"projectId\":\"project-1\"".to_string()),
            mockito::Matcher::Regex("\"parentId\":\"issue-parent\"".to_string()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "data": {
                    "issueCreate": {
                        "success": true,
                        "issue": {
                            "id": "issue-followup",
                            "identifier": "KAT-3",
                            "title": "Follow-up",
                            "url": "https://linear.app/kata/issue/KAT-3/follow-up"
                        }
                    }
                }
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let issue = client
        .create_followup_issue("issue-parent", "Follow-up", "Follow-up body")
        .await
        .expect("follow-up creates");

    context_mock.assert_async().await;
    create_mock.assert_async().await;
    assert_eq!(issue.id, "issue-followup");
    assert_eq!(issue.identifier, "KAT-3");
}
```

- [ ] **Step 4: Run the new tests and verify failure**

Run:

```bash
cargo test --manifest-path apps/symphony/Cargo.toml --test linear_client_tests linear_helper
```

Expected: FAIL because `fetch_helper_issue`, `upsert_comment`, and `create_followup_issue` do not exist.

- [ ] **Step 5: Add helper-facing Linear types**

In `apps/symphony/src/linear/client.rs`, add:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct LinearCommentRecord {
    pub id: String,
    pub body: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LinearHelperIssueDetail {
    pub issue: Issue,
    pub children: Vec<Issue>,
    pub comments: Vec<LinearCommentRecord>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct LinearCreatedIssue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    #[serde(default)]
    pub url: Option<String>,
}
```

- [ ] **Step 6: Add GraphQL operations**

In `apps/symphony/src/linear/client.rs`, add constants near the existing query constants:

```rust
const QUERY_HELPER_ISSUE: &str = r#"
query SymphonyLinearHelperIssue($issueId: String!, $commentFirst: Int!, $childFirst: Int!, $relationFirst: Int!) {
  issue(id: $issueId) {
    id
    identifier
    title
    description
    priority
    state { name }
    branchName
    url
    assignee { id }
    labels { nodes { name } }
    inverseRelations(first: $relationFirst) { nodes { type issue { id identifier state { name } } } }
    children(first: $childFirst) {
      nodes {
        id
        identifier
        title
        description
        priority
        state { name }
        branchName
        url
        assignee { id }
        labels { nodes { name } }
        inverseRelations(first: $relationFirst) { nodes { type issue { id identifier state { name } } } }
        children { nodes { id identifier } }
        parent { identifier }
        createdAt
        updatedAt
      }
    }
    parent { identifier }
    comments(first: $commentFirst) { nodes { id body url createdAt updatedAt } }
    createdAt
    updatedAt
  }
}
"#;

const QUERY_HELPER_COMMENTS: &str = r#"
query SymphonyLinearHelperIssueComments($issueId: String!, $commentFirst: Int!) {
  issue(id: $issueId) {
    comments(first: $commentFirst) {
      nodes { id body url createdAt updatedAt }
    }
  }
}
"#;

const MUTATION_UPDATE_COMMENT: &str = r#"
mutation SymphonyLinearUpdateComment($commentId: String!, $body: String!) {
  commentUpdate(id: $commentId, input: { body: $body }) {
    success
    comment { id body url createdAt updatedAt }
  }
}
"#;

const MUTATION_CREATE_COMMENT_RECORD: &str = r#"
mutation SymphonyLinearCreateCommentRecord($issueId: String!, $body: String!) {
  commentCreate(input: { issueId: $issueId, body: $body }) {
    success
    comment { id body url createdAt updatedAt }
  }
}
"#;

const QUERY_FOLLOWUP_CONTEXT: &str = r#"
query SymphonyLinearFollowupContext($issueId: String!) {
  issue(id: $issueId) {
    id
    team { id }
    project { id }
  }
}
"#;

const MUTATION_CREATE_FOLLOWUP: &str = r#"
mutation SymphonyLinearCreateFollowup($teamId: String!, $projectId: String, $parentId: String, $title: String!, $description: String!) {
  issueCreate(input: { teamId: $teamId, projectId: $projectId, parentId: $parentId, title: $title, description: $description }) {
    success
    issue { id identifier title url }
  }
}
"#;
```

- [ ] **Step 7: Implement helper methods on `LinearClient`**

Add public methods to `impl LinearClient`:

```rust
pub async fn fetch_helper_issue(
    &self,
    issue_id: &str,
    include_children: bool,
    include_comments: bool,
) -> Result<LinearHelperIssueDetail> {
    let body = self
        .graphql(
            QUERY_HELPER_ISSUE,
            serde_json::json!({
                "issueId": issue_id,
                "commentFirst": if include_comments { ISSUE_PAGE_SIZE } else { 0 },
                "childFirst": if include_children { ISSUE_PAGE_SIZE } else { 0 },
                "relationFirst": ISSUE_PAGE_SIZE,
            }),
        )
        .await?;

    let issue_value = body
        .get("data")
        .and_then(|data| data.get("issue"))
        .ok_or_else(|| SymphonyError::Other(format!("issue not found: {issue_id}")))?;

    let assignee_filter = self.routing_assignee_filter().await?;
    let issue = normalize_issue(issue_value, assignee_filter.as_ref())
        .ok_or_else(|| SymphonyError::LinearUnknownPayload)?;

    let children = issue_value
        .get("children")
        .and_then(|children| children.get("nodes"))
        .and_then(|nodes| nodes.as_array())
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|node| normalize_issue(node, assignee_filter.as_ref()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let comments = issue_value
        .get("comments")
        .and_then(|comments| comments.get("nodes"))
        .and_then(|nodes| nodes.as_array())
        .map(parse_comment_nodes)
        .unwrap_or_default();

    Ok(LinearHelperIssueDetail {
        issue,
        children,
        comments,
    })
}

pub async fn list_comments(&self, issue_id: &str) -> Result<Vec<LinearCommentRecord>> {
    let body = self
        .graphql(
            QUERY_HELPER_COMMENTS,
            serde_json::json!({ "issueId": issue_id, "commentFirst": ISSUE_PAGE_SIZE }),
        )
        .await?;

    let nodes = body
        .get("data")
        .and_then(|data| data.get("issue"))
        .and_then(|issue| issue.get("comments"))
        .and_then(|comments| comments.get("nodes"))
        .and_then(|nodes| nodes.as_array())
        .ok_or_else(|| SymphonyError::LinearUnknownPayload)?;

    Ok(parse_comment_nodes(nodes))
}

pub async fn upsert_comment(
    &self,
    issue_id: &str,
    marker: Option<&str>,
    body: &str,
) -> Result<LinearCommentRecord> {
    let marker = marker.map(str::trim).filter(|value| !value.is_empty());
    let final_body = match marker {
        Some(marker) if !body.contains(marker) => format!("{marker}\n\n{body}"),
        _ => body.to_string(),
    };

    let existing = match marker {
        Some(marker) => self
            .list_comments(issue_id)
            .await?
            .into_iter()
            .find(|comment| comment.body.contains(marker)),
        None => None,
    };

    match existing {
        Some(comment) => self.update_comment(&comment.id, &final_body).await,
        None => self.create_comment_record(issue_id, &final_body).await,
    }
}
```

Also add private helpers in the same file:

```rust
fn parse_comment_nodes(nodes: &[Value]) -> Vec<LinearCommentRecord> {
    nodes
        .iter()
        .filter_map(|node| {
            Some(LinearCommentRecord {
                id: node.get("id")?.as_str()?.to_string(),
                body: node.get("body")?.as_str()?.to_string(),
                url: node.get("url").and_then(|value| value.as_str()).map(String::from),
                created_at: parse_datetime(node.get("createdAt")),
                updated_at: parse_datetime(node.get("updatedAt")),
            })
        })
        .collect()
}
```

Implement `update_comment`, `create_comment_record`, and `create_followup_issue` using the constants from Step 6. Return `SymphonyError::Other` with the operation name and IDs when GraphQL returns `success: false` or omits the created record.

- [ ] **Step 8: Expose helper methods through `LinearAdapter`**

In `apps/symphony/src/linear/adapter.rs`, add:

```rust
use crate::linear::client::{LinearCommentRecord, LinearCreatedIssue, LinearHelperIssueDetail};

impl LinearAdapter {
    pub async fn fetch_helper_issue(
        &self,
        issue_id: &str,
        include_children: bool,
        include_comments: bool,
    ) -> Result<LinearHelperIssueDetail> {
        self.client
            .fetch_helper_issue(issue_id, include_children, include_comments)
            .await
    }

    pub async fn upsert_comment(
        &self,
        issue_id: &str,
        marker: Option<&str>,
        body: &str,
    ) -> Result<LinearCommentRecord> {
        self.client.upsert_comment(issue_id, marker, body).await
    }

    pub async fn create_followup_issue(
        &self,
        parent_issue_id: &str,
        title: &str,
        description: &str,
    ) -> Result<LinearCreatedIssue> {
        self.client
            .create_followup_issue(parent_issue_id, title, description)
            .await
    }
}
```

- [ ] **Step 9: Run Linear client tests**

Run:

```bash
cargo test --manifest-path apps/symphony/Cargo.toml --test linear_client_tests linear_helper
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add apps/symphony/src/linear/client.rs apps/symphony/src/linear/adapter.rs apps/symphony/tests/linear_client_tests.rs
git commit -m "feat(symphony): add linear helper client methods"
```

## Task 4: Implement Linear Helper Parity

**Files:**

- Modify: `apps/symphony/src/helper.rs`
- Create: `apps/symphony/tests/linear_helper_tests.rs`

- [ ] **Step 1: Write Linear helper operation tests**

Create `apps/symphony/tests/linear_helper_tests.rs` with tests that call `symphony::helper::run_operation` against a mock Linear endpoint:

```rust
use mockito::{Matcher, Server};
use serde_json::json;
use symphony::domain::{ApiKey, TrackerConfig};

fn linear_config(endpoint: String) -> TrackerConfig {
    TrackerConfig {
        kind: Some("linear".to_string()),
        endpoint,
        api_key: Some(ApiKey::new("linear-token")),
        project_slug: Some("kata-project".to_string()),
        workspace_slug: Some("kata-sh".to_string()),
        active_states: vec!["Todo".to_string(), "In Progress".to_string()],
        terminal_states: vec!["Done".to_string()],
        exclude_labels: vec![],
        ..TrackerConfig::default()
    }
}

#[tokio::test]
async fn linear_issue_list_children_returns_normalized_children() {
    let mut server = Server::new_async().await;
    let tracker = linear_config(format!("{}/graphql", server.url()));

    let mock = server
        .mock("POST", "/graphql")
        .match_body(Matcher::Regex("SymphonyLinearHelperIssue".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({
            "data": {
                "issue": {
                    "id": "issue-parent",
                    "identifier": "KAT-1",
                    "title": "Parent",
                    "description": "Parent body",
                    "priority": 1,
                    "state": { "name": "Todo" },
                    "branchName": null,
                    "url": "https://linear.app/kata/issue/KAT-1/parent",
                    "assignee": null,
                    "labels": { "nodes": [{ "name": "kata:slice" }] },
                    "inverseRelations": { "nodes": [] },
                    "children": { "nodes": [{
                        "id": "issue-child",
                        "identifier": "KAT-2",
                        "title": "Child",
                        "description": "Child body",
                        "priority": 2,
                        "state": { "name": "Todo" },
                        "branchName": null,
                        "url": "https://linear.app/kata/issue/KAT-2/child",
                        "assignee": null,
                        "labels": { "nodes": [{ "name": "kata:task" }] },
                        "inverseRelations": { "nodes": [] },
                        "children": { "nodes": [] },
                        "parent": { "identifier": "KAT-1" },
                        "createdAt": "2026-05-07T10:00:00Z",
                        "updatedAt": "2026-05-07T10:10:00Z"
                    }] },
                    "parent": null,
                    "comments": { "nodes": [] },
                    "createdAt": "2026-05-07T09:00:00Z",
                    "updatedAt": "2026-05-07T09:30:00Z"
                }
            }
        }).to_string())
        .expect(1)
        .create_async()
        .await;

    let result = symphony::helper::run_operation(
        &tracker,
        "issue.list-children",
        json!({ "issueId": "issue-parent" }),
    )
    .await
    .expect("helper succeeds");

    mock.assert_async().await;
    assert_eq!(result["children"][0]["identifier"], "KAT-2");
    assert_eq!(result["children"][0]["parent_identifier"], "KAT-1");
}

#[tokio::test]
async fn linear_pr_helpers_return_github_only_error() {
    let tracker = linear_config("http://127.0.0.1:9/graphql".to_string());

    let error = symphony::helper::run_operation(&tracker, "pr.land-status", json!({}))
        .await
        .expect_err("Linear rejects GitHub PR helper");

    assert!(error.contains("only available when tracker.kind is github"));
}
```

Add companion tests for `comment.upsert`, `document.write` plus `document.read`, and `issue.create-followup` using the mocks from Task 3.

- [ ] **Step 2: Run the helper tests and verify failure**

Run:

```bash
cargo test --manifest-path apps/symphony/Cargo.toml --test linear_helper_tests
```

Expected: FAIL for missing Linear shared operations.

- [ ] **Step 3: Implement Linear operations in `helper.rs`**

In `apps/symphony/src/helper.rs`, implement `run_operation` and Linear routing:

```rust
pub async fn run_operation(
    tracker: &TrackerConfig,
    operation: &str,
    input: Value,
) -> Result<Value, String> {
    let tracker_kind = tracker.kind.as_deref().unwrap_or("linear");
    if tracker_kind == "github" {
        return run_github_helper(tracker, operation, input).await;
    }
    run_linear_helper(tracker, operation, input).await
}

async fn run_linear_helper(
    tracker: &TrackerConfig,
    operation: &str,
    input: Value,
) -> Result<Value, String> {
    let adapter = crate::linear::adapter::LinearAdapter::new(
        crate::linear::client::LinearClient::new(tracker.clone()),
    );

    match operation {
        "issue.get" => {
            let issue_id = issue_id(&input, "issueId")?;
            let detail = adapter
                .fetch_helper_issue(
                    &issue_id,
                    bool_input(&input, "includeChildren", true),
                    bool_input(&input, "includeComments", true),
                )
                .await
                .map_err(|err| err.to_string())?;
            Ok(serde_json::json!({
                "issue": detail.issue,
                "children": detail.children,
                "comments": detail.comments,
            }))
        }
        "issue.list-children" => {
            let issue_id = issue_id(&input, "issueId")?;
            let detail = adapter
                .fetch_helper_issue(&issue_id, true, false)
                .await
                .map_err(|err| err.to_string())?;
            Ok(serde_json::json!({ "children": detail.children }))
        }
        "comment.upsert" => {
            let issue_id = issue_id(&input, "issueId")?;
            let body = required_str(&input, "body")?;
            let marker = optional_str(&input, "marker");
            let comment = adapter
                .upsert_comment(&issue_id, marker.as_deref(), &body)
                .await
                .map_err(|err| err.to_string())?;
            Ok(serde_json::json!({ "comment": comment }))
        }
        "issue.update-state" => {
            let issue_id = issue_id(&input, "issueId")?;
            let state = required_str(&input, "state")?;
            adapter
                .update_issue_state(&issue_id, &state)
                .await
                .map_err(|err| err.to_string())?;
            Ok(serde_json::json!({ "issueId": issue_id, "state": state }))
        }
        "issue.create-followup" => {
            let title = required_str(&input, "title")?;
            let description = required_str(&input, "description")?;
            let parent_issue_id = optional_issue_id(&input, "parentIssueId")?
                .ok_or_else(|| "helper input field `parentIssueId` must be provided for Linear follow-up creation".to_string())?;
            let issue = adapter
                .create_followup_issue(&parent_issue_id, &title, &description)
                .await
                .map_err(|err| err.to_string())?;
            Ok(serde_json::json!({ "issue": issue }))
        }
        "document.read" => {
            let issue_id = issue_id(&input, "issueId")?;
            let detail = adapter
                .fetch_helper_issue(&issue_id, false, true)
                .await
                .map_err(|err| err.to_string())?;
            let documents = detail
                .comments
                .into_iter()
                .filter_map(|comment| {
                    let (title, content) = parse_symphony_document_comment(&comment.body)?;
                    Some(serde_json::json!({
                        "title": title,
                        "content": content,
                        "comment": comment,
                    }))
                })
                .collect::<Vec<_>>();
            if let Some(title) = optional_str(&input, "title") {
                let content = documents
                    .iter()
                    .find(|document| document.get("title").and_then(|value| value.as_str()) == Some(title.as_str()))
                    .and_then(|document| document.get("content"))
                    .cloned();
                Ok(serde_json::json!({ "title": title, "content": content }))
            } else {
                Ok(serde_json::json!({ "documents": documents }))
            }
        }
        "document.write" => {
            let issue_id = issue_id(&input, "issueId")?;
            let title = required_str(&input, "title")?;
            let content = required_str(&input, "content")?;
            let marker = symphony_document_marker(&title);
            let body = format!("{marker}\n\n{content}");
            let comment = adapter
                .upsert_comment(&issue_id, Some(&marker), &body)
                .await
                .map_err(|err| err.to_string())?;
            Ok(serde_json::json!({ "title": title, "comment": comment }))
        }
        "pr.inspect-checks" | "pr.inspect-feedback" | "pr.land-status" => Err(format!(
            "operation `{operation}` is only available when tracker.kind is github"
        )),
        other => Err(format!("unsupported Symphony helper operation: {other}")),
    }
}
```

Keep `run_github_helper` behavior byte-for-byte compatible except for moved helper function names.

- [ ] **Step 4: Run helper tests**

Run:

```bash
cargo test --manifest-path apps/symphony/Cargo.toml --test linear_helper_tests
cargo test --manifest-path apps/symphony/Cargo.toml --test github_execution_contract_tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/symphony/src/helper.rs apps/symphony/tests/linear_helper_tests.rs
git commit -m "feat(symphony): add linear helper parity"
```

## Task 5: Lock Linear Dispatch Shape

**Files:**

- Modify: `apps/symphony/tests/orchestrator_tests.rs`
- Modify: `apps/symphony/tests/linear_client_tests.rs`

- [ ] **Step 1: Add Linear-shaped dispatch regression tests**

Append near the existing parent/sub-issue dispatch tests in `apps/symphony/tests/orchestrator_tests.rs`:

```rust
#[test]
fn test_linear_child_sub_issue_is_not_dispatched_independently() {
    let mut orchestrator = Orchestrator::new(test_config(2), String::new());
    let mut task_issue = issue("linear-child", "KAT-2001", "Todo", Some(1), 0);
    task_issue.parent_identifier = Some("KAT-2000".to_string());
    task_issue.labels = vec!["kata:task".to_string()];

    let mut port = FakePort {
        candidate_issues: vec![task_issue],
        ..FakePort::default()
    };

    let tick = orchestrator.tick(&mut port).expect("tick succeeds");

    assert!(tick.dispatched_issue_ids.is_empty());
}

#[test]
fn test_linear_parent_issue_with_children_dispatches_as_parent_work() {
    let mut orchestrator = Orchestrator::new(test_config(2), String::new());
    let mut parent_issue = issue("linear-parent", "KAT-2000", "Todo", Some(1), 0);
    parent_issue.children_count = 3;
    parent_issue.labels = vec!["kata:slice".to_string()];

    let mut port = FakePort {
        candidate_issues: vec![parent_issue.clone()],
        ..FakePort::default()
    };

    let tick = orchestrator.tick(&mut port).expect("tick succeeds");

    assert_eq!(tick.dispatched_issue_ids, vec![parent_issue.id]);
}
```

- [ ] **Step 2: Add Linear normalization test for native relation blockers**

Append to `apps/symphony/tests/linear_client_tests.rs`:

```rust
#[test]
fn test_linear_normalization_extracts_native_blocking_relations() {
    let raw = serde_json::json!({
        "id": "issue-blocked",
        "identifier": "KAT-20",
        "title": "Blocked issue",
        "state": { "name": "Todo" },
        "labels": { "nodes": [] },
        "inverseRelations": {
            "nodes": [{
                "type": "blocks",
                "issue": {
                    "id": "issue-blocker",
                    "identifier": "KAT-19",
                    "state": { "name": "In Progress" }
                }
            }]
        },
        "children": { "nodes": [] },
        "parent": null
    });

    let issue = symphony::linear::client::normalize_issue(&raw, None)
        .expect("issue normalizes");

    assert_eq!(issue.blocked_by.len(), 1);
    assert_eq!(issue.blocked_by[0].id.as_deref(), Some("issue-blocker"));
    assert_eq!(issue.blocked_by[0].identifier.as_deref(), Some("KAT-19"));
    assert_eq!(issue.blocked_by[0].state.as_deref(), Some("In Progress"));
}
```

- [ ] **Step 3: Run dispatch and normalization tests**

Run:

```bash
cargo test --manifest-path apps/symphony/Cargo.toml --test orchestrator_tests linear_
cargo test --manifest-path apps/symphony/Cargo.toml --test linear_client_tests test_linear_normalization_extracts_native_blocking_relations
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add apps/symphony/tests/orchestrator_tests.rs apps/symphony/tests/linear_client_tests.rs
git commit -m "test(symphony): lock linear issue dispatch shape"
```

## Task 6: Lock Prompt Backend Boundaries

**Files:**

- Modify: `apps/symphony/tests/backend_neutral_worker_contract_tests.rs`
- Modify: `apps/symphony/tests/workflow_config_tests.rs`
- Read-only: `apps/symphony/prompts/system.md`
- Read-only: `apps/symphony/prompts/in-progress.md`
- Read-only: `apps/symphony/prompts/rework.md`
- Read-only: `apps/symphony/prompts/agent-review.md`
- Read-only: `apps/symphony/prompts/merging.md`

- [ ] **Step 1: Add prompt contract assertions**

Extend `apps/symphony/tests/backend_neutral_worker_contract_tests.rs` with:

```rust
#[test]
fn worker_prompts_keep_tracker_helpers_backend_neutral() {
    for prompt in ["system.md", "in-progress.md", "rework.md", "agent-review.md", "merging.md"] {
        let content = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("prompts")
                .join(prompt),
        )
        .expect("prompt exists");

        assert!(content.contains("$SYMPHONY_BIN") || !content.contains("helper"));
        assert!(!content.contains("Linear milestone"));
        assert!(!content.contains("Linear project"));
        assert!(!content.contains("Linear-only"));
        assert!(!content.contains("GitHub Projects v2"));
        assert!(!content.contains("tracker.kind == \"linear\""));
    }
}

#[test]
fn worker_prompts_keep_pr_helpers_github_scoped() {
    let system = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("prompts")
            .join("system.md"),
    )
    .expect("system prompt exists");

    assert!(system.contains("issue.get"));
    assert!(system.contains("issue.list-children"));
    assert!(system.contains("document.write"));
    assert!(system.contains("GitHub PR"));
    assert!(system.contains("pr.land-status"));
    assert!(!system.contains("Linear PR"));
}

#[test]
fn worker_prompts_do_not_reference_removed_symphony_skills() {
    for prompt in ["system.md", "in-progress.md", "rework.md", "agent-review.md", "merging.md"] {
        let content = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("prompts")
                .join(prompt),
        )
        .expect("prompt exists");

        assert!(!content.contains(".agents/skills/sym-"));
        assert!(!content.contains("apps/symphony/skills"));
        assert!(!content.contains("sym-state"));
        assert!(!content.contains("sym-linear"));
    }
}
```

- [ ] **Step 2: Run prompt boundary tests**

Run:

```bash
cargo test --manifest-path apps/symphony/Cargo.toml --test backend_neutral_worker_contract_tests
cargo test --manifest-path apps/symphony/Cargo.toml --test workflow_config_tests prompt
```

Expected: PASS. If this fails because Linear support needs different worker prompt text, stop and fix the helper or adapter abstraction.

- [ ] **Step 3: Verify no prompt files changed**

Run:

```bash
git diff -- apps/symphony/prompts
```

Expected: no diff.

- [ ] **Step 4: Commit**

```bash
git add apps/symphony/tests/backend_neutral_worker_contract_tests.rs apps/symphony/tests/workflow_config_tests.rs
git commit -m "test(symphony): lock prompt backend boundaries"
```

## Task 7: Create Symphony Backend UAT Skill

**Files:**

- Create: `.agents/skills/symphony-backend-uat/SKILL.md`
- Create: `.agents/skills/symphony-backend-uat/scripts/symphony-backend-uat.mjs`
- Create: `.agents/skills/symphony-backend-uat/references/backend-config.md`
- Create: `.agents/skills/symphony-backend-uat/references/evidence.md`
- Create: `.agents/skills/symphony-backend-uat/references/generated-symphony-contract.json`
- Create: `.agents/skills/symphony-backend-uat/references/self-update.md`
- Create: `.agents/skills/symphony-backend-uat/references/workflow.md`
- Create: `.agents/skills/symphony-backend-uat/evals/evals.json`

- [ ] **Step 1: Use the skill creator workflow**

Read:

```bash
sed -n '1,220p' /Users/gannonhall/dotfiles/agents/.agents/skills/skill-creator/SKILL.md
```

Apply its authoring rules while creating the skill.

- [ ] **Step 2: Write `SKILL.md`**

Create `.agents/skills/symphony-backend-uat/SKILL.md`:

```markdown
---
name: symphony-backend-uat
description: Use this skill when the user wants to prove, test, UAT, validate, or clean up Symphony helper backend integrations against real GitHub Projects v2 or Linear instances. Use it for direct helper operation proof runs, backend health checks, generated proof links, helper contract updates, and cleanup of prior Symphony backend UAT runs.
---

# Symphony Backend UAT

## Operating Brief

Use this skill to prove that the Symphony direct helper contract works end to end against a real backend.

Ask which action to run unless the user already specified it:

1. Test a backend
2. Update this skill from Symphony changes
3. Clean up a prior test run

The bundled script builds or locates the Symphony binary, writes isolated workflow fixtures, runs health checks, calls every helper operation supported by the selected backend, captures provider proof links, and records cleanup state.

## Required Reading

Read before acting:

- `references/workflow.md`
- `references/backend-config.md`
- `references/evidence.md`

Read `references/self-update.md` when updating the skill from Symphony source or prompt changes.

## Script

```bash
node <skill-directory>/scripts/symphony-backend-uat.mjs --help
```

Common commands:

```bash
node <skill-directory>/scripts/symphony-backend-uat.mjs test --backend github
node <skill-directory>/scripts/symphony-backend-uat.mjs test --backend linear
node <skill-directory>/scripts/symphony-backend-uat.mjs update
node <skill-directory>/scripts/symphony-backend-uat.mjs cleanup --evidence /path/to/evidence.json
```

## Result Format

Report:

- Backend tested.
- Health result.
- Helper operation coverage.
- Created issue, child issue, comments, documents, and follow-up issue.
- GitHub PR helper results or skip reason.
- Provider proof links.
- Evidence file path and report file path.
- Cleanup result or cleanup command.

Keep the final answer concise and factual.

## Rules

- Use real backends only when the user requested integration proof or UAT.
- Use an isolated temporary run directory for every test.
- Do not claim success until health checks, helper coverage, provider reads, proof links, and evidence files are recorded.
- Treat missing shared helper operation coverage as a failed proof.
- Record GitHub PR helper skips with a concrete reason when no PR is discoverable.
- If the test creates backend state and fails, run cleanup with the evidence file when the user asks.
```

- [ ] **Step 3: Write generated contract seed**

Create `.agents/skills/symphony-backend-uat/references/generated-symphony-contract.json`:

```json
{
  "generatedAt": "2026-05-07T00:00:00.000Z",
  "workspace": "/Volumes/EVO/kata/kata-mono",
  "symphonyRoot": "/Volumes/EVO/kata/kata-mono/apps/symphony",
  "gitCommit": "unknown",
  "sharedHelperOperations": [
    "issue.get",
    "issue.list-children",
    "comment.upsert",
    "issue.update-state",
    "issue.create-followup",
    "document.read",
    "document.write"
  ],
  "githubOnlyHelperOperations": [
    "pr.inspect-feedback",
    "pr.inspect-checks",
    "pr.land-status"
  ],
  "backends": ["github", "linear"],
  "promptFiles": [
    "apps/symphony/prompts/system.md",
    "apps/symphony/prompts/in-progress.md",
    "apps/symphony/prompts/rework.md",
    "apps/symphony/prompts/agent-review.md",
    "apps/symphony/prompts/merging.md"
  ]
}
```

- [ ] **Step 4: Write the runner script**

Create `.agents/skills/symphony-backend-uat/scripts/symphony-backend-uat.mjs`. Follow the structure of `.agents/skills/kata-backend-uat/scripts/kata-backend-uat.mjs` and implement these commands:

```js
#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const SKILL_ROOT = path.resolve(path.dirname(new URL(import.meta.url).pathname), "..");
const SHARED_HELPER_OPERATIONS = [
  "issue.get",
  "issue.list-children",
  "comment.upsert",
  "issue.update-state",
  "issue.create-followup",
  "document.read",
  "document.write",
];
const GITHUB_ONLY_HELPER_OPERATIONS = [
  "pr.inspect-feedback",
  "pr.inspect-checks",
  "pr.land-status",
];

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const command = args._[0] ?? "help";
  if (command === "help" || args.help) return printHelp();
  if (command === "update") return updateGeneratedContract(args);
  if (command === "cleanup") return cleanupRun(args);
  if (command === "test") return testBackend(args);
  throw new Error(`Unknown command: ${command}`);
}

function printHelp() {
  console.log(`symphony-backend-uat

Commands:
  test --backend github|linear [--workspace path] [--symphony-root path] [--output-dir path] [--dry-run]
  update [--workspace path] [--symphony-root path]
  cleanup --evidence /path/to/evidence.json
`);
}
```

Implement `testBackend(args)` with this flow:

1. Resolve workspace, Symphony root, run directory, and environment from `.env`.
2. Build the binary with `cargo build --manifest-path apps/symphony/Cargo.toml`.
3. Write a `WORKFLOW.md` fixture for the selected backend.
4. Run `symphony doctor --workflow <fixture>`.
5. Create or locate a real issue for the backend.
6. For Linear, create a child issue through Linear GraphQL so `issue.list-children` has provider state to read.
7. Call every shared helper operation by invoking:

```js
const result = spawnSync(binaryPath, [
  "helper",
  operation,
  "--workflow",
  workflowPath,
  "--input",
  inputPath,
], { cwd: workspace, env, encoding: "utf8" });
```

8. For GitHub, attempt PR helper operations when `gh pr view --json number,url` succeeds.
9. Fetch provider records back through `gh api` or Linear GraphQL.
10. Write `evidence.json` and `evidence.md`.

Implement `updateGeneratedContract(args)` by parsing `apps/symphony/src/helper.rs` for `SHARED_HELPER_OPERATIONS` and `GITHUB_ONLY_HELPER_OPERATIONS`, parsing prompt files for `$SYMPHONY_BIN" helper`, and writing `references/generated-symphony-contract.json`.

Implement `cleanupRun(args)` by reading evidence and closing/completing created provider issues. Record cleanup success and failures in the evidence file.

- [ ] **Step 5: Write references and evals**

Create concise references:

```markdown
# Backend Config

GitHub uses `GH_TOKEN` or `GITHUB_TOKEN`, repository owner/name, and GitHub Projects v2 config already supported by Symphony `WORKFLOW.md`.

Linear uses `LINEAR_API_KEY`, `tracker.project_slug`, and `tracker.workspace_slug` already supported by Symphony `WORKFLOW.md`.
```

```markdown
# Evidence

Evidence must include backend, binary path, git commit, workflow fixture, health result, expected operations, observed operations, helper payloads, provider proof links, and cleanup status.
```

```markdown
# Self Update

Run `node .agents/skills/symphony-backend-uat/scripts/symphony-backend-uat.mjs update` after Symphony helper operations, prompt files, or backend kinds change.
```

```markdown
# Workflow

Use `test --backend github` or `test --backend linear` for proof. Use `cleanup --evidence <path>` for prior runs.
```

Create `.agents/skills/symphony-backend-uat/evals/evals.json` with prompts for GitHub proof, Linear proof, update, and cleanup, mirroring the structure in `.agents/skills/kata-backend-uat/evals/evals.json`.

- [ ] **Step 6: Dry-run the skill**

Run:

```bash
node .agents/skills/symphony-backend-uat/scripts/symphony-backend-uat.mjs update
node .agents/skills/symphony-backend-uat/scripts/symphony-backend-uat.mjs test --backend github --dry-run
node .agents/skills/symphony-backend-uat/scripts/symphony-backend-uat.mjs test --backend linear --dry-run
```

Expected: all commands exit 0 and write evidence for dry runs.

- [ ] **Step 7: Commit**

```bash
git add .agents/skills/symphony-backend-uat
git commit -m "feat(agents): add symphony backend uat skill"
```

## Task 8: Full Regression And Live UAT

**Files:**

- No planned source edits.

- [ ] **Step 1: Run targeted Symphony tests**

Run:

```bash
cargo test --manifest-path apps/symphony/Cargo.toml --test linear_client_tests
cargo test --manifest-path apps/symphony/Cargo.toml --test linear_helper_tests
cargo test --manifest-path apps/symphony/Cargo.toml --test orchestrator_tests
cargo test --manifest-path apps/symphony/Cargo.toml --test backend_neutral_worker_contract_tests
cargo test --manifest-path apps/symphony/Cargo.toml --test github_execution_contract_tests
```

Expected: PASS.

- [ ] **Step 2: Run the full Symphony suite**

Run:

```bash
cargo test --manifest-path apps/symphony/Cargo.toml
```

Expected: PASS.

- [ ] **Step 3: Run monorepo validation**

Run:

```bash
pnpm run validate:affected
```

Expected: PASS.

- [ ] **Step 4: Run real backend UAT when requested**

Run:

```bash
node .agents/skills/symphony-backend-uat/scripts/symphony-backend-uat.mjs test --backend github
node .agents/skills/symphony-backend-uat/scripts/symphony-backend-uat.mjs test --backend linear
```

Expected: both runs record health checks, all shared helper operations, provider proof links, and evidence files. GitHub PR helpers pass when a PR is discoverable, or record a skip reason.

- [ ] **Step 5: Inspect generated evidence**

Run:

```bash
node -e 'for (const p of process.argv.slice(1)) { const e = JSON.parse(require("fs").readFileSync(p, "utf8")); console.log(p, e.backend, e.operationCoverage); }' /path/to/github/evidence.json /path/to/linear/evidence.json
```

Expected: both evidence files show no missing shared helper operations.

- [ ] **Step 6: Commit final fixes if validation required changes**

```bash
git status --short
git add <changed-files>
git commit -m "fix(symphony): complete backend helper validation"
```

## Self-Review Checklist

- [ ] The plan adds no Symphony milestone, team, or Kata planning metadata fields.
- [ ] Linear helper parity covers all shared helper operations.
- [ ] GitHub PR helpers remain GitHub-scoped.
- [ ] Child Linear issues remain parent context and are not dispatched independently.
- [ ] The UAT skill can update its generated helper contract from source and prompts.
- [ ] Live UAT evidence includes provider links and cleanup state.
