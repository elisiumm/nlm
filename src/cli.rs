// clap's derive API turns annotated structs/enums into a full argument parser.
// #[derive(Parser)] on the root struct, #[derive(Subcommand)] on command enums.

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

// ── Root CLI ──────────────────────────────────────────────────────────────────

#[derive(Debug, Parser)]
#[command(
    name = "nlm",
    about = "NotebookLM toolkit — sync sources, upload, and generate AI artifacts.",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

// ── Shared args ───────────────────────────────────────────────────────────────

// #[command(flatten)] inlines these fields into a parent subcommand.
// Avoids repeating --output-dir / --config-dir in every variant.
#[derive(Debug, Args)]
pub struct DirArgs {
    /// Working directory (default: ./output)
    #[arg(long, value_name = "DIR", default_value = "output")]
    pub output_dir: PathBuf,

    /// Config directory (default: ./config)
    #[arg(long, value_name = "DIR", default_value = "config")]
    pub config_dir: PathBuf,
}

// ── Artifact type ─────────────────────────────────────────────────────────────

// ValueEnum generates clap parsing + --help display from enum variants.
// #[value(name = "...")] customises the string accepted on the CLI.
#[derive(Debug, Clone, PartialEq, ValueEnum)]
pub enum ArtifactType {
    #[value(name = "slide-deck")]
    SlideDeck,
    #[value(name = "study-guide")]
    StudyGuide,
    #[value(name = "briefing-doc")]
    BriefingDoc,
    Audio,
}

impl ArtifactType {
    /// Parse from config string (e.g. "study-guide").
    pub fn from_config(s: &str) -> Option<Self> {
        match s {
            "slide-deck" => Some(Self::SlideDeck),
            "study-guide" => Some(Self::StudyGuide),
            "briefing-doc" => Some(Self::BriefingDoc),
            "audio" => Some(Self::Audio),
            _ => None,
        }
    }
}

impl std::fmt::Display for ArtifactType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtifactType::SlideDeck => write!(f, "slide-deck"),
            ArtifactType::StudyGuide => write!(f, "study-guide"),
            ArtifactType::BriefingDoc => write!(f, "briefing-doc"),
            ArtifactType::Audio => write!(f, "audio"),
        }
    }
}

// ── Subcommands ───────────────────────────────────────────────────────────────

// Each variant maps to one CLI subcommand.
// Doc comments become the --help descriptions.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Pull sources → output/markdown/
    Sync {
        /// Project config to load (config/projects/<NAME>.yaml)
        #[arg(short, long, value_name = "NAME")]
        project: Option<String>,

        /// Legacy: path to adr_sources.yaml
        #[arg(long, value_name = "FILE")]
        sources: Option<PathBuf>,

        #[command(flatten)]
        dirs: DirArgs,
    },

    /// Extract brand charter from a PPTX template
    Import {
        /// Path to the .pptx file
        pptx: PathBuf,

        /// Output directory for extracted assets
        #[arg(short, long, default_value = "output/markdown")]
        output: PathBuf,

        /// Skip writing asset files (preview only)
        #[arg(long)]
        dry_run: bool,
    },

    /// Drop & re-upload sources to NotebookLM
    Upload {
        #[arg(short, long, value_name = "NAME")]
        project: Option<String>,

        #[arg(long, value_name = "ID")]
        notebook_id: Option<String>,

        /// Print raw RPC response bodies to stderr for debugging
        #[arg(long)]
        debug: bool,

        #[command(flatten)]
        dirs: DirArgs,
    },

    /// Generate an artifact from an existing notebook
    Generate {
        #[arg(short, long, value_name = "NAME")]
        project: Option<String>,

        /// Artifact type to generate
        #[arg(short = 't', long, value_name = "TYPE")]
        artifact_type: Option<ArtifactType>,

        /// BCP-47 language code (e.g. fr, en)
        #[arg(short, long, value_name = "LANG")]
        language: Option<String>,

        /// NotebookLM notebook ID (required)
        #[arg(long, value_name = "ID")]
        notebook_id: String,

        #[command(flatten)]
        dirs: DirArgs,
    },

    /// Download the latest slide deck without regenerating
    Fetch {
        /// NotebookLM notebook ID (required)
        #[arg(long, value_name = "ID")]
        notebook_id: String,

        #[arg(short, long, value_name = "NAME")]
        project: Option<String>,

        #[command(flatten)]
        dirs: DirArgs,
    },

    /// Full pipeline: sync + upload + generate
    Run {
        #[arg(short, long, value_name = "NAME")]
        project: Option<String>,

        #[arg(short = 't', long, value_name = "TYPE")]
        artifact_type: Option<ArtifactType>,

        #[arg(short, long, value_name = "LANG")]
        language: Option<String>,

        #[arg(long, value_name = "ID")]
        notebook_id: Option<String>,

        /// Skip upload and reuse existing notebook (requires --notebook-id)
        #[arg(long)]
        skip_upload: bool,

        /// Print raw RPC response bodies to stderr for debugging
        #[arg(long)]
        debug: bool,

        #[command(flatten)]
        dirs: DirArgs,
    },

    /// Re-generate a slide deck with a targeted correction
    Correct {
        /// Correction instruction for the selected slide
        prompt: String,

        /// Slide number to correct (1-based)
        #[arg(short, long, value_name = "N")]
        slide: u32,

        #[arg(long, value_name = "ID")]
        notebook_id: String,

        #[arg(short, long, value_name = "NAME")]
        project: Option<String>,

        #[arg(short, long, value_name = "LANG")]
        language: Option<String>,

        #[command(flatten)]
        dirs: DirArgs,
    },

    /// List all NotebookLM notebooks on the account
    List {
        /// Print raw RPC response for debugging
        #[arg(long)]
        debug: bool,
    },

    /// Authenticate with Google (opens browser via notebooklm-py)
    Login,

    /// List all available project configs
    Projects {
        #[arg(long, default_value = "config")]
        config_dir: PathBuf,
    },

    /// Scaffold a new project config file
    New {
        /// Project name (creates config/projects/<name>.yaml)
        name: String,

        #[arg(long, default_value = "config")]
        config_dir: PathBuf,
    },
}
