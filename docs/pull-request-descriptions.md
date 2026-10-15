# Pull request descriptions

Write for a reviewer who has not read the conversation or issue history. Apply the `unslop` skill when writing or editing a PR description. Use plain English, concrete behavior, and links that help the reviewer check your claims. Keep the description current as the scope changes.

Use the sections below, with the opening description first. Scale the detail to the change. A documentation fix may need only a sentence per section; a protocol change needs enough detail to assess compatibility and deployment. Replace examples with facts about the PR.

## Opening description

Start with a short paragraph explaining the problem, what motivated the change, and the resulting behavior. Name the user action or failure that prompted the work when relevant. Link the originating issue if there is one, but include enough context to understand the PR without opening it.

For example, explain that switching tasks could leave a remote client's elapsed timer on the previous task, and that the change refreshes tracking state after a confirmed switch. Avoid opening with a list of files or a chronology of implementation work.

## Architecture impact

Use the names in the [architecture catalog](architecture/primitives.md). Include a table with one row for each affected crate or client. Describe changes to its responsibilities, dependencies, contracts, or communication with other components. Include affected application rules, such as worklog overlap or active tracking, where relevant.

The following rows illustrate a hypothetical protocol change.

| Architectural primitive | What changed | Effect on callers or boundaries |
| --- | --- | --- |
| `tracker-protocol` | Added a required field to tracking observations. | Clients must understand the new response contract. |
| `tracker-server` | Populates the field when reading tracking state. | The deployed server must provide the updated response. |
| `tracker-remote` | Validates the field before publishing tracking state. | Older server responses fail validation. |

If no architectural primitive changes, keep the table and use a single row stating that, with the actual scope, such as documentation or tooling. Link any updated architecture documentation or ADRs. Do not claim an ownership or dependency change when the PR only updates behavior inside an existing boundary.

## Important decisions

Explain choices that a reviewer could reasonably question. State the chosen approach, the constraint it addresses, and the tradeoff. Mention a rejected alternative only when it explains the decision. Link relevant ADRs rather than repeating them. If the PR introduces no such decisions, say so briefly.

## After merge

State the actions needed after merging, who should perform them, and their order when it matters. Include server redeployment, client releases, database migrations, configuration changes, or follow-up work that the PR requires.

For a protocol change, spell out which server and client versions work together and whether deployment order matters. For example, if the new client requires a response field, explain that the server must be redeployed before releasing that client, provided existing clients still accept the new response. State any restart, migration, or rollback constraints. If merging requires no further action, write "No post-merge action required."

## Risk

Choose `small`, `medium`, or `high` and explain the rating in terms of what could fail and who would be affected. Diff size alone does not determine risk.

- `small` fits a contained change with limited consequences and a straightforward revert, such as a documentation correction.
- `medium` fits a behavior change that can affect a client workflow or several components but has a practical recovery path.
- `high` fits a change that could lose data, break deployed clients, or require a coordinated rollout or difficult rollback.

Name the main failure case, the validation that reduces the risk, and any remaining uncertainty. Summarize relevant checks and their results; report checks that failed or could not run. Explain recovery steps when reverting the commit alone would not restore service or data.

## Review focus

Direct the reviewer to the parts that need judgment. Link the relevant files, tests, or ADRs and explain what to verify, such as transaction boundaries, stale-response handling, protocol compatibility, or a changed domain rule. Mention test gaps and assumptions that need confirmation.

Keep this specific to the PR. "Check that a failed refresh preserves the confirmed command result" gives the reviewer a useful question; "review the code carefully" does not.
