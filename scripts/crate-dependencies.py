#!/usr/bin/env python3
"""Check that the crate graph in architecture.md matches the Cargo manifests.

Cargo is the source of truth: `cargo metadata` gives the normal dependencies between
workspace packages. The document shows them twice, inside marker comments:

- the "Rust core crates" Mermaid diagram (`crate-dependencies: core`) holds the edges
  that start at a core crate;
- the "Which core crates each application may use" table (`crate-dependencies: applications`)
  holds the edges that start at an application package.

Both directions must match exactly. The script only checks; it never rewrites the document.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOC = Path("docs") / "architecture" / "architecture.md"
APPLICATIONS = ("tracker-tui", "tracker-cli", "tracker-bridge", "tracker-server-app")
PREFIX = "tracker-"

NODE = re.compile(r'^\s*(?P<alias>\w+)\["(?P<label>[^"]*)"\](?::::\w+)?\s*$')
EDGE = re.compile(r"^\s*(?P<sources>\w+(?:\s*&\s*\w+)*)\s*-->\s*(?P<targets>\w+(?:\s*&\s*\w+)*)\s*$")
ARROW = re.compile(r"--|==|-\.")
ROW = re.compile(r"^\|\s*`(?P<crate>[^`]+)`\s*\|\s*(?P<uses>[^|]*?)\s*\|\s*$")

Edge = tuple[str, str]


def section(text: str, name: str) -> str | None:
    """Return the text between `<!-- crate-dependencies: name -->` and its closing marker."""
    pattern = rf"<!--\s*crate-dependencies:\s*{name}\s*-->(.*?)<!--\s*/crate-dependencies:\s*{name}\s*-->"
    found = re.search(pattern, text, re.DOTALL)
    return found.group(1) if found else None


def parse_core(block: str) -> tuple[set[str], set[Edge], list[str]]:
    """Return the crate names, the edges and the problems of the core Mermaid block."""
    aliases: dict[str, str] = {}
    pairs: list[tuple[str, str]] = []
    unreadable: list[str] = []
    for line in block.splitlines():
        node = NODE.match(line)
        if node:
            aliases[node["alias"]] = node["label"].split("<br>")[0].strip()
            continue
        edge = EDGE.match(line)
        if edge:
            for source in re.split(r"\s*&\s*", edge["sources"]):
                for target in re.split(r"\s*&\s*", edge["targets"]):
                    pairs.append((source, target))
        elif ARROW.search(line) and line.strip() != "---":
            # A labelled, dotted or chained arrow would otherwise drop out of the comparison.
            unreadable.append(f"The core block has an edge the check can't read: `{line.strip()}`. Write it as `a --> b & c`.")
    problems: list[str] = list(unreadable)
    edges: set[Edge] = set()
    for source, target in pairs:
        unknown = [alias for alias in (source, target) if alias not in aliases]
        for alias in unknown:
            problems.append(f"The core block uses the alias `{alias}`, which has no node line. Define it or remove the edge.")
        if not unknown:
            edges.add((aliases[source], aliases[target]))
    return set(aliases.values()), edges, sorted(set(problems))


def parse_applications(block: str) -> tuple[set[str], set[Edge]]:
    """Return the application names and the edges of the applications table."""
    crates: set[str] = set()
    edges: set[Edge] = set()
    for line in block.splitlines():
        row = ROW.match(line)
        if not row:
            continue
        crates.add(row["crate"])
        # An application without core crates has an empty or `-` cell.
        if set(row["uses"]) <= set("- "):
            continue
        for short in filter(None, (part.strip() for part in row["uses"].split(","))):
            edges.add((row["crate"], PREFIX + short))
    return crates, edges


def cargo_graph(metadata: dict) -> tuple[set[str], set[Edge]]:
    """Return the workspace packages and the normal dependencies between them."""
    names = {package["name"] for package in metadata["packages"]}
    edges = {
        (package["name"], dependency["name"])
        for package in metadata["packages"]
        for dependency in package["dependencies"]
        if dependency.get("kind") is None and dependency["name"] in names
    }
    return names, edges


def short(name: str) -> str:
    return name.removeprefix(PREFIX)


def describe(edge: Edge, adding: bool) -> str:
    source, target = edge
    if source in APPLICATIONS:
        where = f"the `{source}` row of the applications table"
    else:
        where = f"the `{short(source)} -->` line of the core block"
    if adding:
        return f"Cargo has {source} -> {target}. Add `{short(target)}` to {where}."
    return f"The doc has {source} -> {target}, which Cargo does not declare. Remove `{short(target)}` from {where}."


def check(text: str, metadata: dict) -> list[str]:
    """Compare the document with Cargo and return one message per problem."""
    problems: list[str] = []
    core = section(text, "core")
    table = section(text, "applications")
    if core is None:
        problems.append("Missing `<!-- crate-dependencies: core -->` markers around the core crates diagram.")
    if table is None:
        problems.append("Missing `<!-- crate-dependencies: applications -->` markers around the applications table.")
    if core is None or table is None:
        return problems

    packages, cargo_edges = cargo_graph(metadata)
    core_names, core_edges, core_problems = parse_core(core)
    table_names, table_edges = parse_applications(table)
    problems += core_problems

    for name in sorted((core_names | table_names) - packages):
        problems.append(f"`{name}` is in the doc but is not a workspace package. Remove it or fix the name.")
    for name in sorted(packages - core_names - table_names):
        problems.append(f"Workspace package `{name}` is not in the doc. Add a node to the core block or a row to the table.")
    for edge in sorted(core_edges):
        if edge[0] in APPLICATIONS:
            problems.append(f"The core block has an edge from the application `{edge[0]}`. Move it to the applications table.")
    for name in sorted(table_names - set(APPLICATIONS)):
        problems.append(f"The applications table has a row for `{name}`, which is not an application. Move it to the core block.")

    documented = core_edges | table_edges
    problems += [describe(edge, True) for edge in sorted(cargo_edges - documented)]
    problems += [describe(edge, False) for edge in sorted(documented - cargo_edges)]
    return problems


class CargoError(Exception):
    """`cargo metadata` failed, for example because Cargo.lock is out of date."""


def load_metadata(path: Path | None) -> dict:
    if path is not None:
        return json.loads(path.read_text(encoding="utf-8"))
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise CargoError(result.stderr.strip())
    return json.loads(result.stdout)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--doc", type=Path, default=ROOT / DOC, help="architecture document to check")
    parser.add_argument("--metadata", type=Path, help="JSON file with `cargo metadata` output, instead of running cargo")
    args = parser.parse_args(argv)

    try:
        metadata = load_metadata(args.metadata)
    except (CargoError, OSError) as error:
        print(f"Can't read the crate graph from cargo metadata: {error}", file=sys.stderr)
        return 1
    problems = check(args.doc.read_text(encoding="utf-8"), metadata)
    for problem in problems:
        print(problem, file=sys.stderr)
    if problems:
        print(f"{len(problems)} problem(s): {args.doc} does not match the Cargo manifests.", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
