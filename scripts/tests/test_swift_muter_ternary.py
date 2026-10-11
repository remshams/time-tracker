"""Compile optional comparisons and nested branches after Muter instrumentation."""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import tempfile
import unittest

from scripts.swift_muter_tools import run_tool


ROOT = Path(__file__).resolve().parents[2]
SWIFT = shutil.which(os.environ.get("SWIFT_EXECUTABLE", "swift"))
MUTER = shutil.which(os.environ.get("SWIFT_MUTER_EXECUTABLE", str(ROOT / ".build/swift-tools/muter")))


@unittest.skipUnless(SWIFT and MUTER, "Requires Swift and the repository-pinned Muter")
class SwiftMuterTernaryTests(unittest.TestCase):
    def test_optional_comparison_and_nested_branches_compile_and_keep_swap_mutations(self) -> None:
        with tempfile.TemporaryDirectory(prefix="tracker-muter-ternary-") as directory:
            root = Path(directory)
            package = root / "ComparisonProbe"
            source = package / "Sources/ComparisonProbe/ComparisonProbe.swift"
            source.parent.mkdir(parents=True)
            (package / "Package.swift").write_text(
                '// swift-tools-version: 5.9\nimport PackageDescription\n'
                'let package = Package(name: "ComparisonProbe", targets: [.target(name: "ComparisonProbe")])\n'
            )
            source.write_text(
                'struct Worklog: Equatable { let id: String; let end: String? }\n'
                'func valid(_ worklog: Worklog, _ active: Worklog?) -> Bool {\n'
                '    return worklog.end == nil ? active == worklog : active?.id != worklog.id\n'
                '}\n'
                'func nested(_ worklog: Worklog, _ active: Worklog?, _ enabled: Bool) -> Bool {\n'
                '    if enabled == true {\n'
                '        return worklog.end == nil ? active == worklog : active?.id != worklog.id\n'
                '    }\n'
                '    return false\n'
                '}\n', encoding="utf-8")
            configuration = package / "muter.conf.yml"
            configuration.write_text(json.dumps({"executable": SWIFT, "arguments": ["build"],
                                                  "exclude": ["/Package.swift"]}))
            compiler = str(Path(SWIFT).with_name("swiftc"))
            checks = root / "main.swift"
            checks.write_text(
                'let open = Worklog(id: "task", end: nil)\n'
                'let closed = Worklog(id: "task", end: "finished")\n'
                'let other = Worklog(id: "other", end: nil)\n'
                'let cases: [(Worklog, Worklog?, Bool)] = [\n'
                '    (open, open, true), (open, other, false), (open, nil, false),\n'
                '    (closed, open, false), (closed, other, true), (closed, nil, true)\n'
                ']\n'
                'for (worklog, active, expected) in cases {\n'
                '    assert(valid(worklog, active) == expected)\n'
                '    assert(nested(worklog, active, true) == expected)\n'
                '    assert(nested(worklog, active, false) == false)\n'
                '}\n', encoding="utf-8")
            binary = root / "comparison-checks"

            def check_semantics(path: Path) -> None:
                self.run_command([compiler, "-module-cache-path", str(root / "cache"), str(path),
                                  str(checks), "-o", str(binary)], package)
                self.run_command([str(binary)], package)

            check_semantics(source)
            self.run_command([MUTER, "mutate-without-running", "--configuration", str(configuration),
                              "--skip-coverage", "--skip-update-check"], package)
            generated = package.with_name("ComparisonProbe_mutated") / source.relative_to(package)
            check_semantics(generated)
            swaps = re.findall(r"worklog\.end\s*==\s*nil\s*\?\s*\(?active\?\.id\s*!=\s*worklog\.id\)?\s*:\s*\(?active\s*==\s*worklog\)?",
                               generated.read_text())
            self.assertEqual(len(swaps), 2, "Muter must insert both complete branch swaps")
            mappings = json.loads((package / "muter-mappings.json").read_text())
            operators: list[str] = []

            def collect(value: object) -> None:
                if isinstance(value, dict):
                    for key, entry in value.items():
                        if key == "mutationOperatorId":
                            operators.append(entry)
                        else:
                            collect(entry)
                elif isinstance(value, list):
                    for entry in value:
                        collect(entry)

            collect(mappings)
            self.assertEqual(operators.count("SwapTernary"), 2)
            self.assertEqual(operators.count("RelationalOperatorReplacement"), 7)
            self.assertRegex(generated.read_text(), r"enabled\s*!=\s*true",
                             "The enclosing conditional must retain its mutation")

    def run_command(self, arguments: list[str], package: Path) -> None:
        result = run_tool(arguments, cwd=package, timeout=60)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
