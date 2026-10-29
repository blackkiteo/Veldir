# Veldir — public repository checklist

Repository: https://github.com/blackkiteo/Veldir

## Protecting the main branch (owner action)

The GitHub integration currently has no administrative ruleset-write
capability. The repository owner can configure the ruleset in
**Settings > Rules > Rulesets > New branch ruleset**:

1. Name: Protect main. Enforcement: **Active**.
2. Target: **Default branch** (main).
3. Turn on **Restrict deletions**, **Block force pushes**, and **Require a pull request before merging**.
4. Turn on **Require status checks to pass**; once the jobs have run, require:
   - `build-windows-x64` — Windows build and tests.
   - `gitleaks` — full-history secret scan.
5. If working alone, do not require another person's approval yet.
6. Leave any force-push, deletion and CI bypasses disabled.

Public repositories support branch rulesets on GitHub Free.
Verify enforcement really says **Active**, not Evaluate or Disabled.
Changing workflow job names may require updating required checks.

## Security settings (owner action)

- Enable **Private vulnerability reporting** and GitHub secret scanning and push protection where available.
- Keep workflow token permissions to read-only unless a specific workflow needs more.
- Review Dependabot and secret scanning alerts; rotate any genuinely exposed credentials.
- Keep personal test files and production data out of all repository branches.

## Release gate

- Keep PR #12 (`feat/roadmap-to-1-0`) as a **draft** until the manual Windows acceptance
  cases in `RELEASE_CHECKLIST.md` and issue #13 are documented.
- A successful Rust CI build is not proof of Windows GUI usability.
- The final release needs an exact verified commit, checksum and release notes.
- Signing is a separate task requiring the owner's real code-signing certificate.
