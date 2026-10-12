"""Measure first calendar presentation while a private CalDAV request is blocked."""
import argparse
import collections
import json
import math
import os
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile
import threading
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("clock")
    parser.add_argument("--output")
    args = parser.parse_args()
    if os.environ.get("OXIBAR_TEST_ISOLATED") != "1" or os.environ.get("OXIBAR_TEST_COMPOSITOR") != "Sway":
        raise SystemExit("Run scripts/run-isolated.py --parent for advertised monotonic presentation feedback")
    with tempfile.TemporaryDirectory(prefix="oxibar-calendar-") as temporary, tempfile.TemporaryDirectory(prefix="oxibar-calendar-trace-") as trace_directory:
        root = Path(temporary)
        config = root / "config" / "oxibar"
        plugins = config / "plugins"
        plugins.mkdir(parents=True)
        (plugins / "libclock.so").symlink_to(Path(args.clock).resolve())
        shims = root / "bin"
        shims.mkdir()
        # This fixture deliberately keeps the first sync in flight during opening.
        curl = shims / "curl"
        curl.write_text(f'#!{sys.executable}\nfrom pathlib import Path\nimport time\nPath({str(root / "sync-started")!r}).touch()\ntime.sleep(2)\nprint("<multistatus/>")\nPath({str(root / "sync-finished")!r}).touch()\n')
        curl.chmod(0o700)
        (config / "config.toml").write_text(f'''plugins=["libclock.so"]
[bar]
start=[]
center=["Clock","control-center"]
end=[]
[dock]
enabled=false
[control_center]
widgets=["Clock"]
[clock.caldav]
url="https://private.invalid/calendar/"
username="private"
password_env="OXIBAR_TEST_CALENDAR_PASSWORD"
[instance]
lock_path={json.dumps(str(root / "lock"))}
[ipc]
socket_path={json.dumps(str(root / "socket"))}
''')
        trace = Path(trace_directory) / "presentation.ndjson"
        env = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), XDG_DATA_DIRS=str(root / "empty"), XDG_STATE_HOME=str(root / "state"), OXIBAR_TEST_CALENDAR_PASSWORD="private", OXIBAR_PERF_TRACE=str(trace), PATH=str(shims)+os.pathsep+os.environ["PATH"])
        logs = collections.deque(maxlen=100)
        daemon = subprocess.Popen([args.binary], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        def drain():
            for line in daemon.stdout:
                logs.append(line)
        threading.Thread(target=drain, daemon=True).start()
        def cli(*arguments):
            return json.loads(subprocess.check_output([args.binary, *arguments], env=env, text=True, timeout=12))
        def status():
            return cli("status")["data"]
        def records():
            return [json.loads(line) for line in trace.read_text().splitlines() if line.endswith("}")] if trace.exists() else []
        def wait(predicate, label):
            deadline = time.monotonic()+10
            while time.monotonic()<deadline:
                if daemon.poll() is not None:
                    raise RuntimeError("daemon stopped\n"+"".join(logs))
                try:
                    value = predicate()
                    if value:
                        return value
                except (FileNotFoundError, subprocess.SubprocessError, json.JSONDecodeError):
                    pass
                time.sleep(.005)
            raise AssertionError(label+"\n"+"".join(logs))
        def open_calendar():
            request = time.monotonic_ns()
            cli("control-center", "open")
            assert status()["control_center_attached"]
            frame = wait(lambda: next((item for item in records() if item["event"] == "presented" and item["namespace"] == "OxiBar" and item["stamp_ns"] >= request), None), "actual calendar bar presentation")
            return (frame["stamp_ns"]-request)/1_000_000
        def close_calendar():
            cli("control-center", "close")
            time.sleep(.2)
        try:
            wait(lambda: cli("status"), "daemon startup")
            wait(lambda: (root / "sync-started").exists(), "private CalDAV worker")
            wait(lambda: any(item["event"] == "presented" and item["namespace"] == "OxiBar" for item in records()), "initial bar presentation")
            clocks = {item["clock_id"] for item in records() if item["event"] == "presentation_clock"}
            assert clocks == {1}, "advertised CLOCK_MONOTONIC required"
            assert not (root / "sync-finished").exists(), "slow sync must still be in flight"
            first = open_calendar()
            assert not (root / "sync-finished").exists(), "opening blocked on the calendar worker"
            request = time.monotonic_ns()
            status()
            responsive = (time.monotonic_ns()-request)/1_000_000
            close_calendar()
            warm = []
            for _ in range(20):
                warm.append(open_calendar())
                close_calendar()
            p95 = sorted(warm)[math.ceil(len(warm)*.95)-1]
            report = {"compositor": os.environ["OXIBAR_TEST_COMPOSITOR_VERSION"], "presentation_clock": 1, "first_open_presented_ms": first, "warm_presented_ms": {"median": statistics.median(warm), "p95": p95, "max": max(warm)}, "status_during_sync_ms": responsive, "private_caldav_delay_seconds": 2, "first_open_did_not_wait_for_sync": True, "gates": {"first_open_under_50ms": first < 50, "warm_p95_under_50ms": p95 < 50, "status_under_50ms": responsive < 50}}
            print(json.dumps(report, indent=2), flush=True)
            if args.output:
                Path(args.output).write_text(json.dumps(report, indent=2)+"\n")
            assert all(report["gates"].values())
        finally:
            daemon.terminate()
            try:
                daemon.wait(timeout=6)
            except subprocess.TimeoutExpired:
                daemon.kill()
                daemon.wait()


if __name__ == "__main__":
    main()
