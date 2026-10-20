"""Owned test shims and a private StatusNotifier/dbusmenu service."""
import asyncio
import datetime
import json
from pathlib import Path
import sys


def record(root, kind, arguments):
    path = root / (kind + ".jsonl")
    with path.open("a") as output:
        output.write(json.dumps(arguments, default=str) + "\n")


def shim(kind, root, arguments):
    record(root, kind, arguments)
    if kind == "nmcli":
        if arguments == ["--version"]:
            print("nmcli tool, version 1.52.0")
        elif "TYPE" in arguments and "device" in arguments:
            print("wifi")
        elif arguments[-2:] == ["radio", "wifi"]:
            print("enabled")
        elif "--active" in arguments:
            print("Private WiFi:fixture-uuid:802-11-wireless:wlan-fixture")
        elif "NAME,UUID,TYPE" in arguments:
            print("Private WiFi:fixture-uuid:802-11-wireless")
        elif "IN-USE,SSID,BSSID,SIGNAL,SECURITY" in arguments:
            print("*:Private WiFi:AA\\:BB\\:CC\\:DD\\:EE\\:FF:90:WPA2")
    elif kind == "curl":
        today = datetime.date.today().strftime("%Y%m%d")
        print(f'''<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:propstat><d:prop><c:calendar-data>BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:oxibar-private-calendar
DTSTART;VALUE=DATE:{today}
SUMMARY:Private CalDAV fixture
END:VEVENT
END:VCALENDAR
</c:calendar-data></d:prop></d:propstat></d:response></d:multistatus>''')


async def serve(root):
    from dbus_next import Message, MessageType, Variant
    from dbus_next.aio import MessageBus
    from dbus_next.constants import PropertyAccess
    from dbus_next.service import ServiceInterface, dbus_property, method

    class Item(ServiceInterface):
        def __init__(self):
            super().__init__("org.kde.StatusNotifierItem")
        @dbus_property(access=PropertyAccess.READ)
        def Id(self) -> "s": return "oxibar-fixture"
        @dbus_property(access=PropertyAccess.READ)
        def Title(self) -> "s": return "Private tray"
        @dbus_property(access=PropertyAccess.READ)
        def Category(self) -> "s": return "ApplicationStatus"
        @dbus_property(access=PropertyAccess.READ)
        def Status(self) -> "s": return "Active"
        @dbus_property(access=PropertyAccess.READ)
        def IconName(self) -> "s": return "dialog-information"
        @dbus_property(access=PropertyAccess.READ)
        def IconThemePath(self) -> "s": return ""
        @dbus_property(access=PropertyAccess.READ)
        def Menu(self) -> "o": return "/Menu"
        @dbus_property(access=PropertyAccess.READ)
        def ItemIsMenu(self) -> "b": return False
        @dbus_property(access=PropertyAccess.READ)
        def WindowId(self) -> "u": return 0
        @method()
        def Activate(self, x: "i", y: "i"):
            record(root, "tray", ["activate", x, y])
        @method()
        def SecondaryActivate(self, x: "i", y: "i"):
            record(root, "tray", ["secondary", x, y])
        @method()
        def ContextMenu(self, x: "i", y: "i"):
            record(root, "tray", ["context", x, y])

    class Menu(ServiceInterface):
        def __init__(self): super().__init__("com.canonical.dbusmenu")
        @method()
        def AboutToShow(self, item: "i") -> "b": return False
        @method()
        def GetLayout(self, parent: "i", depth: "i", names: "as") -> "u(ia{sv}av)":
            row = Variant("(ia{sv}av)", [1, {"label": Variant("s", "Private menu action"), "enabled": Variant("b", True), "visible": Variant("b", True)}, []])
            return [1, [0, {}, [row]]]
        @method()
        def Event(self, item: "i", event: "s", data: "v", timestamp: "u"):
            record(root, "tray", ["menu", item, event])

    bus = await MessageBus().connect()
    bus.export("/StatusNotifierItem", Item())
    bus.export("/Menu", Menu())
    await bus.request_name("org.oxibar.TestTray")
    def signals(message):
        if message.message_type == MessageType.SIGNAL and message.interface == "org.freedesktop.Notifications":
            record(root, "notification-signals", [message.member, message.body])
    bus.add_message_handler(signals)
    await bus.call(Message(destination="org.freedesktop.DBus", path="/org/freedesktop/DBus", interface="org.freedesktop.DBus", member="AddMatch", signature="s", body=["type='signal',interface='org.freedesktop.Notifications'"]))
    for _ in range(200):
        result = await bus.call(Message(destination="org.kde.StatusNotifierWatcher", path="/StatusNotifierWatcher", interface="org.kde.StatusNotifierWatcher", member="RegisterStatusNotifierItem", signature="s", body=["org.oxibar.TestTray"]))
        if result.message_type != MessageType.ERROR:
            (root / "fixture-ready").touch()
            break
        await asyncio.sleep(.05)
    else:
        raise RuntimeError("private tray watcher did not appear")
    await asyncio.Future()


if __name__ == "__main__":
    kind = sys.argv[1]
    root = Path(sys.argv[2])
    if kind == "serve": asyncio.run(serve(root))
    elif kind == "marker": record(root, "calendar-launch", sys.argv[3:])
    else: shim(kind, root, sys.argv[3:])
