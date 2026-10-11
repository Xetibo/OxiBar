"""Real shell flows on an owned compositor, bus and file fixtures."""
import argparse
import asyncio
import collections
import datetime
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import threading
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("libraries", nargs="+")
    parser.add_argument("--screenshots")
    args = parser.parse_args()
    if os.environ.get("OXIBAR_TEST_ISOLATED") != "1" or not os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"):
        raise SystemExit("Run through scripts/run-isolated.py (nested Hyprland mode)")
    scripts = Path(__file__).resolve().parent
    fixture = scripts / "fixtures.py"
    with tempfile.TemporaryDirectory(prefix="oxibar-smoke-") as temporary:
        root = Path(temporary)
        config = root / "config" / "oxibar"
        (config / "plugins").mkdir(parents=True)
        for library in args.libraries:
            (config / "plugins" / Path(library).name).symlink_to(Path(library).resolve())
        required = {"libapplications.so", "libtools.so", "libexample.so", "libclock.so", "libnetwork.so", "libnotifications.so", "libtray.so", "libworkspaces.so"}
        assert required <= {Path(library).name for library in args.libraries}, "Supply all eight smoke providers"
        (config/"plugins"/"libcatalog-example.so").symlink_to((config/"plugins"/"libexample.so").resolve())
        data = root / "data" / "applications"
        data.mkdir(parents=True)
        (data/"fixture.desktop").write_text(f"[Desktop Entry]\nType=Application\nName=Release fixture\nExec={shlex.join([sys.executable,str(fixture),'marker',str(root),'application'])}\n")
        shim_directory = root / "bin"
        shim_directory.mkdir()
        for name in ("nmcli", "curl"):
            shim = shim_directory / name
            shim.write_text(f"#!{sys.executable}\nimport runpy,sys\nsys.argv=[{str(fixture)!r},{name!r},{str(root)!r},*sys.argv[1:]]\nrunpy.run_path({str(fixture)!r},run_name='__main__')\n")
            shim.chmod(0o700)
        pointer = root / "pointer"
        xml = os.environ["OXIBAR_TEST_VIRTUAL_POINTER_XML"]
        subprocess.run(["wayland-scanner","client-header",xml,str(root/"virtual-pointer.h")],check=True)
        subprocess.run(["wayland-scanner","private-code",xml,str(root/"virtual-pointer.c")],check=True)
        flags = shlex.split(subprocess.check_output(["pkg-config","--cflags","--libs","wayland-client"],text=True))
        subprocess.run(["gcc","-O2","-I",str(root),str(scripts/"virtual-pointer.c"),str(root/"virtual-pointer.c"),*flags,"-o",str(pointer)],check=True)
        base = root / "base.toml"
        base.write_text(f'''plugins={json.dumps(sorted(required))}
[bar]
outputs=["*"]
start=["launcher","settings","example"]
center=["clock","workspaces"]
end=["network","notifications","tray"]
[dock]
enabled=false
[instance]
lock_path={json.dumps(str(root/"lock"))}
[ipc]
socket_path={json.dumps(str(root/"socket"))}
[control_center]
widgets=["Example","Network","Notifications","Clock"]
[clock]
calendar_command={json.dumps(shlex.join([sys.executable,str(fixture),'marker',str(root),'calendar','{date}']))}
[clock.caldav]
url="https://example.invalid/private-calendar"
username="fixture"
password_env="OXIBAR_FIXTURE_PASSWORD"
[network]
scan_seconds=3600
[notifications]
timeout=3600
[tools]
file_roots=[]
clipboard_enabled=false
refresh_seconds=3600
[applications]
refresh_seconds=3600
''')
        base.chmod(0o444)
        (config/"config.toml").symlink_to(base)
        env = dict(os.environ,XDG_CONFIG_HOME=str(root/"config"),XDG_DATA_HOME=str(root/"data"),XDG_DATA_DIRS=str(root/"empty"),XDG_STATE_HOME=str(root/"state"),OXIBAR_FIXTURE_PASSWORD="private-fixture-password",PATH=str(shim_directory)+os.pathsep+os.environ["PATH"])
        logs = collections.deque(maxlen=100)
        daemon = service = None
        def cli(*arguments,check=True):
            result = subprocess.run([args.binary,*arguments],env=env,text=True,capture_output=True,timeout=12)
            if check: result.check_returncode()
            if not check: return result
            return json.loads(result.stdout) if result.stdout.startswith("{") else result
        def status(): return cli("status")["data"]
        def wait(predicate,label):
            deadline = time.monotonic()+10
            while time.monotonic()<deadline:
                if daemon and daemon.poll() is not None: raise RuntimeError("daemon stopped\n"+"".join(logs))
                try:
                    result = predicate()
                    if result: return result
                except (OSError,ValueError,subprocess.SubprocessError): pass
                time.sleep(.025)
            raise AssertionError(label+"\n"+"".join(logs))
        def layers():
            raw = json.loads(subprocess.check_output(["hyprctl","-j","layers"],env=env))
            return [dict(layer,output=output) for output,groups in raw.items() for group in groups["levels"].values() for layer in group if layer.get("pid")==daemon.pid]
        def layer(namespace,output_name=None): return wait(lambda:next((item for item in layers() if item["namespace"]==namespace and (output_name is None or item["output"]==output_name)),None),namespace)
        def click(namespace,x,y,dx=0,dy=0,output_name=None):
            selected = layer(namespace,output_name)
            monitors = json.loads(subprocess.check_output(["hyprctl","-j","monitors"],env=env))
            output = next(item for item in monitors if item["name"]==selected["output"])
            subprocess.run([str(pointer),str(dx),str(dy),output["name"],str(selected["x"]-output["x"]+x),str(selected["y"]-output["y"]+y),str(round(output["width"]/output["scale"])),str(round(output["height"]/output["scale"]))],env=env,check=True)
        def keyboard(*arguments): subprocess.run(["wtype","-s","250",*arguments],env=env,check=True)
        def picture(name,namespace):
            if not args.screenshots: return
            selected = layer(namespace)
            time.sleep(.3)
            target = Path(args.screenshots)
            target.mkdir(parents=True,exist_ok=True)
            subprocess.run(["grim","-g",f"{selected['x']},{selected['y']} {selected['w']}x{selected['h']}",str(target/("oxibar-release-"+name+".png"))],env=env,check=True)
        async def call(destination,path,interface,member,signature="",body=None):
            from dbus_next import Message,MessageType
            from dbus_next.aio import MessageBus
            bus = await MessageBus().connect()
            try:
                reply = await bus.call(Message(destination=destination,path=path,interface=interface,member=member,signature=signature,body=body or []))
                if reply.message_type==MessageType.ERROR: raise RuntimeError(str(reply.body))
                return reply.body
            finally: bus.disconnect()
        def notify(summary,actions=None,hints=None):
            return asyncio.run(call("org.freedesktop.Notifications","/org/freedesktop/Notifications","org.freedesktop.Notifications","Notify","susssasa{sv}i",["Private release",0,"",summary,"Private body",actions or [],hints or {},0]))[0]
        def notification_call(member,signature="",body=None): return asyncio.run(call("org.freedesktop.Notifications","/org/freedesktop/Notifications","org.freedesktop.Notifications",member,signature,body))
        def stop(process):
            if process and process.poll() is None:
                process.terminate()
                try: process.wait(timeout=6)
                except subprocess.TimeoutExpired: process.kill(); process.wait()
        def drain(process):
            def read():
                for line in process.stdout:
                    logs.append(line)
            threading.Thread(target=read,daemon=True).start()
        try:
            daemon = subprocess.Popen([args.binary],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
            drain(daemon)
            wait(lambda:cli("status",check=False).returncode==0,"daemon ready")
            service = subprocess.Popen([sys.executable,str(fixture),"serve",str(root)],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
            drain(service)
            wait(lambda:(root/"fixture-ready").exists(),"private SNI registration")
            catalog = cli("plugins")["data"]["plugins"]
            extra = next(item for item in catalog if item["filename"]=="libcatalog-example.so")
            assert extra["abi"]==6 and not extra["enabled"] and "desktop" in extra["capabilities"]
            assert set(status()["loaded_plugins"])=={"Applications","Tools","Example","Clock","Network","Notifications","Tray","Workspaces"}
            if len(status()["bars"])<2:
                subprocess.run(["hyprctl","output","create","headless","OXIBAR-SMOKE-SECOND"],env=env,check=True,capture_output=True)
            wait(lambda:len(status()["bars"])>=2,"multiple output bars")
            cli("action","Example","increment")
            cli("action","Example","reset")
            layer("OxiBar modal")
            keyboard("-k","Escape")
            wait(lambda:not status()["modal_open"],"scoped modal Escape")
            cli("action","Example","reset")
            cli("settings","open")
            wait(lambda:status()["settings_open"] and not status()["modal_open"],"modal replacement cleanup")
            cli("config","set","osd.timeout_ms","6000")
            cli("action","Example","osd")
            layer("OxiBar OSD")
            keyboard("-M","ctrl","-k","f","-m","ctrl","theme.density")
            time.sleep(.15)
            keyboard("-k","Tab")
            time.sleep(.1)
            keyboard("-M","ctrl","-k","a","-m","ctrl","1.2","-k","Return")
            wait(lambda:cli("config","get","theme.density")["data"]["value"]==1.2,"settings input while noninteractive OSD is visible")
            picture("input-through-osd","OxiBar settings")
            cli("config","reset","theme.density")
            cli("config","reset","osd.timeout_ms")
            cli("settings","close")
            cli("osd","close")
            cli("config","set","example.label",'"Live SDK"')
            assert cli("config","set","example.label",'""',check=False).returncode!=0
            cli("launcher","open")
            layer("OxiBar launcher")
            keyboard("a Release fixture")
            wait(lambda:status()["launcher_results"],"resident runner results")
            keyboard("-k","Return")
            wait(lambda:(root/"calendar-launch.jsonl").exists(),"application handoff fixture")
            cli("action","Example","popup")
            wait(lambda:status()["popup_open"],"shared provider popup")
            cli("action","Clock","popup")
            wait(lambda:status()["popup_provider"]=="Clock","calendar popup replacement")
            wait(lambda:(root/"curl.jsonl").exists(),"CalDAV through private HTTPS-only shim")
            today = datetime.date.today().isoformat()
            cli("action","Clock","day",today)
            wait(lambda:any(today in line for line in (root/"calendar-launch.jsonl").read_text().splitlines()),"calendar day action")
            cli("action","Network","edit","fixture-uuid")
            layer("OxiBar modal")
            picture("network","OxiBar modal")
            click("OxiBar modal",170,140)
            keyboard("fixture-password")
            picture("network-filled","OxiBar modal")
            click("OxiBar modal",415,188)
            wait(lambda:any("802-11-wireless-security.psk" in line and "fixture-password" in line for line in (root/"nmcli.jsonl").read_text().splitlines()),"actual network password form")
            wait(lambda:not status()["modal_open"],"network action completion")
            cli("action","Tray","popup")
            cli("action","Tray","menu")
            wait(lambda:status()["popup_provider"]=="Tray","tray popup")
            picture("tray-menu","OxiBar")
            bar = next(item for item in layers() if item["namespace"]=="OxiBar")
            click("OxiBar",bar["w"]-150,80)
            wait(lambda:(root/"tray.jsonl").exists() and any('"menu", 1, "clicked"' in line for line in (root/"tray.jsonl").read_text().splitlines()),"native dbusmenu click")
            notify("Private reply",["inline-reply","Reply"])
            wait(lambda:status()["toast_count"]>0,"notification toast")
            notification_call("ToggleNotificationCenter")
            wait(lambda:status()["panel_open"],"notification panel")
            picture("notification-panel","OxiBar notification panel")
            click("OxiBar notification panel",130,165)
            keyboard("private reply","-k","Return")
            wait(lambda:(root/"notification-signals.jsonl").exists() and "private reply" in (root/"notification-signals.jsonl").read_text(),"actual inline notification reply")
            notification_call("ToggleNotificationCenter")
            wait(lambda:not status()["panel_open"],"notification panel close")
            notification_call("RemoveAllNotifications")
            notify("DND retained")
            notification_call("DoNotDisturb")
            notify("Quiet retained")
            assert len(notification_call("GetAllNotifications")[0])>=2
            notification_call("DoNotDisturb")
            notification_call("RemoveAllNotifications")
            output = status()["bars"][0]["output"]
            widgets = '[{id="first",plugin="Example",output='+json.dumps(output)+',x=64,y=128,width=256,height=224,enabled=true,future="keep"},{id="second",plugin="Example",output='+json.dumps(output)+',x=420,y=128,width=256,height=224,enabled=true}]'
            cli("config","set","desktop.widgets",widgets)
            wait(lambda:len(status()["desktop_widgets"])==2,"shared desktop instances")
            cli("desktop","edit")
            layer("OxiBar widget editor")
            click("OxiBar widget editor",94,160,80,48,output_name=output)
            click("OxiBar widget editor",644,320,64,48,output_name=output)
            cli("desktop","save")
            saved = cli("config","get","desktop.widgets")["data"]["value"]
            assert saved[0]["x"]==144 and saved[0]["y"]==176 and saved[0]["future"]=="keep",saved
            assert saved[1]["width"]==320 and saved[1]["height"]==272,saved
            cli("desktop","edit")
            cli("config","set","bar.transparent","false")
            assert cli("desktop","save",check=False).returncode!=0
            assert status()["desktop_editing"]
            cli("desktop","cancel")
            original_outputs = len(cli("outputs")["data"]["outputs"])
            subprocess.run(["hyprctl","output","create","headless","OXIBAR-HOTPLUG"],env=env,check=True,capture_output=True)
            wait(lambda:len(cli("outputs")["data"]["outputs"])==original_outputs+1,"output hotplug")
            subprocess.run(["hyprctl","output","remove","OXIBAR-HOTPLUG"],env=env,check=True,capture_output=True)
            wait(lambda:len(cli("outputs")["data"]["outputs"])==original_outputs,"output retirement")
            for _ in range(10):
                cli("control-center","open")
                cli("control-center","close")
            assert (config/"config.toml").is_symlink() and '"libcatalog-example.so"' not in base.read_text()
            assert daemon.poll() is None
            print(json.dumps(dict(catalog_sdk_lifecycle=True,launcher_settings_osd_input=True,calendar_caldav_day=True,network_password=True,tray_menu=True,notification_reply_dnd_toast_panel=True,desktop_revision_and_hotplug=True,ten_reopens=True)),flush=True)
        finally:
            stop(service)
            stop(daemon)


if __name__ == "__main__": main()
