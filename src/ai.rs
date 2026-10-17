use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::env;

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

// ─── AI reviewer ────────────────────────────────────────────────────────────

pub async fn call_ai_reviewer(
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
    "event": "COMMENT"
}
Rules:
- Be concise. Only report real issues.
- If no issues, set issues to [] and event to "COMMENT".
- Line numbers must reference the new file (the + side of the diff).
- Do NOT include markdown code fences around your JSON output.
"#;

    let user_prompt = format!(
        "PR Title: {}\nBase Branch: {}\n\nDiff:\n{}",
        pr_title, base_branch, diff_text
    );

    let response = client
        .post(format!(
            "{}/chat/completions",
            openai_base_url.trim_end_matches('/')
        ))
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
    let content = json
        .choices
        .first()
        .context("No choices in AI response")?
        .message
        .content
        .clone();

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