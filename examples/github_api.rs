#![allow(dead_code, unused)]

use fetch_happen::{get, Client, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct GitHubBranch {
    name: String,
    commit: Commit,
}

#[derive(Debug, Deserialize)]
struct Commit {
    sha: String,
    url: String,
}

/// Simple GET request using convenience function
async fn get_github_branch(repo: String) -> Result<GitHubBranch> {
    let url = format!("https://api.github.com/repos/{}/branches/master", repo);

    let response = get(&url).await?.error_for_status()?;

    response.json().await
}

/// Advanced GET request with custom headers
async fn get_github_branch_advanced(repo: String) -> Result<GitHubBranch> {
    let client = Client;
    let url = format!("https://api.github.com/repos/{}/branches/master", repo);

    let response = client
        .get(url)
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "rust-wasm-fetch")
        .send()
        .await?
        .error_for_status()?;

    response.json().await
}

#[derive(Serialize)]
struct CreateIssue {
    title: String,
    body: String,
}

/// POST request with JSON body
async fn create_github_issue(repo: String, title: String, body: String) -> Result<Value> {
    let client = Client;
    let url = format!("https://api.github.com/repos/{}/issues", repo);

    let issue = CreateIssue { title, body };

    let response = client
        .post(url)
        .header("Accept", "application/vnd.github.v3+json")
        .header("Authorization", "token YOUR_GITHUB_TOKEN")
        .json(&issue)?
        .send()
        .await?
        .error_for_status()?;

    response.json_value().await
}

fn main() {}
