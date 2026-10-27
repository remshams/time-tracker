# GitHub issues

Follow this guide when creating or updating a GitHub issue for Time Tracker. Apply the `unslop` skill to the title and body before publishing.

## Opening summary

Start every issue body with a short paragraph summarizing the problem or requested feature. State what goes wrong or what the user should be able to do. Keep the title short and specific too.

For example, "The CLI cannot rename an archived task. Allow renaming without restoring the task so users can correct names while keeping the task archived."

Add supporting details after the summary as needed. For a bug, include reproduction steps, actual behavior, and expected behavior. For a feature, describe the intended behavior and how to verify it. Distinguish confirmed facts from assumptions.

## Labels

Assign at least one relevant GitHub label to every issue. Check the repository's existing labels and select those that match the work, such as a bug, feature, or documentation label if available. Apply labels through GitHub's issue metadata; mentioning them in the body does not assign them.

## Affected architectural primitives

Include an "Affected architectural primitives" section. Use the names in the [architecture catalog](architecture/architecture.md) and list each affected crate or client when you can identify it. Briefly explain its connection to the issue. Include affected application rules where relevant.

For example, a CLI task-renaming issue might list `tracker-cli` for command handling and `tracker-application` for the shared rename use case. Check the code before treating either as a confirmed cause.

If the impact is uncertain, say "Unknown" and describe what needs investigation. If no architectural primitive is affected, say "None" and state the scope, such as documentation. Do not invent an architectural impact to fill the section.

## Before publishing

- Confirm that the body starts with a concise problem or feature summary.
- Assign at least one relevant repository label.
- List affected architectural primitives, or explain why they are unknown or none apply.
- Apply the `unslop` skill to the title and body, removing filler and keeping concrete details.
