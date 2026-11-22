"""Black-box harness for tt-desktop.

Runs the real binary in a headless sway session. Keys go in through wtype, as a person
would type them. The tests read the window through AT-SPI and the stored data through
tt-cli. `bin/e2e` starts the private D-Bus session this module expects.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
import time
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402

APP_DIR = Path(__file__).resolve().parent.parent
REPO_DIR = APP_DIR.parent.parent
APP_BINARY = Path(os.environ.get("TT_DESKTOP_BINARY", APP_DIR / "build" / "tt-desktop"))
CLI_BINARY = Path(os.environ.get("TT_CLI_BINARY", REPO_DIR / "target" / "debug" / "tt-cli"))
TIMEOUT = float(os.environ.get("TT_E2E_TIMEOUT", "10"))
# wtype uploads a new keymap on each run; the client needs a moment before the first key.
KEY_DELAY_MS = "250"


class Desktop:
    """The accessibility bus and a headless sway compositor, shared by a test module."""

    def __init__(self) -> None:
        self.runtime = Path(os.environ["XDG_RUNTIME_DIR"])
        self.processes: list[subprocess.Popen] = []

    def start(self) -> None:
        log = open(self.runtime / "desktop.log", "w")
        self.processes.append(
            subprocess.Popen(["/usr/lib/at-spi-bus-launcher", "--launch-immediately"], stdout=log, stderr=log)
        )
        time.sleep(0.5)
        self.processes.append(subprocess.Popen(["/usr/lib/at-spi2-registryd"], stdout=log, stderr=log))
        config = self.runtime / "sway.conf"
        config.write_text("output HEADLESS-1 resolution 1280x800\ndefault_border none\n")
        env = dict(os.environ, WLR_BACKENDS="headless", WLR_LIBINPUT_NO_DEVICES="1", WLR_RENDERER="pixman")
        self.processes.append(subprocess.Popen(["sway", "-c", str(config)], stdout=log, stderr=log, env=env))
        socket = wait_for(lambda: next(iter(sorted(self.runtime.glob("wayland-[0-9]"))), None), "sway's socket")
        os.environ["WAYLAND_DISPLAY"] = socket.name
        os.environ["SWAYSOCK"] = str(wait_for(lambda: next(iter(self.runtime.glob("sway-ipc.*.sock")), None),
                                              "sway's IPC socket"))

    def stop(self) -> None:
        for process in reversed(self.processes):
            process.terminate()
            try:
                process.wait(5)
            except subprocess.TimeoutExpired:
                process.kill()


def wait_for(probe, what: str, timeout: float = TIMEOUT):
    deadline = time.monotonic() + timeout
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            value = probe()
            if value:
                return value
        except Exception as error:  # AT-SPI objects vanish while the window changes.
            last_error = error
        time.sleep(0.1)
    raise AssertionError(f"Timed out waiting for {what}" + (f": {last_error}" if last_error else ""))


@dataclass
class Node:
    role: str
    name: str
    accessible: Atspi.Accessible


class TrackerData:
    """The stored data, read and written through tt-cli against the app's database."""

    def __init__(self, data_home: Path) -> None:
        self.database = data_home / "Time Tracker" / "tt.db"
        self.database.parent.mkdir(parents=True, exist_ok=True)

    def run(self, *args: str, at: datetime | None = None):
        command = [str(CLI_BINARY), "--db", str(self.database)]
        if at is not None:
            command += ["--at", at.astimezone(timezone.utc).isoformat()]
        result = subprocess.run(command + list(args), capture_output=True, text=True, check=False)
        payload = json.loads(result.stdout)
        if not payload.get("ok"):
            raise AssertionError(f"tt-cli {' '.join(args)} failed: {result.stdout} {result.stderr}")
        return payload["data"]

    def create_task(self, name: str, at: datetime | None = None) -> str:
        return self.run("tasks", "create", name, at=at)["id"]

    def work(self, task_id: str, start: datetime, minutes: int) -> str:
        worklog = self.run("tracking", "start", task_id, at=start)["worklog"]
        self.run("tracking", "stop", "--expected-active", worklog["id"], at=start + timedelta(minutes=minutes))
        return worklog["id"]

    def start(self, task_id: str, at: datetime | None = None) -> str:
        return self.run("tracking", "start", task_id, at=at)["worklog"]["id"]

    def tasks(self, state: str = "active") -> dict[str, dict]:
        rows = self.run("tasks", "list", "--state", state)["tasks"]
        return {row["task"]["name"]: row["task"] for row in rows}

    def tracking(self) -> dict:
        return self.run("tracking", "status")

    def worklogs(self, task_id: str) -> list[dict]:
        return self.run("worklogs", "list", "--task", task_id)["worklogs"]


class TrackerApp:
    """One running tt-desktop with its own data and settings directories."""

    def __init__(self, data_home: Path, config_home: Path) -> None:
        env = dict(
            os.environ,
            QT_QPA_PLATFORM="wayland",
            QT_LINUX_ACCESSIBILITY_ALWAYS_ON="1",
            XDG_DATA_HOME=str(data_home),
            XDG_CONFIG_HOME=str(config_home),
            # No Omarchy theme: the app falls back to its built-in palette.
            HOME=str(config_home.parent),
        )
        self.log = open(config_home.parent / "app.log", "w")
        self.process = subprocess.Popen([str(APP_BINARY)], env=env, stdout=self.log, stderr=self.log)
        wait_for(self.application, "tt-desktop on the accessibility bus")
        # The first read lands after the window shows.
        self.wait_for(contains="Local database")

    def close(self) -> None:
        self.process.terminate()
        try:
            self.process.wait(5)
        except subprocess.TimeoutExpired:
            self.process.kill()
        self.log.close()

    def application(self) -> Atspi.Accessible | None:
        desktop = Atspi.get_desktop(0)
        for index in range(desktop.get_child_count()):
            child = desktop.get_child_at_index(index)
            if child is not None and child.get_process_id() == self.process.pid:
                return child
        return None

    def nodes(self) -> list[Node]:
        found: list[Node] = []

        def walk(accessible: Atspi.Accessible, depth: int) -> None:
            if depth > 40:
                return
            for index in range(accessible.get_child_count()):
                child = accessible.get_child_at_index(index)
                if child is None:
                    continue
                found.append(Node(child.get_role_name(), child.get_name() or "", child))
                walk(child, depth + 1)

        application = self.application()
        if application is not None:
            application.clear_cache()
            walk(application, 0)
        return found

    def find(self, role: str | None = None, name: str | None = None, contains: str | None = None) -> Node | None:
        for node in self.nodes():
            if role is not None and node.role != role:
                continue
            if name is not None and node.name != name:
                continue
            if contains is not None and contains not in node.name:
                continue
            return node
        return None

    def wait_for(self, role: str | None = None, name: str | None = None, contains: str | None = None) -> Node:
        return wait_for(lambda: self.find(role, name, contains), f"{role} {name or contains!r}")

    def wait_gone(self, role: str | None = None, name: str | None = None, contains: str | None = None) -> None:
        wait_for(lambda: self.find(role, name, contains) is None, f"{role} {name or contains!r} to go away")

    def type(self, text: str) -> None:
        subprocess.run(["wtype", "-s", KEY_DELAY_MS, text], check=True)

    def key(self, *keys: str) -> None:
        for key in keys:
            subprocess.run(["wtype", "-s", KEY_DELAY_MS, "-k", key], check=True)

    def shifted(self, letter: str) -> None:
        # wtype's own keymap sends "A" as a plain key, so Shift goes along explicitly.
        subprocess.run(["wtype", "-s", KEY_DELAY_MS, "-M", "shift", letter, "-m", "shift"], check=True)


class Workspace:
    """Temporary directories for one test, removed afterwards."""

    def __init__(self) -> None:
        self.root = Path(tempfile.mkdtemp(prefix="tt-desktop-e2e-"))
        self.data_home = self.root / "data"
        self.config_home = self.root / "config"
        self.data_home.mkdir()
        self.config_home.mkdir()
        self.data = TrackerData(self.data_home)

    def launch(self) -> TrackerApp:
        return TrackerApp(self.data_home, self.config_home)

    def remove(self) -> None:
        shutil.rmtree(self.root, ignore_errors=True)

