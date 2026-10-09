# AGENTS.md

Time Tracker is a Rust time-tracking application. It starts with a TUI and may later add synced storage, web clients, and external integrations.

## General

- Always apply the `unslop` skill when writing or editing any text (responses, code comments, docs, commit messages, etc.)
- Use English text for test names, fixtures, inputs, and expected values. When a test needs a non-language Unicode glyph, such as a double-width terminal character, use a symbol or emoji instead of Chinese, Japanese, or Korean text.

## Architecture documentation

- The architecture diagrams are in `docs/architecture.html`. Open the file in a browser; its diagrams and controls work offline.
- When implementing a feature that changes application components, dependencies, ownership, or communication paths, update the affected diagrams and method tables in the same change. Keep the source snapshot date and revision accurate, and state the scope and revision of any retained quality measurements.
- Keep the HTML self-contained. Use portable source links and avoid machine-specific paths or links to temporary reports.

## Git

- When pushing a branch, push it to every configured remote unless the user explicitly requests otherwise.

## End-to-end tests

- Keep E2E tests unchanged when a task does not change requirements or externally observable behavior.
- If changed requirements or behavior warrant E2E updates, obtain explicit user consent before modifying those tests.

## Validation

- After completing a task or feature, run `python3 -m unittest discover -s scripts/tests -p 'test_*.py'`, `cargo test`, `cargo llvm-cov --lcov --output-path lcov.info`, and `cargo crap --workspace --lcov lcov.info` before handoff. Run mutation checks according to the scope rules below.
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
- Routine full mutation runs will be scheduled when CI is set up. Until then, focused checks satisfy the handoff requirement; do not add a last-run metadata file or automatic full-run scheduler.
