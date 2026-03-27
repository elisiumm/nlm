# Security Policy

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✓         |

## Reporting a vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

To report a vulnerability, use one of the following channels:

- **GitHub private disclosure**: [Security advisories](https://github.com/elisiumm/nlm/security/advisories/new)

Include the following in your report:

1. A description of the vulnerability and its potential impact
2. Steps to reproduce the issue
3. Any suggested mitigations or fixes

**What to expect:**
- Acknowledgement within 5 business days
- A status update within 14 days
- Coordinated disclosure once a fix is available

## Credential handling

`nlm` reads credentials exclusively from environment variables or a local `.env` file. Credentials are **never hardcoded** in the source code and should **never be committed to version control**. The `.gitignore` provided in this project excludes `.env` files by default.

## Out of scope

The following are **not** considered security vulnerabilities for this project:

- NotebookLM or Google authentication design (upstream responsibility)
- Issues requiring physical access to the user's machine
- Denial-of-service via intentionally malformed config files on a local machine
