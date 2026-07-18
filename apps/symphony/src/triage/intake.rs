use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashSet;

use crate::error::{Result, SymphonyError};
use crate::github::client::{GithubClient, GithubIssue, GithubIssueComment};
use crate::github::projects_v2::ProjectsV2Client;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntakeMilestone {
    pub number: u64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntakeComment {
    pub id: u64,
    pub author_login: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriageIntakeIssue {
    pub issue_id: String,
    pub issue_number: u64,
    pub identifier: String,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub non_managed_labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestone: Option<IntakeMilestone>,
    pub comments: Vec<IntakeComment>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub forge_host: String,
    pub repository: String,
    pub in_project: bool,
}

#[async_trait]
pub trait TriageIntakePort: Send + Sync {
    async fn fetch_intake_issues(
        &self,
        intake_label: &str,
        max_pages: u32,
    ) -> Result<Vec<TriageIntakeIssue>>;

    async fn list_issue_comments(
        &self,
        issue_number: u64,
        max_pages: u32,
    ) -> Result<Vec<IntakeComment>>;

    async fn authenticated_login(&self) -> Result<String>;
}

#[derive(Debug, Clone)]
pub struct GithubTriageIntake {
    github: GithubClient,
    projects: ProjectsV2Client,
    owner: String,
    repo: String,
    project_number: u64,
    forge_host: String,
    managed_labels: Vec<String>,
}

impl GithubTriageIntake {
    pub fn new(
        github: GithubClient,
        projects: ProjectsV2Client,
        owner: impl Into<String>,
        repo: impl Into<String>,
        project_number: u64,
        forge_host: impl Into<String>,
        managed_labels: Vec<String>,
    ) -> Self {
        Self {
            github,
            projects,
            owner: owner.into(),
            repo: repo.into(),
            project_number,
            forge_host: forge_host.into(),
            managed_labels,
        }
    }

    fn repository(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    fn normalize_issue(
        &self,
        issue: GithubIssue,
        comments: Vec<IntakeComment>,
        in_project: bool,
        intake_label: &str,
    ) -> TriageIntakeIssue {
        let labels: Vec<String> = issue
            .labels
            .iter()
            .map(|label| label.name.clone())
            .collect();
        let managed: HashSet<String> = self
            .managed_labels
            .iter()
            .cloned()
            .chain(std::iter::once(intake_label.to_string()))
            .map(|label| label.to_ascii_lowercase())
            .collect();
        let non_managed_labels: Vec<String> = labels
            .iter()
            .filter(|label| !managed.contains(&label.to_ascii_lowercase()))
            .cloned()
            .collect();
        let assignees: Vec<String> = if !issue.assignees.is_empty() {
            issue
                .assignees
                .iter()
                .map(|user| user.login.clone())
                .collect()
        } else {
            issue.assignee.into_iter().map(|user| user.login).collect()
        };

        TriageIntakeIssue {
            issue_id: issue.number.to_string(),
            issue_number: issue.number,
            identifier: format!("#{}", issue.number),
            title: issue.title,
            body: issue.body.unwrap_or_default(),
            labels,
            non_managed_labels,
            assignees,
            milestone: issue.milestone.map(|milestone| IntakeMilestone {
                number: milestone.number,
                title: milestone.title,
            }),
            comments,
            created_at: issue.created_at.unwrap_or_else(Utc::now),
            updated_at: issue.updated_at.unwrap_or_else(Utc::now),
            forge_host: self.forge_host.clone(),
            repository: self.repository(),
            in_project,
        }
    }
}

#[async_trait]
impl TriageIntakePort for GithubTriageIntake {
    async fn fetch_intake_issues(
        &self,
        intake_label: &str,
        max_pages: u32,
    ) -> Result<Vec<TriageIntakeIssue>> {
        let issues = self
            .github
            .list_issues_paginated("open", &[intake_label.to_string()], max_pages)
            .await?;

        let status_field = self
            .projects
            .resolve_status_field(&self.owner, self.project_number)
            .await?;
        let project_items = self
            .projects
            .query_all_items(&status_field.project_id, max_pages)
            .await?;
        // Issue numbers are only unique within a repository. Match on both
        // repository and number so multi-repo projects cannot false-positive.
        let configured_repository = self.repository();
        let in_project_keys: HashSet<(String, u64)> = project_items
            .into_iter()
            .map(|item| {
                let repository = item
                    .repository
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| configured_repository.clone());
                (repository.to_ascii_lowercase(), item.issue_number)
            })
            .collect();

        let mut results = Vec::new();
        for issue in issues {
            if issue.pull_request.is_some() {
                continue;
            }
            let comments = self.list_issue_comments(issue.number, max_pages).await?;
            let membership_key = (configured_repository.to_ascii_lowercase(), issue.number);
            let in_project = in_project_keys.contains(&membership_key);
            results.push(self.normalize_issue(issue, comments, in_project, intake_label));
        }
        Ok(results)
    }

    async fn list_issue_comments(
        &self,
        issue_number: u64,
        max_pages: u32,
    ) -> Result<Vec<IntakeComment>> {
        let comments = self
            .github
            .list_comments_paginated(issue_number, max_pages)
            .await?;
        Ok(comments.into_iter().map(normalize_comment).collect())
    }

    async fn authenticated_login(&self) -> Result<String> {
        let user = self.github.get_authenticated_user().await?;
        if user.login.trim().is_empty() {
            return Err(SymphonyError::GithubApiRequest(
                "authenticated GitHub user login is empty".to_string(),
            ));
        }
        Ok(user.login)
    }
}

fn normalize_comment(comment: GithubIssueComment) -> IntakeComment {
    IntakeComment {
        id: comment.id,
        author_login: comment.user.map(|user| user.login).unwrap_or_default(),
        body: comment.body.unwrap_or_default(),
        created_at: comment.created_at.unwrap_or_else(Utc::now),
        updated_at: comment.updated_at.unwrap_or_else(Utc::now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::client::{GithubLabel, GithubMilestone, GithubUser};
    use crate::triage::domain::TriageRoutesConfig;

    #[test]
    fn normalize_issue_separates_managed_labels() {
        let github =
            GithubClient::with_base_url("token", "owner", "repo", "symphony", "http://example.com");
        let projects = ProjectsV2Client::new(github.clone());
        let routes = TriageRoutesConfig::default();
        let intake = GithubTriageIntake::new(
            github,
            projects,
            "owner",
            "repo",
            1,
            "github.com",
            routes.managed_labels(),
        );

        let issue = GithubIssue {
            number: 12,
            title: "Fix docs".to_string(),
            body: Some("body".to_string()),
            state: "open".to_string(),
            user: Some(GithubUser {
                login: "reporter".to_string(),
            }),
            assignee: None,
            assignees: vec![GithubUser {
                login: "alice".to_string(),
            }],
            labels: vec![
                GithubLabel {
                    name: "needs-triage".to_string(),
                    color: None,
                    description: None,
                },
                GithubLabel {
                    name: "ready-for-agent".to_string(),
                    color: None,
                    description: None,
                },
                GithubLabel {
                    name: "docs".to_string(),
                    color: None,
                    description: None,
                },
            ],
            milestone: Some(GithubMilestone {
                number: 3,
                title: "M3".to_string(),
            }),
            created_at: Some(Utc::now()),
            updated_at: Some(Utc::now()),
            html_url: None,
            pull_request: None,
            sub_issues_summary: None,
            parent_issue_url: None,
        };

        let normalized = intake.normalize_issue(issue, Vec::new(), true, "needs-triage");
        assert_eq!(normalized.issue_id, "12");
        assert_eq!(normalized.identifier, "#12");
        assert_eq!(normalized.non_managed_labels, vec!["docs".to_string()]);
        assert_eq!(normalized.assignees, vec!["alice".to_string()]);
        assert_eq!(normalized.milestone.as_ref().unwrap().number, 3);
        assert!(normalized.in_project);
    }
}
