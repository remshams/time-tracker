import importlib.util
import io
from pathlib import Path
import shutil
import tarfile
import tempfile
import unittest
import warnings
import zipfile
from unittest.mock import patch


SCRIPTS = Path(__file__).parents[1]
SPEC = importlib.util.spec_from_file_location("swift_style", SCRIPTS / "swift-style.py")
STYLE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(STYLE)
INSTALLER = STYLE.INSTALLER


def write_tar(path, members):
    with tarfile.open(path, "w:gz") as archive:
        for name, kind, value in members:
            member = tarfile.TarInfo(name)
            member.type = kind
            if kind in (tarfile.SYMTYPE, tarfile.LNKTYPE):
                member.linkname = value
                archive.addfile(member)
            else:
                member.size = len(value)
                archive.addfile(member, io.BytesIO(value))


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



class ArchiveExtractionTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.target = Path(temporary.name) / "target"
        self.outside = Path(temporary.name) / "outside"
        self.target.mkdir()
        self.outside.mkdir()
        (self.outside / "file").write_bytes(b"Outside")
        self.archive = Path(temporary.name) / "package.tar.gz"

    def unsafe_archives(self):
        return {
            "absolute path": [(str(self.outside / "escape"), tarfile.REGTYPE, b"Escape")],
            "parent component": [("package/../../outside/escape", tarfile.REGTYPE, b"Escape")],
            "symlink outside": [("package/link", tarfile.SYMTYPE, "../../outside")],
            "absolute symlink": [("package/link", tarfile.SYMTYPE, str(self.outside))],
            "hard link outside": [("package/link", tarfile.LNKTYPE, "../outside/file")],
            "symlink through an earlier symlink": [("here", tarfile.SYMTYPE, "."),
                                                   ("here/link", tarfile.SYMTYPE, "..")],
            "fifo": [("package/pipe", tarfile.FIFOTYPE, b"")],
            "write through a symlink that a later symlink redirects": [
                ("redirect", tarfile.SYMTYPE, "missing/child/../../outside"),
                ("missing", tarfile.SYMTYPE, "."),
                ("redirect/escape", tarfile.REGTYPE, b"Escape")],
        }

    def write_unsafe_archive(self, members):
        shutil.rmtree(self.target)
        self.target.mkdir()
        write_tar(self.archive, members)

    def assert_outside_untouched(self):
        self.assertEqual([path.name for path in self.outside.iterdir()], ["file"])
        self.assertEqual((self.outside / "file").read_bytes(), b"Outside")

    def extract_without_filter(self):
        # Python 3.12 and 3.13 warn when extract() runs without a filter.
        with patch.object(INSTALLER, "SUPPORTS_TAR_FILTER", False), warnings.catch_warnings():
            warnings.simplefilter("ignore", DeprecationWarning)
            INSTALLER.extract_tar(self.archive, self.target)

    def test_extraction_without_filter_keeps_files_and_links_inside_the_target(self):
        write_tar(self.archive, [("package/Sources/main.swift", tarfile.REGTYPE, b"Formatter"),
                                 ("package/link.swift", tarfile.SYMTYPE, "Sources/main.swift"),
                                 ("package/copy.swift", tarfile.LNKTYPE, "package/Sources/main.swift")])
        self.extract_without_filter()
        package = self.target / "package"
        self.assertEqual((package / "Sources/main.swift").read_bytes(), b"Formatter")
        self.assertEqual((package / "link.swift").read_bytes(), b"Formatter")
        self.assertEqual((package / "copy.swift").read_bytes(), b"Formatter")

    def test_extraction_without_filter_rejects_members_that_leave_the_target(self):
        for case, members in self.unsafe_archives().items():
            with self.subTest(case):
                self.write_unsafe_archive(members)
                with self.assertRaisesRegex(ValueError, "Archive (member|link)"):
                    self.extract_without_filter()
                self.assert_outside_untouched()

    def test_extraction_without_filter_rejects_unsafe_names_that_resolve_inside_the_target(self):
        cases = {"absolute path": str(self.target.resolve() / "package/main.swift"),
                 "parent component": "package/../package/main.swift"}
        for case, name in cases.items():
            with self.subTest(case):
                self.write_unsafe_archive([(name, tarfile.REGTYPE, b"Formatter")])
                with self.assertRaisesRegex(ValueError, "leaves the target directory"):
                    self.extract_without_filter()
                self.assertEqual(list(self.target.iterdir()), [])

    def test_extraction_with_filter_support_uses_the_data_filter(self):
        write_tar(self.archive, [("package/main.swift", tarfile.REGTYPE, b"Formatter")])
        with patch.object(INSTALLER, "SUPPORTS_TAR_FILTER", True), \
                patch.object(INSTALLER.tarfile.TarFile, "extractall") as extractall:
            INSTALLER.extract_tar(self.archive, self.target)
        extractall.assert_called_once_with(self.target, filter="data")

    @unittest.skipUnless(hasattr(tarfile, "data_filter"), "Requires a Python with tar extraction filters")
    def test_extraction_with_filter_keeps_members_inside_the_target(self):
        for case, members in self.unsafe_archives().items():
            with self.subTest(case):
                self.write_unsafe_archive(members)
                if case == "absolute path":
                    # The data filter strips the leading slash instead of rejecting the member.
                    INSTALLER.extract_tar(self.archive, self.target)
                    self.assertTrue((self.target / str(self.outside / "escape").lstrip("/")).exists())
                else:
                    # The filter rejects most members by name. For the redirected symlink, extraction
                    # fails with an OSError when it creates the parent directory instead.
                    with self.assertRaises((tarfile.TarError, OSError)):
                        INSTALLER.extract_tar(self.archive, self.target)
                self.assert_outside_untouched()

    def test_zip_extraction_writes_only_the_named_member_to_the_destination(self):
        archive = self.target.parent / "tool.zip"
        with zipfile.ZipFile(archive, "w") as source:
            source.writestr("../escape", b"Escape")
            source.writestr("swiftlint", b"Linter")
        destination = self.target / "swiftlint"
        INSTALLER.extract_zip_member(archive, "swiftlint", destination)
        self.assertEqual(destination.read_bytes(), b"Linter")
        self.assertEqual([path.name for path in self.target.iterdir()], ["swiftlint"])
        self.assertFalse((self.target.parent / "escape").exists())


if __name__ == "__main__":
    unittest.main()
