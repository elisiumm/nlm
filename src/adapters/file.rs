// FileAdapter — copies a local .md / .txt / .pdf file into output/markdown/.
// Mirrors nlm/adapters/file.py.
//
// Rust concepts:
//   - PathBuf vs &Path: PathBuf owns the path data, &Path is a borrowed view
//   - std::env::current_dir() for resolving relative paths
//   - tokio::fs::copy for async file copy

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::SourceAdapter;
use crate::config::Source;

const SUPPORTED_EXTS: &[&str] = &["md", "txt", "pdf"];

pub struct FileAdapter;

impl SourceAdapter for FileAdapter {
    async fn fetch(&self, source: &Source, output_dir: &Path) -> Result<Option<PathBuf>> {
        let (path_str, title) = match source {
            Source::File { path, title } => (path.as_str(), title.as_str()),
            _ => unreachable!("FileAdapter called with non-File source"),
        };

        // Resolve the source path:
        //   ~/...  → $HOME/...
        //   ./...  → cwd/...
        //   /...   → absolute, use as-is
        let src: PathBuf = if let Some(rest) = path_str.strip_prefix("~/") {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(rest)
        } else {
            // relative paths are resolved from cwd (the user's project root)
            let raw = PathBuf::from(path_str);
            if raw.is_absolute() {
                raw
            } else {
                std::env::current_dir()?.join(raw)
            }
        };

        // canonicalize() resolves symlinks and `..` segments; returns Err if
        // the path does not exist — which gives us the "not found" error for free.
        let src = src
            .canonicalize()
            .with_context(|| format!("Source file not found: {}", src.display()))?;

        let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("");

        if !SUPPORTED_EXTS.contains(&ext) {
            anyhow::bail!(
                "Unsupported extension {ext:?} for file {}. Supported: {}",
                src.display(),
                SUPPORTED_EXTS.join(", ")
            );
        }

        let dest = output_dir.join(format!("{title}.{ext}"));

        // tokio::fs::copy is the async equivalent of std::fs::copy.
        // It preserves file contents but not permissions (fine for our use case).
        tokio::fs::copy(&src, &dest)
            .await
            .with_context(|| format!("Failed to copy {} → {}", src.display(), dest.display()))?;

        Ok(Some(dest))
    }
}
