# Security policy — Veldir

Veldir is pre-1.0 software. Windows file operations and integrations have not
completed manual release acceptance tests. **Do not rely on development
builds as the only copy of important files.**

## Reporting a security vulnerability

Please **do not** post exploits, credentials, or sensitive reproduction
details in public GitHub issues or pull requests.

Use [GitHub's private vulnerability reporting](https://github.com/blackkiteo/Veldir/security/advisories/new)
when available. The owner can enable private vulnerability reporting
under the repository's Security settings.

For ordinary functional bugs without sensitive information, open a public
issue with the Windows version, Veldir version, steps to reproduce and
expected/actual behavior.

## Contributors

Never commit passwords, API keys, access tokens, private certificates,
signing keys, personal documents or sample data containing private information.
Keep signing credentials outside the repository and GitHub Actions logs.

The public workflow scans Git history for potential secrets and Dependabot
proposes dependency updates. **Automated scans do not guarantee the
absence of vulnerabilities or exposed credentials.**

## Supported versions

Until the first stable release, only the latest development version is actively
maintained. The release checklist in the repository must pass before a 1.0
version is labeled stable.
