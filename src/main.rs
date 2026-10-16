use anyhow::{Context, Result};
use dotenv::dotenv;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;

// ─── Gitea API response types ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct GiteaPullRequest {
    pub id: i64,
    pub number: i64,
    pub title: String,
    pub html_url: String,
    pub state: String,
    pub base: GiteaBranch,
    pub head: GiteaBranch,
}

#[derive(Deserialize)]
pub struct GiteaBranch {
    pub label: String,
    #[serde(rename = "ref")]
    pub branch_ref: String,
}

#[derive(Deserialize)]
pub struct GiteaFileChange {
    pub filename: String,
    pub status: String,
    pub additions: i32,
    pub deletions: i32,
    pub patch: Option<String>,
}

// ─── AI response types ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct OpenAIResponse {
    pub choices: Vec<OpenAIChoice>,
}

#[derive(Deserialize)]
pub struct OpenAIChoice {
    pub message: OpenAIMessage,
}

#[derive(Deserialize)]
pub struct OpenAIMessage {
    pub content: String,
}

#[derive(Deserialize)]
pub struct AIReviewResponse {
    pub summary: String,
    pub issues: Vec<ReviewIssue>,
    pub event: String,
}

#[derive(Deserialize)]
pub struct ReviewIssue {
    pub path: String,
    pub line: i32,
    pub severity: String,
    pub body: String,
}

// ─── Gitea review submission types ──────────────────────────────────────────

#[derive(Serialize)]
pub struct GiteaReviewComment {
    pub path: String,
    pub body: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub new_position: i64,
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}

#[derive(Serialize)]
pub struct GiteaReview {
    pub body: String,
    pub event: String,
    pub comments: Vec<GiteaReviewComment>,
}

// ─── Gitea API calls ────────────────────────────────────────────────────────

async fn fetch_pr(
    client: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    pr_id: i64,
    base_url: &str,
) -> Result<GiteaPullRequest> {
    let url = format!(
        "{}/api/v1/repos/{}/{}/pulls/{}",
        base_url.trim_end_matches('/'),
        owner,
        repo,
        pr_id
    );
    let res = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()
        .context("Failed to fetch PR")?;
    let pr: GiteaPullRequest = res.json().await?;
    Ok(pr)
}

async fn fetch_pr_files(
    client: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    pr_id: i64,
    base_url: &str,
) -> Result<Vec<GiteaFileChange>> {
    let url = format!(
        "{}/api/v1/repos/{}/{}/pulls/{}/files",
        base_url.trim_end_matches('/'),
        owner,
        repo,
        pr_id
    );
    let res = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()
        .context("Failed to fetch PR files")?;
    let files: Vec<GiteaFileChange> = res.json().await?;
    Ok(files)
}

async fn fetch_pr_diff(
    client: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    pr_id: i64,
    base_url: &str,
) -> Result<String> {
    let url = format!(
        "{}/api/v1/repos/{}/{}/pulls/{}.diff",
        base_url.trim_end_matches('/'),
        owner,
        repo,
        pr_id
    );
    let res = client
        .get(&url)
        .bearer_auth(token)
        .header("Accept", "text/plain")
        .send()
        .await?
        .error_for_status()
        .context("Failed to fetch PR diff")?;
    let diff_text = res.text().await?;
    Ok(diff_text)
}   

// ─── AI reviewer ────────────────────────────────────────────────────────────

async fn call_ai_reviewer(
    client: &Client,
    diff_text: &str,
    pr_title: &str,
    base_branch: &str,
) -> Result<AIReviewResponse> {
    let openai_base_url = env::var("OPENAI_API_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".into());
    let openai_model = env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".into());
    let api_key = env::var("OPENAI_API_KEY").context("OPENAI_API_KEY not set")?;

    let system_prompt = r#"
You are a senior software engineer reviewing pull requests. Analyze the diff, identify bugs, logic errors, security issues, and style violations.
Provide specific, actionable feedback. Output strictly valid JSON matching this schema:
{
    "summary": "string",
    "issues": [
        {
            "path": "string (file path)",
            "line": integer (line number in the NEW file, 1-indexed),
            "severity": "critical" | "major" | "minor",
            "body": "string"
        }
    ],
    "event": "APPROVED" | "REQUEST_CHANGES" | "COMMENT"
}
Rules:
- Be concise. Only report real issues.
- If no issues, set issues to [] and event to "APPROVED".
- Line numbers must reference the new file (the + side of the diff).
- Do NOT include markdown code fences around your JSON output.
"#;

    let user_prompt = format!(
        "PR Title: {}\nBase Branch: {}\n\nDiff:\n{}",
        pr_title, base_branch, diff_text
    );

    let response = client
        .post(format!("{}/chat/completions", openai_base_url.trim_end_matches('/')))
        .bearer_auth(&api_key)
        .json(&serde_json::json!({
            "model": openai_model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "temperature": 0.2,
            "max_tokens": 4096,
            "response_format": {"type": "json_object"}
        }))
        .send()
        .await?
        .error_for_status()
        .context("AI API request failed")?;

    let json: OpenAIResponse = response.json().await?;
    let content = json.choices.first().context("No choices in AI response")?.message.content.clone();

    // Strip markdown code fences if the model added them despite instructions
    let content = content.trim();
    let content = content
        .strip_prefix("```json")
        .or_else(|| content.strip_prefix("```"))
        .unwrap_or(content);
    let content = content.strip_suffix("```").unwrap_or(content).trim();

    let review: AIReviewResponse = serde_json::from_str(content)
        .with_context(|| format!("Failed to parse AI review JSON: {}", content))?;
    Ok(review)
}

// ─── Submit review ──────────────────────────────────────────────────────────

async fn submit_review(
    client: &Client,
    token: &str,
    owner: &str,
    repo: &str,
    pr_id: i64,
    review: &AIReviewResponse,
    base_url: &str,
) -> Result<()> {
    let comments: Vec<GiteaReviewComment> = review
        .issues
        .iter()
        .map(|issue| GiteaReviewComment {
            path: issue.path.clone(),
            body: format!("[{}] {}", issue.severity.to_uppercase(), issue.body),
            new_position: issue.line as i64,
        })
        .collect();

    let gitea_review = GiteaReview {
        body: format!(
            "## AI Review Summary\n\n{}\n\n---\n*Generated by AI PR Reviewer*",
            review.summary
        ),
        event: review.event.clone(),
        comments,
    };

    let url = format!(
        "{}/api/v1/repos/{}/{}/pulls/{}/reviews",
        base_url.trim_end_matches('/'),
        owner,
        repo,
        pr_id
    );
    
    let res = client
        .post(&url)
        .bearer_auth(token)
        .json(&gitea_review)
        .send()
        .await?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await?;
        anyhow::bail!("Review submission failed ({}): {}", status, body);
    }           


    Ok(())
}

// ─── Main ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let client = Client::new();
    let owner = env::var("GITEA_OWNER").context("GITEA_OWNER not set")?;
    let repo = env::var("GITEA_REPO").context("GITEA_REPO not set")?;
    let token = env::var("GITEA_TOKEN").context("GITEA_TOKEN not set")?;
    let base_url = env::var("GITEA_BASE_URL").context("GITEA_BASE_URL not set")?;

    // Accept PR number as first CLI arg, default to 1
    let pr_id: i64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(1);

    tracing::info!("Fetching PR #{}...", pr_id);
    let pr = fetch_pr(&client, &token, &owner, &repo, pr_id, &base_url).await?;
    tracing::info!("PR: \"{}\" ({} → {})", pr.title, pr.head.branch_ref, pr.base.branch_ref);

    tracing::info!("Fetching changed files...");
    let files = fetch_pr_files(&client, &token, &owner, &repo, pr_id, &base_url).await?;
    tracing::info!("{} files changed", files.len());

    tracing::info!("Fetching PR diff...");
    let diff_text = fetch_pr_diff(&client, &token, &owner, &repo, pr_id, &base_url).await?;
    tracing::info!("Diff length: {} bytes", diff_text.len());

    tracing::info!("Calling AI reviewer...");
    let review = call_ai_reviewer(&client, &diff_text, &pr.title, &pr.base.branch_ref).await?;
    tracing::info!("AI verdict: {} ({} issues)", review.event, review.issues.len());

    tracing::info!("Submitting review...");
    submit_review(&client, &token, &owner, &repo, pr_id, &review, &base_url).await?;

    tracing::info!("✅ Review complete!");
    Ok(())
}   