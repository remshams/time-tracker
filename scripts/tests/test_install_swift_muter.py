import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    "install_swift_muter", Path(__file__).parents[1] / "install-swift-muter.py"
)
INSTALLER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INSTALLER)


class MuterInstallerTests(unittest.TestCase):
    @unittest.skipUnless(shutil.which("swift") and (INSTALLER.OUTPUT / "muter").exists(),
                         "Requires Swift and the installed Muter tool")
    def test_ternary_comparison_mutants_compile_and_are_killed(self):
        with tempfile.TemporaryDirectory() as directory:
            package = Path(directory) / "Probe"
            sources = package / "Sources/Probe"
            tests = package / "Tests/ProbeTests"
            sources.mkdir(parents=True)
            tests.mkdir(parents=True)
            (package / "Package.swift").write_text(
                '// swift-tools-version: 5.9\nimport PackageDescription\n'
                'let package = Package(name: "Probe", targets: [.target(name: "Probe"), '
                '.testTarget(name: "ProbeTests", dependencies: ["Probe"])])\n', encoding="utf-8")
            (sources / "Probe.swift").write_text(
                'func choose(_ condition: Bool, _ active: String?, _ expected: String) -> Bool {\n'
                '    condition ? active == expected : active != expected\n}\n', encoding="utf-8")
            (tests / "ProbeTests.swift").write_text(
                'import XCTest\n@testable import Probe\nfinal class ProbeTests: XCTestCase {\n'
                '    func testBothComparisonBranches() {\n'
                '        XCTAssertTrue(choose(true, "task", "task"))\n'
                '        XCTAssertFalse(choose(true, "other", "task"))\n'
                '        XCTAssertFalse(choose(false, "task", "task"))\n'
                '        XCTAssertTrue(choose(false, "other", "task"))\n    }\n}\n', encoding="utf-8")
            configuration = package / "muter.conf.yml"
            configuration.write_text(json.dumps({"executable": shutil.which("swift"),
                                                 "arguments": ["test", "-j", "2"],
                                                 "exclude": ["/Tests/", "/Package.swift"]}), encoding="utf-8")
            report = Path(directory) / "report.json"
            result = subprocess.run([str(INSTALLER.OUTPUT / "muter"), "--configuration", str(configuration),
                                     "--skip-update-check", "--skip-coverage", "--format", "json",
                                     "--output", str(report)], cwd=package, capture_output=True, text=True, timeout=120)
            self.assertTrue(report.exists(), result.stdout + result.stderr)
            contents = json.loads(report.read_text(encoding="utf-8"))
            self.assertGreater(contents["totalAppliedMutationOperators"], 0)
            self.assertEqual(contents["numberOfKilledMutants"], contents["totalAppliedMutationOperators"])
            self.assertTrue(any(entry["mutationPoint"]["mutationOperatorId"] == "SwapTernary"
                                for file in contents["fileReports"] for entry in file["appliedOperators"]))

    def test_download_checksum_rejects_changed_source_before_extraction(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "source.tar.gz"
            archive.write_bytes(b"Changed source")
            with self.assertRaisesRegex(ValueError, "checksum"):
                INSTALLER.unpack(archive, Path(directory))

    def test_patches_are_guarded_for_linux_and_match_reparsed_code_by_position_and_text(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory)
            mapping = source / "Sources/muterCore/MutationSchemata/SchemataMutationMapping.swift"
            mapping.parent.mkdir(parents=True)
            mapping.write_text("        mappings[codeBlockSyntax]\n" + "map.fileName\n" * 3, encoding="utf-8")
            rewriter = source / "Sources/muterCore/Rewriters/MuterRewriter.swift"
            rewriter.parent.mkdir()
            rewriter.write_text(
                "        guard let mutationSchemata = schemataMappings.schemata(node) else {\n"
                "            return super.visit(node)\n"
                "            with: node\n"
                "        return super.visit(newNode)\n", encoding="utf-8"
            )
            effects = source / "Sources/muterCore/MutationOperators/RemoveSideEffectsOperator.swift"
            effects.parent.mkdir()
            effects.write_text('            "NSRecursiveLock",\n', encoding="utf-8")
            ternary = effects.with_name("SwapTernaryOperator.swift")
            ternary.write_text("            let secondChoice = children[index + 1]\n"
                               "            children[index + 1] = firstChoice\n", encoding="utf-8")
            INSTALLER.patch_source(source)
            patched = mapping.read_text(encoding="utf-8")
            self.assertIn("$0.key.position == codeBlockSyntax.position", patched)
            self.assertIn("$0.key.description == codeBlockSyntax.description", patched)
            self.assertNotIn("map.fileName", patched)
            self.assertIn("map.filePath", patched)
            self.assertIn("with: rewritten", rewriter.read_text(encoding="utf-8"))
            self.assertIn('"NSLock"', effects.read_text(encoding="utf-8"))
            self.assertIn("Array(children[(index + 1)...])", ternary.read_text(encoding="utf-8"))
            self.assertIn("replaceSubrange", ternary.read_text(encoding="utf-8"))
            shim = (source / "Sources/muterCore/LinuxAutoreleasepool.swift").read_text(encoding="utf-8")
            self.assertTrue(shim.startswith("#if os(Linux)\n"))

    def test_syntax_patch_rejects_unexpected_upstream_source(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory)
            mapping = source / "Sources/muterCore/MutationSchemata/SchemataMutationMapping.swift"
            mapping.parent.mkdir(parents=True)
            mapping.write_text("Changed upstream source", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "no longer matches"):
                INSTALLER.patch_source(source)


if __name__ == "__main__":
    unittest.main()
