use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use reqwest::{header, Method, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::error::{Result, SymphonyError};

const REQUEST_TIMEOUT_SECS: u64 = 30;
const MAX_PAGES: usize = 10;
const ERROR_BODY_PREVIEW_CHARS: usize = 200;
const GITHUB_API_URL: &str = "https://api.github.com";

#[derive(Debug, Clone)]
pub struct RateLimitState {
    pub remaining: u32,
    pub limit: u32,
    pub reset: DateTime<Utc>,
}

impl Default for RateLimitState {
    fn default() -> Self {
        Self {
            remaining: u32::MAX,
            limit: u32::MAX,
            reset: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct GithubUser {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct GithubLabel {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct GithubMilestone {
    pub number: u64,
    pub title: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct GithubSubIssuesSummary {
    #[serde(default)]
    pub total: u32,
    #[serde(default)]
    pub completed: u32,
    #[serde(default)]
    pub percent_completed: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GithubIssue {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    pub state: String,
    #[serde(default)]
    pub user: Option<GithubUser>,
    #[serde(default)]
    pub assignee: Option<GithubUser>,
    #[serde(default)]
    pub assignees: Vec<GithubUser>,
    #[serde(default)]
    pub labels: Vec<GithubLabel>,
    #[serde(default)]
    pub milestone: Option<GithubMilestone>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub pull_request: Option<Value>,
    #[serde(default)]
    pub sub_issues_summary: Option<GithubSubIssuesSummary>,
    #[serde(default)]
    pub parent_issue_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct GithubIssueComment {
    pub id: u64,
    #[serde(default)]
    pub user: Option<GithubUser>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub html_url: Option<String>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone)]
pub struct GithubClient {
    pub http_client: reqwest::Client,
    pub base_url: String,
    pub repo_owner: String,
    pub repo_name: String,
    pub label_prefix: String,
    rate_limit_state: Arc<Mutex<RateLimitState>>,
}

impl std::fmt::Debug for GithubClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GithubClient")
            .field("base_url", &self.base_url)
            .field("repo_owner", &self.repo_owner)
            .field("repo_name", &self.repo_name)
            .field("label_prefix", &self.label_prefix)
            .finish()
    }
}

impl GithubClient {
    pub fn new(
        token: impl Into<String>,
        repo_owner: impl Into<String>,
        repo_name: impl Into<String>,
        label_prefix: impl Into<String>,
    ) -> Self {
        Self::with_base_url(token, repo_owner, repo_name, label_prefix, GITHUB_API_URL)
    }

    pub fn with_base_url(
        token: impl Into<String>,
        repo_owner: impl Into<String>,
        repo_name: impl Into<String>,
        label_prefix: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        let token = token.into();

        let mut default_headers = header::HeaderMap::new();
        default_headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {token}"))
                .expect("valid GitHub authorization header"),
        );
        default_headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static("application/vnd.github+json"),
        );
        default_headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static("symphony"),
        );

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .default_headers(default_headers)
            .build()
            .expect("failed to build GitHub reqwest client");

        Self {
            http_client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            repo_owner: repo_owner.into(),
            repo_name: repo_name.into(),
            label_prefix: label_prefix.into(),
            rate_limit_state: Arc::new(Mutex::new(RateLimitState::default())),
        }
    }

    pub async fn rate_limit_state(&self) -> RateLimitState {
        self.rate_limit_state.lock().await.clone()
    }

    pub async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Response> {
        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            format!("{}{}", self.base_url, path)
        };

        self.wait_for_rate_limit_if_needed().await;

        let mut request_builder = self.http_client.request(method, &url);
        if let Some(json_body) = body {
            request_builder = request_builder.json(json_body);
        }

        let response = request_builder.send().await.map_err(|err| {
            SymphonyError::GithubApiRequest(format!("request to {url} failed: {err}"))
        })?;

        self.update_rate_limit_state(response.headers()).await;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body_text = response.text().await.unwrap_or_default();
            return Err(SymphonyError::GithubApiStatus {
                status,
                message: truncate_error_preview(&body_text),
            });
        }

        Ok(response)
    }

    pub async fn list_issues(
        &self,
        state_filter: &str,
        labels: &[String],
    ) -> Result<Vec<GithubIssue>> {
        let mut url = reqwest::Url::parse(&format!(
            "{}/repos/{}/{}/issues",
            self.base_url, self.repo_owner, self.repo_name
        ))
        .map_err(|err| SymphonyError::GithubApiRequest(format!("invalid issues URL: {err}")))?;

        {
            let mut query = url.query_pairs_mut();
            query.append_pair("state", state_filter);
            query.append_pair("per_page", "100");
            if !labels.is_empty() {
                query.append_pair("labels", &labels.join(","));
            }
        }

        self.paginated_get(url.as_ref()).await
    }

    /// Paginate issues to exhaustion under `max_pages`.
    ///
    /// Unlike [`list_issues`], this fails visibly when pagination is truncated
    /// at the page cap so triage never acts on a partial intake set.
    pub async fn list_issues_paginated(
        &self,
        state_filter: &str,
        labels: &[String],
        max_pages: u32,
    ) -> Result<Vec<GithubIssue>> {
        let mut url = reqwest::Url::parse(&format!(
            "{}/repos/{}/{}/issues",
            self.base_url, self.repo_owner, self.repo_name
        ))
        .map_err(|err| SymphonyError::GithubApiRequest(format!("invalid issues URL: {err}")))?;

        {
            let mut query = url.query_pairs_mut();
            query.append_pair("state", state_filter);
            query.append_pair("per_page", "100");
            if !labels.is_empty() {
                query.append_pair("labels", &labels.join(","));
            }
        }

        self.paginated_get_capped(url.as_ref(), max_pages, "issues")
            .await
    }

    pub async fn get_issue(&self, number: u64) -> Result<GithubIssue> {
        let path = format!(
            "/repos/{}/{}/issues/{number}",
            self.repo_owner, self.repo_name
        );
        self.request_json(Method::GET, &path, None).await
    }

    pub async fn get_authenticated_user(&self) -> Result<GithubUser> {
        self.request_json(Method::GET, "/user", None).await
    }

    pub async fn get_comment(&self, comment_id: u64) -> Result<GithubIssueComment> {
        let path = format!(
            "/repos/{}/{}/issues/comments/{comment_id}",
            self.repo_owner, self.repo_name
        );
        self.request_json(Method::GET, &path, None).await
    }

    pub async fn create_comment(&self, number: u64, body: &str) -> Result<()> {
        let path = format!(
            "/repos/{}/{}/issues/{number}/comments",
            self.repo_owner, self.repo_name
        );
        let payload = serde_json::json!({ "body": body });
        self.request_empty(Method::POST, &path, Some(&payload))
            .await
    }

    pub async fn create_comment_record(
        &self,
        number: u64,
        body: &str,
    ) -> Result<GithubIssueComment> {
        let path = format!(
            "/repos/{}/{}/issues/{number}/comments",
            self.repo_owner, self.repo_name
        );
        let payload = serde_json::json!({ "body": body });
        self.request_json(Method::POST, &path, Some(&payload)).await
    }

    pub async fn update_comment(&self, comment_id: u64, body: &str) -> Result<GithubIssueComment> {
        let path = format!(
            "/repos/{}/{}/issues/comments/{comment_id}",
            self.repo_owner, self.repo_name
        );
        let payload = serde_json::json!({ "body": body });
        self.request_json(Method::PATCH, &path, Some(&payload))
            .await
    }

    pub async fn list_comments(&self, number: u64) -> Result<Vec<GithubIssueComment>> {
        let mut url = reqwest::Url::parse(&format!(
            "{}/repos/{}/{}/issues/{number}/comments",
            self.base_url, self.repo_owner, self.repo_name
        ))
        .map_err(|err| SymphonyError::GithubApiRequest(format!("invalid comments URL: {err}")))?;

        {
            let mut query = url.query_pairs_mut();
            query.append_pair("per_page", "100");
        }

        self.paginated_get(url.as_ref()).await
    }

    /// Paginate issue comments to exhaustion under `max_pages`.
    ///
    /// Fails when truncated so fingerprinting and comment recovery never
    /// treat a partial comment list as complete.
    pub async fn list_comments_paginated(
        &self,
        number: u64,
        max_pages: u32,
    ) -> Result<Vec<GithubIssueComment>> {
        let mut url = reqwest::Url::parse(&format!(
            "{}/repos/{}/{}/issues/{number}/comments",
            self.base_url, self.repo_owner, self.repo_name
        ))
        .map_err(|err| SymphonyError::GithubApiRequest(format!("invalid comments URL: {err}")))?;

        {
            let mut query = url.query_pairs_mut();
            query.append_pair("per_page", "100");
        }

        self.paginated_get_capped(url.as_ref(), max_pages, "comments")
            .await
    }

    pub async fn create_issue(&self, title: &str, body: &str) -> Result<GithubIssue> {
        let path = format!("/repos/{}/{}/issues", self.repo_owner, self.repo_name);
        let payload = serde_json::json!({
            "title": title,
            "body": body,
        });
        self.request_json(Method::POST, &path, Some(&payload)).await
    }

    pub async fn list_sub_issues(&self, number: u64) -> Result<Vec<GithubIssue>> {
        let path = format!(
            "/repos/{}/{}/issues/{number}/sub_issues",
            self.repo_owner, self.repo_name
        );
        self.request_json(Method::GET, &path, None).await
    }

    pub async fn add_label(&self, number: u64, label: &str) -> Result<()> {
        let path = format!(
            "/repos/{}/{}/issues/{number}/labels",
            self.repo_owner, self.repo_name
        );
        let payload = serde_json::json!({ "labels": [label] });
        self.request_empty(Method::POST, &path, Some(&payload))
            .await
    }

    pub async fn remove_label(&self, number: u64, label: &str) -> Result<()> {
        let mut url = reqwest::Url::parse(&format!(
            "{}/repos/{}/{}/issues/{number}/labels",
            self.base_url, self.repo_owner, self.repo_name
        ))
        .map_err(|err| {
            SymphonyError::GithubApiRequest(format!("invalid remove-label URL: {err}"))
        })?;

        url.path_segments_mut()
            .map_err(|_| {
                SymphonyError::GithubApiRequest(
                    "invalid remove-label URL path segments".to_string(),
                )
            })?
            .push(label);

        self.request_empty(Method::DELETE, url.as_ref(), None).await
    }

    pub async fn list_labels(&self) -> Result<Vec<GithubLabel>> {
        let mut url = reqwest::Url::parse(&format!(
            "{}/repos/{}/{}/labels",
            self.base_url, self.repo_owner, self.repo_name
        ))
        .map_err(|err| SymphonyError::GithubApiRequest(format!("invalid labels URL: {err}")))?;

        {
            let mut query = url.query_pairs_mut();
            query.append_pair("per_page", "100");
        }

        self.paginated_get(url.as_ref()).await
    }

    async fn request_json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<T> {
        let response = self.request(method, path, body).await?;
        response.json::<T>().await.map_err(|err| {
            SymphonyError::GithubApiRequest(format!("failed to decode GitHub response JSON: {err}"))
        })
    }

    async fn request_empty(&self, method: Method, path: &str, body: Option<&Value>) -> Result<()> {
        let _ = self.request(method, path, body).await?;
        Ok(())
    }

    async fn paginated_get<T: DeserializeOwned>(&self, initial_url: &str) -> Result<Vec<T>> {
        let mut results = Vec::new();
        let mut next_url = Some(initial_url.to_string());

        for _ in 0..MAX_PAGES {
            let Some(current_url) = next_url.take() else {
                break;
            };

            let response = self.request(Method::GET, &current_url, None).await?;
            let extracted_next = extract_next_link(response.headers());
            let page: Vec<T> = response.json().await.map_err(|err| {
                SymphonyError::GithubApiRequest(format!(
                    "failed to decode paginated GitHub response JSON: {err}"
                ))
            })?;
            results.extend(page);
            next_url = extracted_next;
        }

        if next_url.is_some() {
            tracing::warn!(
                max_pages = MAX_PAGES,
                "GitHub paginated response truncated at page cap; results may be incomplete"
            );
        }

        Ok(results)
    }

    async fn paginated_get_capped<T: DeserializeOwned>(
        &self,
        initial_url: &str,
        max_pages: u32,
        resource: &str,
    ) -> Result<Vec<T>> {
        if max_pages == 0 {
            return Err(SymphonyError::GithubApiRequest(format!(
                "GitHub {resource} pagination max_pages must be greater than zero"
            )));
        }

        let mut results = Vec::new();
        let mut next_url = Some(initial_url.to_string());
        let mut pages_fetched = 0u32;

        while let Some(current_url) = next_url.take() {
            if pages_fetched >= max_pages {
                return Err(SymphonyError::GithubApiRequest(format!(
                    "GitHub {resource} pagination truncated at max_pages={max_pages}; refusing partial results"
                )));
            }

            let response = self.request(Method::GET, &current_url, None).await?;
            let extracted_next = extract_next_link(response.headers());
            let page: Vec<T> = response.json().await.map_err(|err| {
                SymphonyError::GithubApiRequest(format!(
                    "failed to decode paginated GitHub response JSON: {err}"
                ))
            })?;
            results.extend(page);
            pages_fetched += 1;
            next_url = extracted_next;
        }

        Ok(results)
    }

    async fn wait_for_rate_limit_if_needed(&self) {
        let snapshot = self.rate_limit_state.lock().await.clone();
        let now = Utc::now();

        if snapshot.remaining == 0 && snapshot.reset > now {
            let sleep_for = (snapshot.reset - now)
                .to_std()
                .unwrap_or_else(|_| std::time::Duration::from_millis(0));
            tracing::info!(
                rate_limit_remaining = snapshot.remaining,
                rate_limit_limit = snapshot.limit,
                rate_limit_reset = %snapshot.reset,
                sleep_ms = sleep_for.as_millis(),
                "GitHub API rate limit exhausted; delaying request until reset"
            );
            tokio::time::sleep(sleep_for).await;
        }
    }

    async fn update_rate_limit_state(&self, headers: &header::HeaderMap) {
        let previous = self.rate_limit_state.lock().await.clone();

        let remaining = header_u32(headers, "x-ratelimit-remaining").unwrap_or(previous.remaining);
        let limit = header_u32(headers, "x-ratelimit-limit").unwrap_or(previous.limit);
        let reset = header_u64(headers, "x-ratelimit-reset")
            .and_then(|raw| Utc.timestamp_opt(raw as i64, 0).single())
            .unwrap_or(previous.reset);

        {
            let mut state = self.rate_limit_state.lock().await;
            state.remaining = remaining;
            state.limit = limit;
            state.reset = reset;
        }

        tracing::debug!(
            rate_limit_remaining = remaining,
            rate_limit_limit = limit,
            rate_limit_reset = %reset,
            "Updated GitHub API rate limit state"
        );

        if limit > 0 && remaining.saturating_mul(10) <= limit {
            tracing::warn!(
                rate_limit_remaining = remaining,
                rate_limit_limit = limit,
                rate_limit_reset = %reset,
                "GitHub API rate limit low"
            );
        }

        if previous.remaining == 0 && remaining > 0 {
            tracing::info!(
                rate_limit_remaining = remaining,
                rate_limit_limit = limit,
                rate_limit_reset = %reset,
                "GitHub API rate limit reset"
            );
        }
    }
}

fn extract_next_link(headers: &header::HeaderMap) -> Option<String> {
    let link_header = headers.get(header::LINK)?.to_str().ok()?;

    link_header.split(',').find_map(|part| {
        let trimmed = part.trim();
        if !trimmed.contains("rel=\"next\"") {
            return None;
        }

        let start = trimmed.find('<')?;
        let end = trimmed.find('>')?;
        Some(trimmed[start + 1..end].to_string())
    })
}

fn header_u32(headers: &header::HeaderMap, name: &str) -> Option<u32> {
    headers.get(name)?.to_str().ok()?.parse::<u32>().ok()
}

fn header_u64(headers: &header::HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.parse::<u64>().ok()
}

fn truncate_error_preview(message: &str) -> String {
    message.chars().take(ERROR_BODY_PREVIEW_CHARS).collect()
}
