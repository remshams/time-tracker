# AGENTS.md

Time Tracker shares a Rust core across a terminal UI, an automation CLI, and a native macOS app. Clients use local SQLite storage or connect to a separate HTTP server that owns its SQLite database. Web clients and external integrations remain future work.

## Project context

- One person uses the application. Design for that user, not for teams or shared accounts.
- The user works in one window at a time. For example, while the macOS menu bar panel is open, they don't edit data in the main window.
- The user may switch between computers but rarely sends commands from two of them at once. Concurrent use of several windows or machines can happen, but don't optimize for it.
- Don't complicate the code for rare cases. If another window or machine changed data in the meantime, telling the user to reload is acceptable when handling the change automatically would add a lot of code or effort.
- Simple doesn't mean sloppy. The quality bar stays the same: correct behavior, clear errors, tests, and the validation steps below.

## Code organization

- Follow domain-driven design. Model code around the domain concepts, such as tasks, tracking, and worklogs, and use their names for types, functions, and modules.
- Inside a crate or app, split code by feature, not by technical layer. Keep a feature's state, logic, and views together, as `crates/tracker-domain/src/` and `apps/swiftui/Sources/TimeTrackerSwiftUI/Features/` do. Don't add folders such as `models/`, `services/`, or `views/` that collect one technical role across all features.
- Features may depend on other features, but their dependencies must not form a cycle. The compiler doesn't catch cycles between folders in one target, so check them in review. In the Swift app and package, only `App/` composes features at runtime, `Shared/` holds helpers that no feature owns, and `Contracts/` holds protocols and wire models ([ADR 0030](docs/adr/0030-replace-the-session-with-feature-stores.md)).
- Keep the crate boundaries described in `docs/architecture/primitives.md`. This rule covers the code inside each crate and app, not the crate split.

## General

- Always apply the `unslop` skill when writing or editing any text (responses, code comments, docs, commit messages, etc.)
- Use English text for test names, fixtures, inputs, and expected values. When a test needs a non-language Unicode glyph, such as a double-width terminal character, use a symbol or emoji instead of Chinese, Japanese, or Korean text.

## Architecture documentation

- Start with `docs/architecture/README.md`. The concise architecture and application catalog is `docs/architecture/primitives.md`; its human-readable companion is `docs/architecture/primitives.html`.
- Keep each architecture document in both Markdown and HTML. Agents read the Markdown and reviewers diff it; people read the HTML. When you add or change an architecture document, update both formats in the same change and keep their concepts, rules, ownership, and source snapshot aligned.
- The architecture diagrams are in `docs/architecture/architecture.html`. Open the file in a browser; its diagrams and controls work offline.
- When implementing a feature that changes application components, dependencies, ownership, or communication paths, update the affected diagrams and method tables in the same change. Keep the source snapshot date and revision accurate, and state the scope and revision of any retained quality measurements.
- Keep the HTML self-contained. Use portable source links and avoid machine-specific paths or links to temporary reports.
- Use the `frontend-design` skill when creating or redesigning an HTML architecture page.

## Git

- When pushing a branch, push it to every configured remote unless the user explicitly requests otherwise.

## GitHub issues

- Follow the [GitHub issue guide](docs/github-issues.md) when creating or updating an issue. Apply the `unslop` skill before publishing it.

## Pull request descriptions

- Follow the [pull request description guide](docs/pull-request-descriptions.md) when creating or updating a PR description. Apply the `unslop` skill to the description before publishing it.

## CodeRabbit

- CodeRabbit does not review pull requests automatically. Each PR gets one initial CodeRabbit review.
- Request it only when the PR is finished: your own review rounds are done, CI passes, and the branch is rebased onto its base. For a stack, wait until every layer is finished, then request one review per layer. Use `gh pr comment <number> --body "@coderabbitai review"`.
- Fix all actionable findings in one push and add a regression test for every fix that changes behavior. Reply on each thread with the commit that fixes it, or the reason for declining.
- Before replying, review the fix commits against the commit CodeRabbit reviewed, and check that each "Fixed in <sha>" reply points to a commit that addresses that finding.
- Do not request another review, except when a fix changes production behavior for a Major or Critical finding. Then request one incremental review with `@coderabbitai review`.
- When several agents work on PRs in parallel, the orchestrating agent requests the reviews, one PR at a time. Sub-agents do not post `@coderabbitai` commands.
- If CodeRabbit replies that the rate limit is exceeded, wait the stated time, then trigger the request again. Count from when its message was posted; the number in it does not count down.

## End-to-end tests

- Keep E2E tests unchanged when a task does not change requirements or externally observable behavior.
- If changed requirements or behavior warrant E2E updates, update or add those tests without asking for permission. At handoff, summarize which E2E tests were updated or added and why.
- Write E2E tests as black-box tests. Drive the application only through its public interface (CLI, HTTP API, or UI) and assert only on externally observable results such as output, exit codes, responses, persisted files, or rendered UI.
- Do not import internal modules, call internal functions, mock internal components, or inspect internal state from E2E tests. Set up preconditions through the public interface where possible. Test harness code that starts real external dependencies (servers, databases, proxies) is fine.

## Validation

- After completing a task or feature, run `python3 -m unittest discover -s scripts/tests -p 'test_*.py'`, `cargo llvm-cov --lcov --output-path lcov.info` (it runs the full test suite, so a separate `cargo test` only rebuilds and reruns the same tests), and `cargo crap --workspace --lcov lcov.info` before handoff. Run mutation checks according to the scope rules below.
- For Swift production or portable unit-test changes, also run `python3 scripts/swift-style.py`. Install its pinned tools with `python3 scripts/install-swift-style-tools.py` if needed. Native UI E2E and layout tests remain outside the initial style scope.
- Use cargo-mutants 27.1.0. Install it with `cargo install --locked cargo-mutants --version 27.1.0` if it is unavailable.
- Use cargo-llvm-cov 0.9.0 and cargo-crap 0.4.3. Install them with `rustup component add llvm-tools-preview`, `cargo +stable install --locked --version 0.9.0 cargo-llvm-cov`, and `cargo +stable install --locked --version 0.4.3 cargo-crap` if they are unavailable.
- Treat missed and timed-out mutants, and CRAP scores above the configured threshold, as failures. Report any that cannot be resolved.
- `.cargo/mutants.toml` excludes one named glue mutant: `RealEffects::show_cursor` in `apps/tui/src/terminal.rs`. Ratatui's `Terminal::drop` re-shows the cursor after our restore, so removing that crossterm call is not observable by any test; the same bytes reach the terminal either way.
- `cargo-crap` cannot match the macro-generated functions in `crates/tracker-domain/src/ids.rs`. Direct tests for those functions remain required; do not refactor the ID types to satisfy the report.

### Mutation check scope

- Select the scope from the entire task's changes, including committed and uncommitted changes, rather than only the latest edit.
- For Rust production changes, run `cargo mutants --workspace --file <path>` for each changed source file, or repeat `--file` in one command. Use repository-relative paths. Keep unit and integration tests enabled; add `--test-workspace true` when selected mutants rely on tests in other crates.
- For test-only changes, mutate the production files exercised by the changed tests. If that mapping is unclear, use `cargo mutants --package <name>` for each affected crate. For shared code, dependency, feature, or build configuration changes, expand the scope to affected crates; use `cargo mutants --workspace` when the impact cannot be bounded.
- For portable Swift production changes, run `python3 scripts/swift-mutations.py --files <path> ...` with package-relative source paths. Include production files exercised by changed tests. Use a full package run when the affected files cannot be identified.
- Skip mutation checks for documentation-only changes and other changes that cannot affect production behavior, tests, or build inputs. Report the selected scope or reason for skipping at handoff.
- `--in-diff` and `--iterate` are optional diagnostic shortcuts, not substitutes for the required file or crate checks. They can miss reduced test coverage in unchanged production code.
- Pull requests run a focused portable Swift mutation check. Keep full Rust and Swift mutation runs manual; do not schedule them or add a repository file that tracks the last run.
