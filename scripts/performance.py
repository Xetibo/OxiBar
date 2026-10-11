"""Measure real presentation feedback and bounded resident-runner release gates."""
import argparse
import collections
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile
import threading
import time


def percentile(values, fraction):
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)]


def process_sample(pid):
    process = Path("/proc") / str(pid)
    fields = (process / "stat").read_text().rsplit(")", 1)[1].split()
    status = dict(line.split(":", 1) for line in (process / "status").read_text().splitlines() if ":" in line)
    switches = 0
    tasks = list((process / "task").iterdir())
    for task in tasks:
        try:
            lines = dict(line.split(":", 1) for line in (task / "status").read_text().splitlines() if ":" in line)
            switches += int(lines.get("voluntary_ctxt_switches", "0"))
        except FileNotFoundError:
            pass
    return {"ticks": int(fields[11]) + int(fields[12]), "rss_kib": int(status["VmRSS"].split()[0]), "threads": len(tasks), "voluntary_switches": switches}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("applications")
    parser.add_argument("tools")
    parser.add_argument("--entries", type=int, default=2000)
    parser.add_argument("--opens", type=int, default=50)
    parser.add_argument("--idle-seconds", type=float, default=10)
    parser.add_argument("--presentation-clock", type=int, choices=[1], help="Explicit CLOCK_MONOTONIC override for a source-verified compositor that omits clock_id")
    parser.add_argument("--output")
    args = parser.parse_args()
    if os.environ.get("OXIBAR_TEST_ISOLATED") != "1":
        raise SystemExit("Run through scripts/run-isolated.py")
    if args.entries < 100 or args.opens < 20 or args.idle_seconds < 5:
        raise SystemExit("Release samples require >=100 entries, >=20 opens and >=5 idle seconds")
    with tempfile.TemporaryDirectory(prefix="oxibar-perf-") as directory, tempfile.TemporaryDirectory(prefix="oxibar-trace-") as trace_directory:
        root = Path(directory)
        config = root / "config" / "oxibar"
        plugins = config / "plugins"
        plugins.mkdir(parents=True)
        data = root / "data" / "applications"
        data.mkdir(parents=True)
        for index in range(args.entries):
            (data / f"dataset-{index:05d}.desktop").write_text(f"[Desktop Entry]\nType=Application\nName=Dataset {index:05d}\nExec=/bin/true\n")
        for library in (args.applications, args.tools):
            (plugins / Path(library).name).symlink_to(Path(library).resolve())
        (config / "config.toml").write_text(f'''plugins=["libapplications.so","libtools.so"]
[bar]
start=["launcher","settings"]
center=[]
end=[]
[dock]
enabled=false
[instance]
lock_path={json.dumps(str(root/"lock"))}
[ipc]
socket_path={json.dumps(str(root/"socket"))}
[applications]
refresh_seconds=3600
[tools]
file_roots=[]
clipboard_enabled=false
refresh_seconds=3600
''')
        trace = Path(trace_directory) / "presentation.ndjson"
        env = dict(os.environ, XDG_CONFIG_HOME=str(root/"config"), XDG_DATA_HOME=str(root/"data"), XDG_DATA_DIRS=str(root/"empty"), XDG_STATE_HOME=str(root/"state"), OXIBAR_PERF_TRACE=str(trace))
        logs = collections.deque(maxlen=100)
        started = time.monotonic_ns()
        daemon = subprocess.Popen([args.binary], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        def drain():
            for line in daemon.stdout:
                logs.append(line)
        threading.Thread(target=drain, daemon=True).start()
        def cli(*arguments):
            result = subprocess.run([args.binary, *arguments], env=env, capture_output=True, text=True, timeout=12, check=True)
            return json.loads(result.stdout) if result.stdout.startswith("{") else None
        def status():
            return cli("status")["data"]
        def wait(predicate, label, timeout=10):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                if daemon.poll() is not None:
                    raise RuntimeError("daemon stopped\n" + "".join(logs))
                try:
                    result = predicate()
                    if result:
                        return result
                except (FileNotFoundError, json.JSONDecodeError, subprocess.SubprocessError):
                    pass
                time.sleep(0.005)
            raise AssertionError(label + "\n" + "".join(logs))
        def records():
            if not trace.exists():
                return []
            lines = trace.read_text().splitlines()
            return [json.loads(line) for line in lines if line.endswith("}")]
        def open_presented():
            before = {record["surface"] for record in records() if record["event"] == "surface"}
            request = time.monotonic_ns()
            cli("launcher", "open")
            feedback = wait(lambda: next((record for record in records() if record["event"] == "presented" and record["namespace"] == "OxiBar launcher" and record["surface"] not in before and record["stamp_ns"] >= request), None), "wp_presentation launcher frame")
            return (feedback["stamp_ns"] - request) / 1_000_000
        def close():
            surface = next(record["surface"] for record in reversed(records()) if record["event"] == "surface" and record["namespace"] == "OxiBar launcher")
            cli("launcher", "close")
            wait(lambda: any(record["event"] == "closed" and record["surface"] == surface for record in records()), "native launcher retirement")
        try:
            wait(lambda: cli("status"), "cold daemon IPC initialization")
            cold_control_ms = (time.monotonic_ns() - started) / 1_000_000
            bar = wait(lambda: next((record for record in records() if record["event"] == "presented" and record["namespace"] == "OxiBar"), None), "cold bar presentation")
            clocks = [record["clock_id"] for record in records() if record["event"] == "presentation_clock"]
            assert set(clocks) == {1} if clocks else args.presentation_clock == 1, "Compositor omitted clock_id; a source-verified --presentation-clock 1 override is required"
            assert 0 <= time.monotonic_ns() - bar["stamp_ns"] < 10_000_000_000, "Presentation timestamp does not match the declared clock domain"
            cold_bar_ms = (bar["stamp_ns"] - started) / 1_000_000
            assert cold_bar_ms >= 0, "Compositor returned a stale presentation timestamp before daemon startup"
            wait(lambda: not status()["theme"]["font_loading"], "font upload")
            for _ in range(5):
                open_presented()
                wait(lambda: status()["launcher_results"], "resident index results")
                close()
            baseline = process_sample(daemon.pid)
            opens = []
            memory = []
            for _ in range(args.opens):
                opens.append(open_presented())
                close()
                time.sleep(0.04)
                memory.append(process_sample(daemon.pid))
            open_presented()
            time.sleep(0.3)
            queries = []
            for index in range(20):
                query = f"Dataset {args.entries - 1 - index:05d}"
                revision = status()["launcher_query_revision"]
                subprocess.run(["wtype", "-s", "250", "-M", "ctrl", "-k", "a", "-m", "ctrl", query], env=env, check=True)
                def query_ready():
                    current = status()
                    if current["launcher_query_revision"] >= revision + len(query) and current["launcher_query_us"] is not None and current["launcher_results"]:
                        return current
                result = wait(query_ready, "complete accepted query")
                queries.append(result["launcher_query_us"] / 1000)
            close()
            time.sleep(0.5)
            before = process_sample(daemon.pid)
            before_draws = sum(record["event"] == "draw" for record in records())
            idle_start = time.monotonic()
            time.sleep(args.idle_seconds)
            elapsed = time.monotonic() - idle_start
            after = process_sample(daemon.pid)
            draws = sum(record["event"] == "draw" for record in records()) - before_draws
            cpu_percent = (after["ticks"] - before["ticks"]) / os.sysconf("SC_CLK_TCK") / elapsed * 100
            rss_start = statistics.median(item["rss_kib"] for item in memory[:10])
            rss_end = statistics.median(item["rss_kib"] for item in memory[-10:])
            gpu = []
            for device in Path("/sys/class/drm").glob("card*/device"):
                try:
                    gpu.append({"vendor": (device/"vendor").read_text().strip(), "device": (device/"device").read_text().strip()})
                except OSError:
                    pass
            cpu_model = next((line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name")), platform.machine())
            compositor = {"name": os.environ["OXIBAR_TEST_COMPOSITOR"], "version": os.environ["OXIBAR_TEST_COMPOSITOR_VERSION"]} if "OXIBAR_TEST_COMPOSITOR" in os.environ else json.loads(subprocess.check_output(["hyprctl", "-j", "version"], env=env))
            report = {"reference": {"cpu": cpu_model, "gpu_pci_ids": gpu, "kernel": platform.release(), "compositor": compositor, "presentation_clock": {"id": 1, "advertised": bool(clocks), "explicit_override": not clocks}, "backend": "isolated headless GLES2", "profile": f"release / Apps+Tools / no dock / {args.entries}-desktop-entry local index"}, "samples": {"desktop_entries": args.entries, "warm_opens": args.opens, "queries": len(queries), "idle_seconds": elapsed}, "cold_control_ms": cold_control_ms, "cold_bar_presented_ms": cold_bar_ms, "warm_presented_ms": {"median": statistics.median(opens), "p95": percentile(opens, .95), "max": max(opens)}, "query_to_results_ms": {"median": statistics.median(queries), "p95": percentile(queries, .95), "max": max(queries)}, "idle": {"cpu_percent_one_core": cpu_percent, "draws": draws, "voluntary_context_switches_per_second_all_threads": (after["voluntary_switches"] - before["voluntary_switches"]) / elapsed}, "reopen_memory": {"baseline_rss_kib": baseline["rss_kib"], "first10_median_kib": rss_start, "last10_median_kib": rss_end, "growth_kib": rss_end-rss_start, "initial_threads": baseline["threads"], "final_threads": after["threads"], "max_threads": max(item["threads"] for item in memory)}}
            report["gates"] = {"warm_presented_p95_under_50ms": report["warm_presented_ms"]["p95"] < 50, "all_query_results_under_one_60hz_frame": max(queries) <= 1000/60, "idle_under_one_percent_core": cpu_percent < 1, "no_continuous_idle_render": draws <= math.ceil(elapsed * 2), "bounded_rss_after_reopen": rss_end-rss_start <= 2048, "no_worker_growth": after["threads"] <= baseline["threads"]}
            print(json.dumps(report, indent=2), flush=True)
            if args.output:
                Path(args.output).write_text(json.dumps(report, indent=2) + "\n")
            assert all(report["gates"].values()), "release performance gate failed"
        finally:
            daemon.terminate()
            try:
                daemon.wait(timeout=6)
            except subprocess.TimeoutExpired:
                daemon.kill()
                daemon.wait()


if __name__ == "__main__":
    main()
