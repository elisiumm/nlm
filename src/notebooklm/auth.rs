// auth.rs — load saved Playwright cookies and fetch NotebookLM session tokens.
//
// Two-step process (same as notebooklm-py auth.py):
//   1. Load ~/.notebooklm/storage_state.json → extract Google-domain cookies
//   2. GET https://notebooklm.google.com/ with those cookies
//      → regex-extract "SNlM0e":"<token>" and "FdrFJe":"<session_id>"
//
// Rust concepts:
//   - serde_json::Value for untyped JSON traversal (same pattern as Confluence adapter)
//   - regex::Regex (compiled once) for text extraction
//   - HashMap<String, String> for cookie → header assembly
//   - std::fs for sync file read (small file, no async needed)

use anyhow::{Context, Result};
use regex::Regex;
use std::path::PathBuf;

/// All session material needed for every RPC call.
#[derive(Debug, Clone)]
pub struct AuthTokens {
    /// Raw cookies as a single `Cookie: k=v; k2=v2` header value.
    pub cookie_header: String,
    /// CSRF token — passed as `at=<snlm0e>` in every batchexecute call.
    pub snlm0e: String,
    /// Session/stream ID — passed as `f.sid=<fdrf_je>` in URL params.
    pub fdrfje: String,
}

/// Default path for the Playwright storage state file.
pub fn default_storage_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join(".notebooklm")
        .join("storage_state.json")
}

/// Load cookies from `storage_state.json` and fetch CSRF + session tokens
/// from the NotebookLM homepage.
pub async fn load_tokens(storage_path: Option<&std::path::Path>) -> Result<AuthTokens> {
    let path = storage_path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(default_storage_path);

    // Sync read is fine for a small JSON config file.
    let raw = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "Cannot read storage state: {}\n  Run: nlm login",
            path.display()
        )
    })?;

    let state: serde_json::Value =
        serde_json::from_str(&raw).context("storage_state.json is not valid JSON")?;

    // Playwright storage_state format:
    // { "cookies": [{ "name": "...", "value": "...", "domain": "...", ... }], ... }
    let cookies = state["cookies"]
        .as_array()
        .context("storage_state.json: missing 'cookies' array")?;

    // Filter to Google domains only (same as Python: domain.endswith(".google.com"))
    let cookie_header = cookies
        .iter()
        .filter(|c| {
            c["domain"]
                .as_str()
                .map(|d| d.ends_with(".google.com") || d.ends_with("google.com"))
                .unwrap_or(false)
        })
        .filter_map(|c| {
            let name = c["name"].as_str()?;
            let value = c["value"].as_str()?;
            Some(format!("{name}={value}"))
        })
        .collect::<Vec<_>>()
        .join("; ");

    if cookie_header.is_empty() {
        anyhow::bail!(
            "No Google cookies found in {}.\n  Run: nlm login",
            path.display()
        );
    }

    // Fetch homepage to get CSRF tokens.
    let (snlm0e, fdrfje) = fetch_csrf_tokens(&cookie_header).await?;

    Ok(AuthTokens {
        cookie_header,
        snlm0e,
        fdrfje,
    })
}

// ── CSRF token extraction ──────────────────────────────────────────────────

const NOTEBOOKLM_URL: &str = "https://notebooklm.google.com/";

async fn fetch_csrf_tokens(cookie_header: &str) -> Result<(String, String)> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let html = client
        .get(NOTEBOOKLM_URL)
        .header(reqwest::header::COOKIE, cookie_header)
        // Impersonate a real browser to avoid bot detection.
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/120.0.0.0 Safari/537.36",
        )
        .send()
        .await
        .context("Failed to reach NotebookLM homepage")?
        .text()
        .await
        .context("Failed to read NotebookLM homepage body")?;

    // Extract "SNlM0e":"<token>" — the CSRF token.
    // Regex::new() is cheap to compile for small patterns; called once per login.
    let snlm0e = extract_token(&html, "SNlM0e")
        .context("SNlM0e token not found — cookies may be expired. Run: nlm login")?;

    // Extract "FdrFJe":"<session_id>"
    let fdrfje = extract_token(&html, "FdrFJe")
        .context("FdrFJe token not found — cookies may be expired. Run: nlm login")?;

    Ok((snlm0e, fdrfje))
}

/// Extract the value of a JSON-embedded token like `"KEY":"VALUE"` from an HTML page.
fn extract_token(html: &str, key: &str) -> Option<String> {
    // Pattern: "KEY":"anything-that-is-not-a-quote"
    // The value may contain escaped characters but not a raw '"'.
    let pattern = format!(r#""{key}":"([^"]+)""#);
    // unwrap: pattern is always valid since key comes from our own constants
    let re = Regex::new(&pattern).unwrap();
    re.captures(html)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
}

// ── Cookie Jar (for authenticated downloads across redirect chains) ────────────
//
// Problem: reqwest does NOT forward a raw `Cookie` header when following HTTP
// redirects to a different domain (correct security behavior). So the download
// URL on `contribution.usercontent.google.com` gets no cookies after a redirect.
//
// Solution: use reqwest's cookie_store backed by a `Jar` pre-populated with
// all Google-family cookies, each associated with the right domain URL.
// The Jar sends cookies per-domain on every request including redirects.
//
// This mirrors Python's `httpx.Cookies().set(name, value, domain=domain)`.

/// Build a reqwest cookie Jar from the storage_state.json cookies.
///
/// Each cookie is set for its own domain (e.g. `.google.com`,
/// `.googleusercontent.com`), so reqwest sends them automatically on
/// redirects to any Google-family domain.
pub fn build_cookie_jar(storage_path: Option<&std::path::Path>) -> Result<reqwest::cookie::Jar> {
    let path = storage_path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(default_storage_path);

    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("Cannot read storage state: {}", path.display()))?;
    let state: serde_json::Value = serde_json::from_str(&raw)?;

    let jar = reqwest::cookie::Jar::default();
    let cookies = state["cookies"].as_array().unwrap_or(&vec![]).clone();

    for c in &cookies {
        let domain = c["domain"].as_str().unwrap_or("");
        let name = c["name"].as_str().unwrap_or("");
        let value = c["value"].as_str().unwrap_or("");

        if name.is_empty() || value.is_empty() {
            continue;
        }

        // Accept all Google-family domains:
        // .google.com, .googleapis.com, .googleusercontent.com, etc.
        let is_google = domain.ends_with(".google.com")
            || domain.ends_with("google.com")
            || domain.ends_with(".googleapis.com")
            || domain.ends_with(".googleusercontent.com")
            || domain.ends_with("googleusercontent.com");

        if !is_google {
            continue;
        }

        // add_cookie_str takes a full Set-Cookie header value.
        // Including "Domain=<domain>" makes it a wildcard domain cookie —
        // reqwest will then send it to ALL subdomains (e.g. contribution.usercontent.google.com
        // matches Domain=.google.com).
        // Without the Domain attribute it would only match the exact host in the URL.
        let host = domain.trim_start_matches('.');
        let url_str = format!("https://{host}/");
        if let Ok(url) = url_str.parse::<reqwest::Url>() {
            jar.add_cookie_str(&format!("{name}={value}; Domain={domain}; Path=/"), &url);
        }
    }

    Ok(jar)
}
