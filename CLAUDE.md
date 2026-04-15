# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`nlm` is a single-binary Rust CLI (edition 2021, stable toolchain) that automates NotebookLM end-to-end: sync sources from Confluence/Notion/URL/file/PPTX into markdown, upload them to a NotebookLM notebook via Google's internal batchexecute RPC, and generate AI artifacts. See `README.md` for user-facing command reference.

The code is intentionally pedagogical — inline comments explain Rust concepts on first use in a new context. Preserve that style.

## Build, test, run

```bash
cargo build                         # debug build
cargo build --release               # release build
cargo test                          # run tests
cargo test <name_substring>         # run a subset by name
cargo clippy --locked -- -D warnings  # same invocation as CI
cargo fmt --check                   # CI blocks on this — run `cargo fmt` before pushing
cargo install --path . --force      # install the branch's binary into ~/.cargo/bin/nlm
```

CI pipeline (`.github/workflows/ci.yml`): `fmt → clippy → build → test`, with `-D warnings` on clippy. Commits starting with `chore(release):` bypass CI (Release Please version-bump commits only).

## Architecture

**Module layout** — flat, no layering beyond module boundaries:

```
src/
  main.rs          — tokio::main entrypoint, delegates to commands::dispatch
  cli.rs           — clap derive structs only (no logic)
  commands/mod.rs  — dispatch() + one cmd_* async fn per subcommand
  config.rs        — YAML config loading + deep merge (global ← project override)
  adapters/        — source-sync adapters (confluence, notion, url, file, pptx)
  notebooklm/      — auth.rs, rpc.rs, client.rs (all NotebookLM interaction)
```

**Hard invariants — do not cross these module lines:**

- `cli.rs` defines the CLI shape only. No behavior.
- `commands/mod.rs` owns all command logic. One `cmd_<name>` async fn per subcommand. Add a new command = add variant in `cli.rs` + arm in `dispatch()` + `cmd_<name>` in this file.
- `notebooklm/client.rs` owns all RPC calls. Command handlers never touch `rpc::rpc_url` / `rpc_body` / `decode_response` directly — they call `NotebookLMClient` methods. If you need a new RPC operation: add the method id to `notebooklm/rpc.rs` and a method to `NotebookLMClient`.
- Config: every command that loads config uses `load_config(Option<project>, &config_dir)` and supports project override on top of global `notebook.yaml`.

**NotebookLM RPC specifics — non-obvious, easy to get wrong:**

- All operations go through a single endpoint: `POST https://notebooklm.google.com/_/LabsTailwindUi/data/batchexecute`. The wire format (double-encoded envelope, chunked anti-XSSI response) is documented at the top of `notebooklm/rpc.rs`.
- `source_path` in the RPC URL must be `/notebook/{id}` for notebook-scoped calls (add/delete source, get notebook, generate artifact) and `/` for homepage calls (list/create notebook). The wrong `source_path` causes silent RPC failures. Pass the correct one via the third arg of `NotebookLMClient::rpc()`.
- When a `wrb.fr` item has `item[2] == null`, batchexecute has **rejected** the call. `decode_response` bails with `raw_item=… raw_body_head=…` — read the raw item to see the real error code.
- Common error signature: `[["e", 4, null, null, <N>]]` chunk alongside a null-result item ⇒ code 4 = INVALID_ARGUMENT. Usually a missing required field (e.g. `CREATE_ARTIFACT` needs non-empty source IDs; `generate_slide_deck(nb, &[], …)` will fail with this).
- `--debug` flag on `list`, `upload`, `generate`, `fetch`, `correct`, `run` prints the raw RPC body to stderr via `decode_response_debug` — use it to diagnose any null-result bail.

**Auth flow** — `nlm login` shells out to the Python `notebooklm` CLI (via `std::process::Command`) which handles Playwright browser login and writes `~/.notebooklm/storage_state.json`. `notebooklm/auth.rs` reads that file. There is no Rust browser automation; `notebooklm-py` is a hard runtime dep for first-time login only.

**Resolution chains to reuse (do not reinvent):**

- Language: `cli_flag → cfg.notebook.language → "fr"`.
- Instructions: `cfg.generate.<artifact>.instructions → ""` then `if empty { None } else { Some(...) }`.
- Notebook ID: `resolve_notebook_id(&client, cli_id, &cfg)` when ID is `Option<&str>`; pass directly when it is a required `String`.
- Ready source IDs: `list_sources → filter src[3][1] == STATUS_COMPLETED → map src[0].as_str() or src[0][0].as_str()`. This pattern exists in `cmd_generate` and `cmd_run --skip-upload` — extract to a helper if adding a third call site.

**Error handling** — `anyhow::Result` + `?` everywhere; user-facing errors use `anyhow::bail!` with actionable messages; no `unwrap()` in production paths.

**Output UX convention** — every command prints the same shape. Match it when adding new commands:
```
── Section header  (context info)
  Key      : value
  Status…  done
  Output   : path
  Next     : nlm <next-command>
```

## Conventions

- Commit messages: **Conventional Commits** (`feat(scope): …`, `fix(scope): …`, `chore:`, `ci:`, `style:`, `docs:`, `refactor:`, `test:`, `perf:`). Scopes used so far: `rpc`, `generate`, `run`, `artifact`, `cli`, `release`, `deps`. Breaking changes: `!` after scope.
- Files: `snake_case.rs`. Types: `PascalCase`. Functions/vars: `snake_case`. Constants: `SCREAMING_SNAKE_CASE`. Command handlers: `cmd_<name>`.
- No test suite currently exists (`cargo test` passes with zero tests). New functions are manually verified via `cargo build` + live CLI run against a real notebook. If you add tests, add them alongside the code (`#[cfg(test)] mod tests`) — no separate `tests/` directory yet.
- Section separators in source: `// ── Label ─...` to 80 cols.
- Keep `#[allow(dead_code)]` on unused constants/fields that are documented reference values (e.g. RPC method IDs not yet wired up).
- **Never commit** `.claude/` (per-machine Claude Code config — already gitignored), `.env`, `config/notebook.yaml`, or `config/projects/*.yaml`. See `.gitignore`.

## Specialist agent

`agents/repo-specialist.agent.md` is the authoritative reference for senior-* agents delegated to this repo. If you change tech stack, naming, architecture, or add a new strategic pattern, update it in the same PR.

## Releases

Managed by **Release Please** (`.github/workflows/release-please.yml`). Merging to `main` opens/updates a release PR that bumps `Cargo.toml` + `CHANGELOG.md`. Merging that release PR publishes a GitHub release and triggers the binary build workflow (`release.yml`) which uploads platform binaries to the tag. Do not manually bump the version or edit `CHANGELOG.md`.
