# Security Policy

Report vulnerabilities privately by emailing
luisfuturist+9aia@gmail.com with "SECURITY" in the subject — do
not open public issues, do not use GitHub's advisory or
private-reporting features, and do not disclose details publicly;
contact us first and we will coordinate disclosure privately.

## Scope

- `myco` writes only inside the repo's `project/` worktree — every
  mutation is one atomic, attributable commit on the `project`
  branch; history is the audit trail.
- Config (`myco.toml`) holds names and paths, never secrets; secrets
  resolve via env var → OS keyring → `~/.config/myco/env` (chmod 600)
  when a phase needs them.
- Item IDs and revisions are path-validated before they touch file
  paths or git args — no `..`, no `/`, bounded length.

## Notes

We respond to reports as quickly as we can; expect acknowledgement
within a few days.
