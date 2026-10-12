"""Measure overview presentation and closed-owner resource release in isolation."""
import argparse
import collections
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import threading
import time

from performance import percentile, process_sample


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("fixture_binary")
    parser.add_argument("--opens", type=int, default=30)
    parser.add_argument("--idle-seconds", type=float, default=5)
    parser.add_argument("--output")
    args = parser.parse_args()
    if os.environ.get("OXIBAR_TEST_ISOLATED") != "1" or os.environ.get("OXIBAR_TEST_COMPOSITOR") != "Sway":
        raise SystemExit("Run scripts/run-isolated.py --parent for advertised monotonic presentation feedback")
    if args.opens < 20 or args.idle_seconds < 5:
        raise SystemExit("Require >=20 reopen samples and >=5 closed idle seconds")
    with tempfile.TemporaryDirectory(prefix="oxibar-overview-perf-") as temporary, tempfile.TemporaryDirectory(prefix="oxibar-overview-trace-") as trace_directory:
        root = Path(temporary)
        root.chmod(0o700)
        config = root / "config" / "oxibar"
        config.mkdir(parents=True)
        (config / "config.toml").write_text(f'''plugins=[]
[bar]
start=["overview"]
center=[]
end=[]
[dock]
enabled=false
[instance]
lock_path={json.dumps(str(root / "lock"))}
[ipc]
socket_path={json.dumps(str(root / "socket"))}
''')
        trace = Path(trace_directory) / "presentation.ndjson"
        env = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), XDG_DATA_DIRS=str(root / "empty"), XDG_STATE_HOME=str(root / "state"), OXIBAR_PERF_TRACE=str(trace))
        wayland_lib = subprocess.check_output(["pkg-config", "--variable=libdir", "wayland-client"], text=True).strip()
        fixture_env = dict(env, OXIBAR_TEST_OVERVIEW_FIXTURE=str(root), LD_LIBRARY_PATH=wayland_lib + (":" + env["LD_LIBRARY_PATH"] if env.get("LD_LIBRARY_PATH") else ""))
        fixture_env.pop("OXIBAR_PERF_TRACE", None)
        logs = collections.deque(maxlen=100)
        fixture = subprocess.Popen([args.fixture_binary, "overview::capture_tests::private_overview_window_fixture", "--exact", "--ignored", "--nocapture"], env=fixture_env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        daemon = None

        def drain(process):
            def read():
                for line in process.stdout:
                    logs.append(line)
            threading.Thread(target=read, daemon=True).start()

        def cli(*arguments):
            result = subprocess.run([args.binary, *arguments], env=env, capture_output=True, text=True, check=True, timeout=12)
            return json.loads(result.stdout)

        def status():
            return cli("status")["data"]

        def wait(predicate, label):
            deadline = time.monotonic() + 12
            while time.monotonic() < deadline:
                for process in (fixture, daemon):
                    if process is not None and process.poll() is not None:
                        raise RuntimeError("fixture/daemon stopped\n" + "".join(logs))
                try:
                    value = predicate()
                    if value:
                        return value
                except (OSError, ValueError, subprocess.SubprocessError):
                    pass
                time.sleep(.005)
            raise AssertionError(label + "\n" + "".join(logs))

        def records():
            return [json.loads(line) for line in trace.read_text().splitlines() if line.endswith("}")] if trace.exists() else []

        def capture_workers():
            result = 0
            for task in (Path("/proc") / str(daemon.pid) / "task").iterdir():
                try:
                    result += task.joinpath("comm").read_text().strip() == "oxibar-overview"
                except FileNotFoundError:
                    pass
            return result

        def native_windows_ready():
            value = status()["overview"]
            return value if len(value["windows"]) == 2 else None

        def open_presented():
            old = {item["surface"] for item in records() if item["event"] == "surface"}
            requested = time.monotonic_ns()
            cli("overview", "open")
            frame = wait(lambda: next((item for item in records() if item["event"] == "presented" and item["namespace"] == "OxiBar overview" and item["surface"] not in old and item["stamp_ns"] >= requested), None), "actual overview presentation")
            state = wait(native_windows_ready, "two native windows")
            assert capture_workers() == 1, "overview must own exactly one native worker"
            return (frame["stamp_ns"] - requested) / 1_000_000, state

        def close():
            current = {item["surface"] for item in records() if item["event"] == "surface" and item["namespace"] == "OxiBar overview"}
            cli("overview", "close")
            wait(lambda: current <= {item["surface"] for item in records() if item["event"] == "closed"}, "native overview retirement")
            wait(lambda: not status()["overview"]["open"] and capture_workers() == 0, "owned capture worker release")
            state = status()["overview"]
            assert not state["windows"] and state["staging_bytes"] == state["thumbnail_bytes"] == state["in_flight"] == 0

        def stop(process):
            if process is not None and process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()

        try:
            drain(fixture)
            wait(lambda: (root / "ready").exists(), "private native source windows")
            daemon = subprocess.Popen([args.binary], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            drain(daemon)
            wait(lambda: cli("status"), "private daemon IPC")
            wait(lambda: any(item["event"] == "presented" and item["namespace"] == "OxiBar" for item in records()), "initial bar presentation")
            clocks = {item["clock_id"] for item in records() if item["event"] == "presentation_clock"}
            assert clocks == {1}, "advertised CLOCK_MONOTONIC required"
            first, native = open_presented()
            close()
            for _ in range(3):
                open_presented()
                close()
            time.sleep(.2)
            baseline = process_sample(daemon.pid)
            warm, memory = [], []
            for _ in range(args.opens):
                elapsed, state = open_presented()
                warm.append(elapsed)
                assert state["in_flight"] <= 2 and state["staging_bytes"] <= 64 * 1024 * 1024 and state["thumbnail_bytes"] <= 96 * 1024 * 1024
                close()
                time.sleep(.04)
                memory.append(process_sample(daemon.pid))
            time.sleep(.3)
            before = process_sample(daemon.pid)
            draws_before = sum(item["event"] == "draw" for item in records())
            started = time.monotonic()
            time.sleep(args.idle_seconds)
            elapsed = time.monotonic() - started
            after = process_sample(daemon.pid)
            draws = sum(item["event"] == "draw" for item in records()) - draws_before
            cpu = (after["ticks"] - before["ticks"]) / os.sysconf("SC_CLK_TCK") / elapsed * 100
            growth = statistics.median(item["rss_kib"] for item in memory[-10:]) - statistics.median(item["rss_kib"] for item in memory[:10])
            report = {"compositor": os.environ["OXIBAR_TEST_COMPOSITOR_VERSION"], "presentation_clock": 1, "source_windows": 2, "capture_backend": native["backend"], "capture_supported": any(item["preview"] for item in native["windows"]), "capture_diagnostic": native["error"], "samples": {"warm_opens": args.opens, "closed_idle_seconds": elapsed}, "first_presented_ms": first, "warm_presented_ms": {"median": statistics.median(warm), "p95": percentile(warm, .95), "max": max(warm)}, "closed": {"capture_workers": capture_workers(), "cpu_percent_one_core": cpu, "draws": draws, "voluntary_context_switches_per_second_all_threads": (after["voluntary_switches"] - before["voluntary_switches"]) / elapsed}, "reopen_memory": {"growth_kib": growth, "initial_threads": baseline["threads"], "final_threads": after["threads"], "max_threads": max(item["threads"] for item in memory)}}
            report["gates"] = {"first_presentation_under_50ms": first < 50, "warm_presented_p95_under_50ms": percentile(warm, .95) < 50, "no_closed_capture_worker": report["closed"]["capture_workers"] == 0, "no_continuous_closed_render": draws == 0, "closed_cpu_under_one_percent_core": cpu < 1, "bounded_reopen_rss": growth <= 2048, "no_worker_growth": after["threads"] <= baseline["threads"]}
            print(json.dumps(report, indent=2), flush=True)
            if args.output:
                Path(args.output).write_text(json.dumps(report, indent=2) + "\n")
            assert all(report["gates"].values()), "overview presentation/resource gate failed"
        finally:
            (root / "stop").touch()
            stop(daemon)
            stop(fixture)


if __name__ == "__main__":
    main()
