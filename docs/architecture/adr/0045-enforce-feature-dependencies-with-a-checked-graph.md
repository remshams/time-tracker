# 0045: enforce feature dependencies with a checked graph

## Status

Proposed. It partially supersedes ADR 0034 for the clause listed under [Superseded clauses](#superseded-clauses).

## Date

2026-11-25

## Context

[ADR 0034](0034-allow-no-cycles-between-folders-in-a-target.md) forbids cycles between folders in a target, and review was the only check. Swift and C++ compile a target as one unit, so neither compiler rejects a dependency between two of its folders. A one-way dependency from one feature onto another is legal under ADR 0034, so a reviewer had no rule to point at, and nothing showed it in the diff.

The Rust crates have the opposite gap. Cargo enforces each crate's declared dependencies, but nothing checked that the crate graph in `architecture.md` still matched the manifests.

Instructions to agents can be ignored, so the check has to come from scripts that CI runs.

## Decision

### Folder graph for the Omarchy and Swift apps

- `scripts/feature-dependencies.py --stack omarchy|swift` reads the production sources and collects the edges between folders. A file in folder A that uses something defined in folder B is an edge A → B.
- The nodes follow ADR 0034: `App/`, `Shared/`, `Contracts/` and `Bridge/` are one node each, and so is each folder under `Features/` or `Infrastructure/`. Deeper folders belong to their node. The Swift app and the `TrackerClient` package are checked as separate targets.
- Each app keeps a config next to it: `apps/omarchy/feature-dependencies.toml`, and `apps/swiftui/feature-dependencies.toml` with one section per target. It lists every node and the nodes it uses. Nodes in `implicit`, such as `Shared` and `Contracts`, may be used by every node without being listed. A TOML comment can explain an edge.
- The check fails when:
  - the code has an edge the config doesn't list;
  - the config lists an edge the code no longer has, or lists an implicit node;
  - a folder has no section, or a section names a folder that doesn't exist;
  - the folders form a cycle. The edges to implicit nodes count here, so a dependency from `Shared/` onto a feature that uses `Shared/` fails, and so does any dependency onto `App/` from a feature that `App/` uses.
- The config always matches the code. Every added or removed dependency shows up as a diff in the config, where review sees it.
- The scripts only check. On a failure they print the edge, up to three `file:line` examples and the line to add or remove. Nothing rewrites the config, so each new edge is a deliberate edit.
- Test sources aren't scanned. Tests may use any feature.

### What counts as a use

- **Omarchy:** a C++ `#include` of a file in another folder, and a QML `import` of another folder. QML also reaches the stores through the context properties that `App/AppController.cpp` sets. The check follows each property to the member it names, then to the member's type in `AppController.h`, then to the folder of that type's header. A QML file that writes `catalog.` uses `Features/TaskCatalog`. A property whose type the check can't find fails the check. A QML `id` or property that shares a context property's name counts as a use too. Rename it.
- **Swift:** a use of a top-level type, protocol, type alias, function or constant declared in another folder of the same target, outside comments and strings. `extension X` counts as a use of X. Names are unique within a target, so matching by name is reliable. A member that one folder adds to another folder's type through an extension counts only as a use of the type's folder.

### Crate graph for Rust

- `scripts/crate-dependencies.py` compares the normal dependencies between workspace crates from `cargo metadata` with `architecture.md`. It reads the core crate diagram and the table of core crates each application uses, both between marker comments.
- Cargo is the source of truth. A crate dependency that the document doesn't show, or one the document shows that Cargo doesn't declare, fails the check. A new crate dependency therefore shows up as a diff in the architecture document.

### Where it runs

- `apps/omarchy/bin/lint` runs the Omarchy check.
- The `swift-quality` CI job runs the Swift check. It needs only Python, so it runs on Linux for every change.
- The `lint` CI job runs the crate check, and its Python test step runs the tests of both scripts.

### Rejected alternatives

- **Layer tags such as surface, domain and shared, with rules between them.** A feature folder holds its views and its stores together by design, so a folder has no single layer. The listed edges and the cycle check cover the remaining cases.
- **An exceptions list for known violations.** No target has a cycle today, and the listed edges already record every dependency.
- **Folder rules without a full list of edges.** A new dependency would pass without a diff.
- **SwiftSyntax, Harmonize or Tuist for Swift.** They need a build step or a toolchain on the CI runner, or they check only module imports. A name index is enough, because names are unique within a target.
- **Using the architecture document as the rule for crates.** Cargo already enforces the manifests. The check keeps the document honest instead of adding a second rule.

### Superseded clauses

This record replaces this clause:

- ADR 0034, "Checking the rule": "Review checks the rule, because Swift compiles a target as one module and doesn't reject cycles between its folders. Add a script if cycles creep back." The Swift check now enforces the rule.

## Consequences

- A new dependency between features, or between crates, fails CI until the config or the architecture document lists it, so review always sees it.
- A removed dependency fails too, until its entry goes. The config can't fill up with stale edges.
- The Swift check matches names, not types. A local variable that shares a top-level name with a declaration in another folder counts as a use. Renaming either one fixes it.
- Each feature can still use another feature's internal types, as long as the edge is listed. A list of the types each feature exposes would catch that, and is possible later.
- References from the macOS app into the `TrackerClient` package, and modules inside a Rust crate, aren't checked.
