# Issue tracker: GitHub

Issues and specifications for this repository live as GitHub issues.
Use the `gh` CLI for all operations.

## Conventions

- **Create an issue**: `gh issue create --title "..." --body "..."`.
- **Read an issue**: `gh issue view <number> --comments`, including labels and relevant comments.
- **List issues**: `gh issue list --state open --json number,title,body,labels,comments` with suitable label and state filters.
- **Comment on an issue**: `gh issue comment <number> --body "..."`.
- **Apply or remove labels**: `gh issue edit <number> --add-label "..."` or `--remove-label "..."`.
- **Close an issue**: `gh issue close <number> --comment "..."`.

Infer the repository from `git remote -v`.
The `gh` CLI does this automatically when run inside the clone.

## Pull requests as a triage surface

**PRs as a request surface: no.**

GitHub shares one number space across issues and pull requests.
Resolve an ambiguous `#42` with `gh pr view 42` and fall back to `gh issue view 42`.

## When a skill says "publish to the issue tracker"

Create a GitHub issue.

## When a skill says "fetch the relevant ticket"

Run `gh issue view <number> --comments`.

## Wayfinding operations

The `/wayfinder` skill represents a map as one GitHub issue with child issues as decision tickets.

- **Map**: Create one issue labelled `wayfinder:map` that holds Notes, Decisions-so-far, and Fog.
- **Child ticket**: Link a child as a GitHub sub-issue and apply a `wayfinder:<type>` label.
- **Blocking**: Use GitHub's native issue dependencies, falling back to a `Blocked by: #<n>` line only when dependencies are unavailable.
- **Frontier query**: Select the first open, unassigned child without an open blocker.
- **Claim**: Assign the child to the driving developer as the session's first write.
- **Resolve**: Comment with the answer, close the child, and add a context pointer to the map's Decisions-so-far.
