use anyhow::{Context, Result};
use clap::Parser;
use dotenv::dotenv;
use std::env;

mod ai;
mod gitea;

use gitea::GiteaClient;

/// AI-based PR reviewer for Gitea
#[derive(Parser, Debug)]
#[command(name = "pr-reviewer-agent", version, about)]
struct Args {
    /// Repository type (e.g. gitea)
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
        other => anyhow::bail!("Unsupported repotype: {}", other),
    }
}

async fn run_gitea(args: &Args) -> Result<()> {
    let owner = env::var("GITEA_OWNER").context("GITEA_OWNER not set")?;
    let token = env::var("GITEA_TOKEN").context("GITEA_TOKEN not set")?;
    let base_url = env::var("GITEA_BASE_URL").context("GITEA_BASE_URL not set")?;

    // Support "owner/repo" format in --repo
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
        "PR: {},{},\"{}\" ({} → {}),{},{}",
        pr.id,
        pr.number,
        pr.title,
        pr.head.branch_ref,
        pr.base.branch_ref,
        pr.html_url,
        pr.state
    );

    tracing::info!("Fetching changed files...");
    let files = gitea.fetch_pr_files(args.prid).await?;
    tracing::info!("{} files changed", files.len());

    tracing::info!("Fetching PR diff...");
    let diff_text = gitea.fetch_pr_diff(args.prid).await?;
    tracing::info!("Diff length: {} bytes", diff_text.len());

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