import importlib.util
from pathlib import Path
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    "install_swift_muter", Path(__file__).parents[1] / "install-swift-muter.py"
)
INSTALLER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INSTALLER)


class MuterInstallerTests(unittest.TestCase):
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
            INSTALLER.patch_source(source)
            patched = mapping.read_text(encoding="utf-8")
            self.assertIn("$0.key.position == codeBlockSyntax.position", patched)
            self.assertIn("$0.key.description == codeBlockSyntax.description", patched)
            self.assertNotIn("map.fileName", patched)
            self.assertIn("map.filePath", patched)
            self.assertIn("with: rewritten", rewriter.read_text(encoding="utf-8"))
            self.assertIn('"NSLock"', effects.read_text(encoding="utf-8"))
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
