"""Compile the session's optional-comparison ternaries after Muter instrumentation."""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SWIFT = shutil.which(os.environ.get("SWIFT_EXECUTABLE", "swift"))
MUTER = shutil.which(os.environ.get("SWIFT_MUTER_EXECUTABLE", str(ROOT / ".build/swift-tools/muter")))


@unittest.skipUnless(SWIFT and MUTER, "Requires Swift and the repository-pinned Muter")
class SwiftMuterTernaryTests(unittest.TestCase):
    def test_session_optional_comparison_branches_compile_and_keep_swap_mutations(self) -> None:
        session = ROOT / "apps/swiftui/TrackerClient/Sources/TrackerClient/App/TrackerSession.swift"
        expressions = re.findall(r"\(result\.worklog\.end == nil\s*\?[^{}]+?\)\s*else\s*\{", session.read_text())
        self.assertEqual(len(expressions), 2, "Expected the move and correction response checks")
        with tempfile.TemporaryDirectory(prefix="tracker-muter-ternary-") as directory:
            root = Path(directory)
            package = root / "ComparisonProbe"
            source = package / "Sources/ComparisonProbe/ComparisonProbe.swift"
            source.parent.mkdir(parents=True)
            (package / "Package.swift").write_text(
                '// swift-tools-version: 5.9\nimport PackageDescription\n'
                'let package = Package(name: "ComparisonProbe", targets: [.target(name: "ComparisonProbe")])\n'
            )
            contents = "struct Worklog: Equatable { let id: String; let end: String? }\n"
            for index, expression in enumerate(expressions):
                expression = re.sub(r"\s*else\s*\{$", "", expression)
                expression = expression.replace("result.snapshot.active", "active").replace("result.worklog", "worklog")
                contents += (f"func valid{index}(_ worklog: Worklog, _ active: Worklog?) -> Bool {{\n"
                             f"    return {expression}\n}}\n")
            source.write_text(contents)
            configuration = package / "muter.conf.yml"
            configuration.write_text(json.dumps({"executable": SWIFT, "arguments": ["build"],
                                                  "exclude": ["/Package.swift"]}))
            compiler = str(Path(SWIFT).with_name("swiftc"))
            self.run_command([compiler, "-typecheck", "-module-cache-path", str(root / "cache"), str(source)], package)
            self.run_command([MUTER, "mutate-without-running", "--configuration", str(configuration),
                              "--skip-coverage", "--skip-update-check"], package)
            generated = package.with_name("ComparisonProbe_mutated") / source.relative_to(package)
            self.run_command([compiler, "-typecheck", "-module-cache-path", str(root / "cache"), str(generated)], package)
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
            self.assertEqual(operators.count("RelationalOperatorReplacement"), 6)

    def run_command(self, arguments: list[str], package: Path) -> None:
        result = subprocess.run(arguments, cwd=package, capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
