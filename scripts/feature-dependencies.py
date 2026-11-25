#!/usr/bin/env python3
"""Check the dependencies between feature folders against a checked-in config.

Each app lists, per folder (a node), the other folders it uses. The check reads the
production sources, collects the edges that actually exist, and fails when the code uses a
folder the config doesn't list, when the config lists a folder the code no longer uses, and
when the folders form a cycle. Folders named in `implicit` may be used by every node without
being listed, but their edges still count for the cycle check. See ADR 0045.
"""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EXAMPLES = 3


def display(path: Path) -> str:
    """The path relative to the repository, for messages."""
    return str(path.relative_to(ROOT)) if path.is_relative_to(ROOT) else str(path)


@dataclass(frozen=True, order=True)
class Location:
    path: Path
    line: int

    def __str__(self) -> str:
        return f"{display(self.path)}:{self.line}"


@dataclass
class Target:
    """The nodes of one compilation target and the edges found between them."""

    name: str | None
    nodes: set[str] = field(default_factory=set)
    edges: dict[tuple[str, str], list[Location]] = field(default_factory=lambda: defaultdict(list))
    problems: list[str] = field(default_factory=list)

    def add(self, source: str, target: str, location: Location) -> None:
        if source != target:
            self.edges[(source, target)].append(location)


@dataclass(frozen=True)
class Config:
    implicit: tuple[str, ...]
    uses: dict[str, tuple[str, ...]]


def node_of(relative: Path) -> str:
    """The node of a path under a target's source folder (ADR 0034).

    Each folder under Features or Infrastructure is a node, every other top-level folder is
    one node, and a file at the top, such as main.cpp, belongs to App.
    """
    parts = relative.parts
    if len(parts) == 1:
        return "App"
    if parts[0] in {"Features", "Infrastructure"} and len(parts) > 2:
        return f"{parts[0]}/{parts[1]}"
    return parts[0]


# Source text without comments and string contents, so names inside them don't count.


def strip_comments_and_strings(text: str) -> str:
    """Blank out comments and the contents of string literals, keeping lines and columns."""
    result = list(text)
    index = 0
    length = len(text)

    def blank(start: int, end: int) -> None:
        for position in range(start, end):
            if result[position] != "\n":
                result[position] = " "

    while index < length:
        pair = text[index : index + 2]
        if pair == "//":
            end = text.find("\n", index)
            end = length if end == -1 else end
            blank(index, end)
            index = end
        elif pair == "/*":
            end = text.find("*/", index + 2)
            end = length if end == -1 else end + 2
            blank(index, end)
            index = end
        elif text[index] in "\"'`":
            quote = text[index]
            end = index + 1
            while end < length and text[end] != quote and (quote == "`" or text[end] != "\n"):
                end += 2 if text[end] == "\\" else 1
            blank(index + 1, min(end, length))
            index = end + 1
        else:
            index += 1
    return "".join(result)


# The Omarchy app: C++ includes, QML imports and QML use of context properties.

OMARCHY_CONFIG = ROOT / "apps" / "omarchy" / "feature-dependencies.toml"
OMARCHY_SOURCE = ROOT / "apps" / "omarchy" / "src"
INCLUDE = re.compile(r'^[ \t]*#\s*include\s+"(?P<path>[^"]+)"', re.MULTILINE)
QML_IMPORT = re.compile(r'^[ \t]*import\s+"(?P<path>[^"]+)"', re.MULTILINE)
CONTEXT_PROPERTY = re.compile(r'setContextProperty\(\s*QStringLiteral\("(?P<name>\w+)"\)\s*,\s*(?P<member>\w+)\s*\)')
CLASS = re.compile(r"^\s*class\s+(?P<name>\w+)\s*(?::|\{)", re.MULTILINE)


def line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def omarchy_context_properties(source: Path, target: Target) -> dict[str, str]:
    """Map each QML context property to the node that defines its type.

    AppController.cpp names the member behind each property, AppController.h declares the
    member's type, and the header that defines that class sits in the node.
    """
    controller = source / "App" / "AppController.cpp"
    header = source / "App" / "AppController.h"
    if not controller.exists() or not header.exists():
        return {}
    classes: dict[str, str] = {}
    for path in sorted(source.rglob("*.h")):
        for match in CLASS.finditer(strip_comments_and_strings(path.read_text())):
            classes[match.group("name")] = node_of(path.relative_to(source))
    header_text = strip_comments_and_strings(header.read_text())
    controller_text = controller.read_text()
    properties: dict[str, str] = {}
    controller_code = strip_comments_and_strings(controller_text)
    # The property name is a string, so the calls are read from the raw text; a call that is
    # commented out is blank in the stripped text.
    calls = [match for match in CONTEXT_PROPERTY.finditer(controller_text) if controller_code[match.start()] != " "]
    matched = {match.start() for match in calls}
    for call in re.finditer(r"setContextProperty\(", controller_code):
        if call.start() not in matched:
            location = Location(controller, line_of(controller_text, call.start()))
            target.problems.append(
                f'{location}: can\'t read this context property. Write it as setContextProperty(QStringLiteral("name"), member_).'
            )
    for match in calls:
        name, member = match.group("name"), match.group("member")
        declaration = re.search(rf"(?P<type>\w+)\s*\*\s*{re.escape(member)}\s*;", header_text)
        node = classes.get(declaration.group("type")) if declaration else None
        if node is None:
            location = Location(controller, line_of(controller_text, match.start()))
            target.problems.append(
                f"{location}: can't find the folder of context property {name!r}. Declare {member} with a"
                " type defined in a header under src."
            )
            continue
        properties[name] = node
    return properties


def omarchy_resolve(source: Path, path: Path, relative: str) -> str | None:
    """The node an include or import points to, or None for a file outside src, such as a Qt header.

    QML imports are relative to the file. C++ includes are relative to the file's folder, then to src.
    """
    root = source.resolve()
    bases = [path.parent] if path.suffix == ".qml" else [path.parent, source]
    for base in bases:
        resolved = (base / relative).resolve()
        if resolved.exists() and resolved.is_relative_to(root):
            inside = resolved.relative_to(root)
            # An imported folder, such as "../../Shared", belongs to its own node.
            return node_of(inside / "qmldir" if resolved.is_dir() else inside)
    return None


def context_property_uses(name: str, code: str) -> list[re.Match[str]]:
    """The uses of a context property in QML code, such as `catalog.tasks` or `target: catalog`.

    A member access (`model.catalog`), a property declaration (`property var catalog`) and a
    binding to a property of that name (`catalog: value` at the start of a line) aren't uses.
    A line that continues an expression, such as the branch after `ready ?`, is never a binding.
    """
    uses = []
    for match in re.finditer(rf"(?<![\w.]){re.escape(name)}(?!\w)", code):
        line_start = code.rfind("\n", 0, match.start()) + 1
        before = code[line_start : match.start()]
        after = code[match.end() :]
        if re.search(r"\bproperty\s+[\w<>.]+\s+$", before):
            continue
        continues = code[:line_start].rstrip()[-1:] in tuple("?:(,[=+-*/%&|!<>")
        if not continues and re.fullmatch(r"\s*(?:readonly\s+|required\s+)*", before) and re.match(r"[ \t]*:", after):
            continue
        uses.append(match)
    return uses


def extract_omarchy(source: Path) -> list[Target]:
    target = Target(None)
    files = sorted(path for path in source.rglob("*") if path.suffix in {".cpp", ".h", ".qml"})
    for path in files:
        target.nodes.add(node_of(path.relative_to(source)))
    properties = omarchy_context_properties(source, target)
    for path in files:
        node = node_of(path.relative_to(source))
        text = path.read_text()
        code = strip_comments_and_strings(text)
        pattern = QML_IMPORT if path.suffix == ".qml" else INCLUDE
        for match in pattern.finditer(text):
            # A commented-out include or import is blanked in the stripped text.
            if not code[match.start() : match.end()].strip():
                continue
            used = omarchy_resolve(source, path, match.group("path"))
            if used is not None:
                target.add(node, used, Location(path, line_of(text, match.start())))
        if path.suffix == ".qml":
            for name, used in properties.items():
                for match in context_property_uses(name, code):
                    target.add(node, used, Location(path, line_of(text, match.start())))
    return [target]


# The Swift app: two targets, with edges from uses of top-level declarations in other folders.

SWIFT_SOURCE = ROOT / "apps" / "swiftui"
SWIFT_CONFIG = ROOT / "apps" / "swiftui" / "feature-dependencies.toml"
SWIFT_TARGETS = {
    "TimeTrackerSwiftUI": Path("Sources") / "TimeTrackerSwiftUI",
    "TrackerClient": Path("TrackerClient") / "Sources" / "TrackerClient",
}
SWIFT_MODIFIERS = (
    "public|internal|open|final|indirect|nonisolated|distributed|package|static|lazy|dynamic|override"
    "|required|convenience|mutating|nonmutating|weak|unowned"
)
SWIFT_DECLARATION = re.compile(
    r"^[ \t]*(?P<modifiers>(?:(?:@\w+(?:\([^)\n]*\))?|(?:private|fileprivate|" + SWIFT_MODIFIERS + r")(?:\(\w+\))?)\s+)*)"
    r"(?:class|struct|enum|actor|protocol|typealias|func|let|var)\s+(?P<name>[A-Za-z_]\w*)",
    re.MULTILINE,
)
SWIFT_PRIVATE = re.compile(r"\b(?:private|fileprivate)\b(?!\()")
SWIFT_WORD = re.compile(r"\b[A-Za-z_]\w*\b")
SWIFT_STRING_START = re.compile(r'(#*)(""")|(#*)"')


def strip_swift(text: str) -> str:
    """Blank out comments and string contents, keeping lines and columns.

    Block comments nest. Code inside a string interpolation, `\\( ... )`, stays as code.
    Backticks and single quotes are ordinary characters in Swift.
    """
    result = list(text)
    length = len(text)

    def blank(start: int, end: int) -> None:
        for position in range(start, min(end, length)):
            if result[position] != "\n":
                result[position] = " "

    def code(index: int, in_interpolation: bool) -> int:
        """Scan code from index; inside an interpolation, stop after the closing parenthesis."""
        depth = 1 if in_interpolation else 0
        while index < length:
            pair = text[index : index + 2]
            if pair == "//":
                end = text.find("\n", index)
                end = length if end == -1 else end
                blank(index, end)
                index = end
                continue
            if pair == "/*":
                start, nesting = index, 1
                index += 2
                while index < length and nesting:
                    inner = text[index : index + 2]
                    if inner == "/*":
                        nesting += 1
                        index += 2
                    elif inner == "*/":
                        nesting -= 1
                        index += 2
                    else:
                        index += 1
                blank(start, index)
                continue
            char = text[index]
            if char in '#"':
                match = SWIFT_STRING_START.match(text, index)
                if match:
                    hashes = len(match.group(1) if match.group(2) else match.group(3))
                    index = string(match.end(), hashes, match.group(2) is not None)
                    continue
            elif char == "(":
                depth += 1
            elif char == ")" and in_interpolation:
                depth -= 1
                if depth == 0:
                    return index + 1
            index += 1
        return index

    def string(index: int, hashes: int, multiline: bool) -> int:
        """Scan a string body from index and return the index after its closing quote."""
        closing = ('"""' if multiline else '"') + "#" * hashes
        escape = "\\" + "#" * hashes
        start = index
        while index < length:
            if text.startswith(closing, index):
                blank(start, index)
                return index + len(closing)
            if text.startswith(escape, index):
                after = index + len(escape)
                if text.startswith("(", after):
                    blank(start, index)
                    index = code(after + 1, True)
                    start = index
                else:
                    index = after + 1
                continue
            if text[index] == "\n" and not multiline:
                break
            index += 1
        blank(start, index)
        return index

    code(0, False)
    return "".join(result)


def swift_top_level_names(code: str) -> list[tuple[str, int]]:
    """Names and offsets of the non-private declarations at brace depth 0."""
    depths = []
    depth = 0
    for char in code:
        depths.append(depth)
        if char == "{":
            depth += 1
        elif char == "}":
            depth = max(depth - 1, 0)
    names = []
    for match in SWIFT_DECLARATION.finditer(code):
        keyword_start = match.start("name")
        if depths[keyword_start] != 0 or SWIFT_PRIVATE.search(match.group("modifiers")):
            continue
        names.append((match.group("name"), keyword_start))
    return names


def extract_swift_target(name: str, root: Path) -> Target:
    target = Target(name)
    files = sorted(root.rglob("*.swift"))
    declared: dict[str, dict[str, Location]] = defaultdict(dict)
    stripped = {}
    for path in files:
        node = node_of(path.relative_to(root))
        target.nodes.add(node)
        text = path.read_text()
        code = strip_swift(text)
        stripped[path] = (node, text, code)
        for declared_name, offset in swift_top_level_names(code):
            declared[declared_name].setdefault(node, Location(path, line_of(text, offset)))
    owners = {}
    for declared_name, nodes in sorted(declared.items()):
        if len(nodes) > 1:
            places = ", ".join(f"{node} ({location})" for node, location in sorted(nodes.items()))
            target.problems.append(f"{name} declares {declared_name} in more than one folder: {places}. Rename one.")
        else:
            owners[declared_name] = next(iter(nodes))
    for path, (node, text, code) in stripped.items():
        for match in SWIFT_WORD.finditer(code):
            used = owners.get(match.group())
            if used is None or used == node:
                continue
            # A lowercase name after a dot is a member, not the top-level function or constant.
            if match.group()[0].islower() and code[max(match.start() - 1, 0)] == ".":
                continue
            target.add(node, used, Location(path, line_of(text, match.start())))
    return target


def extract_swift(app_root: Path) -> list[Target]:
    return [
        extract_swift_target(name, app_root / relative)
        for name, relative in SWIFT_TARGETS.items()
        if (app_root / relative).is_dir()
    ]


# Config loading, comparison, cycles and the report.


def parse_config(text: str) -> dict[str | None, Config]:
    data = tomllib.loads(text)
    if "targets" in data:
        return {name: config_section(section) for name, section in data["targets"].items()}
    return {None: config_section(data)}


def config_section(section: dict) -> Config:
    nodes = section.get("nodes", {})
    return Config(
        implicit=tuple(section.get("implicit", [])),
        uses={name: tuple(node.get("uses", [])) for name, node in nodes.items()},
    )


def section_name(target: str | None, node: str) -> str:
    prefix = f"targets.{target}." if target else ""
    return f'[{prefix}nodes."{node}"]'


def with_examples(message: str, locations: list[Location]) -> str:
    lines = [message]
    lines.extend(f"    {location}" for location in sorted(set(locations))[:EXAMPLES])
    return "\n".join(lines)


def cycles(edges: set[tuple[str, str]]) -> list[list[str]]:
    """One cycle per strongly connected component with more than one node, as a closed path."""
    successors: dict[str, list[str]] = defaultdict(list)
    for source, target in sorted(edges):
        successors[source].append(target)
    index: dict[str, int] = {}
    low: dict[str, int] = {}
    stack: list[str] = []
    on_stack: set[str] = set()
    components: list[set[str]] = []

    def visit(node: str) -> None:
        index[node] = low[node] = len(index)
        stack.append(node)
        on_stack.add(node)
        for successor in successors[node]:
            if successor not in index:
                visit(successor)
                low[node] = min(low[node], low[successor])
            elif successor in on_stack:
                low[node] = min(low[node], index[successor])
        if low[node] == index[node]:
            component = set()
            while True:
                member = stack.pop()
                on_stack.discard(member)
                component.add(member)
                if member == node:
                    break
            if len(component) > 1:
                components.append(component)

    for node in sorted({node for edge in edges for node in edge}):
        if node not in index:
            visit(node)

    found = []
    for component in components:
        start = min(component)
        # Breadth-first search for the shortest way back to the start inside the component.
        paths = {start: [start]}
        queue = [start]
        while queue:
            node = queue.pop(0)
            for successor in successors[node]:
                if successor == start:
                    found.append(paths[node] + [start])
                    queue = []
                    break
                if successor in component and successor not in paths:
                    paths[successor] = paths[node] + [successor]
                    queue.append(successor)
    return sorted(found)


def check(target: Target, config: Config, config_path: Path) -> list[str]:
    name = display(config_path)
    problems = list(target.problems)
    for node in sorted(set(config.implicit) - target.nodes):
        problems.append(f"{name} makes {node!r} implicit, but no folder has that name. Remove it from implicit.")
    for node in sorted(target.nodes - config.uses.keys()):
        uses = ", ".join(f'"{used}"' for source, used in sorted(target.edges) if source == node and used not in config.implicit)
        problems.append(
            f"{name} has no section for the folder {node}. Add one:\n"
            f"    {section_name(target.name, node)}\n    uses = [{uses}]"
        )
    for node in sorted(config.uses.keys() - target.nodes):
        problems.append(f"{name} has {section_name(target.name, node)}, but no folder has that name. Remove the section.")

    found = {edge for edge in target.edges if edge[1] not in config.implicit}
    for source, used in sorted(found):
        if source in config.uses and used not in config.uses[source]:
            problems.append(
                with_examples(
                    f"{source} uses {used}, which {name} doesn't list."
                    f' Add "{used}" to uses in {section_name(target.name, source)}.',
                    target.edges[(source, used)],
                )
            )
    for source, listed in sorted(config.uses.items()):
        for used in listed:
            section = section_name(target.name, source)
            if used in config.implicit:
                problems.append(f'{section} lists "{used}", which is implicit. Remove it.')
            elif (source, used) not in found and source in target.nodes:
                problems.append(f'{section} lists "{used}", but no file in {source} uses it. Remove it.')

    # Every other node may use an implicit node at any time, so those edges count for cycles too.
    implicit = set(config.implicit) & target.nodes
    graph = set(target.edges) | {(node, used) for node in target.nodes - implicit for used in implicit}
    for cycle in cycles(graph):
        edges = list(zip(cycle, cycle[1:]))
        lines = [f"The folders form a cycle: {' -> '.join(cycle)}. Remove one of these dependencies:"]
        for source, used in edges:
            where = min(target.edges[(source, used)]) if (source, used) in target.edges else f"{used} is implicit"
            lines.append(f"    {source} -> {used}: {where}")
        problems.append("\n".join(lines))
    return problems


def check_all(targets: list[Target], configs: dict[str | None, Config], config_path: Path) -> list[str]:
    problems = []
    name = display(config_path)
    names = {target.name for target in targets}
    for missing in sorted(configs.keys() - names, key=str):
        problems.append(f"{name} has a section for target {missing!r}, which doesn't exist. Remove it.")
    for target in targets:
        if target.name not in configs:
            label = f"[targets.{target.name}]" if target.name else "the top level"
            problems.append(f"{name} has no config for target {target.name!r}. Add it under {label}.")
            continue
        problems.extend(check(target, configs[target.name], config_path))
    return problems


STACKS = {
    "omarchy": (extract_omarchy, OMARCHY_SOURCE, OMARCHY_CONFIG),
    "swift": (extract_swift, SWIFT_SOURCE, SWIFT_CONFIG),
}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--stack", choices=sorted(STACKS), required=True, help="the app to check")
    parser.add_argument("--source", type=Path, help="the source folder (defaults to the stack's)")
    parser.add_argument("--config", type=Path, help="the config file (defaults to the stack's)")
    arguments = parser.parse_args(argv)
    extract, source, config_path = STACKS[arguments.stack]
    source = arguments.source or source
    config_path = arguments.config or config_path
    try:
        configs = parse_config(config_path.read_text())
    except (OSError, tomllib.TOMLDecodeError) as error:
        print(f"Can't read {config_path}: {error}", file=sys.stderr)
        return 1
    problems = check_all(extract(source), configs, config_path)
    for problem in problems:
        print(problem, file=sys.stderr)
    if problems:
        print(f"\n{len(problems)} feature dependency problem(s). See ADR 0045.", file=sys.stderr)
        return 1
    print(f"Feature dependencies match {display(config_path)}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
