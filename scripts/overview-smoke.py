#!/usr/bin/env python3
"""Real window previews and dock icons on an owned isolated compositor."""
import argparse
import collections
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import threading
import time

from performance import process_sample


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("fixture_binary")
    parser.add_argument("applications")
    parser.add_argument("clock")
    parser.add_argument("--screenshots")
    args = parser.parse_args()
    if os.environ.get("OXIBAR_TEST_ISOLATED") != "1":
        raise SystemExit("Run through scripts/run-isolated.py")
    scripts = Path(__file__).resolve().parent
    with tempfile.TemporaryDirectory(prefix="oxibar-overview-") as directory:
        root = Path(directory)
        root.chmod(0o700)
        config = root / "config" / "oxibar"
        plugins = config / "plugins"
        plugins.mkdir(parents=True)
        for library in (args.clock,):
            (plugins / Path(library).name).symlink_to(Path(library).resolve())
        applications = root / "data" / "applications"
        applications.mkdir(parents=True)
        icons = root / "data" / "icons" / "hicolor" / "scalable" / "apps"
        icons.mkdir(parents=True)
        (icons / "private.svg").write_text('<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect x="4" y="4" width="56" height="56" rx="8" fill="#00ff00"/></svg>')
        (applications / "private.desktop").write_text("[Desktop Entry]\nType=Application\nName=Private SVG application\nExec=/bin/true\nIcon=private.svg\nStartupWMClass=org.oxibar.preview.0\n")
        (applications / "missing.desktop").write_text("[Desktop Entry]\nType=Application\nName=Portable fallback application\nExec=/bin/true\n")
        (config / "config.toml").write_text(f'''plugins=["libapplications.so","libclock.so"]
[bar]
outputs=["*"]
start=["overview","settings"]
center=["clock","control-center"]
end=[]
[dock]
outputs=["*"]
pinned=["private.desktop","missing.desktop"]
show_running=false
[control_center]
widgets=["Clock"]
[instance]
lock_path={json.dumps(str(root / "lock"))}
[ipc]
socket_path={json.dumps(str(root / "socket"))}
[monitor]
apply_on_start=false
[applications]
refresh_seconds=3600
''')
        env = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), XDG_DATA_DIRS=str(root / "empty"), XDG_STATE_HOME=str(root / "state"), OXIBAR_PLUGIN_DIR=str(Path(args.applications).resolve().parent))
        pointer = root / "pointer"
        xml = os.environ["OXIBAR_TEST_VIRTUAL_POINTER_XML"]
        subprocess.run(["wayland-scanner", "client-header", xml, str(root / "virtual-pointer.h")], check=True)
        subprocess.run(["wayland-scanner", "private-code", xml, str(root / "virtual-pointer.c")], check=True)
        flags = shlex.split(subprocess.check_output(["pkg-config", "--cflags", "--libs", "wayland-client"], text=True))
        subprocess.run(["gcc", "-O2", "-I", str(root), str(scripts / "virtual-pointer.c"), str(root / "virtual-pointer.c"), *flags, "-o", str(pointer)], check=True)
        logs = collections.deque(maxlen=100)
        daemon = fixture = None
        def drain(process):
            def read():
                for line in process.stdout: logs.append(line)
            threading.Thread(target=read, daemon=True).start()
        def cli(*arguments):
            result = subprocess.run([args.binary, *arguments], env=env, capture_output=True, text=True, check=True, timeout=12)
            return json.loads(result.stdout) if result.stdout.startswith("{") else None
        def status(): return cli("status")["data"]
        def wait(predicate, label, seconds=12):
            deadline = time.monotonic() + seconds
            while time.monotonic() < deadline:
                if daemon and daemon.poll() is not None: raise AssertionError("daemon stopped\n" + "".join(logs))
                try:
                    value = predicate()
                    if value: return value
                except (OSError, ValueError, subprocess.SubprocessError): pass
                time.sleep(.025)
            raise AssertionError(label + "\n" + (json.dumps(status()) if daemon else "") + "\nactive window: " + json.dumps(hypr("activewindow")) + "\n" + "".join(logs))
        def hypr(query): return json.loads(subprocess.check_output(["hyprctl", "-j", query], env=env))
        def dispatch(command): subprocess.run(["hyprctl", "dispatch", command], env=env, check=True, stdout=subprocess.DEVNULL)
        def layers():
            return [dict(layer, output=output) for output, groups in hypr("layers").items() for group in groups["levels"].values() for layer in group if layer.get("pid") == daemon.pid]
        def layer(namespace, output): return wait(lambda: next((value for value in layers() if value["namespace"] == namespace and value["output"] == output), None), namespace)
        def click(namespace, output, x, y):
            selected = layer(namespace, output)
            monitor = next(value for value in hypr("monitors") if value["name"] == output)
            subprocess.run([str(pointer), "0", "0", output, str(selected["x"] - monitor["x"] + x), str(selected["y"] - monitor["y"] + y), str(round(monitor["width"] / monitor["scale"])), str(round(monitor["height"] / monitor["scale"]))], env=env, check=True)
        def keyboard(*arguments): subprocess.run(["wtype", "-s", "250", *arguments], env=env, check=True)
        def rgb(namespace, output):
            selected = layer(namespace, output)
            data = subprocess.check_output(["grim", "-t", "ppm", "-g", f"{selected['x']},{selected['y']} {selected['w']}x{selected['h']}", "-"], env=env)
            header, pixels = data.split(b"\n255\n", 1)
            assert header.startswith(b"P6\n")
            return pixels
        def color_count(pixels, component):
            return sum(pixels[index+component] > 240 and all(pixels[index+other] < 20 for other in range(3) if other != component) for index in range(0, len(pixels), 3))
        def picture(name, namespace, output):
            if args.screenshots:
                selected = layer(namespace, output)
                target = Path(args.screenshots)
                target.mkdir(parents=True, exist_ok=True)
                subprocess.run(["grim", "-g", f"{selected['x']},{selected['y']} {selected['w']}x{selected['h']}", str(target / f"oxibar-overview-{name}.png")], env=env, check=True)
        def stop(process):
            if process and process.poll() is None:
                process.terminate()
                try: process.wait(timeout=5)
                except subprocess.TimeoutExpired: process.kill(); process.wait()
        def ready_windows():
            windows = [window for window in status()["overview"]["windows"] if window["app_id"].startswith("org.oxibar.preview.")]
            return windows if len(windows) == 2 and all(window["preview"] and window["frames"] >= 4 for window in windows) else None
        def capture_workers():
            result = 0
            for task in (Path("/proc") / str(daemon.pid) / "task").iterdir():
                try: result += task.joinpath("comm").read_text().strip() == "oxibar-overview"
                except FileNotFoundError: pass
            return result
        try:
            outputs = hypr("monitors")
            first = outputs[0]["name"]
            second = "OXIBAR-OVERVIEW-SECOND"
            subprocess.run(["hyprctl", "output", "create", "headless", second], env=env, check=True, stdout=subprocess.DEVNULL)
            before={output["name"] for output in outputs}
            second=wait(lambda: next((output["name"] for output in hypr("monitors") if output["name"] not in before),None),"owned second output")
            subprocess.run(["hyprctl", "keyword", "monitor", f"{first},1920x1080@60,0x0,1"], env=env, check=True, stdout=subprocess.DEVNULL)
            subprocess.run(["hyprctl", "keyword", "monitor", f"{second},1920x1080@60,1920x0,1"], env=env, check=True, stdout=subprocess.DEVNULL)
            wait(lambda: {output["name"]: output["x"] for output in hypr("monitors")}.get(first) == 0 and {output["name"]: output["x"] for output in hypr("monitors")}.get(second) == 1920, "owned spatial monitor layout")
            dispatch("focusmonitor " + first)
            fixture_env = dict(env, OXIBAR_TEST_OVERVIEW_FIXTURE=str(root))
            wayland_lib = subprocess.check_output(["pkg-config", "--variable=libdir", "wayland-client"], text=True).strip()
            fixture_env["LD_LIBRARY_PATH"] = wayland_lib + (":" + env["LD_LIBRARY_PATH"] if env.get("LD_LIBRARY_PATH") else "")
            fixture = subprocess.Popen([args.fixture_binary, "overview::capture_tests::private_overview_window_fixture", "--exact", "--ignored", "--nocapture"], env=fixture_env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            drain(fixture)
            wait(lambda: (root / "ready").exists() and len([window for window in hypr("clients") if window["class"].startswith("org.oxibar.preview.")]) == 2, "source windows")
            clients = {window["class"]: window for window in hypr("clients") if window["class"].startswith("org.oxibar.preview.")}
            dispatch("focusmonitor " + second)
            dispatch("workspace 22")
            dispatch("focusmonitor " + first)
            dispatch("movetoworkspacesilent 22,address:" + clients["org.oxibar.preview.1"]["address"])
            wait(lambda: next(value for value in hypr("clients") if value["class"]=="org.oxibar.preview.1")["monitor"] == next(value for value in hypr("monitors") if value["name"]==second)["id"], "fixture on second monitor")
            daemon = subprocess.Popen([args.binary], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            drain(daemon)
            wait(lambda: status()["bar_buttons"] and len(status()["docks"]) >= 2, "bar and dock")
            wait(lambda: color_count(rgb("OxiBar dock", first), 1) > 300, "actual SVG dock icon")
            picture("dock", "OxiBar dock", first)
            cli("overview", "open")
            windows = wait(ready_windows, "real previews")
            expected = {"org.oxibar.preview.0": first, "org.oxibar.preview.1": second}
            assert all(expected[window["app_id"]] in window["outputs"] for window in windows), windows
            assert status()["overview"]["in_flight"] <= 2 and status()["overview"]["staging_bytes"] <= 64*1024*1024
            assert color_count(rgb("OxiBar overview", first), 0) > 500
            assert color_count(rgb("OxiBar overview", second), 2) > 500
            picture("first-monitor", "OxiBar overview", first)
            picture("second-monitor", "OxiBar overview", second)
            for output in (first, second):
                modal = layer("OxiBar overview", output)
                monitor = next(value for value in hypr("monitors") if value["name"] == output)
                assert modal["w"] == 1400 and modal["h"] == 900, modal
                assert abs(modal["x"] - monitor["x"] - (monitor["width"] - modal["w"]) / 2) < 2, modal
                assert abs(modal["y"] - monitor["y"] - (monitor["height"] - modal["h"]) / 2) < 2, modal
            active = status()["overview"]["active_output"]
            keyboard("-k", "Tab")
            assert status()["overview"]["active_output"] == active, "Tab must stay within its monitor"
            keyboard("h" if active == second else "l")
            wait(lambda: status()["overview"]["active_output"] != active, "vim spatial monitor navigation")
            if active == second:
                keyboard("l")
            wait(lambda: status()["overview"]["active_output"] == second, "right monitor keyboard ownership")
            cli("config", "set", "shortcuts.overview.monitor_left", '"Ctrl+h"')
            keyboard("h")
            assert status()["overview"]["active_output"] == second, "old shortcut must stop working"
            keyboard("-M", "ctrl", "h", "-m", "ctrl")
            wait(lambda: status()["overview"]["active_output"] == first, "live shortcut remap")
            keyboard("l")
            wait(lambda: status()["overview"]["active_output"] == second, "back to activation target")
            keyboard("-k", "Return")
            wait(lambda: not status()["overview"]["open"] and hypr("activewindow").get("class") == "org.oxibar.preview.1", "exact keyboard activation after overlay retirement")
            cli("overview", "open")
            wait(ready_windows, "reopened previews")
            click("OxiBar overview", first, 245, 225)
            wait(lambda: not status()["overview"]["open"] and hypr("activewindow").get("class") == "org.oxibar.preview.0", "exact mouse activation on another monitor")
            cli("overview", "open")
            wait(ready_windows, "live previews")
            (root / "change").touch()
            wait(lambda: color_count(rgb("OxiBar overview", first), 2) > 500 and color_count(rgb("OxiBar overview", second), 0) > 500, "fresh changed window pixels")
            subprocess.run(["hyprctl", "output", "remove", second], env=env, check=True, stdout=subprocess.DEVNULL)
            wait(lambda: second not in status()["overview"]["outputs"], "output retirement")
            keyboard("-k", "Escape")
            wait(lambda: not status()["overview"]["open"], "scoped Escape")
            memory = []
            for _ in range(10):
                cli("overview", "open")
                wait(ready_windows, "reopen bounded owner")
                assert capture_workers() == 1
                cli("overview", "close")
                wait(lambda: not status()["overview"]["open"] and not any(value["namespace"] == "OxiBar overview" for value in layers()) and capture_workers() == 0, "owned capture cleanup")
                memory.append(process_sample(daemon.pid))
            assert not status()["overview"]["windows"] and status()["overview"]["staging_bytes"] == 0
            assert memory[-1]["threads"] <= memory[0]["threads"], memory
            print(json.dumps({"real_per_window_pixels": True, "centered_large_modal": True, "per_monitor_grouping": True, "tab_windows_vim_monitors_and_live_remap": True, "exact_keyboard_and_cross_monitor_mouse_activation": True, "live_changed_pixels": True, "hotplug": True, "ten_owned_reopens": True, "svg_dock_icon_and_portable_fallback": True, "capture_workers_after_close": capture_workers(), "closed_rss_kib": [sample["rss_kib"] for sample in memory], "closed_threads": [sample["threads"] for sample in memory]}))
        finally:
            (root / "stop").touch()
            stop(daemon)
            stop(fixture)


if __name__ == "__main__":
    main()
