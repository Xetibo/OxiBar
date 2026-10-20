"""Verify real clipboard ownership and paste focus on an owned Wayland session."""
import argparse
import asyncio
import collections
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import struct
import sys
import tempfile
import threading
import time
import tty
import zlib


def receive(root):
    tty.setraw(sys.stdin.fileno())
    os.write(sys.stdout.fileno(), b"\x1b[?2004h")
    (root / "receiver-ready").touch()
    with (root / "received").open("ab", buffering=0) as output:
        while data := os.read(sys.stdin.fileno(), 4096):
            output.write(data)


async def main():
    from dbus_next import Message, MessageType
    from dbus_next.aio import MessageBus

    parser = argparse.ArgumentParser()
    parser.add_argument("oxibar")
    parser.add_argument("plugin")
    parser.add_argument("oxipaste_directory", nargs="?", help="existing OxiPaste binaries for the external-daemon compatibility scenario")
    parser.add_argument("--screenshots")
    parser.add_argument("--trace-focus", action="store_true")
    args = parser.parse_args()
    if os.environ.get("OXIBAR_TEST_ISOLATED") != "1" or not os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"):
        raise SystemExit("Run through scripts/run-isolated.py, nested Hyprland mode")
    scripts = Path(__file__).resolve().parent
    binaries = Path(args.oxipaste_directory).resolve() if args.oxipaste_directory else None
    host = str(Path(args.oxibar).resolve())
    processes = []
    logs = collections.deque(maxlen=300)
    with tempfile.TemporaryDirectory(prefix="oxibar-clipboard-") as temporary:
        root = Path(temporary)
        config = root / "config"
        (config / "oxibar" / "plugins").mkdir(parents=True)
        (config / "oxipaste").mkdir()
        (config / "oxibar" / "plugins" / "liboxipaste.so").symlink_to(Path(args.plugin).resolve())
        (config / "oxipaste" / "config.toml").write_text('keepOpen=false\npasteShortcut="ctrl+v"\nmax_items=100\n')
        shim = root / "helpers"
        shim.mkdir()
        failure = root / "fail-copy"
        copy = shim / "wl-copy"
        copy.write_text(f"#!/bin/sh\nif test -e {shlex.quote(str(failure))}; then exit 7; fi\nexec {shlex.quote(shutil.which('wl-copy'))} \"$@\"\n")
        copy.chmod(0o700)
        env = dict(os.environ, HOME=str(root), XDG_CONFIG_HOME=str(config), XDG_DATA_HOME=str(root / "data"), XDG_STATE_HOME=str(root / "state"), XDG_DATA_DIRS=str(root / "empty"), RUST_BACKTRACE="1")
        bus = await MessageBus().connect()

        def run(*command, **options):
            options.setdefault("check", True)
            return subprocess.run(command, env=env, timeout=12, capture_output=True, **options)

        def start(*command, child_env=None):
            process = subprocess.Popen(command, env=child_env or env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            processes.append(process)
            def drain():
                for line in process.stdout:
                    if child_env and child_env.get("WAYLAND_DEBUG") and not any(word in line for word in ["wl_keyboard", "set_keyboard_interactivity", "keyboard focus", "releasing keyboard", "WARN", "ERROR"]):
                        continue
                    logs.append(line)
            threading.Thread(target=drain, daemon=True).start()
            return process

        def stop(process):
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=6)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()

        async def wait(predicate, label):
            deadline = time.monotonic() + 12
            while time.monotonic() < deadline:
                try:
                    value = predicate()
                    if asyncio.iscoroutine(value):
                        value = await value
                    if value:
                        return value
                except (OSError, ValueError, subprocess.SubprocessError):
                    pass
                await asyncio.sleep(.025)
            try:
                state = json.dumps(status()) + "\nreceived=" + repr(received())
            except (OSError, ValueError, subprocess.SubprocessError):
                state = "host unavailable"
            raise AssertionError(label + "\n" + state + "\n" + "".join(logs))

        async def call(member, signature="", body=None, failed=False):
            reply = await asyncio.wait_for(bus.call(Message(destination="org.Xetibo.OxiPasteDaemon", path="/org/Xetibo/OxiPasteDaemon", interface="org.Xetibo.OxiPasteDaemon", member=member, signature=signature, body=body or [])), 10)
            if failed:
                assert reply.message_type == MessageType.ERROR, reply.body
            elif reply.message_type != MessageType.METHOD_RETURN:
                raise ValueError(str(reply.body))
            return reply.body

        async def history():
            return (await call("GetAll"))[0]

        async def contains(payload):
            return any(bytes(entry[0]) == payload for entry in await history())

        def status():
            return json.loads(run(host, "status").stdout)["data"]

        def hypr(topic):
            return json.loads(run("hyprctl", "-j", topic).stdout)

        def layers(process):
            return [dict(layer, output=output) for output, groups in hypr("layers").items() for group in groups["levels"].values() for layer in group if layer.get("pid") == process.pid]

        async def layer(process, namespace):
            return await wait(lambda: next((value for value in layers(process) if value["namespace"] == namespace), None), namespace)

        def received():
            path = root / "received"
            return path.read_bytes().replace(b"\x1b[200~", b"").replace(b"\x1b[201~", b"") if path.exists() else b""

        pointer = root / "pointer"
        xml = os.environ["OXIBAR_TEST_VIRTUAL_POINTER_XML"]
        run("wayland-scanner", "client-header", xml, str(root / "virtual-pointer.h"))
        run("wayland-scanner", "private-code", xml, str(root / "virtual-pointer.c"))
        flags = shlex.split(run("pkg-config", "--cflags", "--libs", "wayland-client", text=True).stdout)
        run("gcc", "-O2", "-I", str(root), str(scripts / "virtual-pointer.c"), str(root / "virtual-pointer.c"), *flags, "-o", str(pointer))

        async def click(process, namespace, x, y):
            selected = await layer(process, namespace)
            monitor = next(value for value in hypr("monitors") if value["name"] == selected["output"])
            run(str(pointer), "0", "0", monitor["name"], str(round(selected["x"] - monitor["x"] + x)), str(round(selected["y"] - monitor["y"] + y)), str(round(monitor["width"] / monitor["scale"])), str(round(monitor["height"] / monitor["scale"])))
            logs.append(f"clicked {namespace} {x},{y}: {selected} cursor={hypr('cursorpos')}\n")

        async def picture(name, process, namespace):
            if args.screenshots:
                target = Path(args.screenshots)
                target.mkdir(parents=True, exist_ok=True)
                selected = await layer(process, namespace)
                run("grim", "-g", f"{selected['x']},{selected['y']} {selected['w']}x{selected['h']}", str(target / (name + ".png")))

        async def embedded(bar, target, selected):
            async def owned():
                reply = await bus.call(Message(destination="org.freedesktop.DBus", path="/org/freedesktop/DBus", interface="org.freedesktop.DBus", member="GetConnectionUnixProcessID", signature="s", body=["org.Xetibo.OxiPasteDaemon"]))
                return reply.message_type == MessageType.METHOD_RETURN and reply.body == [bar.pid]

            async def absent():
                reply = await bus.call(Message(destination="org.freedesktop.DBus", path="/org/freedesktop/DBus", interface="org.freedesktop.DBus", member="NameHasOwner", signature="s", body=["org.Xetibo.OxiPasteDaemon"]))
                return reply.message_type == MessageType.METHOD_RETURN and reply.body == [False]

            def copy_payload(payload, mime="text/plain;charset=utf-8"):
                subprocess.run(["wl-copy", "--type", mime], env=env, input=payload, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=12, check=True)

            await wait(lambda: call("GetAll"), "bundled backend ready without external daemon")
            assert await owned(), "Oxibar itself must own clipboard backend"
            assert not status()["oxipaste_open"] and status()["active_subscriptions"] == ["OxiPaste"]
            watched = b"watch original bytes\n\n"
            copy_payload(watched)
            await wait(lambda: contains(watched), "capture while all clipboard views closed")
            copy_payload(selected)
            await wait(lambda: contains(selected), "capture exact text and trailing newline")
            png = bytes.fromhex("89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000b49444154789c636000020000050001a5f645400000000049454e44ae426082")
            # A valid ancillary chunk makes this transfer much larger than a pipe buffer.
            chunk = b"tEXt" + b"Comment\0" + b"clipboard image transfer " * 16384
            png = png[:-12] + struct.pack(">I", len(chunk) - 4) + chunk + struct.pack(">I", zlib.crc32(chunk)) + png[-12:]
            copy_payload(png, "image/png")
            await wait(lambda: contains(png), "capture binary image with original MIME")
            assert any(bytes(item[0]) == png and item[1] == "image/png" for item in await history())
            await call("CopyContent", "ays", [png, "image/png"])
            assert run("wl-paste", "--no-newline", "--type", "image/png").stdout == png
            before = await history()
            await call("CopyContent", "ays", [selected, "text/plain;charset=utf-8"])
            assert run("wl-paste", "--no-newline").stdout == selected
            await asyncio.sleep(.2)
            assert await history() == before, "our writes must not reorder or duplicate history"
            await call("Copy")
            assert bytes((await history())[0][0]) == selected, "legacy Copy records actual current selection"

            await call("Capture", "ays", [b"newer entry", "text/plain;charset=utf-8"])
            await call("DeleteContent", "ays", [watched, "text/plain;charset=utf-8"])
            assert not await contains(watched) and await contains(selected), "delete addresses displayed payload, not shifted index"
            await call("ShutDown", failed=True)
            assert await owned(), "legacy daemon shutdown must not kill Oxibar"
            run(host, "config", "set", "bar.enabled", "false")
            await wait(lambda: not status()["bars"], "capture backend outlives hidden bar")
            sentinel = b"hidden bar sentinel"
            copy_payload(sentinel)
            await wait(lambda: contains(sentinel), "background capture with bar disabled")
            run("hyprctl", "dispatch", "focuswindow", "address:" + target["address"])
            run(host, "oxipaste", "open")
            await layer(bar, "OxiBar clipboard")
            await asyncio.sleep(.2)
            run("wtype", "-s", "500", "-d", "10", "clipboard selected")
            await asyncio.sleep(.2)
            await picture("clipboard-embedded-search", bar, "OxiBar clipboard")
            await click(bar, "OxiBar clipboard", 441, 193)
            await wait(lambda: run("wl-paste", "--no-newline").stdout == selected, "bundled backend Copy writes selected bytes")
            assert status()["oxipaste_open"], "Copy must keep picker open"
            assert received() == b"", "Copy must not type: " + repr(received())
            await click(bar, "OxiBar clipboard", 160, 190)
            await wait(lambda: received() == selected, "bundled backend paste releases focus before typing")
            await wait(lambda: not status()["oxipaste_open"], "successful paste closes picker")
            await wait(lambda: hypr("activewindow").get("address") == target["address"], "application regains focus")

            run(host, "config", "set", "plugins", "[]")
            await wait(absent, "disabling plugin releases backend bus name")
            disabled = b"copied while disabled"
            copy_payload(disabled)
            path = config / "oxipaste" / "items"
            persisted = path.read_bytes()
            assert path.stat().st_mode & 0o777 == 0o600
            copy_payload(b"current selection on reenable")
            assert path.read_bytes() == persisted, "disabled backend must not capture clipboard"
            run(host, "config", "set", "plugins", '["liboxipaste.so"]')
            await wait(owned, "reenabling plugin starts one backend again")
            await wait(lambda: contains(b"current selection on reenable"), "reenabled backend captures current selection")
            assert await contains(selected) and await contains(png)
            assert not await contains(disabled), "disabled interval must not be recorded"
            await call("CopyContent", "ays", [selected, "text/plain;charset=utf-8"])
            await call("PasteAndDelete", "u", [0])
            await asyncio.sleep(.2)
            assert not await contains(b"current selection on reenable"), "acknowledged copy-and-delete must not recapture own selection"
            stop(bar)
            await wait(absent, "stopping Oxibar releases integrated backend")
            bar = start(host)
            await wait(owned, "restarted Oxibar restores integrated backend")
            await wait(lambda: contains(png), "binary history survives host restart")
            assert await contains(selected)
            await call("DeleteAll")
            assert await history() == []
            print(json.dumps(dict(plugin_owned_backend=True, hidden_view_capture=True, hidden_bar_capture=True, exact_text_and_newlines=True, binary_mime_capture_and_copy=True, copy_keeps_open=True, native_focus_handoff=True, content_addressed_delete=True, private_persistent_history=True, disable_stops_capture=True, reenable_and_restart=True, own_copy_not_recaptured=True)), flush=True)

        try:
            selected = b"clipboard selected\nline two\n"
            if binaries:
                daemon_env = dict(env, PATH=str(shim) + os.pathsep + env["PATH"])
                daemon = start(str(binaries / "oxipaste_daemon"), child_env=daemon_env)
                await wait(lambda: call("GetAll"), "clipboard daemon ready")
                watched = b"watch original bytes\n\n"
                subprocess.run(["wl-copy", "--type", "text/plain;charset=utf-8"], env=env, input=watched, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=12, check=True)
                await wait(lambda: contains(watched), "watcher captures original bytes and trailing newlines")
                await call("DeleteAll")
                await call("Capture", "ays", [selected, "text/plain;charset=utf-8"])
            kitty_config = root / "kitty.conf"
            kitty_config.write_text("map ctrl+v paste_from_clipboard\nmap ctrl+shift+v paste_from_clipboard\nconfirm_os_window_close 0\n")
            terminal = start("kitty", "--config", str(kitty_config), "--class", "oxibar-clipboard-target", sys.executable, str(Path(__file__).resolve()), "receive", str(root))
            await wait(lambda: (root / "receiver-ready").exists(), "owned receiver ready")
            target = await wait(lambda: next((value for value in hypr("clients") if value["class"] == "oxibar-clipboard-target"), None), "owned receiver window")
            # Keep native keyboard capability present between short-lived input helpers.
            keyboard = start("wtype", "-s", "120000")
            output = next(value["name"] for value in hypr("monitors") if value["id"] == target["monitor"])
            (config / "oxibar" / "config.toml").write_text(f'''plugins=["liboxipaste.so"]
[bar]
outputs=[{json.dumps(output)}]
start=["OxiPaste"]
center=[]
end=[]
[dock]
enabled=false
[instance]
lock_path={json.dumps(str(root / 'lock'))}
[ipc]
socket_path={json.dumps(str(root / 'socket'))}
[launcher]
default_plugins=[]
[control_center]
widgets=["OxiPaste"]
[monitor]
apply_on_start=false
[theme]
animation_ms=0
[oxipaste]
poll_seconds=3600
paste_shortcut="ctrl+v"
''')
            bar_env = dict(env, RUST_LOG="warn,oxibar::paste=debug")
            if args.trace_focus:
                bar_env["WAYLAND_DEBUG"] = "1"
            bar = start(host, child_env=bar_env)
            await wait(lambda: status()["bar_buttons"], "clipboard bar ready")
            if not binaries:
                await embedded(bar, target, selected)
                return
            await asyncio.sleep(.3)
            await picture("clipboard-bar", bar, "OxiBar")

            async def open_popup():
                run("hyprctl", "dispatch", "focuswindow", "address:" + target["address"])
                button = next(value for value in status()["bar_buttons"] if value["provider"] == "OxiPaste")
                await click(bar, "OxiBar", button["x"] + button["width"] / 2, button["y"] + button["height"] / 2)
                await wait(lambda: status()["popup_open"], "clipboard dropdown open")
                await asyncio.sleep(.35)
                dropdown = status()["dropdown"]
                return dropdown["x"], 31 + 8

            x, y = await open_popup()
            await picture("clipboard-popup", bar, "OxiBar")
            # Reorder daemon history after the rendered snapshot; selection must stay exact.
            await call("Capture", "ays", [b"newer clipboard entry", "text/plain;charset=utf-8"])
            await click(bar, "OxiBar", x + 457, y + 177)
            await wait(lambda: run("wl-paste", "--no-newline").stdout == selected, "Copy writes selected snapshot despite reordered history")
            assert status()["popup_open"] and received() == b"", "Copy must retain dropdown and not type"
            await picture("clipboard-copy", bar, "OxiBar")
            # Successful Copy refreshes history. Filter the selected item before paste.
            await click(bar, "OxiBar", x + 120, y + 66)
            run("wtype", "clipboard selected")
            await asyncio.sleep(.2)
            await click(bar, "OxiBar", x + 160, y + 176)
            await wait(lambda: received() == selected, "row click pastes exact bytes into focused application")
            assert not status()["popup_open"]
            await wait(lambda: hypr("activewindow").get("address") == target["address"], "keyboard focus returned to application")

            # Native write failures must keep the picker open and preserve history.
            x, y = await open_popup()
            unwrapped = binaries / ".oxipaste_daemon-wrapped"
            if unwrapped.exists():
                # Nix's helper PATH prefix intentionally overrides the failure shim.
                # Verify normal writes through the wrapper, then isolate failed IO.
                await call("ShutDown")
                await wait(lambda: daemon.poll() == 0, "packaged daemon shutdown")
                daemon = start(str(unwrapped), child_env=daemon_env)
                await wait(lambda: call("GetAll"), "failure-injectable daemon ready")
            before = await history()
            failure.touch()
            await click(bar, "OxiBar", x + 160, y + 176)
            await asyncio.sleep(.3)
            assert status()["popup_open"] and received() == selected
            await call("PasteAndDelete", "t", [0], failed=True)
            assert await history() == before, "failed copy must not delete history"
            await picture("clipboard-failure", bar, "OxiBar")
            failure.unlink()
            run("wtype", "-k", "Escape")
            await wait(lambda: not status()["popup_open"], "close failed picker")

            # Exercise the alternate terminal shortcut through the real host path.
            run(host, "config", "set", "oxipaste.paste_shortcut", '"ctrl+shift+v"')
            stop(bar)
            bar = start(host, child_env=bar_env)
            await wait(lambda: status()["bar_buttons"], "clipboard bar restarted")
            x, y = await open_popup()
            await click(bar, "OxiBar", x + 120, y + 66)
            run("wtype", "-s", "500", "-d", "10", "clipboard selected")
            await asyncio.sleep(.2)
            await picture("clipboard-shiftv", bar, "OxiBar")
            # Device removal must publish focus loss even without keyboard Leave.
            stop(keyboard)
            await asyncio.sleep(.15)
            await click(bar, "OxiBar", x + 160, y + 176)
            await wait(lambda: received() == selected * 2, "Ctrl+Shift+V pastes into owned terminal")

            # Shell commands own a centered picker, independent of a visible bar.
            start("wtype", "-s", "120000")
            run(host, "oxipaste")
            await wait(lambda: status()["oxipaste_open"], "bare clipboard command opens picker")
            picker = await layer(bar, "OxiBar clipboard")
            monitor = next(value for value in hypr("monitors") if value["name"] == picker["output"])
            assert status()["oxipaste_output"] == output
            assert abs(picker["x"] - monitor["x"] - (monitor["width"] / monitor["scale"] - picker["w"]) / 2) <= 1
            assert abs(picker["y"] - monitor["y"] - (monitor["height"] / monitor["scale"] - picker["h"]) / 2) <= 1
            run(host, "oxipaste", "open")
            assert (await layer(bar, "OxiBar clipboard"))["address"] == picker["address"], "open must reuse visible picker"
            await asyncio.sleep(.2)
            run("wtype", "missing clipboard query")
            run("wtype", "-k", "Escape")
            await wait(lambda: not status()["oxipaste_open"], "captured search Escape closes picker")
            run(host, "oxipaste", "toggle")
            await layer(bar, "OxiBar clipboard")
            run(host, "launcher", "open")
            await wait(lambda: status()["launcher_open"] and not status()["oxipaste_open"], "launcher dismisses clipboard picker")
            run(host, "oxipaste", "open")
            await wait(lambda: status()["oxipaste_open"] and not status()["launcher_open"], "clipboard picker dismisses launcher")
            run(host, "oxipaste")
            await wait(lambda: not status()["oxipaste_open"], "bare clipboard command closes picker")
            run(host, "oxipaste", "close")
            run(host, "oxipaste", "close")

            run(host, "config", "set", "bar.enabled", "false")
            await wait(lambda: not status()["bars"], "bar hidden before clipboard command")
            sentinel = b"command copy sentinel"
            await call("Capture", "ays", [sentinel, "text/plain;charset=utf-8"])
            await call("CopyContent", "ays", [sentinel, "text/plain;charset=utf-8"])
            run("hyprctl", "dispatch", "focuswindow", "address:" + target["address"])
            run(host, "oxipaste", "open")
            await layer(bar, "OxiBar clipboard")
            await asyncio.sleep(.2)
            run("wtype", "-M", "ctrl", "-k", "a", "-m", "ctrl", "-k", "BackSpace", "clipboard selected")
            await asyncio.sleep(.2)
            await picture("clipboard-command-search", bar, "OxiBar clipboard")
            await click(bar, "OxiBar clipboard", 441, 193)
            await wait(lambda: run("wl-paste", "--no-newline").stdout == selected, "command picker Copy selects filtered history")
            assert status()["oxipaste_open"] and received() == selected * 2
            await asyncio.sleep(.2)
            await picture("clipboard-command-copy", bar, "OxiBar clipboard")
            await click(bar, "OxiBar clipboard", 160, 190)
            await wait(lambda: received() == selected * 3, "command picker row pastes into owned terminal with bar hidden")
            await wait(lambda: not status()["oxipaste_open"] and not any(value["namespace"] == "OxiBar clipboard" for value in layers(bar)), "native clipboard picker closes after paste")
            await wait(lambda: hypr("activewindow").get("address") == target["address"], "command paste returns application focus")
            run(host, "oxipaste", "open")
            await layer(bar, "OxiBar clipboard")
            run(host, "config", "set", "plugins", "[]")
            await wait(lambda: not status()["oxipaste_open"], "disabling provider retires command picker")
            missing = run(host, "oxipaste", "open", check=False)
            assert missing.returncode != 0 and b"not enabled" in missing.stderr, missing
            run(host, "oxipaste", "close")
            stop(bar)

            # Standalone Copy stays open; Enter pastes only after native focus release.
            start("wtype", "-s", "120000")
            run("hyprctl", "dispatch", "focuswindow", "address:" + target["address"])
            ui = start(str(binaries / "oxipaste"))
            await layer(ui, "OxiPaste")
            await asyncio.sleep(.35)
            await picture("clipboard-standalone", ui, "OxiPaste")
            await click(ui, "OxiPaste", 378, 236)
            await asyncio.sleep(.2)
            assert ui.poll() is None and received() == selected * 3
            assert run("wl-paste", "--no-newline").stdout == selected
            run("wtype", "-k", "Return")
            await wait(lambda: received() == selected * 4, "standalone Enter pastes into focused application")
            await wait(lambda: ui.poll() == 0, "standalone closes only after successful paste")

            binary = b"\x00\xffbinary<>:<>\n"
            await call("Capture", "ays", [binary, "application/octet-stream"])
            await call("CopyContent", "ays", [binary, "application/octet-stream"])
            assert run("wl-paste", "--no-newline", "--type", "application/octet-stream").stdout == binary
            await call("ShutDown")
            await wait(lambda: daemon.poll() == 0, "clean daemon shutdown")
            assert (config / "oxipaste" / "items").stat().st_mode & 0o777 == 0o600
            daemon = start(str(binaries / "oxipaste_daemon"), child_env=daemon_env)
            await wait(lambda: contains(binary), "binary history survives daemon restart")
            assert await contains(selected)
            print(json.dumps(dict(original_watch_payload=True, trailing_newlines=True, content_addressed_copy=True, copy_keeps_open=True, native_focus_handoff=True, keyboard_device_loss=True, row_click_paste=True, terminal_shortcut=True, shell_clipboard_commands=True, clipboard_without_bar=True, failed_copy_preserves_picker_and_history=True, standalone_copy_and_paste=True, binary_copy_and_private_persistence=True)), flush=True)
        finally:
            bus.disconnect()
            for process in reversed(processes):
                stop(process)


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "receive":
        receive(Path(sys.argv[2]))
    else:
        asyncio.run(main())
