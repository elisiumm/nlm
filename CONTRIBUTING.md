# Contributing to nlm

Thank you for your interest in contributing.

## Prerequisites

- [Rust stable](https://rustup.rs/) (check with `rustc --version`)
- `cargo` (bundled with Rust)

## Build from source

```bash
git clone https://github.com/elisiumm/nlm.git
cd nlm
cargo build
```

## Run tests

```bash
cargo test
```

## Code style

This project uses standard Rust formatting and linting:

```bash
cargo fmt          # Format code
cargo clippy       # Lint (zero warnings expected)
```

Please ensure both pass before submitting a pull request.

## Commit convention

All commits must follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(cli): add --dry-run flag to sync command
fix(adapters): handle empty confluence response
docs(readme): update configuration section
```

**Allowed types**: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`

Breaking changes must use `!` after the type or include a `BREAKING CHANGE:` footer.

## Pull request process

1. Fork the repository and create your branch from `main`.
2. Make your changes with tests where applicable.
3. Ensure `cargo fmt`, `cargo clippy`, and `cargo test` all pass.
4. Open a pull request with a clear description of what and why.
5. A maintainer will review and merge or request changes.

## Reporting issues

Use [GitHub Issues](https://github.com/elisiumm/nlm/issues) to report bugs or request features. Include:
- Your OS and Rust version (`rustc --version`)
- The command you ran
- The output or error message
