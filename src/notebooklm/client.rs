// client.rs — NotebookLMClient: all notebook/source/artifact operations.
//
// All params were validated against notebooklm-py source code (v0.x).
// Key lesson: many RPC calls need source-path = "/notebook/{id}" in the URL,
// not just "/". The Python _core.rpc_call(source_path=...) parameter controls this.
//
// Rust concepts in this file:
//   - `impl NotebookLMClient` with `&self` methods (shared reference)
//   - `tokio::time::sleep` + `std::time::Duration` for polling
//   - serde_json::json! for building JSON params inline
//   - Indexing into serde_json::Value: value[0], value["key"]
//   - format! to build dynamic source_path strings

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;

use crate::notebooklm::auth::{build_cookie_jar, AuthTokens};
use crate::notebooklm::rpc::{self, *};
use std::sync::Arc;

// ── NotebookLMClient ────────────────────────────────────────────────────────

pub struct NotebookLMClient {
    tokens: AuthTokens,
    http: reqwest::Client,
    /// When true, print the raw RPC response body to stderr.
    pub debug: bool,
}

impl NotebookLMClient {
    pub fn new(tokens: AuthTokens) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            tokens,
            http,
            debug: false,
        })
    }

    // ── Core RPC call ──────────────────────────────────────────────────────

    /// Send one batchexecute RPC call and decode the response.
    ///
    /// `source_path` is included in the URL query string (mirrors the browser page URL).
    /// Use "/" for homepage-context calls (list notebooks, create notebook).
    /// Use "/notebook/{id}" for calls made from inside a notebook.
    async fn rpc(&self, method_id: &str, params: &Value, source_path: &str) -> Result<Value> {
        let url = rpc::rpc_url(method_id, &self.tokens.fdrfje, source_path);
        let body = rpc::rpc_body(method_id, params, &self.tokens.snlm0e)?;

        let resp = self
            .http
            .post(&url)
            .header(reqwest::header::COOKIE, &self.tokens.cookie_header)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                 AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/120.0.0.0 Safari/537.36",
            )
            .body(body)
            .send()
            .await
            .with_context(|| format!("RPC call {method_id} failed"))?;

        resp.error_for_status_ref()
            .with_context(|| format!("RPC {method_id} returned HTTP error"))?;

        let text = resp
            .text()
            .await
            .with_context(|| format!("Failed to read RPC {method_id} response body"))?;

        if self.debug {
            rpc::decode_response_debug(&text, method_id)
        } else {
            rpc::decode_response(&text, method_id)
        }
    }

    // ── Notebooks ─────────────────────────────────────────────────────────

    /// List all notebooks.
    ///
    /// Params: `[null, 1, null, [2]]` (validated against notebooklm-py _notebooks.py)
    /// Returns: array of notebook arrays, or empty array if none.
    pub async fn list_notebooks(&self) -> Result<Value> {
        let result = self
            .rpc(LIST_NOTEBOOKS, &json!([null, 1, null, [2]]), "/")
            .await?;
        // result is null when no notebooks, otherwise result[0] = array of notebooks.
        match result {
            Value::Null => Ok(Value::Array(vec![])),
            other => Ok(other[0].clone()),
        }
    }

    /// Find a notebook by title, or create it if it doesn't exist.
    /// Returns the notebook ID string.
    pub async fn find_or_create_notebook(&self, name: &str) -> Result<String> {
        let notebooks = self.list_notebooks().await?;
        let empty = vec![];
        let arr = notebooks.as_array().unwrap_or(&empty);

        for nb in arr {
            // nb[0] = title, nb[2] = ID (UUID)
            if nb[0].as_str() == Some(name) {
                let id = nb[2]
                    .as_str()
                    .context("Notebook ID is not a string")?
                    .to_string();
                return Ok(id);
            }
        }

        // Create — params: [title, null, null, [2], [1]]
        let result = self
            .rpc(CREATE_NOTEBOOK, &json!([name, null, null, [2], [1]]), "/")
            .await?;
        // result[0] = notebook_id
        let id = result[0]
            .as_str()
            .context("CREATE_NOTEBOOK: missing notebook ID in response")?
            .to_string();
        Ok(id)
    }

    // ── Sources ───────────────────────────────────────────────────────────

    /// List all sources for a notebook.
    ///
    /// Uses GET_NOTEBOOK (not a dedicated list-sources RPC).
    /// Sources are at `notebook[0][1]`.
    /// Source ID is at `src[0][0]` (if src[0] is an array) or `src[0]`.
    /// Status is at `src[3][1]`.
    pub async fn list_sources(&self, notebook_id: &str) -> Result<Vec<Value>> {
        let source_path = format!("/notebook/{notebook_id}");
        let result = self
            .rpc(
                GET_NOTEBOOK,
                &json!([notebook_id, null, [2], null, 0]),
                &source_path,
            )
            .await?;

        // result[0] = nb_info, nb_info[1] = sources list
        let sources = result[0][1].clone();
        match sources {
            Value::Array(arr) => Ok(arr),
            _ => Ok(vec![]),
        }
    }

    /// Delete all sources from a notebook.
    pub async fn delete_all_sources(&self, notebook_id: &str) -> Result<()> {
        let sources = self.list_sources(notebook_id).await?;
        let source_path = format!("/notebook/{notebook_id}");

        for src in &sources {
            // source_id at src[0][0] if src[0] is array, else src[0]
            let src_id = if src[0].is_array() {
                src[0][0].as_str().map(|s| s.to_string())
            } else {
                src[0].as_str().map(|s| s.to_string())
            };

            if let Some(id) = src_id {
                // params: [[[source_id]]]  (validated from _sources.py delete())
                self.rpc(DELETE_SOURCE, &json!([[[id]]]), &source_path)
                    .await
                    .with_context(|| format!("Failed to delete source {id}"))?;
            }
        }
        Ok(())
    }

    /// Add a text/markdown source to a notebook. Returns the new source ID.
    ///
    /// Params validated from _sources.py add_text():
    /// `[[[null, [title, content], null×6]], notebook_id, [2], null, null]`
    pub async fn add_text_source(
        &self,
        notebook_id: &str,
        title: &str,
        content: &str,
    ) -> Result<String> {
        let source_path = format!("/notebook/{notebook_id}");
        let params = json!([
            [[null, [title, content], null, null, null, null, null, null]],
            notebook_id,
            [2],
            null,
            null
        ]);

        let result = self.rpc(ADD_SOURCE, &params, &source_path).await?;

        // Response: deeply nested array; source ID is a string at result[0][0][0][0]
        let src_id = result[0][0][0][0]
            .as_str()
            .with_context(|| format!("ADD_SOURCE: missing source ID in response: {result}"))?
            .to_string();
        Ok(src_id)
    }

    /// Add a binary file source (PDF, image, Word doc, plain text…) via the
    /// Google resumable-upload protocol used by the NotebookLM web UI.
    ///
    /// Three steps, all validated against notebooklm-py `_sources.py::add_file`:
    ///   1. RPC `ADD_SOURCE_FILE` with `[[[filename]], notebook_id, [2], [1, null×9, [1]]]`
    ///      → returns a source ID (deeply nested string; we walk the first
    ///      element of every array until we find one).
    ///   2. POST to `UPLOAD_URL?authuser=0` with JSON body
    ///      `{"PROJECT_ID": notebook_id, "SOURCE_NAME": filename, "SOURCE_ID": source_id}`
    ///      and `x-goog-upload-command: start` header → response carries the
    ///      real upload URL in `x-goog-upload-url`.
    ///   3. POST file bytes to that URL with `x-goog-upload-command: upload, finalize`
    ///      and `x-goog-upload-offset: 0` → NotebookLM ingests the file.
    ///
    /// NotebookLM officially supports PDF / plain text / markdown / Word. Raw
    /// images (PNG/JPEG) are accepted by this flow but may be rejected server-
    /// side during processing — the caller sees that as `STATUS_FAILED` on the
    /// returned source later.
    pub async fn add_file_source(&self, notebook_id: &str, file_path: &Path) -> Result<String> {
        let filename = file_path
            .file_name()
            .and_then(|s| s.to_str())
            .context("File path has no filename component")?
            .to_string();

        let bytes = tokio::fs::read(file_path)
            .await
            .with_context(|| format!("Cannot read {}", file_path.display()))?;

        // ── Step 1: register the source and get its id ───────────────────
        let source_path = format!("/notebook/{notebook_id}");
        let params = json!([
            [[filename.clone()]],
            notebook_id,
            [2],
            [1, null, null, null, null, null, null, null, null, null, [1]],
        ]);
        let result = self
            .rpc(rpc::ADD_SOURCE_FILE, &params, &source_path)
            .await?;

        // Response shape varies: [[[[id]]]], [[[id]]], [[id]], etc. Walk the
        // first element of each nested array until we hit a string.
        fn extract_id(v: &Value) -> Option<String> {
            match v {
                Value::String(s) => Some(s.clone()),
                Value::Array(a) if !a.is_empty() => extract_id(&a[0]),
                _ => None,
            }
        }
        let source_id = extract_id(&result).with_context(|| {
            format!("ADD_SOURCE_FILE: could not find source_id in response: {result}")
        })?;

        // ── Step 2: start the resumable session ──────────────────────────
        let start_url = format!("{}?authuser=0", rpc::UPLOAD_URL);
        let start_body = json!({
            "PROJECT_ID": notebook_id,
            "SOURCE_NAME": filename,
            "SOURCE_ID": source_id,
        })
        .to_string();

        let start_resp = self
            .http
            .post(&start_url)
            .header(reqwest::header::ACCEPT, "*/*")
            .header(reqwest::header::COOKIE, &self.tokens.cookie_header)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded;charset=UTF-8",
            )
            .header("Origin", "https://notebooklm.google.com")
            .header("Referer", "https://notebooklm.google.com/")
            .header("x-goog-authuser", "0")
            .header("x-goog-upload-command", "start")
            .header(
                "x-goog-upload-header-content-length",
                bytes.len().to_string(),
            )
            .header("x-goog-upload-protocol", "resumable")
            .body(start_body)
            .send()
            .await
            .with_context(|| format!("Failed to start upload for {filename}"))?;
        start_resp
            .error_for_status_ref()
            .with_context(|| format!("Upload-start HTTP error for {filename}"))?;

        let upload_url = start_resp
            .headers()
            .get("x-goog-upload-url")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .context("Upload-start response missing x-goog-upload-url header")?;

        // ── Step 3: upload the bytes ─────────────────────────────────────
        let upload_resp = self
            .http
            .post(&upload_url)
            .header(reqwest::header::ACCEPT, "*/*")
            .header(reqwest::header::COOKIE, &self.tokens.cookie_header)
            // Matches notebooklm-py::_upload_file_streaming. Without it,
            // reqwest falls back to application/octet-stream, which some
            // NotebookLM upload endpoints reject or silently ingest as the
            // wrong source type.
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded;charset=utf-8",
            )
            .header("Origin", "https://notebooklm.google.com")
            .header("Referer", "https://notebooklm.google.com/")
            .header("x-goog-authuser", "0")
            .header("x-goog-upload-command", "upload, finalize")
            .header("x-goog-upload-offset", "0")
            .body(bytes)
            .send()
            .await
            .with_context(|| format!("Failed to upload bytes for {filename}"))?;
        upload_resp
            .error_for_status_ref()
            .with_context(|| format!("Upload-finalize HTTP error for {filename}"))?;

        Ok(source_id)
    }

    /// Upload every file under `root`:
    ///   - `.md` files go through `add_text_source` (same as before).
    ///   - Other files (png, jpg, pdf, txt, docx…) go through `add_file_source`
    ///     (resumable binary upload).
    ///
    /// Recurses one level so `<root>/assets/imageN.png` produced by
    /// `nlm import` is picked up alongside `<root>/charter.md`.
    pub async fn upload_dir(&self, notebook_id: &str, root: &Path) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        let mut stack: Vec<std::path::PathBuf> = vec![root.to_path_buf()];

        while let Some(dir) = stack.pop() {
            let mut entries = tokio::fs::read_dir(&dir)
                .await
                .with_context(|| format!("Cannot read directory: {}", dir.display()))?;

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let ft = entry.file_type().await?;
                if ft.is_dir() {
                    stack.push(path);
                    continue;
                }
                if !ft.is_file() {
                    continue;
                }

                let label = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("source")
                    .to_string();

                print!("  uploading {label:<55}");
                use std::io::Write as _;
                std::io::stdout().flush().ok();

                let is_md = path.extension().and_then(|e| e.to_str()) == Some("md");

                let src_id = if is_md {
                    let title = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("source")
                        .to_string();
                    let content = tokio::fs::read_to_string(&path)
                        .await
                        .with_context(|| format!("Failed to read {}", path.display()))?;
                    self.add_text_source(notebook_id, &title, &content).await
                } else {
                    self.add_file_source(notebook_id, &path).await
                };

                match src_id {
                    Ok(id) => {
                        println!("✓");
                        ids.push(id);
                    }
                    Err(e) => {
                        println!("✗  {e}");
                    }
                }
            }
        }

        Ok(ids)
    }

    // ── Artifacts ─────────────────────────────────────────────────────────

    /// Generate a slide deck artifact. Returns the artifact ID.
    ///
    /// Params validated from _artifacts.py generate_slide_deck():
    /// `[[2], notebook_id, [null, null, 8, source_ids_triple, null×12, [[instructions, language, null, null]]]]`
    /// where source_ids_triple = `[[[sid1]], [[sid2]], ...]` — each element is `[[sid]]`.
    pub async fn generate_slide_deck(
        &self,
        notebook_id: &str,
        source_ids: &[String],
        instructions: Option<&str>,
        language: &str,
    ) -> Result<String> {
        let source_path = format!("/notebook/{notebook_id}");

        // source_ids_triple: [[[sid1]], [[sid2]], ...] — each element is [[sid]]
        let src_triple: Value = source_ids
            .iter()
            .map(|id| json!([[id]]))
            .collect::<Vec<_>>()
            .into();

        let params = json!([
            [2],
            notebook_id,
            [
                null,
                null,
                ARTIFACT_SLIDE_DECK,
                src_triple,
                null,
                null,
                null,
                null,
                null,
                null,
                null,
                null,
                null,
                null,
                null,
                null,
                [[instructions, language, null, null]]
            ]
        ]);

        let result = self.rpc(CREATE_ARTIFACT, &params, &source_path).await?;

        let artifact_id = result[0][0]
            .as_str()
            .or_else(|| result[0][0][0].as_str())
            .with_context(|| format!("CREATE_ARTIFACT: missing artifact ID in response: {result}"))?
            .to_string();
        Ok(artifact_id)
    }

    /// Generate a report artifact (study-guide, briefing-doc). Returns the artifact ID.
    ///
    /// Params structure from _artifacts.py generate_report():
    /// `[[2], notebook_id, [null, null, 2, source_ids_triple, null×3,
    ///   [null, [title, description, null, source_ids_double, language, prompt, null, true]]]]`
    pub async fn generate_report(
        &self,
        notebook_id: &str,
        source_ids: &[String],
        title: &str,
        description: &str,
        prompt: &str,
        language: &str,
    ) -> Result<String> {
        let source_path = format!("/notebook/{notebook_id}");

        let src_triple: Value = source_ids
            .iter()
            .map(|id| json!([[id]]))
            .collect::<Vec<_>>()
            .into();

        let src_double: Value = source_ids
            .iter()
            .map(|id| json!([id]))
            .collect::<Vec<_>>()
            .into();

        let params = json!([
            [2],
            notebook_id,
            [
                null,
                null,
                ARTIFACT_REPORT,
                src_triple,
                null,
                null,
                null,
                [
                    null,
                    [
                        title,
                        description,
                        null,
                        src_double,
                        language,
                        prompt,
                        null,
                        true
                    ]
                ]
            ]
        ]);

        let result = self.rpc(CREATE_ARTIFACT, &params, &source_path).await?;

        let artifact_id = result[0][0]
            .as_str()
            .or_else(|| result[0][0][0].as_str())
            .with_context(|| {
                format!("CREATE_ARTIFACT (report): missing artifact ID in response: {result}")
            })?
            .to_string();
        Ok(artifact_id)
    }

    /// Download a completed report artifact as markdown.
    /// Report content is at `artifact[7][0]` (or `artifact[7]` if already a string).
    pub fn extract_report_markdown(artifact: &Value) -> Result<String> {
        let content = &artifact[7];
        if let Some(s) = content.as_str() {
            return Ok(s.to_string());
        }
        if let Some(s) = content[0].as_str() {
            return Ok(s.to_string());
        }
        anyhow::bail!("Report content not found at artifact[7]: {content}")
    }

    /// Revise a single slide in a completed slide deck. Returns the artifact ID for polling.
    ///
    /// Params validated from notebooklm-py _artifacts.py revise_slide():
    /// `[[2], artifact_id, [[[slide_index, prompt]]]]`
    /// where `slide_index` is zero-based.
    ///
    /// The returned artifact ID is used for polling via `wait_for_artifact`.
    pub async fn revise_slide(
        &self,
        notebook_id: &str,
        artifact_id: &str,
        slide_index: u32,
        prompt: &str,
    ) -> Result<String> {
        let source_path = format!("/notebook/{notebook_id}");
        let params = json!([[2], artifact_id, [[[slide_index, prompt]]]]);

        let result = self.rpc(REVISE_SLIDE, &params, &source_path).await?;

        // result[0][0] = artifact_id (same structure as CREATE_ARTIFACT)
        let revised_id = result[0][0]
            .as_str()
            .context("REVISE_SLIDE: missing artifact ID at result[0][0]")?
            .to_string();
        Ok(revised_id)
    }

    /// List raw artifacts for a notebook.
    ///
    /// Params validated from _artifacts.py list():
    /// `[[2], notebook_id, 'NOT artifact.status = "ARTIFACT_STATUS_SUGGESTED"']`
    pub async fn list_artifacts_raw(&self, notebook_id: &str) -> Result<Vec<Value>> {
        let source_path = format!("/notebook/{notebook_id}");
        let params = json!([
            [2],
            notebook_id,
            "NOT artifact.status = \"ARTIFACT_STATUS_SUGGESTED\""
        ]);

        let result = self.rpc(LIST_ARTIFACTS, &params, &source_path).await?;

        match result {
            Value::Null => Ok(vec![]),
            other => {
                // result[0] if result[0] is array, else result
                let arr = if other[0].is_array() {
                    other[0].clone()
                } else {
                    other
                };
                Ok(arr.as_array().cloned().unwrap_or_default())
            }
        }
    }

    /// Poll artifact status until COMPLETED or FAILED.
    /// Returns the full artifact JSON array.
    pub async fn wait_for_artifact(&self, notebook_id: &str, artifact_id: &str) -> Result<Value> {
        for attempt in 0..200 {
            let artifacts = self.list_artifacts_raw(notebook_id).await?;

            for art in &artifacts {
                // art[0] = artifact_id
                if art[0].as_str() == Some(artifact_id) {
                    let status = art[4].as_i64().unwrap_or(0);
                    if status == STATUS_COMPLETED {
                        return Ok(art.clone());
                    }
                    if status == STATUS_FAILED {
                        anyhow::bail!("Artifact {artifact_id} generation failed");
                    }
                    break;
                }
            }

            let delay = if attempt < 10 { 3 } else { 10 };
            sleep(Duration::from_secs(delay)).await;
        }

        anyhow::bail!("Timed out waiting for artifact {artifact_id}")
    }

    // ── Download ───────────────────────────────────────────────────────────

    /// Download a completed slide deck artifact as a PDF.
    ///
    /// PDF URL is at `artifact[16][3]` (validated from _artifacts.py download_slide_deck()).
    ///
    /// Why a separate cookie-jar client?
    /// The download URL is on `contribution.usercontent.google.com` and may redirect
    /// across Google domains. reqwest does NOT forward the `Cookie` header on redirects
    /// (correct security behaviour). We must use reqwest's built-in cookie Jar so that
    /// cookies are sent per-domain on every hop — identical to Python's httpx.Cookies.
    pub async fn download_slide_deck(&self, artifact: &Value, dest: &Path) -> Result<()> {
        let pdf_url = artifact[16][3]
            .as_str()
            .context("artifact[16][3] (PDF URL) is missing — slide deck may not be ready")?;

        // Build a cookie-jar client that forwards cookies on redirects.
        let jar = build_cookie_jar(None).context("Failed to build cookie jar for download")?;

        let download_client = reqwest::Client::builder()
            .cookie_provider(Arc::new(jar))
            .timeout(Duration::from_secs(120))
            .build()?;

        let bytes = download_client
            .get(pdf_url)
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
                 AppleWebKit/537.36 (KHTML, like Gecko) \
                 Chrome/120.0.0.0 Safari/537.36",
            )
            .send()
            .await
            .context("Failed to download slide deck PDF")?
            .error_for_status()
            .context("PDF download returned HTTP error")?
            .bytes()
            .await
            .context("Failed to read PDF bytes")?;

        tokio::fs::write(dest, &bytes)
            .await
            .with_context(|| format!("Failed to write PDF to {}", dest.display()))?;

        Ok(())
    }
}
