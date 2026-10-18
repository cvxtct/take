# Development workflow

The initial project setup is committed directly to main at the owner's request.
For every subsequent change:

1. Create or identify a GitHub issue describing the change.
2. Create a dedicated work branch from current main (use `codex/<issue-number>-<short-description>` for Codex work).
3. Implement and validate the change on that branch.
4. Open a pull request linked to the issue; do not push changes directly to main.

## Validation

Run on macOS: `cargo fmt --all -- --check`, `cargo test --locked`, and
`cargo clippy --locked --all-targets -- -D warnings`.
Live capture checks require macOS capture permission; unit tests do not.

Keep recordings, built app bundles, backups, and target output out of Git.
Keep one launchable installed app; archive backup bundles to avoid duplicate
Spotlight entries and ambiguous capture permission entries.

## Agent skills

### Issue tracker

Use GitHub Issues in `cvxtct/take`.
See `docs/agents/issue-tracker.md`.

### Triage labels

Use the five default triage labels when helpful.
See `docs/agents/triage-labels.md`.

### Domain docs

Use one root `GLOSSARY.md` and `docs/adr/`.
See `docs/agents/domain.md`.
