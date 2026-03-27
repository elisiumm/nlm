// notebooklm module — HTTP client for the NotebookLM batchexecute RPC API.
//
// Key insight: NotebookLM uses Google's internal "batchexecute" HTTP RPC
// mechanism — NOT a public REST API, NOT live browser automation.
// The browser (Playwright) is only needed once for `nlm login` to save cookies.
// All subsequent calls are pure reqwest HTTP + JSON parsing.
//
// Module layout:
//   auth   — load ~/.notebooklm/storage_state.json, extract cookies + CSRF tokens
//   rpc    — encode/decode the batchexecute wire format + all RPC method IDs
//   client — NotebookLMClient with all notebook / source / artifact operations

pub mod auth;
pub mod client;
pub mod rpc;

// Re-export the two types callers need most.
pub use auth::load_tokens;
pub use client::NotebookLMClient;
