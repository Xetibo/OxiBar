"""Run verification in owned compositors and a private bus, never the desktop session."""
import collections
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time


def stop(process):
    if process and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def collect(process, logs):
    def drain():
        for line in process.stdout:
            logs.append(line)
    threading.Thread(target=drain, daemon=True).start()


def main():
    arguments = sys.argv[1:]
    parent_only = arguments[:1] == ["--parent"]
    if parent_only:
        arguments = arguments[1:]
    if arguments[:1] == ["--"]:
        arguments = arguments[1:]
    if not arguments:
        raise SystemExit("usage: python scripts/run-isolated.py -- <command> [arguments]")
    if os.environ.get("OXIBAR_TEST_ISOLATED_BUS") != "1":
        env = dict(os.environ, OXIBAR_TEST_ISOLATED_BUS="1")
        config = str(Path(__file__).with_name("private-bus.conf"))
        result = subprocess.run(["dbus-run-session", "--config-file="+config, "--", sys.executable, str(Path(__file__).resolve()), *(["--parent"] if parent_only else []), "--", *arguments], env=env)
        return result.returncode
    with tempfile.TemporaryDirectory(prefix="o-", dir="/tmp") as temporary:
        root = Path(temporary)
        binary = os.environ.get("OXIBAR_TEST_HYPRLAND", "Hyprland")
        version_text = subprocess.check_output([binary, "--version"], text=True)
        modern = "0.56" in version_text
        config = root / ("hyprland.lua" if modern else "hyprland.conf")
        config.write_text('hl.monitor({output="",mode="1920x1080@60",scale="1"})\n' if modern else 'monitor=,1920x1080@60,auto,1\ndebug:disable_logs=false\nmisc:disable_hyprland_logo=true\n')
        home = root / "home"
        home.mkdir()
        env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home/".config"), XDG_DATA_HOME=str(home/".local/share"), XDG_STATE_HOME=str(home/".local/state"), XDG_RUNTIME_DIR=str(root), XDG_CACHE_HOME=str(root/"cache"), HYPRLAND_NO_SD_VARS="1", HYPRLAND_NO_SD_NOTIFY="1", OXIBAR_TEST_ISOLATED="1", OXIBAR_TEST_PRIVATE_BUS="1", OXIBAR_TEST_PRIVATE_WAYLAND="1")
        env["DBUS_SYSTEM_BUS_ADDRESS"] = env["DBUS_SESSION_BUS_ADDRESS"]
        env.pop("HYPRLAND_INSTANCE_SIGNATURE", None)
        env.pop("WAYLAND_SOCKET", None)
        parent_config = root / "sway.conf"
        parent_config.write_text('output * mode 1920x1080\noutput * scale 1\nxwayland disable\n')
        parent_env = dict(env, WLR_BACKENDS="headless", WLR_RENDERER="gles2", WLR_LIBINPUT_NO_DEVICES="1")
        parent = compositor = None
        parent_logs = collections.deque(maxlen=100)
        logs = collections.deque(maxlen=100)
        try:
            parent = subprocess.Popen(["sway", "--unsupported-gpu", "--config", str(parent_config)], env=parent_env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            collect(parent, parent_logs)
            deadline = time.monotonic() + 10
            while True:
                displays = [path for path in root.glob("wayland-*") if path.is_socket()]
                if displays:
                    parent_display = displays[0]
                    break
                if parent.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError("private parent did not start\n"+"".join(parent_logs))
                time.sleep(0.025)
            env["WAYLAND_DISPLAY"] = parent_display.name
            env["OXIBAR_TEST_PARENT_WAYLAND"] = parent_display.name
            if parent_only:
                env["OXIBAR_TEST_COMPOSITOR"] = "Sway"
                env["OXIBAR_TEST_COMPOSITOR_VERSION"] = subprocess.check_output(["sway", "--version"], text=True).strip()
                print(json.dumps({"isolated_compositor": parent.pid, "version": env["OXIBAR_TEST_COMPOSITOR_VERSION"], "backend": "headless GLES2"}), flush=True)
                return subprocess.run(arguments, env=env).returncode
            runtime = root / "runtime"
            runtime.mkdir(mode=0o700)
            env["XDG_RUNTIME_DIR"] = str(runtime)
            env["WAYLAND_DISPLAY"] = str(parent_display)
            env["OXIBAR_TEST_PARENT_WAYLAND"] = str(parent_display)
            compositor = subprocess.Popen([binary, "--config", str(config)], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            collect(compositor, logs)
            deadline = time.monotonic() + 10
            while True:
                sockets = list((runtime/"hypr").glob("*/.socket.sock"))
                displays = [path for path in runtime.glob("wayland-*") if path.is_socket()]
                if sockets and displays:
                    env["HYPRLAND_INSTANCE_SIGNATURE"] = sockets[0].parent.name
                    env["WAYLAND_DISPLAY"] = displays[0].name
                    break
                if compositor.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError("private compositor did not start\n"+"".join(logs))
                time.sleep(0.025)
            subprocess.run(["hyprctl", "output", "create", "headless", "OXIBAR-REFERENCE"], env=env, check=True, capture_output=True)
            version = json.loads(subprocess.check_output(["hyprctl", "-j", "version"], env=env))
            print(json.dumps({"isolated_compositor": compositor.pid, "version": version.get("tag"), "parent": "Sway headless GLES2"}), flush=True)
            return subprocess.run(arguments, env=env).returncode
        finally:
            stop(compositor)
            stop(parent)


if __name__ == "__main__":
    raise SystemExit(main())
