use std::collections::HashSet;

use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::domain::GithubProjectOwnerType;
use crate::error::{Result, SymphonyError};
use crate::github::client::GithubClient;

const MAX_PAGES: usize = 10;
const PAGE_SIZE: usize = 100;
const ERROR_BODY_PREVIEW_CHARS: usize = 200;

pub const QUERY_PROJECT_FIELDS: &str = r#"
query($projectNumber: Int!, $owner: String!) {
  user(login: $owner) {
    projectV2(number: $projectNumber) {
      id
      field(name: "Status") {
        ... on ProjectV2SingleSelectField {
          id
          options {
            id
            name
          }
        }
      }
    }
  }
  organization(login: $owner) {
    projectV2(number: $projectNumber) {
      id
      field(name: "Status") {
        ... on ProjectV2SingleSelectField {
          id
          options {
            id
            name
          }
        }
      }
    }
  }
}
"#;

pub const QUERY_PROJECT_ITEMS: &str = r#"
query($projectId: ID!, $first: Int!, $after: String) {
  node(id: $projectId) {
    ... on ProjectV2 {
      items(first: $first, after: $after) {
        nodes {
          id
          content {
            ... on Issue {
              number
              repository {
                name
                owner {
                  login
                }
              }
              blockedBy(first: 100) {
                nodes {
                  ... on Issue {
                    number
                  }
                }
              }
            }
          }
          status: fieldValueByName(name: "Status") {
            ... on ProjectV2ItemFieldSingleSelectValue {
              name
              optionId
            }
          }
          kataId: fieldValueByName(name: "Kata ID") {
            ... on ProjectV2ItemFieldTextValue {
              text
            }
          }
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
  }
}
"#;

pub const MUTATION_UPDATE_STATUS: &str = r#"
mutation($projectId: ID!, $itemId: ID!, $fieldId: ID!, $singleSelectOptionId: String!) {
  updateProjectV2ItemFieldValue(
    input: {
      projectId: $projectId
      itemId: $itemId
      fieldId: $fieldId
      value: { singleSelectOptionId: $singleSelectOptionId }
    }
  ) {
    projectV2Item {
      id
    }
  }
}
"#;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StatusOption {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StatusFieldInfo {
    pub project_id: String,
    pub field_id: String,
    pub options: Vec<StatusOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectItem {
    pub item_id: String,
    pub issue_number: u64,
    /// `owner/repo` for the issue content when known.
    pub repository: Option<String>,
    pub status: Option<String>,
    pub kata_id: Option<String>,
    pub blocked_by_issue_numbers: Vec<u64>,
}

#[derive(Debug, Clone)]
pub struct ProjectsV2Client {
    github_client: GithubClient,
}

impl ProjectsV2Client {
    pub fn new(github_client: GithubClient) -> Self {
        Self { github_client }
    }

    pub async fn resolve_status_field(
        &self,
        owner: &str,
        project_number: u64,
    ) -> Result<StatusFieldInfo> {
        self.resolve_status_field_with_owner_type(owner, project_number, None)
            .await
    }

    pub async fn resolve_status_field_for_owner_type(
        &self,
        owner: &str,
        project_number: u64,
        owner_type: GithubProjectOwnerType,
    ) -> Result<StatusFieldInfo> {
        self.resolve_status_field_with_owner_type(owner, project_number, Some(owner_type))
            .await
    }

    async fn resolve_status_field_with_owner_type(
        &self,
        owner: &str,
        project_number: u64,
        owner_type: Option<GithubProjectOwnerType>,
    ) -> Result<StatusFieldInfo> {
        let variables = json!({
            "owner": owner,
            "projectNumber": project_number as i64,
        });

        let data: ProjectFieldsData = self
            .graphql_request(QUERY_PROJECT_FIELDS, variables)
            .await?;

        let project = match owner_type {
            Some(GithubProjectOwnerType::User) => data.user.and_then(|user| user.project_v2),
            Some(GithubProjectOwnerType::Org) => data.organization.and_then(|org| org.project_v2),
            None => data
                .user
                .and_then(|user| user.project_v2)
                .or_else(|| data.organization.and_then(|org| org.project_v2)),
        }
        .ok_or_else(|| {
            SymphonyError::GithubProjectsV2Error(format!(
                "Project #{project_number} not found for owner '{owner}'"
            ))
        })?;

        let Some(field) = project.field else {
            tracing::warn!(project_number, owner, "Projects v2 status field not found");
            return Err(SymphonyError::GithubProjectsV2Error(format!(
                "Status field not found on project #{project_number}"
            )));
        };

        tracing::debug!(
            field_id = %field.id,
            option_count = field.options.len(),
            "Projects v2 status field resolved"
        );

        Ok(StatusFieldInfo {
            project_id: project.id,
            field_id: field.id,
            options: field.options,
        })
    }

    pub async fn query_items_by_status(
        &self,
        project_id: &str,
        status_option_ids: &[String],
    ) -> Result<Vec<ProjectItem>> {
        self.query_items_paginated(project_id, status_option_ids, MAX_PAGES as u32, false)
            .await
    }

    /// List every project item across all statuses, failing if pagination is truncated.
    pub async fn query_all_items(
        &self,
        project_id: &str,
        max_pages: u32,
    ) -> Result<Vec<ProjectItem>> {
        self.query_items_paginated(project_id, &[], max_pages, true)
            .await
    }

    async fn query_items_paginated(
        &self,
        project_id: &str,
        status_option_ids: &[String],
        max_pages: u32,
        fail_on_truncate: bool,
    ) -> Result<Vec<ProjectItem>> {
        if max_pages == 0 {
            return Err(SymphonyError::GithubProjectsV2Error(
                "Projects v2 pagination max_pages must be greater than zero".to_string(),
            ));
        }

        let mut items = Vec::new();
        let mut after: Option<String> = None;
        let status_filter: Option<HashSet<&str>> = if status_option_ids.is_empty() {
            None
        } else {
            Some(status_option_ids.iter().map(String::as_str).collect())
        };

        for page_index in 0..max_pages {
            let variables = json!({
                "projectId": project_id,
                "first": PAGE_SIZE,
                "after": after,
            });

            let data: ProjectItemsData =
                self.graphql_request(QUERY_PROJECT_ITEMS, variables).await?;
            let node = data.node.ok_or_else(|| {
                SymphonyError::GithubProjectsV2Error(format!(
                    "Project node '{project_id}' not found"
                ))
            })?;

            for node in node.items.nodes {
                let Some(content) = node.content else {
                    continue;
                };
                let Some(issue_number) = content.number else {
                    continue;
                };
                let repository = content.repository.and_then(|repo| {
                    let owner = repo.owner?.login;
                    let name = repo.name?;
                    if owner.trim().is_empty() || name.trim().is_empty() {
                        None
                    } else {
                        Some(format!("{owner}/{name}"))
                    }
                });
                let blocked_by_issue_numbers = content
                    .blocked_by
                    .map(|connection| {
                        connection
                            .nodes
                            .into_iter()
                            .filter_map(|node| node.number)
                            .collect()
                    })
                    .unwrap_or_default();

                let status_option_id = node
                    .status
                    .as_ref()
                    .and_then(|status| status.option_id.as_deref());
                if let Some(filter) = &status_filter {
                    let Some(option_id) = status_option_id else {
                        continue;
                    };
                    if !filter.contains(option_id) {
                        continue;
                    }
                }

                items.push(ProjectItem {
                    item_id: node.id,
                    issue_number,
                    repository,
                    status: node.status.and_then(|status| status.name),
                    kata_id: node.kata_id.and_then(|value| value.text),
                    blocked_by_issue_numbers,
                });
            }

            tracing::debug!(
                item_count = items.len(),
                page_index,
                "Projects v2 items queried"
            );

            if node.items.page_info.has_next_page {
                after = node.items.page_info.end_cursor;
                if after.is_none() {
                    return Err(SymphonyError::GithubProjectsV2Error(
                        "Projects v2 query indicated next page but provided no cursor".to_string(),
                    ));
                }
            } else {
                after = None;
                break;
            }
        }

        if after.is_some() {
            if fail_on_truncate {
                return Err(SymphonyError::GithubProjectsV2Error(format!(
                    "Projects v2 item query truncated at max_pages={max_pages}; refusing partial results"
                )));
            }
            tracing::warn!(max_pages, "Projects v2 item query truncated at page cap");
        }

        Ok(items)
    }

    pub async fn update_item_status(
        &self,
        project_id: &str,
        item_id: &str,
        field_id: &str,
        option_id: &str,
    ) -> Result<()> {
        let variables = json!({
            "projectId": project_id,
            "itemId": item_id,
            "fieldId": field_id,
            "singleSelectOptionId": option_id,
        });

        let data: UpdateStatusData = self
            .graphql_request(MUTATION_UPDATE_STATUS, variables)
            .await?;

        let mutation_result = data.update_project_item.ok_or_else(|| {
            SymphonyError::GithubProjectsV2Error(
                "Projects v2 status mutation did not return a result".to_string(),
            )
        })?;

        if mutation_result.item.id.is_empty() {
            return Err(SymphonyError::GithubProjectsV2Error(
                "Projects v2 status mutation returned empty item id".to_string(),
            ));
        }

        Ok(())
    }

    async fn graphql_request<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: Value,
    ) -> Result<T> {
        let payload = json!({
            "query": query,
            "variables": variables,
        });

        let response = self
            .github_client
            .request(Method::POST, "/graphql", Some(&payload))
            .await?;

        let body = response.text().await.map_err(|err| {
            SymphonyError::GithubProjectsV2Error(format!(
                "Failed to read GraphQL response body: {err}"
            ))
        })?;

        let envelope: GraphqlEnvelope<T> = serde_json::from_str(&body).map_err(|err| {
            SymphonyError::GithubProjectsV2Error(format!(
                "Failed to decode GraphQL response: {err}; body={}",
                truncate_preview(&body)
            ))
        })?;

        // GitHub GraphQL returns partial errors when one of several top-level
        // fields fails (e.g. `organization` not found but `user` succeeds).
        // Only treat errors as fatal when `data` is absent.
        if envelope.data.is_none() {
            if let Some(errors) = envelope.errors.filter(|errors| !errors.is_empty()) {
                let message = &errors[0].message;
                return Err(SymphonyError::GithubProjectsV2Error(format!(
                    "{message}; response={}",
                    truncate_preview(&body)
                )));
            }
        }

        envelope.data.ok_or_else(|| {
            SymphonyError::GithubProjectsV2Error(format!(
                "GraphQL response missing data; body={}",
                truncate_preview(&body)
            ))
        })
    }
}

#[derive(Debug, Deserialize)]
struct GraphqlEnvelope<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Option<Vec<GraphqlError>>,
}

#[derive(Debug, Deserialize)]
struct GraphqlError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct ProjectFieldsData {
    user: Option<ProjectOwner>,
    organization: Option<ProjectOwner>,
}

#[derive(Debug, Deserialize)]
struct ProjectOwner {
    #[serde(rename = "projectV2")]
    project_v2: Option<ProjectNode>,
}

#[derive(Debug, Deserialize)]
struct ProjectNode {
    id: String,
    field: Option<ProjectStatusField>,
}

#[derive(Debug, Deserialize)]
struct ProjectStatusField {
    id: String,
    #[serde(default)]
    options: Vec<StatusOption>,
}

#[derive(Debug, Deserialize)]
struct ProjectItemsData {
    node: Option<ProjectItemsNode>,
}

#[derive(Debug, Deserialize)]
struct ProjectItemsNode {
    items: ProjectItemsConnection,
}

#[derive(Debug, Deserialize)]
struct ProjectItemsConnection {
    nodes: Vec<ProjectItemNode>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Debug, Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectItemNode {
    id: String,
    content: Option<ProjectItemContent>,
    status: Option<ProjectItemStatus>,
    #[serde(rename = "kataId")]
    kata_id: Option<ProjectItemTextValue>,
}

#[derive(Debug, Deserialize)]
struct ProjectItemContent {
    number: Option<u64>,
    repository: Option<ProjectItemRepository>,
    #[serde(rename = "blockedBy")]
    blocked_by: Option<ProjectIssueDependencyConnection>,
}

#[derive(Debug, Deserialize)]
struct ProjectItemRepository {
    name: Option<String>,
    owner: Option<ProjectItemRepositoryOwner>,
}

#[derive(Debug, Deserialize)]
struct ProjectItemRepositoryOwner {
    login: String,
}

#[derive(Debug, Deserialize)]
struct ProjectIssueDependencyConnection {
    #[serde(default)]
    nodes: Vec<ProjectIssueDependencyNode>,
}

#[derive(Debug, Deserialize)]
struct ProjectIssueDependencyNode {
    number: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ProjectItemStatus {
    name: Option<String>,
    #[serde(rename = "optionId")]
    option_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectItemTextValue {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateStatusData {
    #[serde(rename = "updateProjectV2ItemFieldValue")]
    update_project_item: Option<UpdateProjectItemMutation>,
}

#[derive(Debug, Deserialize)]
struct UpdateProjectItemMutation {
    #[serde(rename = "projectV2Item")]
    item: UpdateProjectItem,
}

#[derive(Debug, Deserialize)]
struct UpdateProjectItem {
    id: String,
}

fn truncate_preview(body: &str) -> String {
    body.chars().take(ERROR_BODY_PREVIEW_CHARS).collect()
}
