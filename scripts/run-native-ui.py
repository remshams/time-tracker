#!/usr/bin/env python3
"""Stream native UI tests with deadlines and capture stalled process samples."""

import argparse
import json
import math
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time


def positive_seconds(value):
    seconds = float(value)
    if not math.isfinite(seconds) or seconds <= 0:
        raise ValueError("Native UI deadlines must be positive, finite numbers")
    return seconds


def terminate_group(process):
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


def collect_diagnostics(directory, process_id):
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / "processes.txt").open("w") as output:
        subprocess.run(["ps", "-axo", "pid,ppid,stat,etime,comm"],
                       stdout=output, stderr=subprocess.STDOUT, timeout=10, check=False)
    if sys.platform != "darwin":
        return
    result = subprocess.run(["pgrep", "-x", "TimeTracker|TimeTrackerUITests-Runner"],
                            capture_output=True, text=True, timeout=10, check=False)
    process_ids = {process_id, *(int(pid) for pid in result.stdout.split())}
    for pid in sorted(process_ids):
        subprocess.run(["sample", str(pid), "3", "-file", str(directory / f"sample-{pid}.txt")],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10, check=False)


def run(command, log, *, idle_timeout=180, timeout=1440, output=None,
        diagnose=collect_diagnostics):
    idle_timeout = positive_seconds(idle_timeout)
    timeout = positive_seconds(timeout)
    output = sys.stdout if output is None else output
    log.parent.mkdir(parents=True, exist_ok=True)
    environment = {**os.environ, "NSUnbufferedIO": "YES"}
    with log.open("wb") as child_output, log.open("rb") as reader:
        process = subprocess.Popen(command, stdout=child_output, stderr=subprocess.STDOUT,
                                   start_new_session=True, env=environment)
        started = last_output = time.monotonic()

        def forward_output():
            nonlocal last_output
            data = reader.read()
            if data:
                output.write(data.decode("utf-8", errors="replace"))
                output.flush()
                last_output = time.monotonic()

        try:
            while True:
                forward_output()
                status = process.poll()
                if status is not None:
                    forward_output()
                    return status
                now = time.monotonic()
                reason = None
                if now - started >= timeout:
                    reason = f"total execution exceeded {timeout:g} seconds"
                elif now - last_output >= idle_timeout:
                    reason = f"no output for {idle_timeout:g} seconds"
                if reason:
                    message = f"Native UI tests stopped: {reason}.\n"
                    output.write(message)
                    output.flush()
                    try:
                        diagnose(log.parent / "native-ui-hang", process.pid)
                    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
                        output.write(f"Could not finish native UI diagnostics: {error}\n")
                        output.flush()
                    return 124
                time.sleep(0.05)
        finally:
            terminate_group(process)
            forward_output()


def summarize(log):
    text = log.read_text(encoding="utf-8", errors="replace")
    starts = re.findall(r"Test Case '-\[([^\]]+)\]' started\.", text)
    completions = re.findall(r"Test Case '-\[([^\]]+)\]' (passed|failed) \(([0-9.]+) seconds\)", text)
    completed_names = {name for name, _, _ in completions}
    tests = [{"name": name, "result": result, "seconds": float(seconds)}
             for name, result, seconds in completions]
    return {"completed_tests": len(tests), "test_seconds": sum(test["seconds"] for test in tests),
            "unfinished_tests": [name for name in starts if name not in completed_names],
            "slowest_tests": sorted(tests, key=lambda test: test["seconds"], reverse=True)[:10]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("--idle-timeout", type=positive_seconds, default=180)
    parser.add_argument("--timeout", type=positive_seconds, default=1440)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    arguments = parser.parse_args()
    command = arguments.command[1:] if arguments.command[:1] == ["--"] else arguments.command
    if not command:
        parser.error("A native UI test command is required")

    def interrupted(signum, _frame):
        raise SystemExit(128 + signum)

    signal.signal(signal.SIGTERM, interrupted)
    started = time.monotonic()
    status = run(command, arguments.log, idle_timeout=arguments.idle_timeout, timeout=arguments.timeout)
    report = {**summarize(arguments.log), "wall_seconds": time.monotonic() - started, "exit_status": status,
              "timeout_seconds": arguments.timeout, "idle_timeout_seconds": arguments.idle_timeout}
    arguments.log.with_name("native-ui-timings.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Native UI timing: {report['completed_tests']} tests in {report['wall_seconds']:.1f}s")
    for test in report["slowest_tests"]:
        print(f"  {test['seconds']:.1f}s {test['name']}")
    return status if status >= 0 else 128 - status


if __name__ == "__main__":
    sys.exit(main())
