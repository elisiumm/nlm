// Command handlers — one function per subcommand.
//
// Phase 1: projects, new, sync  (config loading + source listing)
// Phase 2: sync → adapters::sync_all_sources (HTTP fetch for url/confluence/file)
// Phase 3: list, upload, generate, fetch, run (NotebookLM RPC client)
//
// Rust concepts:
//   - async fn with .await? for sequential async operations
//   - if let Some(x) for optional config fields
//   - String::from / to_string() vs &str for ownership at call sites

use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::adapters;
use crate::cli::{ArtifactType, Cli, Command};
use crate::config::{load_config, list_projects};
use crate::notebooklm::{load_tokens, NotebookLMClient};
use crate::notebooklm::rpc::{ARTIFACT_SLIDE_DECK, STATUS_COMPLETED};

// ── Dispatcher ────────────────────────────────────────────────────────────────

pub async fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        // ── Phase 1 ────────────────────────────────────────────────────────
        Command::Projects { config_dir } => cmd_projects(&config_dir),

        Command::New { name, config_dir } => cmd_new(&name, &config_dir),

        Command::Sync { project, dirs, .. } => {
            let cfg = load_config(project.as_deref(), &dirs.config_dir)?;
            let sources = cfg.sources.unwrap_or_default();
            let md_dir = dirs.output_dir.join("markdown");

            println!("\n── Sync  ({} source(s)) {}", sources.len(), "─".repeat(38));
            adapters::sync_all_sources(&sources, &md_dir).await?;

            println!("\n  Output : {}", md_dir.display());
            let project_flag = project
                .as_deref()
                .map(|p| format!(" --project {p}"))
                .unwrap_or_default();
            println!("  Next   : nlm run{project_flag}");
            Ok(())
        }

        // ── Phase 3: NotebookLM commands ───────────────────────────────────
        Command::Login => cmd_login().await,

        Command::List { debug } => cmd_list(debug).await,

        Command::Upload { project, notebook_id, dirs } => {
            cmd_upload(project.as_deref(), notebook_id.as_deref(), &dirs).await
        }

        Command::Generate {
            project,
            artifact_type,
            language,
            notebook_id,
            dirs,
        } => {
            cmd_generate(
                project.as_deref(),
                artifact_type,
                language.as_deref(),
                &notebook_id,
                &dirs,
            )
            .await
        }

        Command::Fetch { notebook_id, project, dirs } => {
            cmd_fetch(&notebook_id, project.as_deref(), &dirs).await
        }

        Command::Run {
            project,
            artifact_type,
            language,
            notebook_id,
            skip_upload,
            dirs,
        } => {
            cmd_run(
                project.as_deref(),
                artifact_type,
                language.as_deref(),
                notebook_id.as_deref(),
                skip_upload,
                &dirs,
            )
            .await
        }

        // ── Stubbed — Phase 4 (PPTX parsing) ──────────────────────────────
        Command::Import { pptx, .. } => {
            println!("import {} [Phase 4 — PPTX parsing]", pptx.display());
            Ok(())
        }

        // `correct` is a specialised generate — Phase 3b
        Command::Correct { .. } => stub("correct (Phase 3b — targeted slide correction)"),
    }
}

fn stub(label: &str) -> Result<()> {
    println!("[not yet implemented — {}]", label);
    Ok(())
}

// ── Helper: build a NotebookLMClient from saved session tokens ────────────────

async fn make_client() -> Result<NotebookLMClient> {
    make_client_with_debug(false).await
}

async fn make_client_with_debug(debug: bool) -> Result<NotebookLMClient> {
    let tokens = load_tokens(None).await?;
    let mut client = NotebookLMClient::new(tokens)?;
    client.debug = debug;
    Ok(client)
}


// ── login ─────────────────────────────────────────────────────────────────────

async fn cmd_login() -> Result<()> {
    // Browser automation in Rust is heavy (chromiumoxide). We delegate to the
    // Python notebooklm-py CLI which already handles Playwright login and saves
    // cookies to ~/.notebooklm/storage_state.json — exactly what we read.
    println!("Opening NotebookLM login via notebooklm-py…");
    println!("(This shells out to `notebooklm login` to save session cookies.)");
    println!();

    let status = std::process::Command::new("notebooklm")
        .arg("login")
        .status();

    match status {
        Ok(s) if s.success() => {
            println!();
            println!("  Login complete. Cookies saved to ~/.notebooklm/storage_state.json");
            println!("  Run: nlm list");
            Ok(())
        }
        Ok(s) => {
            anyhow::bail!("`notebooklm login` exited with status {s}");
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!(
                "`notebooklm` CLI not found.\n\
                 Install it with:  pip install notebooklm\n\
                 Then run:         notebooklm login\n\
                 The saved cookies at ~/.notebooklm/storage_state.json will be\n\
                 used automatically by `nlm`."
            )
        }
        Err(e) => Err(e.into()),
    }
}

// ── list ──────────────────────────────────────────────────────────────────────

async fn cmd_list(debug: bool) -> Result<()> {
    let client = make_client_with_debug(debug).await?;
    let notebooks = client.list_notebooks().await?;

    let empty = vec![];
    let arr = notebooks.as_array().unwrap_or(&empty);

    if arr.is_empty() {
        println!("No notebooks found.");
        return Ok(());
    }

    println!("\nNotebooks ({}):\n", arr.len());
    for nb in arr {
        // nb[0] = title, nb[2] = ID (UUID)
        let name = nb[0].as_str().unwrap_or("(unnamed)");
        let id = nb[2].as_str().unwrap_or("?");
        println!("  {id}  {name}");
    }
    println!();
    Ok(())
}

// ── upload ────────────────────────────────────────────────────────────────────

async fn cmd_upload(
    project: Option<&str>,
    notebook_id: Option<&str>,
    dirs: &crate::cli::DirArgs,
) -> Result<()> {
    let cfg = load_config(project, &dirs.config_dir)?;
    let md_dir = dirs.output_dir.join("markdown");

    let client = make_client().await?;

    // Resolve notebook: --notebook-id > config > find-or-create by project name
    let nb_id = resolve_notebook_id(&client, notebook_id, &cfg).await?;

    println!("\n── Upload  ({})", nb_dir_label(&md_dir));
    println!("  Notebook : {nb_id}");

    println!("  Clearing existing sources…");
    client.delete_all_sources(&nb_id).await?;

    println!("  Uploading markdown files…");
    client.upload_dir(&nb_id, &md_dir).await?;

    println!("\n  Done.  Next: nlm generate --notebook-id {nb_id}");
    Ok(())
}

// ── generate ──────────────────────────────────────────────────────────────────

async fn cmd_generate(
    project: Option<&str>,
    _artifact_type: Option<ArtifactType>,
    language: Option<&str>,
    notebook_id: &str,
    dirs: &crate::cli::DirArgs,
) -> Result<()> {
    let cfg = load_config(project, &dirs.config_dir)?;

    let lang = language
        .or_else(|| cfg.notebook.as_ref().and_then(|n| n.language.as_deref()))
        .unwrap_or("fr");

    let instructions = cfg
        .generate
        .as_ref()
        .and_then(|g| g.slide_deck.as_ref())
        .and_then(|sd| sd.instructions.as_deref())
        .unwrap_or("");

    let client = make_client().await?;

    println!("\n── Generate");
    println!("  Notebook : {notebook_id}");
    println!("  Language : {lang}");

    print!("  Generating…");
    use std::io::Write as _;
    std::io::stdout().flush().ok();

    let instr = if instructions.is_empty() { None } else { Some(instructions) };
    let artifact_id = client
        .generate_slide_deck(notebook_id, &[], instr, lang)
        .await?;

    println!(" queued ({artifact_id})");
    print!("  Waiting for completion…");
    std::io::stdout().flush().ok();

    let artifact = client.wait_for_artifact(notebook_id, &artifact_id).await?;
    println!(" done");

    let out_dir = dirs.output_dir.join("slide-deck");
    tokio::fs::create_dir_all(&out_dir).await?;
    let label = project.unwrap_or(notebook_id);
    let dest = out_dir.join(format!("{label}.pdf"));

    print!("  Downloading PDF…");
    std::io::stdout().flush().ok();
    client.download_slide_deck(&artifact, &dest).await?;
    println!(" {}", dest.display());

    Ok(())
}

// ── fetch ─────────────────────────────────────────────────────────────────────

async fn cmd_fetch(
    notebook_id: &str,
    project: Option<&str>,
    dirs: &crate::cli::DirArgs,
) -> Result<()> {
    let client = make_client().await?;

    println!("\n── Fetch  (notebook: {notebook_id})");

    let artifacts = client.list_artifacts_raw(notebook_id).await?;

    // Find the most recent completed slide deck.
    let slide_art = artifacts.iter().find(|a| {
        a[2].as_i64() == Some(ARTIFACT_SLIDE_DECK)
            && a[4].as_i64() == Some(STATUS_COMPLETED)
    });

    let Some(art) = slide_art else {
        println!("  No completed slide deck found in this notebook.");
        println!("  Run: nlm generate --notebook-id {notebook_id}");
        return Ok(());
    };

    let out_dir = dirs.output_dir.join("slide-deck");
    tokio::fs::create_dir_all(&out_dir).await?;
    let label = project.unwrap_or(notebook_id);
    let dest = out_dir.join(format!("{label}.pdf"));

    print!("  Downloading PDF…");
    use std::io::Write as _;
    std::io::stdout().flush().ok();
    client.download_slide_deck(art, &dest).await?;
    println!(" {}", dest.display());
    Ok(())
}

// ── run ───────────────────────────────────────────────────────────────────────

async fn cmd_run(
    project: Option<&str>,
    _artifact_type: Option<ArtifactType>,
    language: Option<&str>,
    notebook_id: Option<&str>,
    skip_upload: bool,
    dirs: &crate::cli::DirArgs,
) -> Result<()> {
    let cfg = load_config(project, &dirs.config_dir)?;
    let md_dir = dirs.output_dir.join("markdown");

    // ── Step 1: sync sources → markdown ─────────────────────────────────
    let sources = cfg.sources.clone().unwrap_or_default();
    if !sources.is_empty() {
        println!("\n── Sync  ({} source(s)) {}", sources.len(), "─".repeat(38));
        adapters::sync_all_sources(&sources, &md_dir).await?;
    }

    // ── Step 2: upload to NotebookLM ─────────────────────────────────────
    let client = make_client().await?;
    let nb_id = resolve_notebook_id(&client, notebook_id, &cfg).await?;

    if !skip_upload {
        println!("\n── Upload  ({})", nb_dir_label(&md_dir));
        println!("  Notebook : {nb_id}");
        println!("  Clearing existing sources…");
        client.delete_all_sources(&nb_id).await?;
        println!("  Uploading markdown files…");
        client.upload_dir(&nb_id, &md_dir).await?;
    }

    // ── Step 3: generate slide deck ───────────────────────────────────────
    let lang = language
        .or_else(|| cfg.notebook.as_ref().and_then(|n| n.language.as_deref()))
        .unwrap_or("fr");

    let instructions = cfg
        .generate
        .as_ref()
        .and_then(|g| g.slide_deck.as_ref())
        .and_then(|sd| sd.instructions.as_deref())
        .unwrap_or("");

    println!("\n── Generate  (slide-deck, lang={lang})");

    print!("  Generating…");
    use std::io::Write as _;
    std::io::stdout().flush().ok();

    let instr = if instructions.is_empty() { None } else { Some(instructions) };
    let artifact_id = client
        .generate_slide_deck(&nb_id, &[], instr, lang)
        .await?;

    println!(" queued ({artifact_id})");
    print!("  Waiting for completion…");
    std::io::stdout().flush().ok();

    let artifact = client.wait_for_artifact(&nb_id, &artifact_id).await?;
    println!(" done");

    {
        let out_dir = dirs.output_dir.join("slide-deck");
        tokio::fs::create_dir_all(&out_dir).await?;
        let label = project.unwrap_or(&nb_id);
        let dest = out_dir.join(format!("{label}.pdf"));

        print!("  Downloading PDF…");
        std::io::stdout().flush().ok();
        client.download_slide_deck(&artifact, &dest).await?;
        println!(" {}", dest.display());
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Resolve the notebook ID from: CLI flag > config notebook.name > project name
async fn resolve_notebook_id(
    client: &NotebookLMClient,
    cli_id: Option<&str>,
    cfg: &crate::config::Config,
) -> Result<String> {
    if let Some(id) = cli_id {
        return Ok(id.to_string());
    }

    // Use notebook name from config (or project name) to find or create.
    let nb_name = cfg
        .notebook
        .as_ref()
        .and_then(|n| n.name.as_deref())
        .unwrap_or("Default Notebook");

    client.find_or_create_notebook(nb_name).await
}

fn nb_dir_label(md_dir: &std::path::Path) -> String {
    format!("{}", md_dir.display())
}

// ── projects ──────────────────────────────────────────────────────────────────

fn cmd_projects(config_dir: &Path) -> Result<()> {
    let projects = list_projects(config_dir)?;
    let projects_dir = config_dir.join("projects");

    if projects.is_empty() {
        println!("No projects found in {}", projects_dir.display());
        println!("Create one with:  nlm new <name>");
        return Ok(());
    }

    println!("\nAvailable projects ({}):\n", projects_dir.display());
    for p in &projects {
        println!("  • {p}");
    }
    println!();
    Ok(())
}

// ── new ───────────────────────────────────────────────────────────────────────

const PROJECT_TEMPLATE: &str = r#"# Project: {name}
# Run: nlm run --project {name}
#
# This file overrides config/notebook.yaml for this specific project.

notebook:
  name: "{name}"
  language: fr                  # BCP-47 code: fr, en, de, nl, …
  default_artifact: slide-deck  # slide-deck | study-guide | briefing-doc | audio

generate:
  timeout: 900

  slide_deck:
    instructions: ""

  audio:
    instructions: ""

sources:
  # - type: url
  #   url: "https://docs.example.com/guide"
  #   title: "User Guide"
"#;

fn cmd_new(name: &str, config_dir: &Path) -> Result<()> {
    let projects_dir = config_dir.join("projects");
    fs::create_dir_all(&projects_dir)?;

    let out = projects_dir.join(format!("{name}.yaml"));
    if out.exists() {
        anyhow::bail!("Project already exists: {}", out.display());
    }

    let content = PROJECT_TEMPLATE.replace("{name}", name);
    fs::write(&out, content)?;

    println!("  Created : {}", out.display());
    println!("  Next    : edit the file, then run:  nlm sync --project {name}");
    Ok(())
}
