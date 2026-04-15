# nlm

A fast Rust CLI for [NotebookLM](https://notebooklm.google.com) — sync sources from Confluence, Notion, URLs, and local files, upload them to a notebook, and generate AI artifacts (slide decks, study guides, briefing docs, audio overviews).

---

## Features

- **Sync** sources from Confluence, Notion, web URLs, local files, and PPTX
- **Upload** synced markdown to a NotebookLM notebook (drop & replace)
- **Generate** AI artifacts: slide decks, study guides, briefing docs, audio
- **Full pipeline** with a single `nlm run` command
- **Per-project config** with YAML files and deep merge
- **Correction** loop: re-generate a specific slide with a targeted prompt

---

## Installation

### Pre-built binary (recommended)

Download the latest release for your platform from the [Releases page](https://github.com/elisiumm/nlm/releases).

```bash
# macOS Apple Silicon
curl -L https://github.com/elisiumm/nlm/releases/latest/download/nlm-VERSION-aarch64-apple-darwin.tar.gz | tar -xz
sudo mv nlm /usr/local/bin/

# macOS Intel
curl -L https://github.com/elisiumm/nlm/releases/latest/download/nlm-VERSION-x86_64-apple-darwin.tar.gz | tar -xz
sudo mv nlm /usr/local/bin/

# Linux x86_64
curl -L https://github.com/elisiumm/nlm/releases/latest/download/nlm-VERSION-x86_64-unknown-linux-gnu.tar.gz | tar -xz
sudo mv nlm /usr/local/bin/
```

Replace `VERSION` with the tag from the [Releases page](https://github.com/elisiumm/nlm/releases) (e.g. `v0.1.0`).

### Build from source

**Prerequisites**: [Rust stable](https://rustup.rs/)

```bash
git clone https://github.com/elisiumm/nlm.git
cd nlm
cargo install --path .
```

---

## Quick Start

```bash
# 1. Authenticate with Google
nlm login

# 2. Scaffold a new project
nlm new myproject

# 3. Edit config/projects/myproject.yaml (see Configuration below)

# 4. Run the full pipeline
nlm run -p myproject -t slide-deck
```

---

## Commands

| Command | Description |
|---------|-------------|
| `nlm login` | Authenticate with Google (opens browser) |
| `nlm new <name>` | Scaffold a new project config file |
| `nlm projects` | List all available project configs |
| `nlm sync -p <name>` | Pull sources → `output/markdown/` |
| `nlm upload -p <name>` | Drop & re-upload sources to NotebookLM |
| `nlm generate -p <name> -t <type> --notebook-id <id>` | Generate an artifact from an existing notebook |
| `nlm fetch --notebook-id <id>` | Download the latest slide deck without regenerating |
| `nlm run -p <name>` | Full pipeline: sync + upload + generate |
| `nlm correct "<prompt>" --slide <n> --notebook-id <id>` | Re-generate one slide with a correction |
| `nlm list` | List all NotebookLM notebooks on the account |
| `nlm import <file.pptx>` | Extract brand charter from a PPTX template |

### Artifact types

| Flag | Description |
|------|-------------|
| `slide-deck` | Google Slides-style presentation |
| `study-guide` | Structured study guide |
| `briefing-doc` | Executive briefing document |
| `audio` | Audio overview |

---

## Configuration

### Environment variables (`.env`)

```env
# NotebookLM authentication (required)
NLM_COOKIES=<your-google-auth-cookies>

# Confluence adapter (required if using Confluence sources)
CONFLUENCE_BASE_URL=https://yourcompany.atlassian.net
CONFLUENCE_USERNAME=your@email.com
CONFLUENCE_API_TOKEN=your-api-token

# Notion adapter (required if using Notion sources)
# Create an integration at https://www.notion.so/my-integrations,
# copy the secret, and share the target page(s) with the integration.
NOTION_TOKEN=secret_...
```

### Global config (`config/notebook.yaml`)

```yaml
notebook:
  name: My Notebook
  language: en
  default_artifact: slide-deck

generate:
  timeout: 120
  slide_deck:
    instructions: "Focus on executive summary slides."
```

### Project config (`config/projects/<name>.yaml`)

Project configs are deep-merged over `notebook.yaml`. Any key in the project config overrides the global value.

```yaml
notebook:
  name: Q1 Engineering Review

sources:
  - type: confluence
    id: "123456"
    title: "Architecture Decision Records"
    base_url: https://yourcompany.atlassian.net

  - type: notion
    id: "abc123"
    title: "Product Roadmap"

  - type: url
    url: https://example.com/report.html
    title: "Market Report"

  - type: file
    path: docs/local-notes.md
    title: "Local Notes"

  - type: pptx
    path: templates/brand.pptx
    title: "Brand Charter"
```

### Source types

| Type | Required fields | Description |
|------|----------------|-------------|
| `confluence` | `id`, `title` | Confluence page by ID |
| `notion` | `id`, `title` | Notion page by ID |
| `url` | `url`, `title` | Web page |
| `file` | `path`, `title` | Local file |
| `pptx` | `path`, `title` | PowerPoint file |

---

## Directory structure

```
.
├── config/
│   ├── notebook.yaml          # Global config (base)
│   └── projects/
│       └── myproject.yaml     # Project override
├── output/
│   └── markdown/              # Synced source files
└── .env                       # Secrets (never commit)
```

---

## License

MIT — see [LICENSE](LICENSE).
