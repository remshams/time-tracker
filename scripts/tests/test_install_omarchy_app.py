import importlib.util
import io
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("install_omarchy_app", Path(__file__).parents[1] / "install-omarchy-app.py")
INSTALL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INSTALL)


def completed(stdout="", returncode=0, stderr=""):
    return subprocess.CompletedProcess([], returncode, stdout=stdout, stderr=stderr)


class RevisionTests(unittest.TestCase):
    def test_clean_checkout_uses_the_short_commit(self):
        with patch.object(INSTALL, "output", side_effect=["a9b4a340", ""]):
            self.assertEqual(INSTALL.revision(), "a9b4a340")

    def test_uncommitted_or_untracked_files_mark_the_revision_dirty(self):
        with patch.object(INSTALL, "output", side_effect=["a9b4a340", "?? scripts/new.py"]) as output:
            self.assertEqual(INSTALL.revision(), "a9b4a340-dirty")
        self.assertEqual(output.call_args.args[0], ["git", "status", "--porcelain"])


class BuildTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.work = Path(directory.name)
        self.package = self.work / "tt-desktop-0.1.0-1-x86_64.pkg.tar.zst"
        self.package.write_text("package", encoding="utf-8")

    def build(self, packagelist=None, returncode=0):
        listing = str(self.package) if packagelist is None else packagelist
        with patch.object(INSTALL.subprocess, "run", return_value=completed(returncode=returncode)) as run, \
                patch.object(INSTALL, "output", return_value=listing) as output:
            try:
                return INSTALL.build("a9b4a340-dirty", self.work), run, output
            finally:
                self.run_call, self.output_call = run.call_args, output.call_args

    def test_builds_the_package_in_a_copy_of_the_package_files(self):
        built, _, _ = self.build()
        self.assertEqual(built, self.package)
        command = self.run_call.args[0]
        self.assertEqual(command, ["makepkg", "-fs", "--noconfirm", "--noprogressbar"])
        self.assertEqual(self.run_call.kwargs["cwd"], self.work)
        for name in ("PKGBUILD", "tt-desktop.desktop", "tt-desktop.svg"):
            self.assertTrue((self.work / name).is_file(), name)

    def test_passes_the_checkout_revision_and_cache_to_the_pkgbuild(self):
        self.build()
        environment = self.run_call.kwargs["env"]
        self.assertEqual(environment["TT_SOURCE_DIR"], str(INSTALL.ROOT))
        self.assertEqual(environment["TT_GIT_REVISION"], "a9b4a340-dirty")
        self.assertEqual(environment["CARGO_TARGET_DIR"], str(INSTALL.CARGO_TARGET_DIR))
        self.assertEqual(environment["PKGDEST"], str(self.work))
        self.assertEqual(self.output_call.args[0], ["makepkg", "--packagelist"])

    def test_the_package_files_are_the_ones_the_pkgbuild_installs(self):
        names = {path.name for path in INSTALL.PACKAGE_FILES.iterdir()}
        self.assertEqual(names, {"PKGBUILD", "tt-desktop.desktop", "tt-desktop.svg"})

    def test_failed_makepkg_stops_before_listing_packages(self):
        with patch.object(INSTALL.subprocess, "run", return_value=completed(returncode=4)), \
                patch.object(INSTALL, "output") as output, self.assertRaises(ValueError) as raised:
            INSTALL.build("a9b4a340", self.work)
        self.assertEqual(str(raised.exception), "makepkg failed with exit code 4")
        output.assert_not_called()

    def test_a_package_list_without_a_built_file_is_an_error(self):
        listing = str(self.work / "tt-desktop-9.9.9-1-x86_64.pkg.tar.zst")
        with self.assertRaises(ValueError) as raised:
            self.build(listing)
        self.assertIn("found 0", str(raised.exception))


class QuitTests(unittest.TestCase):
    def test_asks_the_process_to_quit_by_exact_name(self):
        calls = []

        def run(command, **keywords):
            calls.append(command)
            return completed(returncode=1 if command[0] == "pgrep" else 0)

        with patch.object(INSTALL.subprocess, "run", side_effect=run):
            INSTALL.quit_app()
        self.assertEqual(calls, [["pkill", "-x", "tt-desktop"], ["pgrep", "-x", "tt-desktop"]])

    def test_waits_until_the_process_is_gone(self):
        states = iter([0, 0, 1])

        def run(command, **keywords):
            return completed(returncode=next(states) if command[0] == "pgrep" else 1)

        with patch.object(INSTALL.subprocess, "run", side_effect=run), patch.object(INSTALL.time, "sleep") as sleep:
            INSTALL.quit_app()
        self.assertEqual(sleep.call_count, 2)

    def test_a_process_that_keeps_running_fails_after_the_timeout(self):
        clock = iter([0, 3, INSTALL.QUIT_TIMEOUT + 1])
        with patch.object(INSTALL.subprocess, "run", return_value=completed()), \
                patch.object(INSTALL.time, "monotonic", side_effect=lambda: next(clock)), \
                patch.object(INSTALL.time, "sleep"), self.assertRaises(ValueError) as raised:
            INSTALL.quit_app()
        self.assertEqual(str(raised.exception), "Time Tracker did not quit within 10 seconds; quit it and try again")


class InstallTests(unittest.TestCase):
    def test_installs_the_package_with_pacman_through_sudo(self):
        with patch.object(INSTALL.subprocess, "run", return_value=completed()) as run:
            INSTALL.install(Path("/work/tt-desktop.pkg.tar.zst"))
        self.assertEqual(run.call_args.args[0], ["sudo", "pacman", "-U", "--noconfirm", "/work/tt-desktop.pkg.tar.zst"])
        self.assertNotIn("capture_output", run.call_args.kwargs)

    def test_failed_pacman_is_an_error(self):
        with patch.object(INSTALL.subprocess, "run", return_value=completed(returncode=1)), \
                self.assertRaises(ValueError) as raised:
            INSTALL.install(Path("/work/tt-desktop.pkg.tar.zst"))
        self.assertEqual(str(raised.exception), "pacman failed with exit code 1")


class LaunchTests(unittest.TestCase):
    def test_starts_the_installed_binary_detached_from_the_terminal(self):
        with patch.object(INSTALL.subprocess, "Popen") as popen:
            INSTALL.launch()
        self.assertEqual(popen.call_args.args[0], ["/usr/bin/tt-desktop"])
        keywords = popen.call_args.kwargs
        self.assertTrue(keywords["start_new_session"])
        for stream in ("stdin", "stdout", "stderr"):
            self.assertEqual(keywords[stream], subprocess.DEVNULL)


class MainTests(unittest.TestCase):
    def run_main(self, *arguments, failure=None):
        names = ("revision", "build", "quit_app", "install", "launch")
        mocks = {name: patch.object(INSTALL, name).start() for name in names}
        self.addCleanup(patch.stopall)
        mocks["revision"].return_value = "a9b4a340"
        mocks["build"].return_value = Path("/work/tt-desktop.pkg.tar.zst")
        if failure:
            mocks["build"].side_effect = failure
        with patch("sys.argv", ["install-omarchy-app.py", *arguments]), \
                patch("sys.stdout", io.StringIO()) as stdout, patch("sys.stderr", io.StringIO()) as stderr:
            code = INSTALL.main()
        return code, stdout.getvalue(), stderr.getvalue(), mocks

    def test_installs_the_built_package_and_launches_the_app(self):
        code, stdout, _, mocks = self.run_main()
        self.assertEqual(code, 0)
        self.assertEqual(mocks["build"].call_args.args[0], "a9b4a340")
        mocks["install"].assert_called_once_with(Path("/work/tt-desktop.pkg.tar.zst"))
        mocks["quit_app"].assert_called_once_with()
        mocks["launch"].assert_called_once_with()
        self.assertEqual(stdout, "Installed a9b4a340 at /usr/bin/tt-desktop and launched it\n")

    def test_no_launch_leaves_the_app_closed(self):
        code, stdout, _, mocks = self.run_main("--no-launch")
        self.assertEqual(code, 0)
        mocks["install"].assert_called_once()
        mocks["launch"].assert_not_called()
        self.assertEqual(stdout, "Installed a9b4a340 at /usr/bin/tt-desktop\n")

    def test_failed_build_keeps_the_running_app_and_the_installed_package(self):
        code, stdout, stderr, mocks = self.run_main(failure=ValueError("makepkg failed with exit code 4"))
        self.assertEqual(code, 1)
        mocks["quit_app"].assert_not_called()
        mocks["install"].assert_not_called()
        mocks["launch"].assert_not_called()
        self.assertEqual(stdout, "")
        self.assertEqual(stderr, "Install failed: makepkg failed with exit code 4\n")

    def test_failed_command_shows_its_error_output(self):
        error = subprocess.CalledProcessError(128, ["git"], stderr="not a git repository\n")
        code, _, stderr, _ = self.run_main(failure=error)
        self.assertEqual(code, 1)
        self.assertTrue(stderr.endswith("\nnot a git repository\n"))

    def test_the_app_is_only_quit_after_the_build_succeeds(self):
        calls = []
        names = ("revision", "build", "quit_app", "install", "launch")
        for name in names:
            patcher = patch.object(INSTALL, name, side_effect=lambda *_, name=name: calls.append(name) or
                                   Path("/work/tt-desktop.pkg.tar.zst"))
            patcher.start()
            self.addCleanup(patcher.stop)
        with patch("sys.argv", ["install-omarchy-app.py"]), patch("sys.stdout", io.StringIO()):
            INSTALL.main()
        self.assertEqual(calls, list(names))


if __name__ == "__main__":
    unittest.main()
