// Config loading — mirrors nlm/config.py.
//
// Key Rust concepts here:
//   - #[derive(Deserialize)]: serde auto-generates YAML → struct deserialization
//   - Option<T>: field is optional in the YAML file (missing = None)
//   - #[serde(tag = "type")]: use the "type" field as an enum discriminant
//   - serde_yaml::Value: untyped YAML tree, used for deep_merge before deserializing

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

// ── Config structs ────────────────────────────────────────────────────────────

// Every field is Option<T> so missing YAML keys deserialize to None
// instead of returning a parse error.

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    pub notebook: Option<NotebookConfig>,
    pub generate: Option<GenerateConfig>,
    pub sources: Option<Vec<Source>>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
pub struct NotebookConfig {
    pub name: Option<String>,
    pub language: Option<String>,
    pub default_artifact: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
pub struct GenerateConfig {
    pub timeout: Option<u64>,
    pub slide_deck: Option<ArtifactInstructions>,
    pub audio: Option<ArtifactInstructions>,
    pub study_guide: Option<ArtifactInstructions>,
    pub briefing_doc: Option<ArtifactInstructions>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ArtifactInstructions {
    pub instructions: Option<String>,
}

// ── Source enum ───────────────────────────────────────────────────────────────

// #[serde(tag = "type")] tells serde to read the "type" key from the YAML map
// and use its value to pick the right variant.
//
// #[serde(rename_all = "snake_case")] lowercases variant names:
//   Confluence → "confluence", Url → "url", etc.
//
// This mirrors Python's duck-typed dict dispatch but with compile-time guarantees.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)]
pub enum Source {
    Confluence {
        id: String,
        title: String,
        step: Option<u32>,
        step_label: Option<String>,
        base_url: Option<String>,
    },
    File {
        path: String,
        title: String,
    },
    Notion {
        id: String,
        title: String,
    },
    Url {
        url: String,
        title: String,
    },
    Pptx {
        path: String,
        title: String,
        dry_run: Option<bool>,
    },
}

// ── Deep merge ────────────────────────────────────────────────────────────────

// Recursively merge two untyped YAML trees (override wins on conflicts).
// We work on serde_yaml::Value before final deserialization so we can merge
// arbitrary YAML shapes without knowing the full schema upfront.
//
// In Python this was a recursive dict merge; here we match on Value variants.
fn deep_merge(base: serde_yaml::Value, over: serde_yaml::Value) -> serde_yaml::Value {
    use serde_yaml::Value;
    match (base, over) {
        // Both sides are mappings → recurse key by key
        (Value::Mapping(mut b), Value::Mapping(o)) => {
            for (k, v) in o {
                let entry = b.entry(k).or_insert(Value::Null);
                // Take the existing value out (replace with Null temporarily),
                // merge it with the override, then put the result back.
                let merged = deep_merge(std::mem::replace(entry, Value::Null), v);
                *entry = merged;
            }
            Value::Mapping(b)
        }
        // Any other combination: override wins
        (_, over) => over,
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Load and merge config files.
///
/// 1. Load `<config_dir>/notebook.yaml` as base (optional)
/// 2. If `project` is Some, load `<config_dir>/projects/<project>.yaml` and
///    deep-merge it over the base (project wins)
/// 3. Deserialize the merged YAML tree into Config
pub fn load_config(project: Option<&str>, config_dir: &Path) -> Result<Config> {
    // Helper: read a YAML file into an untyped Value, returning an empty
    // mapping if the file does not exist.
    let read_yaml = |path: &Path| -> Result<serde_yaml::Value> {
        if !path.exists() {
            return Ok(serde_yaml::Value::Mapping(Default::default()));
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read {}", path.display()))?;
        serde_yaml::from_str(&content)
            .with_context(|| format!("Cannot parse YAML in {}", path.display()))
    };

    let global_val = read_yaml(&config_dir.join("notebook.yaml"))?;

    let merged_val = match project {
        None => global_val,
        Some(name) => {
            let project_path = config_dir.join("projects").join(format!("{name}.yaml"));
            if !project_path.exists() {
                anyhow::bail!(
                    "Project config not found: {}\nRun `nlm projects` to list available projects.",
                    project_path.display()
                );
            }
            let project_val = read_yaml(&project_path)?;
            deep_merge(global_val, project_val)
        }
    };

    // Final step: typed deserialization from the merged untyped tree.
    serde_yaml::from_value(merged_val).context("Cannot deserialize config")
}

/// Return sorted list of project names found in `<config_dir>/projects/`.
pub fn list_projects(config_dir: &Path) -> Result<Vec<String>> {
    let projects_dir = config_dir.join("projects");
    if !projects_dir.exists() {
        return Ok(vec![]);
    }

    let mut names: Vec<String> = std::fs::read_dir(&projects_dir)
        .with_context(|| format!("Cannot read {}", projects_dir.display()))?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            // Keep only .yaml files; extract the stem (filename without extension)
            if path.extension()?.to_str()? == "yaml" {
                Some(path.file_stem()?.to_str()?.to_string())
            } else {
                None
            }
        })
        .collect();

    names.sort();
    Ok(names)
}
