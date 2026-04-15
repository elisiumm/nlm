// Adapter module — mirrors nlm/adapters/__init__.py
//
// Key Rust concepts introduced here:
//   - Traits: the Rust equivalent of Python Protocols / interfaces
//   - async fn in traits (stable since Rust 1.75)
//   - Pattern matching on enum variants for static dispatch
//   - std::io::Write for explicit stdout flushing

pub mod confluence;
pub mod file;
pub mod notion;
pub mod url;

use anyhow::Result;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::config::Source;

// ── SourceAdapter trait ───────────────────────────────────────────────────────

// In Python: a Protocol with one async method.
// In Rust: a trait with one async method.
//
// Rust 1.75+ supports `async fn` directly in traits. The compiler desugars
// this to an associated future type, enabling static (monomorphized) dispatch.
//
// We don't use `dyn SourceAdapter` here (dynamic dispatch) because that would
// require boxing futures. For our use case, static dispatch via pattern-match
// is simpler and has zero runtime cost.
pub trait SourceAdapter {
    async fn fetch(&self, source: &Source, output_dir: &Path) -> Result<Option<PathBuf>>;
}

// ── Shared helper ─────────────────────────────────────────────────────────────

/// Sanitize a title for safe use as a filename.
/// Mirrors the `.replace(":", "-").replace("/", "-")` in each Python adapter.
pub fn safe_filename(title: &str) -> String {
    title.replace([':', '/'], "-")
}

// ── Dispatcher ────────────────────────────────────────────────────────────────

/// Fetch all sources and write them into `output_dir`.
/// Mirrors `sync_all_sources()` in nlm/adapters/__init__.py.
pub async fn sync_all_sources(sources: &[Source], output_dir: &Path) -> Result<()> {
    tokio::fs::create_dir_all(output_dir).await?;

    for source in sources {
        let type_name = source_type(source);
        let label = source_title(source);

        // print! does not flush automatically in Rust — we must flush explicitly
        // so the label appears before the ✓ / ✗ result on the same line.
        print!("  [{type_name}] {label:<55}");
        std::io::stdout().flush().ok();

        let result: Result<Option<PathBuf>> = match source {
            Source::Url { .. } => url::UrlAdapter.fetch(source, output_dir).await,
            Source::File { .. } => file::FileAdapter.fetch(source, output_dir).await,
            Source::Confluence { .. } => {
                confluence::ConfluenceAdapter
                    .fetch(source, output_dir)
                    .await
            }
            Source::Notion { .. } => notion::NotionAdapter.fetch(source, output_dir).await,
            // PPTX requires ZIP + XML parsing — deferred to Phase 4
            Source::Pptx { .. } => {
                println!("skipped  (PPTX — Phase 4)");
                continue;
            }
        };

        match result {
            Ok(Some(path)) => {
                // std::fs::metadata is sync; fine here since we just wrote the file
                let size_kb = path.metadata().map(|m| m.len() / 1024 + 1).unwrap_or(0);
                println!("✓  ({size_kb} KB)");
            }
            Ok(None) => println!("skipped"),
            // Errors are printed inline, not propagated — mirrors Python's try/except per source
            Err(e) => println!("✗  {e}"),
        }
    }

    Ok(())
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn source_type(source: &Source) -> &'static str {
    match source {
        Source::Url { .. } => "url",
        Source::File { .. } => "file",
        Source::Confluence { .. } => "confluence",
        Source::Notion { .. } => "notion",
        Source::Pptx { .. } => "pptx",
    }
}

// Each Source variant holds a `title` field — extract it uniformly.
// This would be cleaner with a shared Title trait, but that's over-engineering for now.
fn source_title(source: &Source) -> &str {
    match source {
        Source::Url { title, .. }
        | Source::File { title, .. }
        | Source::Confluence { title, .. }
        | Source::Notion { title, .. }
        | Source::Pptx { title, .. } => title,
    }
}
