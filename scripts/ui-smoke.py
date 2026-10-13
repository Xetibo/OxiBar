"""Menus, toast placement, edge reveal and native controls on owned fixtures."""
import argparse
import asyncio
import collections
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import threading
import time


async def session_fixture(root):
    from dbus_next.aio import MessageBus
    from dbus_next.constants import PropertyAccess
    from dbus_next.service import ServiceInterface, dbus_property, method
    from dbus_next import Variant

    class Manager(ServiceInterface):
        def __init__(self): super().__init__("org.freedesktop.login1.Manager")
        @method()
        def GetSession(self, session: 's') -> 'o': return "/org/freedesktop/login1/session/fixture"
        @method(name="GetSessionByPID")
        def get_session_by_pid(self, pid: 'u') -> 'o': return "/org/freedesktop/login1/session/fixture"
        @method()
        def CanSuspend(self) -> 's': return "yes"
        @method()
        def CanHibernate(self) -> 's': return "no"
        @method()
        def CanReboot(self) -> 's': return "yes"
        @method()
        def CanPowerOff(self) -> 's': return "yes"

    class Session(ServiceInterface):
        def __init__(self): super().__init__("org.freedesktop.login1.Session")
        @dbus_property(access=PropertyAccess.READ)
        def Id(self) -> 's': return "fixture"
        @dbus_property(access=PropertyAccess.READ)
        def User(self) -> '(uo)': return [os.getuid(), "/org/freedesktop/login1/user/fixture"]
        @method()
        def Terminate(self): (root / "logout-called").touch()

    class Profiles(ServiceInterface):
        def __init__(self):
            super().__init__("org.freedesktop.UPower.PowerProfiles")
            self.active = "balanced"
        @dbus_property(access=PropertyAccess.READ)
        def Profiles(self) -> 'aa{sv}': return [{"Profile": Variant('s', value)} for value in ["power-saver", "balanced", "performance"]]
        @dbus_property()
        def ActiveProfile(self) -> 's': return self.active
        @ActiveProfile.setter
        def ActiveProfile(self, value: 's'): self.active = value
        @dbus_property(access=PropertyAccess.READ)
        def PerformanceDegraded(self) -> 's': return ""

    class MediaRoot(ServiceInterface):
        def __init__(self): super().__init__("org.mpris.MediaPlayer2")
        @dbus_property(access=PropertyAccess.READ)
        def Identity(self) -> 's': return "Oxibar private player"

    class MediaPlayer(ServiceInterface):
        def __init__(self):
            super().__init__("org.mpris.MediaPlayer2.Player")
            self.status = "Playing"
            self.volume = .6
        @dbus_property(access=PropertyAccess.READ)
        def PlaybackStatus(self) -> 's': return self.status
        @dbus_property()
        def Volume(self) -> 'd': return self.volume
        @Volume.setter
        def Volume(self, value: 'd'): self.volume = value
        @dbus_property(access=PropertyAccess.READ)
        def Metadata(self) -> 'a{sv}': return {"xesam:title": Variant('s', "Private media fixture"), "xesam:artist": Variant('as', ["Oxibar test"])}
        @method()
        def PlayPause(self):
            self.status = "Paused" if self.status == "Playing" else "Playing"
            self.emit_properties_changed({"PlaybackStatus": self.status})
        @method()
        def Next(self): pass
        @method()
        def Previous(self): pass

    bus = await MessageBus().connect()
    bus.export("/org/freedesktop/login1", Manager())
    bus.export("/org/freedesktop/login1/session/fixture", Session())
    bus.export("/org/freedesktop/UPower/PowerProfiles", Profiles())
    bus.export("/org/mpris/MediaPlayer2", MediaRoot())
    bus.export("/org/mpris/MediaPlayer2", MediaPlayer())
    await bus.request_name("org.freedesktop.login1")
    await bus.request_name("org.freedesktop.UPower.PowerProfiles")
    await bus.request_name("org.mpris.MediaPlayer2.oxibar_fixture")
    (root / "session-ready").touch()
    await asyncio.Event().wait()


async def notify():
    from dbus_next import Message, MessageType
    from dbus_next.aio import MessageBus

    bus = await MessageBus().connect()
    try:
        reply = await asyncio.wait_for(bus.call(Message(
            destination="org.freedesktop.Notifications",
            path="/org/freedesktop/Notifications",
            interface="org.freedesktop.Notifications",
            member="Notify",
            signature="susssasa{sv}i",
            body=["Oxibar fixture", 0, "", "Private notification", "Toast placement fixture", [], {}, 60000],
        )), timeout=12)
        assert reply.message_type == MessageType.METHOD_RETURN, reply.body
    finally:
        bus.disconnect()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("libraries", nargs="+")
    parser.add_argument("--screenshots")
    args = parser.parse_args()
    if os.environ.get("OXIBAR_TEST_ISOLATED") != "1" or not os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"):
        raise SystemExit("Run through scripts/run-isolated.py, nested Hyprland mode")
    scripts = Path(__file__).resolve().parent
    with tempfile.TemporaryDirectory(prefix="oxibar-ui-") as temporary:
        root = Path(temporary)
        config = root / "config" / "oxibar"
        (config / "plugins").mkdir(parents=True)
        required = {"libclock.so", "libaudio.so", "libbluetooth.so", "libnotifications.so", "libsystem.so", "libstats.so", "libapplications.so", "libcommands.so", "libtools.so"}
        for library in args.libraries:
            (config / "plugins" / Path(library).name).symlink_to(Path(library).resolve())
        assert required <= {Path(library).name for library in args.libraries}
        initial = sorted(required - {"libapplications.so", "libcommands.so", "libtools.so"})
        base = root / "base.toml"
        base.write_text(f'''plugins={json.dumps(initial)}
[bar]
outputs=["*"]
start=["launcher","settings"]
center=["clock","control-center","session"]
end=["audio","notifications","stats"]
[dock]
enabled=false
[instance]
lock_path={json.dumps(str(root / "lock"))}
[ipc]
socket_path={json.dumps(str(root / "socket"))}
[control_center]
widgets=["Clock","Audio","Bluetooth","Notifications","System","Stats"]
[monitor]
apply_on_start=false
[system]
session_id="fixture"
backlight_root={json.dumps(str(root / "backlight"))}
[tools]
file_roots=[]
clipboard_enabled=false
refresh_seconds=3600
[commands]
command_terminal=false
refresh_seconds=3600
[applications]
refresh_seconds=3600
[notifications]
timeout=3600
[osd]
timeout_ms=10000
''')
        base.chmod(0o444)
        (config / "config.toml").symlink_to(base)
        env = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), XDG_DATA_DIRS=str(root / "empty"), XDG_STATE_HOME=str(root / "state"), PULSE_SERVER="unix:" + str(root / "pulse.sock"), RUST_BACKTRACE=os.environ.get("RUST_BACKTRACE", "1"))
        applications = root / "data" / "applications"
        applications.mkdir(parents=True)
        launcher_action = root / "launcher-action"
        action = 'import json, os; from pathlib import Path; Path(' + repr(str(launcher_action)) + ').write_text(json.dumps(dict(display=os.environ.get("WAYLAND_DISPLAY"), socket=os.environ.get("WAYLAND_SOCKET"))))'
        (applications / "oxibar-private.desktop").write_text(f"[Desktop Entry]\nType=Application\nName=Oxibar private launcher fixture\nExec={shlex.quote(sys.executable)} -c {shlex.quote(action)}\n")
        command_bin = root / "bin"
        command_bin.mkdir()
        command_action = root / "command-action"
        command = command_bin / "oxibar-private-command"
        command.write_text(f"#!{sys.executable}\nimport json, os, sys\nfrom pathlib import Path\nPath({str(command_action)!r}).write_text(json.dumps(dict(argv=sys.argv[1:], display=os.environ.get('WAYLAND_DISPLAY'), socket=os.environ.get('WAYLAND_SOCKET'))))\n")
        command.chmod(0o755)
        env["PATH"] = str(command_bin) + os.pathsep + env["PATH"]
        pointer = root / "pointer"
        xml = os.environ["OXIBAR_TEST_VIRTUAL_POINTER_XML"]
        subprocess.run(["wayland-scanner", "client-header", xml, str(root / "virtual-pointer.h")], check=True)
        subprocess.run(["wayland-scanner", "private-code", xml, str(root / "virtual-pointer.c")], check=True)
        flags = shlex.split(subprocess.check_output(["pkg-config", "--cflags", "--libs", "wayland-client"], text=True))
        subprocess.run(["gcc", "-O2", "-I", str(root), str(scripts / "virtual-pointer.c"), str(root / "virtual-pointer.c"), *flags, "-o", str(pointer)], check=True)
        logs = collections.deque(maxlen=500)
        daemon = service = pulse = hover_pointer = None
        def cli(*arguments, check=True):
            result = subprocess.run([args.binary, *arguments], env=env, text=True, capture_output=True, timeout=12)
            if check and result.returncode:
                raise AssertionError(f"CLI {arguments!r} failed: {result.stdout}{result.stderr}")
            if not check: return result
            return json.loads(result.stdout) if result.stdout.startswith("{") else result
        def status(): return cli("status")["data"]
        def wait(predicate, label):
            deadline = time.monotonic() + 12
            while time.monotonic() < deadline:
                if daemon and daemon.poll() is not None: raise AssertionError("daemon stopped\n" + "".join(logs))
                if any("startup panicked;" in line for line in list(logs)): raise AssertionError("daemon panicked\n" + "".join(logs))
                try:
                    value = predicate()
                    if value: return value
                except (OSError, ValueError, subprocess.SubprocessError): pass
                time.sleep(.025)
            try:
                current = json.dumps(status())
            except (OSError, ValueError, subprocess.SubprocessError) as error:
                current = "status unavailable: " + str(error)
            raise AssertionError(label + "\n" + current + "\n" + "".join(logs))
        def layers():
            raw = json.loads(subprocess.check_output(["hyprctl", "-j", "layers"], env=env))
            return [dict(layer, output=output) for output, groups in raw.items() for group in groups["levels"].values() for layer in group if layer.get("pid") == daemon.pid]
        def layer(namespace, output=None): return wait(lambda: next((value for value in layers() if value["namespace"] == namespace and (output is None or value["output"] == output)), None), namespace)
        def click(namespace, x, y, dx=0, dy=0, output=None, preview=False):
            selected = layer(namespace, output)
            monitors = json.loads(subprocess.check_output(["hyprctl", "-j", "monitors"], env=env))
            monitor = next(value for value in monitors if value["name"] == selected["output"])
            command = [str(pointer), str(dx), str(dy), monitor["name"], str(selected["x"] - monitor["x"] + x), str(selected["y"] - monitor["y"] + y), str(round(monitor["width"] / monitor["scale"])), str(round(monitor["height"] / monitor["scale"]))]
            if preview:
                process = subprocess.Popen(command + ["1000"], env=env)
                try:
                    time.sleep(.65)
                    picture(preview if isinstance(preview, str) else "layout-drag-preview", namespace)
                    assert process.wait(timeout=4) == 0
                finally:
                    stop(process)
            else:
                subprocess.run(command, env=env, check=True)
        def keyboard(*arguments): subprocess.run(["wtype", "-s", "250", *arguments], env=env, check=True)
        def move(output, x, y):
            nonlocal hover_pointer
            stop(hover_pointer)
            monitor = next(value for value in json.loads(subprocess.check_output(["hyprctl", "-j", "monitors"], env=env)) if value["name"] == output)
            hover_pointer = subprocess.Popen([str(pointer), "0", "0", output, str(x), str(y), str(round(monitor["width"] / monitor["scale"])), str(round(monitor["height"] / monitor["scale"])), "move"], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            assert hover_pointer.stdout.readline() == "ready\n"
        def picture(name, namespace="OxiBar settings", whole_output=False):
            if not args.screenshots: return
            time.sleep(.3)
            selected = layer(namespace)
            target = Path(args.screenshots)
            target.mkdir(parents=True, exist_ok=True)
            region = ["-o", selected["output"]] if whole_output else ["-g", f"{selected['x']},{selected['y']} {selected['w']}x{selected['h']}"]
            subprocess.run(["grim", *region, str(target / ("oxibar-ui-" + name + ".png"))], env=env, check=True)
        def drain(process):
            def read():
                for line in process.stdout: logs.append(line)
            threading.Thread(target=read, daemon=True).start()
        def stop(process):
            if process and process.poll() is None:
                process.terminate()
                try: process.wait(timeout=6)
                except subprocess.TimeoutExpired: process.kill(); process.wait()
        def start():
            nonlocal daemon
            daemon = subprocess.Popen([args.binary], env=dict(env, WAYLAND_DISPLAY=""), stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            drain(daemon)
            wait(lambda: cli("status", check=False).returncode == 0, "daemon ready")
            return wait(lambda: status()["bar_buttons"], "actual bar button bounds")[0]["output"]
        def button(provider, output):
            value = wait(lambda: next((value for value in status()["bar_buttons"] if value["output"] == output and value["provider"].lower() == provider.lower()), None), provider)
            click("OxiBar", value["x"] + value["width"] / 2, value["y"] + value["height"] / 2, output=output)
            return value
        def setting(path): return cli("config", "get", path)["data"]["value"]
        def search(value=""):
            keyboard("-M", "ctrl", "-k", "f", "-m", "ctrl")
            keyboard("-M", "ctrl", "-k", "a", "-m", "ctrl", "-k", "BackSpace")
            if value: keyboard(value)
            time.sleep(.2)
        def first_input(value):
            keyboard("-k", "Tab")
            keyboard("-M", "ctrl", "-k", "a", "-m", "ctrl", value)
        def bounds(key):
            return wait(lambda: next((row for row in status()["settings_navigation"]["rows"] if row["page"] == key and row["height"] > 2), None), key)
        def control(key, dx=0, dy=0, preview=False):
            row = bounds(key)
            click("OxiBar settings", row["x"] + row["width"] / 2, row["y"] + row["height"] / 2, dx=dx, dy=dy, preview=preview)
        def page(route):
            roots = dict(appearance="1", bar="2", launcher="3", plugins="4", advanced="5", monitors="6", controls="7", dock="8", desktop="9", theme="0", shortcuts="s")
            parts = route.split("/")
            keyboard("-M", "alt", "-k", roots[parts[0]], "-m", "alt")
            wait(lambda: status()["settings_navigation"]["page"] == parts[0], "settings root " + parts[0])
            time.sleep(.25)
            for index in range(2, len(parts) + 1):
                selected = "/".join(parts[:index])
                control("@page/" + selected)
                wait(lambda: status()["settings_navigation"]["page"] == selected, "settings page " + selected)
                time.sleep(.25)
        def drag_display(name, proposed, preview=False):
            editor = status()["monitor_editor"]
            display = next(value for value in editor["draft"] if value["name"] == name)
            rectangle = next(value for value in editor["diagram"] if value["name"] == name)
            logical_width = round(display["width"] / display["scale"])
            if display["transform"] % 2: logical_width = round(display["height"] / display["scale"])
            scale = rectangle["width"] / logical_width
            click("OxiBar settings", rectangle["x"] + rectangle["width"] / 2, rectangle["y"] + rectangle["height"] / 2, dx=(proposed[0] - display["x"]) * scale, dy=(proposed[1] - display["y"]) * scale, preview=preview)
        def display_position(name):
            display = next(value for value in status()["monitor_editor"]["draft"] if value["name"] == name)
            return [display["x"], display["y"]]
        try:
            service = subprocess.Popen([sys.executable, str(Path(__file__).resolve()), "session-fixture", str(root)], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            drain(service)
            wait(lambda: (root / "session-ready").exists(), "private session manager")
            pulse_env = dict(env, HOME=str(root), XDG_RUNTIME_DIR=str(root), PULSE_RUNTIME_PATH=str(root), PULSE_STATE_PATH=str(root), PULSE_COOKIE=str(root / "cookie"))
            pulse = subprocess.Popen(["pulseaudio", "-n", "--daemonize=no", "--exit-idle-time=-1", "--use-pid-file=no", "--log-target=stderr", "-L", f"module-native-protocol-unix socket={root / 'pulse.sock'} auth-anonymous=1", "-L", "module-null-sink sink_name=oxibar_fixture", "-L", "module-null-source source_name=oxibar_microphone"], env=pulse_env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            drain(pulse)
            wait(lambda: (root / "pulse.sock").exists(), "private audio devices")
            subprocess.run(["hyprctl", "output", "create", "headless", "OXIBAR-SNAP"], env=env, check=True, capture_output=True)
            output = start()
            assert setting("bar.show_labels") is False
            compact_widths = {entry["provider"].lower(): entry["width"] for entry in status()["bar_buttons"] if entry["output"] == output}
            assert compact_widths["settings"] < 55 and compact_widths["stats"] < 55
            picture("icon-only-bar", "OxiBar")
            cli("config", "set", "bar.show_labels", "true")
            wait(lambda: next(entry["width"] for entry in status()["bar_buttons"] if entry["output"] == output and entry["provider"] == "settings") > compact_widths["settings"] + 30 and next(entry["width"] for entry in status()["bar_buttons"] if entry["output"] == output and entry["provider"].lower() == "stats") > compact_widths["stats"] + 30, "live host and plugin bar labels")
            picture("labeled-bar", "OxiBar")
            cli("config", "set", "bar.show_labels", "false")
            wait(lambda: next(entry["width"] for entry in status()["bar_buttons"] if entry["output"] == output and entry["provider"] == "settings") < 55, "restore icon-only bar")
            assert cli("config", "get", "settings.width")["data"]["value"] == 1440
            assert cli("config", "get", "settings.height")["data"]["value"] == 960
            clock = button("Clock", output)
            wait(lambda: status()["control_center_attached"] and status()["control_center_selected"] == "Clock", "clock-attached controls")
            assert all(value["namespace"] != "OxiBar control center" for value in layers())
            dropdown = status()["dropdown"]
            width = next(value["width"] for value in status()["bars"] if value["output"] == output)
            expected = max(0, min(width - dropdown["width"], clock["x"] + clock["width"] / 2 - dropdown["width"] / 2))
            assert abs(dropdown["x"] - expected) < 2, (dropdown, clock)
            picture("clock-controls", "OxiBar")
            keyboard("-M", "alt", "-k", "2", "-m", "alt")
            wait(lambda: status()["control_center_selected"] == "Audio", "left navigation keyboard")
            picture("audio-controls", "OxiBar")
            keyboard("-M", "alt", "-k", str(status()["control_center_widgets"].index("System") + 1), "-m", "alt")
            wait(lambda: status()["control_center_selected"] == "System", "System power controls")
            picture("system-controls", "OxiBar")
            keyboard("-k", "Escape")
            wait(lambda: not status()["popup_open"], "close attached controls")
            button("Audio", output)
            wait(lambda: status()["popup_open"] and status()["popup_provider"] == "Audio", "ordinary audio dropdown")
            assert status()["dropdown"]["height"] == 640
            picture("audio-dropdown", "OxiBar")
            keyboard("-k", "Escape")
            wait(lambda: not status()["popup_open"], "Escape closes ordinary audio dropdown")
            notification = button("Notifications", output)
            wait(lambda: status()["popup_provider"] == "Notifications" and status()["popup_open"], "bar notification inbox")
            assert not status()["panel_open"] and all(value["namespace"] != "OxiBar notification panel" for value in layers())
            assert status()["dropdown"]["x"] > 0
            picture("inbox", "OxiBar")
            keyboard("-k", "Escape")
            button("session", output)
            session = layer("OxiBar session")
            assert session["w"] == 800 and session["h"] == 480
            assert not status()["popup_open"] and status()["session_open"]
            picture("session", "OxiBar session")
            keyboard("-k", "Down", "-k", "Return")
            wait(lambda: status()["modal_provider"] == "System" and status()["modal_open"], "session menu opens native confirmation")
            assert not status()["session_open"]
            assert not (root / "logout-called").exists()
            keyboard("-k", "Escape")
            cli("session", "open")
            session_address = layer("OxiBar session")["address"]
            cli("session", "open")
            assert layer("OxiBar session")["address"] == session_address
            cli("session", "toggle")
            wait(lambda: not status()["session_open"], "session command toggle closes")
            cli("session", "toggle")
            wait(lambda: status()["session_open"], "session command toggle opens")
            cli("session", "close")
            wait(lambda: not status()["session_open"], "session command close")
            button("settings", output)
            selected = layer("OxiBar settings")
            assert selected["w"] == 1440 and selected["h"] == 960
            picture("settings")
            if os.environ.get("OXIBAR_SMOKE_CAPTURE_ONLY") == "1": return
            reset, reload, close = (bounds("@header/" + key) for key in ("reset", "reload", "close"))
            assert reset["x"] + reset["width"] <= reload["x"] and reload["x"] + reload["width"] <= close["x"]
            assert bounds("@footer")["height"] < 40
            cli("config", "set", "theme.animation_ms", "1000")
            initial_height = bounds("appearance/typography")["height"]
            control("appearance")
            wait(lambda: "appearance" not in status()["settings_navigation"]["expanded"], "sidebar collapses")
            picture("sidebar-collapsing")
            time.sleep(1.1)
            wait(lambda: not any(row["page"] == "appearance/typography" and row["height"] > 2 for row in status()["settings_navigation"]["rows"]), "collapsed sidebar finishes")
            picture("sidebar-collapsed")
            control("appearance")
            wait(lambda: bounds("appearance/typography")["height"] >= initial_height - 1, "sidebar expands")
            time.sleep(1.1)
            cli("config", "reset", "theme.animation_ms")
            for motion in ("normal", "reduced", "instant"):
                cli("config", "set", "theme.reduced_motion", "true" if motion == "reduced" else "false")
                cli("config", "set", "theme.animation_ms", "0" if motion == "instant" else "150")
                time.sleep(.3)
                for route in ("dock", "desktop", "theme", "shortcuts", "appearance", "bar", "launcher", "plugins", "advanced", "monitors", "controls", "dock", "dock/placement", "dock/applications", "appearance"):
                    control(route)
                    wait(lambda: status()["settings_navigation"]["page"] == route, f"{motion} sidebar click selects {route}")
                    time.sleep(.3)
            cli("config", "reset", "theme.reduced_motion")
            cli("config", "reset", "theme.animation_ms")
            page("appearance/feedback/notifications")
            picture("nested-notifications")
            settings_geometry = {key: layer("OxiBar settings")[key] for key in ("address", "x", "y", "w", "h")}
            footer_geometry = {key: bounds("@footer")[key] for key in ("x", "y", "width", "height")}
            cli("config", "set", "bar.height", "35")
            control("@header/reset")
            wait(lambda: setting("bar.height") == 31, "header reset removes GUI overrides")
            assert status()["settings_navigation"]["page"] == "appearance/feedback/notifications"
            assert {key: layer("OxiBar settings")[key] for key in settings_geometry} == settings_geometry
            assert {key: bounds("@footer")[key] for key in footer_geometry} == footer_geometry
            picture("nested-notifications-reset")
            cli("config", "set", "theme.reduced_motion", "true")
            page("appearance/feedback/indicators")
            assert status()["theme"]["reduced_motion"]
            cli("config", "reset", "theme.reduced_motion")
            cli("config", "set", "settings.width", "700")
            wait(lambda: layer("OxiBar settings")["w"] == 700, "narrow hierarchical page picker")
            assert status()["settings_navigation"]["page"] == "appearance/feedback/indicators"
            picture("narrow-nested-settings")
            picture("narrow-settings-output", whole_output=True)
            cli("config", "reset", "settings.width")
            wait(lambda: layer("OxiBar settings")["w"] == 1440, "restore settings width")
            time.sleep(.5)
            page("launcher/providers")
            settings_geometry = {key: layer("OxiBar settings")[key] for key in ("address", "x", "y", "w", "h")}
            picture("launcher-preferences")
            control("@launcher/default-providers")
            wait(lambda: setting("launcher.default_plugins") == ["libapplications.so", "libcommands.so", "libtools.so"], "GUI default provider switch")
            assert "launcher.default_plugins" not in status()["restart_required"]
            wait(lambda: {"Applications", "Commands", "Tools"} <= set(status()["loaded_plugins"]), "live default providers")
            assert {key: layer("OxiBar settings")[key] for key in settings_geometry} == settings_geometry
            picture("launcher-preferences-saved")
            keyboard("-k", "Escape")
            button("launcher", output)
            layer("OxiBar launcher")
            keyboard("Oxibar private launcher fixture")
            wait(lambda: any(row["provider"] == "Applications" for row in status()["launcher_results"]), "live application search")
            keyboard("-k", "Return")
            wait(lambda: launcher_action.exists() and not status()["launcher_open"], "owned application activation")
            assert json.loads(launcher_action.read_text()) == dict(display=env["WAYLAND_DISPLAY"], socket=None)
            button("launcher", output)
            layer("OxiBar launcher")
            keyboard('> oxibar-private-command "literal argument" "$HOME;no-shell"')
            wait(lambda: status()["launcher_results"] and all(row["provider"] == "Commands" for row in status()["launcher_results"]), "separate command provider scope")
            keyboard("-k", "Return")
            wait(lambda: command_action.exists() and not status()["launcher_open"], "owned command activation")
            assert json.loads(command_action.read_text()) == dict(argv=["literal argument", "$HOME;no-shell"], display=env["WAYLAND_DISPLAY"], socket=None)
            button("launcher", output)
            layer("OxiBar launcher")
            time.sleep(.4)
            keyboard("= 6*7")
            wait(lambda: any(row["provider"] == "Tools" for row in status()["launcher_results"]), "default calculator provider")
            keyboard("-k", "Return")
            wait(lambda: not status()["launcher_open"], "default provider native action")
            assert subprocess.check_output(["wl-paste", "--no-newline"], env=env, text=True) == "42"
            button("settings", output)
            layer("OxiBar settings")
            time.sleep(.3)
            page("bar/layout")
            picture("layout-before")
            source, target = bounds("@bar/start/0"), bounds("@bar/center/0")
            control("@bar/start/0", dx=target["x"] - source["x"], dy=target["y"] - source["y"], preview=True)
            wait(lambda: setting("bar.center")[:2] == ["launcher", "clock"], "actual inter-section drag")
            assert setting("bar.start") == ["settings"]
            picture("layout-after")
            colors = {"base00": "#1b1d23", "base01": "#20252c", "base05": "#d8dee9", "base0d": "#5e81ac", "base10": "#2e3440", "base12": "#ff8888", "base13": "#ffd080", "base14": "#b7e08d", "base15": "#a3e0e0", "base16": "#8fbcff", "base17": "#c0a0ef"}
            literal = "{" + ",".join(key + "=" + json.dumps(value) for key, value in colors.items()) + "}"
            cli("config", "set", "theme.base24", literal)
            cli("config", "set", "theme.accent", '"#ffc857"')
            wait(lambda: status()["theme"]["primary"].lower() == "ffc857", "Base24 accent")
            assert status()["theme"]["base24"]["base00"].lower() == "1b1d23"
            assert len(status()["theme"]["base24"]) == 24 and not status()["theme"]["failures"]
            search("theme.accent")
            picture("base24-settings")
            keyboard("-k", "Escape")
            button("Clock", output)
            picture("base24-clock", "OxiBar")
            keyboard("-k", "Escape")
            button("settings", output)
            layer("OxiBar settings")
            time.sleep(.3)
            page("monitors/arrangement")
            # Earlier sidebar visits retained a draft across unrelated config writes.
            control("@monitor/refresh")
            wait(lambda: status()["monitor_editor"]["draft"] and not status()["monitor_editor"]["busy"], "native editable monitor inventory")
            wait(lambda: len(status()["monitor_editor"]["diagram"]) >= 2, "two-display snap diagram")
            displays = sorted(status()["monitor_editor"]["draft"], key=lambda value: value["x"])
            peer, moving = displays[:2]
            name = moving["name"]
            peer_width = round(peer["width"] / peer["scale"])
            adjacent = [peer["x"] + peer_width, peer["y"]]
            drag_display(name, [adjacent[0] + 400, adjacent[1]])
            wait(lambda: abs(display_position(name)[0] - adjacent[0] - 400) < 6, "unsnapped distant monitor drag")
            drag_display(name, [adjacent[0] + 80, adjacent[1] + 40], preview="monitor-snap-preview")
            wait(lambda: display_position(name) == adjacent, "nearby monitor edges snap on both axes")
            drag_display(name, [peer["x"] + peer_width // 2, adjacent[1]], preview="monitor-overlap-preview")
            wait(lambda: display_position(name) == adjacent, "overlapping monitor drop restores original")
            search()
            first_input("1.25")
            wait(lambda: status()["monitor_editor"]["draft"][0]["scale"] == 1.25, "GUI monitor scale")
            picture("monitors-before")
            control("@monitor/preview")
            wait(lambda: status()["monitor_editor"]["preview"] is not None, "native monitor preview")
            control("@monitor/keep")
            wait(lambda: status()["monitor_editor"]["preview"] is None and setting("monitor.profiles"), "keep and save native monitor layout")
            profiles = setting("monitor.profiles")
            name = profiles[0]["name"]
            assert profiles[0]["scale"] == 1.25
            assert next(item for item in cli("outputs")["data"]["outputs"] if item["name"] == name)["scale"] == 1.25
            search()
            first_input("1.5")
            control("@monitor/preview")
            wait(lambda: status()["monitor_editor"]["preview"] is not None, "second native preview")
            control("@monitor/revert")
            wait(lambda: status()["monitor_editor"]["preview"] is None and not status()["monitor_editor"]["busy"], "GUI native revert")
            assert next(item for item in cli("outputs")["data"]["outputs"] if item["name"] == name)["scale"] == 1.25
            assert setting("monitor.profiles") == profiles
            search()
            first_input("1")
            control("@monitor/preview")
            wait(lambda: status()["monitor_editor"]["preview"] is not None, "restore fixture scale preview")
            control("@monitor/keep")
            wait(lambda: status()["monitor_editor"]["preview"] is None and setting("monitor.profiles")[0]["scale"] == 1, "restore and save fixture layout")
            picture("monitors")
            keyboard("-k", "Escape")
            cli("config", "set", "bar.position", '"bottom"')
            wait(lambda: next(item for item in status()["bars"] if item["output"] == output)["position"].lower() == "bottom", "bottom bar")
            wait(lambda: any(item["provider"].lower() == "clock" and item["y"] > 31 for item in status()["bar_buttons"]), "bottom button layout settled")
            clock = button("Clock", output)
            wait(lambda: status()["control_center_attached"], "bottom attached controls")
            assert status()["dropdown"]["x"] > 0
            picture("bottom-clock", "OxiBar")
            keyboard("-k", "Escape")
            cli("launcher", "open")
            layer("OxiBar launcher")
            keyboard("-k", "Escape")
            wait(lambda: not status()["launcher_open"], "launcher command and Escape")
            asyncio.run(notify())
            toast = layer("OxiBar toast", output)
            cli("osd", "Audio", "media")
            wait(lambda: layer("OxiBar OSD", output)["y"] >= toast["y"] + toast["h"] + 12, "music stacks below notification")
            media = layer("OxiBar OSD", output)
            assert media["x"] + media["w"] == toast["x"] + toast["w"]
            picture("music-toast", "OxiBar OSD")
            cli("config", "set", "toasts.position", '"bottom-left"')
            wait(lambda: layer("OxiBar toast", output)["x"] == 16 and layer("OxiBar OSD", output)["x"] == 16 and layer("OxiBar OSD", output)["y"] + layer("OxiBar OSD", output)["h"] + 12 == layer("OxiBar toast", output)["y"], "live shared bottom-left toast placement")
            cli("config", "reset", "toasts.position")
            cli("osd", "close")
            assert not (root / "logout-called").exists()
            assert (config / "config.toml").is_symlink() and base.read_text().startswith("plugins=")
            bar_address = layer("OxiBar", output)["address"]
            monitor = next(value for value in json.loads(subprocess.check_output(["hyprctl", "-j", "monitors"], env=env)) if value["name"] == output)
            height = round(monitor["height"] / monitor["scale"])
            width = round(monitor["width"] / monitor["scale"])
            move(output, width // 2, height // 2)
            cli("config", "set", "bar.auto_hide", "true")
            def bar_visible(): return next(value for value in status()["bars"] if value["output"] == output)["visible"]
            wait(lambda: not bar_visible(), "auto-hide begins hidden")
            move(output, width // 2, height - 2)
            wait(bar_visible, "pointer reveals bottom bar")
            button("Audio", output)
            wait(lambda: status()["popup_open"], "revealed bar opens dropdown")
            move(output, 1, height // 2)
            assert bar_visible(), "open dropdown pins bar"
            keyboard("-k", "Escape")
            wait(lambda: not status()["popup_open"] and not bar_visible(), "Escape unpins and hides bar")
            move(output, width // 2, height - 2)
            wait(bar_visible, "bar reveals again")
            move(output, width // 2, height // 2)
            wait(lambda: not bar_visible(), "pointer departure hides bar")
            cli("config", "set", "bar.auto_hide", "false")
            wait(bar_visible, "disabling auto-hide restores bar")
            assert layer("OxiBar", output)["address"] == bar_address
            print(json.dumps(dict(icon_only_bar_and_live_labels=True, oxishut_session_tiles=True, clock_controls_attached=True, left_navigation=True, system_controls_without_duplicate_session=True, button_anchor=True, inbox_in_bar=True, session_confirmation=True, session_commands=True, larger_settings=True, animated_nested_settings=True, sidebar_pointer_selection_all_motion_modes=True, reduced_motion_and_narrow_navigation=True, header_reset_and_compact_stable_footer=True, gui_live_default_providers=True, live_application_search_and_activation=True, separate_command_scope_and_literal_activation=True, actual_three_column_layout_drag=True, base24_accent=True, native_monitor_snapping_and_overlap_restore=True, native_monitors_preview_save_revert=True, bottom_bar_geometry=True, audio_dropdown_escape=True, shared_live_music_notification_placement=True, pointer_reveal_and_dropdown_pinning=True)), flush=True)
        finally:
            stop(hover_pointer)
            stop(daemon)
            stop(service)
            stop(pulse)


if __name__ == "__main__":
    if len(sys.argv) >= 3 and sys.argv[1] == "session-fixture": asyncio.run(session_fixture(Path(sys.argv[2])))
    else: main()
