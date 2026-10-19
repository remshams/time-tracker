# Architecture

Start with the [architecture and application primitives](primitives.md) for each crate and client, their dependencies, and the shared application rules. The [visual guide](primitives.html) presents the same catalog with diagrams and short explanations. Both describe the implemented application.

Read [revisions and concurrency](primitives.md#revisions-and-concurrency) for command expected values, bounded request replay, coherent aggregate reads, database pagination counters, and why these mechanisms coexist. The [HTML section](primitives.html#revisions-and-concurrency) includes the same comparison.

Open the [detailed architecture diagrams](architecture.html) in a browser for crate dependencies, client state ownership, method tables, and Swift/Rust call paths. Both HTML pages work offline.

The [architecture decision records](../adr/README.md) explain the decisions behind the current design. Follow their supersession notes when reading older records.

Keep the Markdown catalog and its HTML companion aligned when a concept, rule, or owner changes. Source links point into this checkout. Each page states its source scope and revision; the detailed diagrams retain their separately dated quality measurements.
