# AGENTS

## Coding
- We use JJ for version control workflows.
- Always run linters and keep code lint-free before finishing changes.
- Always review the code you created for issues before finalizing.

## Commits
- Never create one giant commit for all changes. Break work into small, focused commits where each commit contains a single logical unit of change (e.g., one refactor, one feature, one fix).
- Never leave empty commits in the JJ log history. Use `jj abandon <id>` to remove them.

## Code Size
- If the same string is repeated more than once, extract it into a variable or property.

## Pull Requests
- When merging a PR on GitHub, always use "Rebase and merge" — never "Create a merge commit" or "Squash and merge". This keeps the history linear and avoids merge commits.
