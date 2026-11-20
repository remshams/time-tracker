import importlib.util
import io
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("install_macos_app", Path(__file__).parents[1] / "install-macos-app.py")
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
    def test_release_build_records_the_revision(self):
        with patch.object(INSTALL.subprocess, "run", return_value=completed()) as run:
            INSTALL.build("a9b4a340-dirty")
        command = run.call_args.args[0]
        self.assertEqual(command[command.index("-configuration") + 1], "Release")
        self.assertEqual(command[command.index("-derivedDataPath") + 1], str(INSTALL.DERIVED_DATA))
        self.assertIn("TT_GIT_REVISION=a9b4a340-dirty", command)

    def test_failed_build_reports_only_the_error_lines(self):
        log = "CompileSwift Sources/App.swift\nApp.swift:3: error: missing return\n** BUILD FAILED **"
        with patch.object(INSTALL.subprocess, "run", return_value=completed(log, returncode=65)), \
                self.assertRaises(ValueError) as raised:
            INSTALL.build("a9b4a340")
        self.assertEqual(str(raised.exception), "Release build failed:\nApp.swift:3: error: missing return")

    def test_failed_build_without_error_lines_reports_the_end_of_the_log(self):
        log = "\n".join(f"line {number}" for number in range(30))
        with patch.object(INSTALL.subprocess, "run", return_value=completed(stderr=log, returncode=70)), \
                self.assertRaises(ValueError) as raised:
            INSTALL.build("a9b4a340")
        reported = str(raised.exception).splitlines()[1:]
        self.assertEqual(reported, [f"line {number}" for number in range(10, 30)])


class QuitTests(unittest.TestCase):
    def test_app_that_is_not_running_is_left_alone(self):
        with patch.object(INSTALL, "output", return_value="false") as output:
            INSTALL.quit_app("com.timetracker.swiftui")
        output.assert_called_once_with(["osascript", "-e", 'application id "com.timetracker.swiftui" is running'])

    def test_running_app_is_quit_and_awaited(self):
        answers = iter(["true", "", "true", "false"])
        with patch.object(INSTALL, "output", side_effect=lambda command: next(answers)) as output, \
                patch.object(INSTALL.time, "sleep"):
            INSTALL.quit_app("com.timetracker.swiftui")
        self.assertIn(["osascript", "-e", 'tell application id "com.timetracker.swiftui" to quit'],
                      [call.args[0] for call in output.call_args_list])
        self.assertEqual(output.call_count, 4)

    def test_app_that_keeps_running_fails_after_the_timeout(self):
        clock = iter([0, 5, INSTALL.QUIT_TIMEOUT + 1])
        with patch.object(INSTALL, "output", side_effect=lambda command: "" if "to quit" in command[-1] else "true"), \
                patch.object(INSTALL.time, "monotonic", side_effect=lambda: next(clock)), \
                patch.object(INSTALL.time, "sleep") as sleep, \
                self.assertRaisesRegex(ValueError, "did not quit within 10 seconds"):
            INSTALL.quit_app("com.timetracker.swiftui")
        self.assertEqual(sleep.call_count, 1)


class InstallTests(unittest.TestCase):
    def test_replaces_the_installed_app_and_verifies_the_signature(self):
        with tempfile.TemporaryDirectory() as directory:
            built = Path(directory) / "built/TimeTracker.app"
            installed = Path(directory) / "Applications/TimeTracker.app"
            (installed / "Contents").mkdir(parents=True)
            (installed / "Contents/old").write_text("old build", encoding="utf-8")
            with patch.object(INSTALL, "output") as output:
                INSTALL.install(built, installed)
            self.assertFalse(installed.exists())
            self.assertTrue(installed.parent.is_dir())
        self.assertEqual([call.args[0] for call in output.call_args_list],
                         [["ditto", str(built), str(installed)],
                          ["codesign", "--verify", "--deep", "--strict", str(installed)]])

    def test_creates_the_applications_folder_for_a_first_install(self):
        with tempfile.TemporaryDirectory() as directory:
            installed = Path(directory) / "Applications/TimeTracker.app"
            with patch.object(INSTALL, "output"):
                INSTALL.install(Path(directory) / "built.app", installed)
            self.assertTrue(installed.parent.is_dir())


class MainTests(unittest.TestCase):
    def run_main(self, *arguments, failure=None):
        names = ("revision", "build", "bundle_identifier", "quit_app", "install", "output")
        mocks = {name: patch.object(INSTALL, name).start() for name in names}
        self.addCleanup(patch.stopall)
        mocks["revision"].return_value = "a9b4a340"
        mocks["bundle_identifier"].return_value = "com.timetracker.swiftui"
        if failure:
            mocks["build"].side_effect = failure
        with patch("sys.argv", ["install-macos-app.py", *arguments]), \
                patch("sys.stdout", io.StringIO()) as stdout, patch("sys.stderr", io.StringIO()) as stderr:
            code = INSTALL.main()
        return code, stdout.getvalue(), stderr.getvalue(), mocks

    def test_installs_and_launches_the_build(self):
        code, stdout, _, mocks = self.run_main()
        self.assertEqual(code, 0)
        mocks["build"].assert_called_once_with("a9b4a340")
        mocks["quit_app"].assert_called_once_with("com.timetracker.swiftui")
        mocks["install"].assert_called_once_with(INSTALL.BUILT_APP, INSTALL.INSTALLED_APP)
        mocks["output"].assert_called_once_with(["open", str(INSTALL.INSTALLED_APP)])
        self.assertEqual(stdout, f"Installed a9b4a340 at {INSTALL.INSTALLED_APP} and launched it\n")

    def test_no_launch_leaves_the_app_closed(self):
        code, stdout, _, mocks = self.run_main("--no-launch")
        self.assertEqual(code, 0)
        mocks["output"].assert_not_called()
        self.assertEqual(stdout, f"Installed a9b4a340 at {INSTALL.INSTALLED_APP}\n")

    def test_failed_build_keeps_the_installed_app(self):
        code, stdout, stderr, mocks = self.run_main(failure=ValueError("Release build failed:\nerror: boom"))
        self.assertEqual(code, 1)
        mocks["quit_app"].assert_not_called()
        mocks["install"].assert_not_called()
        self.assertEqual(stdout, "")
        self.assertEqual(stderr, "Install failed: Release build failed:\nerror: boom\n")

    def test_failed_command_shows_its_error_output(self):
        error = subprocess.CalledProcessError(1, ["codesign"], stderr="invalid signature\n")
        code, _, stderr, _ = self.run_main(failure=error)
        self.assertEqual(code, 1)
        self.assertTrue(stderr.endswith("\ninvalid signature\n"))


if __name__ == "__main__":
    unittest.main()
