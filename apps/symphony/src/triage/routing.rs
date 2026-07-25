//! GitHub label and Projects v2 state mutations for automatic triage publication.

use async_trait::async_trait;

use crate::error::{Result, SymphonyError};
use crate::github::client::GithubClient;
use crate::github::projects_v2::ProjectsV2Client;
use crate::triage::publisher::TriageRoutingPort;

/// GitHub-backed routing port used by [`crate::triage::publisher::AutomaticPublisher`].
#[derive(Debug, Clone)]
pub struct GithubTriageRouting {
    client: GithubClient,
    projects: ProjectsV2Client,
    owner: String,
    project_number: u64,
    max_pages: u32,
}

impl GithubTriageRouting {
    pub fn new(
        client: GithubClient,
        projects: ProjectsV2Client,
        owner: impl Into<String>,
        project_number: u64,
        max_pages: u32,
    ) -> Self {
        Self {
            client,
            projects,
            owner: owner.into(),
            project_number,
            max_pages: max_pages.max(1),
        }
    }
}

#[async_trait]
impl TriageRoutingPort for GithubTriageRouting {
    async fn list_issue_labels(&self, issue_number: u64) -> Result<Vec<String>> {
        let issue = self.client.get_issue(issue_number).await?;
        Ok(issue.labels.into_iter().map(|label| label.name).collect())
    }

    async fn issue_project_state(&self, issue_number: u64) -> Result<Option<String>> {
        let status_field = self
            .projects
            .resolve_status_field(&self.owner, self.project_number)
            .await?;
        let items = self
            .projects
            .query_all_items(&status_field.project_id, self.max_pages)
            .await?;
        Ok(items
            .into_iter()
            .find(|item| item.issue_number == issue_number)
            .and_then(|item| item.status))
    }

    async fn add_issue_label(&self, issue_number: u64, label: &str) -> Result<()> {
        self.client.add_label(issue_number, label).await
    }

    async fn remove_issue_label(&self, issue_number: u64, label: &str) -> Result<()> {
        self.client.remove_label(issue_number, label).await
    }

    async fn set_issue_project_state(&self, issue_number: u64, state: &str) -> Result<()> {
        let status_field = self
            .projects
            .resolve_status_field(&self.owner, self.project_number)
            .await?;
        let target = status_field
            .options
            .iter()
            .find(|option| option.name.eq_ignore_ascii_case(state))
            .ok_or_else(|| {
                SymphonyError::GithubProjectsV2Error(format!(
                    "Projects v2 status option '{state}' not found on project #{}",
                    self.project_number
                ))
            })?;
        let items = self
            .projects
            .query_all_items(&status_field.project_id, self.max_pages)
            .await?;
        let item = items
            .into_iter()
            .find(|item| item.issue_number == issue_number)
            .ok_or_else(|| {
                SymphonyError::GithubProjectsV2Error(format!(
                    "issue #{issue_number} is not on project board #{}",
                    self.project_number
                ))
            })?;
        self.projects
            .update_item_status(
                &status_field.project_id,
                &item.item_id,
                &status_field.field_id,
                &target.id,
            )
            .await
    }
}
