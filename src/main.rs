use anyhow::{Context, Result};
use clap::Parser;
use dotenv::dotenv;
use std::env;

mod ai;
mod gitea;
mod github;

use gitea::GiteaClient;
use github::GithubClient;

/// AI-based PR reviewer for Gitea/GitHub
#[derive(Parser, Debug)]
#[command(name = "pr-reviewer-agent", version, about)]
struct Args {
    /// Repository type (gitea, github)
    #[arg(long)]
    repotype: String,

    /// Repository name (or owner/repo)
    #[arg(long)]
    repo: String,

    /// Pull request ID
    #[arg(long)]
    prid: i64,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args = Args::parse();

    match args.repotype.as_str() {
        "gitea" => run_gitea(&args).await,
        "github" => run_github(&args).await,
        other => anyhow::bail!("Unsupported repotype: {}", other),
    }
}

async fn run_gitea(args: &Args) -> Result<()> {
    let owner = env::var("GITEA_OWNER").context("GITEA_OWNER not set")?;
    let token = env::var("GITEA_TOKEN").context("GITEA_TOKEN not set")?;
    let base_url = env::var("GITEA_BASE_URL").context("GITEA_BASE_URL not set")?;

    let (owner, repo) = if args.repo.contains('/') {
        let parts: Vec<&str> = args.repo.splitn(2, '/').collect();
        (parts[0].to_string(), parts[1].to_string())
    } else {
        (owner, args.repo.clone())
    };

    let gitea = GiteaClient::new(token, owner, repo, base_url);

    tracing::info!("Fetching PR #{}...", args.prid);
    let pr = gitea.fetch_pr(args.prid).await?;
    tracing::info!(
        "PR: \"{}\" ({} → {}) [{}]",
        pr.title,
        pr.head.branch_ref,
        pr.base.branch_ref,
        pr.state
    );
    tracing::info!("  URL: {}", pr.html_url);

    tracing::info!("Fetching changed files...");
    let files = gitea.fetch_pr_files(args.prid).await?;
    tracing::info!("{} files changed:", files.len());
    for f in &files {
        tracing::info!("  {} (+{}/-{})", f.filename, f.additions, f.deletions);
    }

    tracing::info!("Fetching PR diff...");
    let diff_text = gitea.fetch_pr_diff(args.prid).await?;
    let diff_text = if diff_text.len() > 5000 {
        tracing::warn!("Diff truncated from {} to 5000 chars", diff_text.len());
        diff_text.chars().take(5000).collect::<String>()
    } else {
        diff_text
    };
    tracing::info!("Diff length: {} chars", diff_text.len());

    tracing::info!("Calling AI reviewer...");
    let client = reqwest::Client::new();
    let review = ai::call_ai_reviewer(&client, &diff_text, &pr.title, &pr.base.branch_ref).await?;
    tracing::info!(
        "AI verdict: {} ({} issues)",
        review.event,
        review.issues.len()
    );

    tracing::info!("Submitting review...");
    gitea.submit_review(args.prid, &review).await?;

    tracing::info!("✅ Review complete!");
    Ok(())
}

async fn run_github(args: &Args) -> Result<()> {
    let token = env::var("GITHUB_TOKEN").context("GITHUB_TOKEN not set")?;

    let (owner, repo) = if args.repo.contains('/') {
        let parts: Vec<&str> = args.repo.splitn(2, '/').collect();
        (parts[0].to_string(), parts[1].to_string())
    } else {
        anyhow::bail!("GitHub requires 'owner/repo' format for --repo");
    };

    let github = GithubClient::new(token, owner, repo);

    tracing::info!("Fetching PR #{}...", args.prid);
    let pr = github.fetch_pr(args.prid).await?;
    tracing::info!(
        "PR: \"{}\" ({} → {}) [{}]",
        pr.title,
        pr.head.ref_name,
        pr.base.ref_name,
        pr.state
    );
    tracing::info!("  URL: {}", pr.html_url);

    tracing::info!("Fetching changed files...");
    let files = github.fetch_pr_files(args.prid).await?;
    tracing::info!("{} files changed:", files.len());
    for f in &files {
        tracing::info!("  {} (+{}/-{})", f.filename, f.additions, f.deletions);
    }

    tracing::info!("Fetching PR diff...");
    let diff_text = github.fetch_pr_diff(args.prid).await?;
    let diff_text = if diff_text.len() > 5000 {
        tracing::warn!("Diff truncated from {} to 5000 chars", diff_text.len());
        diff_text.chars().take(5000).collect::<String>()
    } else {
        diff_text
    };
    tracing::info!("Diff length: {} chars", diff_text.len());

    tracing::info!("Calling AI reviewer...");
    let client = reqwest::Client::new();
    let review = ai::call_ai_reviewer(&client, &diff_text, &pr.title, &pr.base.ref_name).await?;
    tracing::info!(
        "AI verdict: {} ({} issues)",
        review.event,
        review.issues.len()
    );

    tracing::info!("Submitting review...");
    github.submit_review(args.prid, &review).await?;

    tracing::info!("✅ Review complete!");
    Ok(())
}   