// ConfluenceAdapter — fetches a Confluence page via REST API and writes Markdown.
// Mirrors nlm/adapters/confluence.py (REST path only — MCP not yet supported).
//
// Rust concepts:
//   - serde_json::Value: untyped JSON tree (like Python's dict from resp.json())
//   - .basic_auth(): reqwest handles Base64 encoding of user:token automatically
//   - String ownership: env vars return owned Strings; we borrow with .as_str()
//   - Nested JSON access with index operator: data["body"]["export_view"]["value"]

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config::Source;
use super::{safe_filename, SourceAdapter};

pub struct ConfluenceAdapter;

impl SourceAdapter for ConfluenceAdapter {
    async fn fetch(&self, source: &Source, output_dir: &Path) -> Result<Option<PathBuf>> {
        let (id, title, step, step_label, src_base_url) = match source {
            Source::Confluence { id, title, step, step_label, base_url } => {
                (id.as_str(), title.as_str(), *step, step_label.as_deref(), base_url.as_deref())
            }
            _ => unreachable!("ConfluenceAdapter called with non-Confluence source"),
        };

        let out_file = output_dir.join(format!("{}.md", safe_filename(title)));

        let user = std::env::var("CONFLUENCE_USER").ok();
        let rest_token = std::env::var("CONFLUENCE_TOKEN").ok();
        let mcp_token = std::env::var("ATLASSIAN_MCP_TOKEN").ok();

        // Credential dispatch — same priority as the Python adapter
        match (user, rest_token, mcp_token) {
            (Some(user), Some(token), _) => {
                fetch_rest(id, title, step, step_label, src_base_url, &user, &token, &out_file)
                    .await?;
            }
            (_, _, Some(_)) => {
                // MCP requires the `mcp` crate with async streaming sessions.
                // Deferred to a future phase once we have Phase 3 async patterns in place.
                anyhow::bail!(
                    "ATLASSIAN_MCP_TOKEN found but MCP is not yet supported in the Rust CLI.\n\
                     Use CONFLUENCE_USER + CONFLUENCE_TOKEN (REST API) instead."
                );
            }
            _ => {
                anyhow::bail!(
                    "No Confluence credentials found in .env.\n  \
                     Option A: CONFLUENCE_USER + CONFLUENCE_TOKEN\n  \
                     Option B: ATLASSIAN_MCP_TOKEN (not yet supported)"
                );
            }
        }

        Ok(Some(out_file))
    }
}

// ── REST fetch ────────────────────────────────────────────────────────────────

async fn fetch_rest(
    page_id: &str,
    title: &str,
    step: Option<u32>,
    step_label: Option<&str>,
    src_base_url: Option<&str>,
    user: &str,
    token: &str,
    out_file: &Path,
) -> Result<()> {
    // Resolve base URL: source config → CONFLUENCE_BASE_URL env → hardcoded default
    let default_base = std::env::var("CONFLUENCE_BASE_URL")
        .unwrap_or_else(|_| "https://rossel-applications.atlassian.net".to_string());
    let base_url = src_base_url
        .unwrap_or(default_base.as_str())
        .trim_end_matches('/');

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let api_url = format!("{base_url}/wiki/rest/api/content/{page_id}");

    // .basic_auth() sets the Authorization: Basic <base64(user:token)> header.
    // reqwest handles the encoding — we just pass the raw credentials.
    let resp = client
        .get(&api_url)
        .basic_auth(user, Some(token))
        .query(&[("expand", "body.export_view,title")])
        .send()
        .await
        .with_context(|| format!("Failed to reach Confluence page {page_id}"))?;

    resp.error_for_status_ref()
        .with_context(|| format!("Confluence API error for page {page_id}"))?;

    // Deserialize into an untyped JSON tree.
    // serde_json::Value lets us navigate arbitrary JSON with ["key"] indexing,
    // same as Python's resp.json()["body"]["export_view"]["value"].
    let data: serde_json::Value = resp.json().await?;

    let html = data["body"]["export_view"]["value"]
        .as_str()
        .context("Missing body.export_view.value in Confluence response")?;

    let page_title = data["title"].as_str().unwrap_or(title);

    let raw = htmd::convert(html).unwrap_or_else(|_| html.to_string());

    // Only prepend the "# Title" heading when there's no step marker.
    // (With a step, build_md_with_step adds its own headings.)
    let body = if step.is_some() {
        raw
    } else {
        format!("# {page_title}\n\n{raw}")
    };

    let md = build_md_with_step(title, &body, step, step_label);
    tokio::fs::write(out_file, md).await?;

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Prepend "## Étape N — <label>" and "### <title>" when step metadata is present.
/// Mirrors `_build_md_with_step()` in the Python adapter.
fn build_md_with_step(title: &str, body: &str, step: Option<u32>, step_label: Option<&str>) -> String {
    match (step, step_label) {
        (Some(n), Some(label)) => format!("## Étape {n} — {label}\n\n### {title}\n\n{body}"),
        _ => body.to_string(),
    }
}
