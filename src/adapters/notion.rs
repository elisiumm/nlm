// NotionAdapter — fetches a Notion page and converts it to Markdown.
//
// Uses the Notion REST API (https://api.notion.com) directly — no SDK.
// Mirrors the direct-reqwest style of confluence.rs.
//
// API docs: https://developers.notion.com/reference/retrieve-a-page
//           https://developers.notion.com/reference/get-block-children
//
// Rust concepts:
//   - Box<dyn Future> via .boxed() to recurse in an async fn (async recursion
//     would otherwise yield an infinitely-sized future).
//   - serde_json::Value walking the untyped Notion response tree.
//   - String accumulation via writeln!() on a String buffer.

use anyhow::{Context, Result};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::{safe_filename, SourceAdapter};
use crate::config::Source;

const NOTION_API: &str = "https://api.notion.com/v1";
const NOTION_VERSION: &str = "2022-06-28";

pub struct NotionAdapter;

impl SourceAdapter for NotionAdapter {
    async fn fetch(&self, source: &Source, output_dir: &Path) -> Result<Option<PathBuf>> {
        let (id, title) = match source {
            Source::Notion { id, title } => (id.as_str(), title.as_str()),
            _ => unreachable!("NotionAdapter called with non-Notion source"),
        };

        let token = std::env::var("NOTION_TOKEN").context(
            "NOTION_TOKEN is required in .env to sync Notion sources.\n  \
             Create an integration at https://www.notion.so/my-integrations,\n  \
             copy the secret, then share the target page with the integration.",
        )?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to build HTTP client")?;

        // Prefer the page's real Notion title; fall back to the config title
        // if the page has no title property or the request fails (e.g. the
        // integration hasn't been shared with the page — we still want a
        // helpful error when fetching blocks next).
        let page_title = fetch_page_title(&client, id, &token).await.ok().flatten();
        let display_title = page_title.as_deref().unwrap_or(title);

        let mut body = String::new();
        render_children(&client, id, &token, 0, &mut body).await?;

        let md = format!("# {display_title}\n\n{body}");
        let out_file = output_dir.join(format!("{}.md", safe_filename(title)));
        tokio::fs::write(&out_file, md).await?;

        Ok(Some(out_file))
    }
}

// ── Page title ────────────────────────────────────────────────────────────────

/// Fetch the page's human-readable title.
///
/// Notion pages expose their title via a property whose `type` is `"title"`.
/// The property key is the workspace-assigned name ("title", "Name", or the
/// column name for DB rows) — we don't know it up front, so we iterate.
async fn fetch_page_title(
    client: &reqwest::Client,
    page_id: &str,
    token: &str,
) -> Result<Option<String>> {
    let url = format!("{NOTION_API}/pages/{page_id}");
    let resp = client
        .get(&url)
        .bearer_auth(token)
        .header("Notion-Version", NOTION_VERSION)
        .send()
        .await
        .with_context(|| format!("Failed to reach Notion page {page_id}"))?
        .error_for_status()
        .with_context(|| format!("Notion API error on page {page_id}"))?;

    let data: serde_json::Value = resp.json().await?;
    let props = data.get("properties").and_then(|v| v.as_object());

    let Some(props) = props else { return Ok(None) };
    for (_, prop) in props {
        if prop.get("type").and_then(|t| t.as_str()) == Some("title") {
            let plain = prop
                .get("title")
                .and_then(|arr| arr.as_array())
                .map(|items| collect_rich_text_plain(items))
                .unwrap_or_default();
            if !plain.is_empty() {
                return Ok(Some(plain));
            }
        }
    }
    Ok(None)
}

// ── Blocks → Markdown ─────────────────────────────────────────────────────────

/// Fetch all children of `block_id` (paginated) and render them into `out`
/// at the given `indent` level.
///
/// Async recursion in Rust requires boxing the recursive future because the
/// compiler can't compute the size of a type that holds itself. We box by
/// wrapping the call in `Box::pin(...)`.
async fn render_children(
    client: &reqwest::Client,
    block_id: &str,
    token: &str,
    indent: usize,
    out: &mut String,
) -> Result<()> {
    let mut cursor: Option<String> = None;
    loop {
        let mut url = format!("{NOTION_API}/blocks/{block_id}/children?page_size=100");
        if let Some(c) = &cursor {
            write!(url, "&start_cursor={c}").ok();
        }

        let resp = client
            .get(&url)
            .bearer_auth(token)
            .header("Notion-Version", NOTION_VERSION)
            .send()
            .await
            .with_context(|| format!("Failed to fetch children of block {block_id}"))?
            .error_for_status()
            .with_context(|| format!("Notion API error on block {block_id}"))?;

        let page: serde_json::Value = resp.json().await?;

        if let Some(results) = page.get("results").and_then(|v| v.as_array()) {
            for block in results {
                render_block(client, block, token, indent, out).await?;
            }
        }

        let has_more = page
            .get("has_more")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !has_more {
            break;
        }
        cursor = page
            .get("next_cursor")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if cursor.is_none() {
            break;
        }
    }
    Ok(())
}

/// Render a single block into `out` and recurse into its children if any.
///
/// Returns a boxed future to sidestep the infinite-size problem on recursion
/// (a block's children may themselves contain blocks with children…).
fn render_block<'a>(
    client: &'a reqwest::Client,
    block: &'a serde_json::Value,
    token: &'a str,
    indent: usize,
    out: &'a mut String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        let Some(block_type) = block.get("type").and_then(|t| t.as_str()) else {
            return Ok(());
        };
        let data = block
            .get(block_type)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let prefix = " ".repeat(indent);

        match block_type {
            "paragraph" => {
                let text = render_rich_text(data.get("rich_text"));
                if !text.is_empty() {
                    writeln!(out, "{prefix}{text}\n").ok();
                }
            }
            "heading_1" => {
                let text = render_rich_text(data.get("rich_text"));
                writeln!(out, "{prefix}# {text}\n").ok();
            }
            "heading_2" => {
                let text = render_rich_text(data.get("rich_text"));
                writeln!(out, "{prefix}## {text}\n").ok();
            }
            "heading_3" => {
                let text = render_rich_text(data.get("rich_text"));
                writeln!(out, "{prefix}### {text}\n").ok();
            }
            "bulleted_list_item" => {
                let text = render_rich_text(data.get("rich_text"));
                writeln!(out, "{prefix}- {text}").ok();
            }
            "numbered_list_item" => {
                // Markdown renderers renumber automatically — "1." everywhere is fine.
                let text = render_rich_text(data.get("rich_text"));
                writeln!(out, "{prefix}1. {text}").ok();
            }
            "to_do" => {
                let checked = data
                    .get("checked")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let mark = if checked { "x" } else { " " };
                let text = render_rich_text(data.get("rich_text"));
                writeln!(out, "{prefix}- [{mark}] {text}").ok();
            }
            "toggle" => {
                // Markdown has no toggle; degrade to a bold line then render
                // the children normally so nothing is lost.
                let text = render_rich_text(data.get("rich_text"));
                writeln!(out, "{prefix}**{text}**\n").ok();
            }
            "quote" => {
                let text = render_rich_text(data.get("rich_text"));
                writeln!(out, "{prefix}> {text}\n").ok();
            }
            "callout" => {
                let text = render_rich_text(data.get("rich_text"));
                writeln!(out, "{prefix}> {text}\n").ok();
            }
            "code" => {
                let lang = data.get("language").and_then(|v| v.as_str()).unwrap_or("");
                let text = render_rich_text(data.get("rich_text"));
                writeln!(out, "{prefix}```{lang}\n{text}\n{prefix}```\n").ok();
            }
            "divider" => {
                writeln!(out, "{prefix}---\n").ok();
            }
            "equation" => {
                let expr = data
                    .get("expression")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                writeln!(out, "{prefix}$$\n{expr}\n$$\n").ok();
            }
            "bookmark" => {
                let url = data.get("url").and_then(|v| v.as_str()).unwrap_or("");
                if !url.is_empty() {
                    writeln!(out, "{prefix}<{url}>\n").ok();
                }
            }
            "image" => {
                let url = extract_file_url(&data).unwrap_or_default();
                let caption = render_rich_text(data.get("caption"));
                if !url.is_empty() {
                    writeln!(out, "{prefix}![{caption}]({url})\n").ok();
                }
            }
            "child_page" => {
                let title = data
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("untitled");
                writeln!(out, "{prefix}_[child page: {title}]_\n").ok();
            }
            "child_database" => {
                let title = data
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("untitled");
                writeln!(out, "{prefix}_[child database: {title}]_\n").ok();
            }
            "unsupported" => {
                writeln!(out, "{prefix}_[unsupported Notion block skipped]_\n").ok();
            }
            // Tables, embeds, videos, files, synced blocks, columns, etc.
            // Fall through to a best-effort plain-text dump.
            _ => {
                let text = render_rich_text(data.get("rich_text"));
                if !text.is_empty() {
                    writeln!(out, "{prefix}{text}\n").ok();
                }
            }
        }

        // Recurse into children when the block has any.
        let has_children = block
            .get("has_children")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if has_children {
            // List items indent 2 more spaces; other blocks keep the same level
            // so a toggle's children render flush with the toggle label.
            let next_indent = match block_type {
                "bulleted_list_item" | "numbered_list_item" | "to_do" => indent + 2,
                _ => indent,
            };
            if let Some(child_id) = block.get("id").and_then(|v| v.as_str()) {
                render_children(client, child_id, token, next_indent, out).await?;
            }
        }

        Ok(())
    })
}

// ── Rich text ─────────────────────────────────────────────────────────────────

/// Render a `rich_text` array (Notion's inline text representation) into
/// Markdown with bold / italic / code / strikethrough / link annotations.
fn render_rich_text(value: Option<&serde_json::Value>) -> String {
    let Some(arr) = value.and_then(|v| v.as_array()) else {
        return String::new();
    };
    let mut out = String::new();
    for item in arr {
        let plain = item
            .get("plain_text")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if plain.is_empty() {
            continue;
        }
        let ann = item.get("annotations");
        let bold = ann
            .and_then(|a| a.get("bold"))
            .and_then(|b| b.as_bool())
            .unwrap_or(false);
        let italic = ann
            .and_then(|a| a.get("italic"))
            .and_then(|b| b.as_bool())
            .unwrap_or(false);
        let strike = ann
            .and_then(|a| a.get("strikethrough"))
            .and_then(|b| b.as_bool())
            .unwrap_or(false);
        let code = ann
            .and_then(|a| a.get("code"))
            .and_then(|b| b.as_bool())
            .unwrap_or(false);
        let href = item
            .get("href")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());

        let mut piece = plain.to_string();
        if code {
            piece = format!("`{piece}`");
        }
        if strike {
            piece = format!("~~{piece}~~");
        }
        if italic {
            piece = format!("*{piece}*");
        }
        if bold {
            piece = format!("**{piece}**");
        }
        if let Some(link) = href {
            piece = format!("[{piece}]({link})");
        }
        out.push_str(&piece);
    }
    out
}

/// Collect an array of rich_text items as plain text only (no formatting).
/// Used for page titles.
fn collect_rich_text_plain(items: &[serde_json::Value]) -> String {
    items
        .iter()
        .filter_map(|it| it.get("plain_text").and_then(|v| v.as_str()))
        .collect::<Vec<_>>()
        .join("")
}

/// Resolve the URL of a Notion file/image block (hosted or external).
fn extract_file_url(data: &serde_json::Value) -> Option<String> {
    if let Some(url) = data.pointer("/file/url").and_then(|v| v.as_str()) {
        return Some(url.to_string());
    }
    if let Some(url) = data.pointer("/external/url").and_then(|v| v.as_str()) {
        return Some(url.to_string());
    }
    None
}
