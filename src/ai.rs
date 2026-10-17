use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::env;

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

/// Adds line numbers to + lines in a unified diff so the AI can reference them.
/// Format: `  42:+code here` (line number, colon, original diff content)
fn number_diff_lines(diff: &str) -> String {
    let mut result = String::with_capacity(diff.len() + 256);
    let mut new_line: i32 = 0;

    for line in diff.lines() {
        if line.starts_with("@@") {
            // Parse the new-file starting line from hunk header: @@ -a,b +c,d @@
            if let Some(plus_part) = line.split('+').nth(1) {
                let start = plus_part
                    .split(',')
                    .next()
                    .and_then(|s| s.trim().parse::<i32>().ok())
                    .unwrap_or(1);
                new_line = start;
            }
            result.push_str(line);
            result.push('\n');
        } else if line.starts_with('+') && !line.starts_with("+++") {
            new_line += 1;
            result.push_str(&format!("{:4}:{}", new_line, line));
            result.push('\n');
        } else if line.starts_with('-') && !line.starts_with("---") {
            // Removed line — no new-file line number
            result.push_str(line);
            result.push('\n');
        } else if line.starts_with('\\') {
            // "\ No newline at end of file"
            result.push_str(line);
            result.push('\n');
        } else {
            // Context line
            new_line += 1;
            result.push_str(line);
            result.push('\n');
        }
    }

    result
}

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

    let numbered_diff = number_diff_lines(diff_text);

    let system_prompt = r#"
You are a senior software engineer reviewing pull requests. Analyze the diff, identify bugs, logic errors, security issues, and style violations.
Provide specific, actionable feedback. Output strictly valid JSON matching this schema:
{
    "summary": "string",
    "issues": [
        {
            "path": "string (file path)",
            "line": integer,
            "severity": "critical" | "major" | "minor",
            "body": "string"
        }
    ],
    "event": "COMMENT"
}

CRITICAL RULES FOR LINE NUMBERS:
- The diff shows line numbers prefixed to each added (+) line, e.g. "  42:+let x = 5;"
- The "line" field MUST be one of those numbered lines — a line number that is explicitly shown in the diff.
- NEVER invent or guess line numbers. Only use numbers you can see in the diff.
- If an issue spans multiple lines, pick the first numbered line where the problem starts.
- If you cannot identify a specific numbered line, do NOT include that issue.

Other rules:
- Be concise. Only report real issues.
- If no issues, set issues to [] and event to "COMMENT".
- Do NOT include markdown code fences around your JSON output.
"#;

    let user_prompt = format!(
        "PR Title: {}\nBase Branch: {}\n\nNumbered Diff (line numbers shown for + lines):\n{}",
        pr_title, base_branch, numbered_diff
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