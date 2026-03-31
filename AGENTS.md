# AGENTS

## Coding
- We use JJ for version control workflows.
- We usually work in a linked worktree instead of the primary checkout. Default to JJ workflows here, because repository metadata does not always resolve reliably from the current directory in this setup. Only rely on git when you have confirmed the worktree supports it correctly.
- Always run linters and keep code lint-free before finishing changes.
- Always review the code you created for issues before finalizing.

## Commits
- Never create one giant commit for all changes. Break work into small, focused commits where each commit contains a single logical unit of change (e.g., one refactor, one feature, one fix).
- Never leave empty commits in the JJ log history. Use `jj abandon <id>` to remove them.
- After finishing a chunk of work, always leave the repository in a fresh empty JJ changeset on top of the completed work.

## Code Size
- If the same string is repeated more than once, extract it into a variable or property.

## Pull Requests
- Always use the GitHub CLI (`gh`) for pull request creation and related GitHub operations. Do not try to create PRs by pushing ad-hoc branches or by relying on the default shell flow instead of `gh`.
- Before creating a PR, or immediately after updating one, make sure the local gitignored build server file (`buildServer.json`) is present and up to date for your machine. If it is missing or stale after project or scheme changes, regenerate it with `xcode-build-server config -scheme "TimeTracker"`.
- Run the heavier PR review sub-agents (`code-reviewer`, `refactor-specialist`, and `security-reviewer` when the PR touches security-sensitive areas) as part of the initial PR creation workflow and again immediately before merging, not on every incremental PR update.
- Before merging a PR, if the user did not explicitly ask for the refactor-specialist review earlier, prompt the user first to ask whether they want that refactoring review run.
- When merging a PR on GitHub, always use "Rebase and merge" — never "Create a merge commit" or "Squash and merge". This keeps the history linear and avoids merge commits.

## Code Review & Refactoring
- When a code review or refactor-specialist agent returns recommendations that are out of scope for the current task (e.g. a behaviour change in a pure refactor PR, or a separate concern unrelated to the feature being built), do NOT silently drop them. Always include them explicitly in the final summary presented to the user under a clearly labelled section such as "Out-of-scope findings — deferred" so they are visible and can be tracked as follow-up work.

## Sub-agent Recommendations
- When any sub-agent returns a recommendation that is not implemented (for any reason — out of scope, deferred, consciously rejected), do NOT silently drop it. Always include it explicitly in the final summary presented to the user under a clearly labelled section such as "Deferred recommendations" so they are visible and can be tracked as follow-up work.
