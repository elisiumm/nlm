// UrlAdapter — fetches a public URL and converts it to Markdown.
// Mirrors nlm/adapters/url.py.
//
// Rust concepts:
//   - reqwest::Client for async HTTP (builder pattern)
//   - htmd::convert() for HTML → Markdown
//   - &str vs String: function parameters use &str (borrowed), owned data uses String
//   - tokio::fs::write for async file I/O

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config::Source;
use super::{safe_filename, SourceAdapter};

pub struct UrlAdapter;

impl SourceAdapter for UrlAdapter {
    async fn fetch(&self, source: &Source, output_dir: &Path) -> Result<Option<PathBuf>> {
        // Destructure the enum variant we care about.
        // `unreachable!` documents the invariant: the dispatcher in mod.rs
        // guarantees this adapter is only called with Source::Url.
        let (url, title) = match source {
            Source::Url { url, title } => (url.as_str(), title.as_str()),
            _ => unreachable!("UrlAdapter called with non-Url source"),
        };

        let out_file = output_dir.join(format!("{}.md", safe_filename(title)));

        // Build a reusable client with shared config.
        // reqwest::Client is cheap to clone but expensive to create — in a real
        // app you'd share it across requests. Fine to create per-call here.
        let client = reqwest::Client::builder()
            .user_agent("nlm-toolkit/1.0")
            .redirect(reqwest::redirect::Policy::limited(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to build HTTP client")?;

        let resp = client
            .get(url)
            .send()
            .await
            .with_context(|| format!("Failed to fetch {url}"))?;

        // Check HTTP status — error_for_status() consumes the response,
        // error_for_status_ref() borrows it so we can still read the body.
        resp.error_for_status_ref()
            .with_context(|| format!("HTTP error fetching {url}"))?;

        // Extract Content-Type before consuming the response with .text()
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let text = resp.text().await.context("Failed to read response body")?;

        // Convert HTML to Markdown if needed; use raw text otherwise
        let body = if content_type.contains("text/html") {
            // htmd::convert returns Result<String, _>; fall back to raw on error
            htmd::convert(&text).unwrap_or(text)
        } else {
            text
        };

        // Compose the final Markdown — same format as the Python adapter
        let md = format!("# {title}\n\n_Source: {url}_\n\n---\n\n{body}");
        tokio::fs::write(&out_file, md).await?;

        Ok(Some(out_file))
    }
}
