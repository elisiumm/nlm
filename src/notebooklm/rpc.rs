// rpc.rs — Google batchexecute RPC protocol encode/decode.
//
// NotebookLM uses Google's internal "batchexecute" RPC mechanism.
// All operations (list, create, generate…) go through a single endpoint:
//   POST https://notebooklm.google.com/_/LabsTailwindUi/data/batchexecute
//
// ── Encoding ──────────────────────────────────────────────────────────────
// The request body is application/x-www-form-urlencoded:
//   f.req=<url-encoded>[[[method_id, json_params, null, "generic"]]]&at=<url-encoded-csrf>&
//
// URL query params: rpcids=<method>, source-path=/, f.sid=<fdrfje>, rt=c
//
// ── Decoding ──────────────────────────────────────────────────────────────
// Response body has an anti-XSSI prefix: )]}'  followed by newline.
// After stripping that prefix, the body is a "chunked" format:
//   <byte_count>\n
//   <json_payload>\n
//   <byte_count>\n
//   <json_payload>\n
//   ...
// Each json_payload is a JSON array. We want the chunk where:
//   payload[0][0] == "wrb.fr" && payload[0][1] == method_id
// The RPC result is at payload[0][2] (a JSON string we need to re-parse).
//
// Rust concepts:
//   - serde_json::json! macro for building JSON values inline
//   - urlencoding::encode for percent-encoding
//   - String::split_once for two-part splits
//   - iterating &str lines with .lines()

use anyhow::{Context, Result};
use serde_json::Value;

// ── RPC Method IDs ─────────────────────────────────────────────────────────
// Extracted from notebooklm-py rpc/types.py

pub const LIST_NOTEBOOKS: &str = "wXbhsf";
pub const CREATE_NOTEBOOK: &str = "CCqFvf";
pub const GET_NOTEBOOK: &str = "rLM1Ne";
#[allow(dead_code)]
pub const DELETE_NOTEBOOK: &str = "pXA4Ob";
#[allow(dead_code)]
pub const RENAME_NOTEBOOK: &str = "nFMHOc";

pub const ADD_SOURCE: &str = "izAoDd";
pub const DELETE_SOURCE: &str = "tGMBJ";
#[allow(dead_code)]
pub const LIST_SOURCES: &str = "Yl5oTb";

pub const CREATE_ARTIFACT: &str = "R7cb6c";
pub const LIST_ARTIFACTS: &str = "gArtLc";
pub const REVISE_SLIDE: &str = "KmcKPe"; // Targeted single-slide revision

// Artifact type codes (from rpc/types.py ArtifactTypeCode)
#[allow(dead_code)]
pub const ARTIFACT_AUDIO: i64 = 1;
pub const ARTIFACT_REPORT: i64 = 2;
pub const ARTIFACT_SLIDE_DECK: i64 = 8;

// Artifact / source status (3 = COMPLETED / READY, 4 = FAILED)
pub const STATUS_COMPLETED: i64 = 3;
pub const STATUS_FAILED: i64 = 4;

// ── Encoding ───────────────────────────────────────────────────────────────

const BATCHEXECUTE_URL: &str = "https://notebooklm.google.com/_/LabsTailwindUi/data/batchexecute";

/// Build the full POST URL with required query parameters.
///
/// `source_path` matches the page context in the browser, e.g. "/" or "/notebook/{id}".
/// Most notebook/source/artifact operations need "/notebook/{id}" — not "/" — to match
/// the request the browser would send from the notebook page.
pub fn rpc_url(method_id: &str, session_id: &str, source_path: &str) -> String {
    let encoded_path = urlencoding::encode(source_path);
    format!(
        "{BATCHEXECUTE_URL}?rpcids={method_id}&source-path={encoded_path}&f.sid={session_id}&rt=c"
    )
}

/// Build the `application/x-www-form-urlencoded` request body.
///
/// Format: `f.req=<url-encoded-envelope>&at=<url-encoded-csrf>&`
///
/// The envelope is a JSON array: `[[[method_id, serialized_params, null, "generic"]]]`
/// where `serialized_params` is the params JSON serialized to a *string* (double-encoded).
pub fn rpc_body(method_id: &str, params: &Value, csrf: &str) -> Result<String> {
    // Inner: serialize params to a JSON string (so it becomes a quoted string inside the envelope)
    let params_str = serde_json::to_string(params).context("Failed to serialize RPC params")?;

    // Envelope: [[[method_id, params_str, null, "generic"]]]
    let envelope = serde_json::json!([[[method_id, params_str, null, "generic"]]]);
    let envelope_str =
        serde_json::to_string(&envelope).context("Failed to serialize RPC envelope")?;

    // URL-encode both fields
    let f_req = urlencoding::encode(&envelope_str);
    let at = urlencoding::encode(csrf);

    Ok(format!("f.req={f_req}&at={at}&"))
}

// ── Decoding ───────────────────────────────────────────────────────────────

/// Strip the anti-XSSI prefix `)]}'` from the response body.
///
/// Google prepends this to every batchexecute response to prevent
/// JSON hijacking (an old XSS technique that abused JSON arrays as script sources).
fn strip_anti_xssi(body: &str) -> &str {
    body.strip_prefix(")]}'\n").unwrap_or(body)
}

/// Parse the chunked response body and extract the RPC result for `method_id`.
///
/// The chunked format alternates:
///   line 1: decimal byte count (length of the next JSON payload)
///   line 2: the JSON payload itself
///
/// We look for a payload where `payload[0][0] == "wrb.fr"` and
/// `payload[0][1] == method_id`.  The result is at `payload[0][2]`.
/// Same as `decode_response` but also prints the raw body for debugging.
pub fn decode_response_debug(body: &str, method_id: &str) -> Result<Value> {
    eprintln!("\n── Raw RPC response ({method_id}) ──\n{body}\n──────────────────────────────────");
    decode_response(body, method_id)
}

pub fn decode_response(body: &str, method_id: &str) -> Result<Value> {
    let body = strip_anti_xssi(body);

    // Walk through lines: skip byte-count lines, try parsing the rest as JSON.
    // We collect non-numeric lines as candidate payloads.
    let lines = body.lines().peekable();

    for line in lines {
        // Byte-count lines are pure decimal digits (possibly with whitespace).
        // JSON payload lines start with '['.
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        // Try to parse this line as JSON.
        let Ok(chunk): std::result::Result<Value, _> = serde_json::from_str(trimmed) else {
            continue;
        };

        // Each chunk is an array of arrays. Find the one matching our method.
        if let Some(items) = chunk.as_array() {
            for item in items {
                // item = ["wrb.fr", method_id, result_json_string, ...]
                if item[0].as_str() == Some("wrb.fr") && item[1].as_str() == Some(method_id) {
                    // item[2] is the result:
                    //   - Usually a JSON *string* that we must parse again (double-encoded).
                    //   - null when the operation returns no data (e.g. empty list).
                    //   - Occasionally already a parsed Value in some responses.
                    let result = match &item[2] {
                        Value::String(s) => serde_json::from_str(s).with_context(|| {
                            format!("Failed to parse RPC result for {method_id}")
                        })?,
                        Value::Null => Value::Null,
                        other => other.clone(),
                    };

                    return Ok(result);
                }
            }
        }
    }

    anyhow::bail!("RPC response did not contain a result for method '{method_id}'")
}
