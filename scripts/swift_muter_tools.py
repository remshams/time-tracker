"""Run regression tools with bounded waits and compiler/test process cleanup."""

import os
from pathlib import Path
import signal
import subprocess
import tempfile


def terminate_group(process: subprocess.Popen) -> None:
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        process.wait()
        return
    try:
        process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        pass
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait()


def run_tool(arguments: list[str], *, cwd: Path | None = None,
             timeout: float = 30) -> subprocess.CompletedProcess:
    # Descendants can inherit output handles. A file keeps timeout cleanup
    # independent of whether those descendants close a pipe.
    with tempfile.TemporaryFile(mode="w+", encoding="utf-8") as log:
        process = subprocess.Popen(arguments, cwd=cwd, stdout=log, stderr=subprocess.STDOUT,
                                   start_new_session=True)
        try:
            status = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired as error:
            log.seek(0)
            error.output = log.read()
            raise
        finally:
            terminate_group(process)
        log.seek(0)
        return subprocess.CompletedProcess(arguments, status, stdout=log.read(), stderr="")
