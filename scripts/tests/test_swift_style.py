import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SCRIPTS = Path(__file__).parents[1]
SPEC = importlib.util.spec_from_file_location("swift_style", SCRIPTS / "swift-style.py")
STYLE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(STYLE)
INSTALLER = STYLE.INSTALLER


class SwiftStyleTests(unittest.TestCase):
    def test_download_rejects_archive_with_unexpected_checksum(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "tool.zip"
            with patch.object(INSTALLER.urllib.request, "urlopen", return_value=io.BytesIO(b"Changed tool")):
                with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
                    INSTALLER.download("https://example.test/tool.zip", archive, "0" * 64)

    def test_wrong_tool_version_does_not_replace_installed_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            installed = output / "swift-format"
            installed.write_bytes(b"Existing formatter")
            candidate = output / "candidate"
            candidate.write_bytes(b"Different formatter")
            with patch.object(INSTALLER, "OUTPUT", output), \
                    patch.object(INSTALLER.subprocess, "check_output", return_value="Different version\n"):
                with self.assertRaisesRegex(ValueError, "Expected swift-format"):
                    INSTALLER.install_binary(candidate, "swift-format", INSTALLER.FORMAT_VERSION)
            self.assertEqual(installed.read_bytes(), b"Existing formatter")
            self.assertFalse((output / "swift-format.new").exists())

    def test_checks_reject_stale_cached_tool(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            (output / "swift-format").touch()
            with patch.object(INSTALLER, "OUTPUT", output), \
                    patch.object(STYLE.subprocess, "check_output", return_value="Older version\n"):
                with self.assertRaisesRegex(ValueError, "reinstall Swift style tools"):
                    STYLE.tool("swift-format", INSTALLER.FORMAT_VERSION)

    def test_source_discovery_keeps_generated_and_native_e2e_files_out_of_scope(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = [STYLE.MANIFEST, "apps/swiftui/Sources/TimeTrackerSwiftUI/App.swift",
                     "apps/swiftui/TrackerClient/Sources/TrackerClient/State.swift",
                     "apps/swiftui/TrackerClient/Tests/TrackerClientTests/StateTests.swift",
                     "apps/swiftui/TrackerClient/.build/Generated.swift",
                     "apps/swiftui/Tests/UITests/SmokeUITests.swift"]
            for path in paths:
                file = root / path
                file.parent.mkdir(parents=True, exist_ok=True)
                file.touch()
            self.assertEqual({str(path.relative_to(root)) for path in STYLE.source_files(root)}, set(paths[:4]))


if __name__ == "__main__":
    unittest.main()
