# Issue tracker: GitHub

Issues and specifications live in `cvxtct/take`. Use the `gh` CLI with
`--repo cvxtct/take`, or run it inside this repository.

Follow the workflow in `AGENTS.md`: issue → dedicated branch from current
`main` → implementation and tests → review → pull request linked to the issue.
Do not push changes directly to `main`.

## Operations

- Create: `gh issue create --repo cvxtct/take --title "..." --body-file <file>`.
  Write multiline bodies to a file first to preserve their formatting.
- Read: `gh issue view <number> --repo cvxtct/take --json number,title,body,labels,comments`.
- List: `gh issue list --repo cvxtct/take --state open --json number,title,body,labels`.
- Label: `gh issue edit <number> --repo cvxtct/take --add-label "..."` or
  `--remove-label "..."`. See `triage-labels.md` for the label vocabulary.
- Close completed work through the linked PR using `Closes #<number>`.

When a skill says "publish to the issue tracker", create a GitHub issue.
When it says "fetch the relevant ticket", read that issue and its comments.

## Pull requests as a triage surface

**PRs as a request surface: no.**

Pull requests deliver agreed work; external PRs are not a feature-request queue.

## Attribution

Configuration adapted from Matt Pocock's `setup-matt-pocock-skills`:
https://github.com/mattpocock/skills
