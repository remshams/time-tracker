import importlib.util
import io
from pathlib import Path
import shutil
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
    QUIT = ["osascript", "-e", 'if application id "com.timetracker.swiftui" is running then '
            'tell application id "com.timetracker.swiftui" to quit']
    RUNNING = 'ASN:0x1-"Time_Tracker":'

    def test_app_that_is_not_running_is_left_alone(self):
        with patch.object(INSTALL, "output", return_value="") as output, \
                patch.object(INSTALL.subprocess, "run") as run:
            INSTALL.quit_app("com.timetracker.swiftui")
        output.assert_called_once_with(["lsappinfo", "find", "bundleid=com.timetracker.swiftui"])
        run.assert_not_called()

    def test_asks_again_until_every_copy_has_quit(self):
        copies = iter(['ASN:0x1-"Time_Tracker":\nASN:0x2-"Time_Tracker":', 'ASN:0x2-"Time_Tracker":', ""])
        with patch.object(INSTALL, "output", side_effect=lambda command: next(copies)), \
                patch.object(INSTALL.subprocess, "run", return_value=completed(returncode=1)) as run, \
                patch.object(INSTALL.time, "sleep"):
            INSTALL.quit_app("com.timetracker.swiftui")
        self.assertEqual([call.args[0] for call in run.call_args_list], [self.QUIT, self.QUIT])

    def quit_until_timeout(self, request):
        clock = iter([0, 5, INSTALL.QUIT_TIMEOUT + 1])
        with patch.object(INSTALL, "output", return_value=self.RUNNING), \
                patch.object(INSTALL.subprocess, "run", side_effect=request) as run, \
                patch.object(INSTALL.time, "monotonic", side_effect=lambda: next(clock)), \
                patch.object(INSTALL.time, "sleep"), self.assertRaises(ValueError) as raised:
            INSTALL.quit_app("com.timetracker.swiftui")
        self.assertEqual(run.call_count, 1)
        self.assertEqual(run.call_args.kwargs["timeout"], INSTALL.QUIT_TIMEOUT - 5)
        return str(raised.exception)

    def test_app_that_keeps_running_fails_after_the_timeout_with_the_last_error(self):
        message = self.quit_until_timeout(lambda *_, **__: completed(stderr="Not authorized (-1743)\n", returncode=1))
        self.assertEqual(message, "Time Tracker did not quit within 10 seconds; quit it and try again\n"
                                  "Not authorized (-1743)")

    def test_quit_request_that_hangs_is_cut_off_at_the_timeout(self):
        def hang(command, **keywords):
            raise subprocess.TimeoutExpired(command, keywords["timeout"])

        self.assertIn("may be showing a dialog", self.quit_until_timeout(hang))


class InstallTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.built = self.root / "built/TimeTracker.app"
        (self.built / "Contents").mkdir(parents=True)
        (self.built / "Contents/build").write_text("new build", encoding="utf-8")
        self.installed = self.root / "Applications/TimeTracker.app"

    def install(self, codesign_error=None):
        def run(command):
            if command[0] == "ditto":
                shutil.copytree(command[1], command[2])
            elif codesign_error:
                raise codesign_error
            return ""

        with patch.object(INSTALL, "output", side_effect=run) as output:
            INSTALL.install(self.built, self.installed)
        return [call.args[0] for call in output.call_args_list]

    def test_replaces_the_installed_app_with_the_verified_build(self):
        (self.installed / "Contents").mkdir(parents=True)
        (self.installed / "Contents/old").write_text("old build", encoding="utf-8")
        commands = self.install()
        self.assertEqual(sorted(path.name for path in (self.installed / "Contents").iterdir()), ["build"])
        self.assertEqual(commands[1][:4], ["codesign", "--verify", "--deep", "--strict"])
        self.assertEqual(list(self.installed.parent.iterdir()), [self.installed])

    def test_creates_the_applications_folder_for_a_first_install(self):
        self.install()
        self.assertTrue((self.installed / "Contents/build").is_file())

    def test_failed_verification_keeps_the_installed_app(self):
        (self.installed / "Contents").mkdir(parents=True)
        (self.installed / "Contents/old").write_text("old build", encoding="utf-8")
        with self.assertRaises(subprocess.CalledProcessError):
            self.install(codesign_error=subprocess.CalledProcessError(1, ["codesign"], stderr="invalid"))
        self.assertTrue((self.installed / "Contents/old").is_file())
        self.assertEqual(list(self.installed.parent.iterdir()), [self.installed])

    def test_failed_swap_puts_the_installed_app_back(self):
        (self.installed / "Contents").mkdir(parents=True)
        (self.installed / "Contents/old").write_text("old build", encoding="utf-8")
        rename = Path.rename

        def fail_for_staged(path, target):
            if path.name == self.installed.name and path != self.installed:
                raise OSError("rename failed")
            return rename(path, target)

        with patch.object(INSTALL.Path, "rename", fail_for_staged), self.assertRaisesRegex(OSError, "rename failed"):
            self.install()
        self.assertTrue((self.installed / "Contents/old").is_file())
        self.assertEqual(list(self.installed.parent.iterdir()), [self.installed])

    def test_replaces_a_symlinked_app(self):
        target = self.root / "elsewhere/TimeTracker.app"
        target.mkdir(parents=True)
        self.installed.parent.mkdir()
        self.installed.symlink_to(target)
        self.install()
        self.assertFalse(self.installed.is_symlink())
        self.assertTrue((self.installed / "Contents/build").is_file())
        self.assertTrue(target.is_dir())


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
