use std::collections::HashMap;

use zbus::zvariant::{OwnedObjectPath, OwnedValue};

pub(crate) const BLUEZ: &str = "org.bluez";
pub(crate) const ADAPTER: &str = "org.bluez.Adapter1";
pub(crate) const DEVICE: &str = "org.bluez.Device1";
pub(crate) const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
pub(crate) const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
pub(crate) type Properties = HashMap<String, OwnedValue>;
pub(crate) type Interfaces = HashMap<String, Properties>;
pub(crate) type Objects = HashMap<OwnedObjectPath, Interfaces>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BluetoothDevice {
    pub(crate) path: String,
    pub(crate) adapter: String,
    pub(crate) mac: String,
    pub(crate) name: String,
    pub(crate) icon: String,
    pub(crate) paired: bool,
    pub(crate) trusted: bool,
    pub(crate) connected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Adapter {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) powered: bool,
    pub(crate) discovering: bool,
    pub(crate) scanning: bool,
}

impl std::fmt::Display for Adapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) adapters: Vec<Adapter>,
    pub(crate) connected: Vec<BluetoothDevice>,
    pub(crate) available: Vec<BluetoothDevice>,
}

impl Snapshot {
    pub(crate) fn device(&self, path: &str) -> Option<&BluetoothDevice> {
        self.connected
            .iter()
            .chain(&self.available)
            .find(|device| device.path == path)
    }

    pub(crate) fn adapter(&self, path: &str) -> Option<&Adapter> {
        self.adapters.iter().find(|adapter| adapter.path == path)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Action {
    Power(String, bool),
    Discovery(String, bool),
    Disconnect(String),
    Trust(String, bool),
    Forget(String),
}

impl Action {
    pub(crate) fn path(&self) -> &str {
        match self {
            Self::Power(path, _)
            | Self::Discovery(path, _)
            | Self::Disconnect(path)
            | Self::Trust(path, _)
            | Self::Forget(path) => path,
        }
    }
}

pub(crate) fn snapshot(objects: &Objects) -> Snapshot {
    let mut state = Snapshot::default();
    for (path, interfaces) in objects {
        if let Some(properties) = interfaces.get(ADAPTER) {
            state.adapters.push(Adapter {
                path: path.to_string(),
                name: label(properties, path.as_str()),
                powered: boolean(properties, "Powered"),
                discovering: boolean(properties, "Discovering"),
                scanning: false,
            });
        }
    }
    for (path, interfaces) in objects {
        let Some(properties) = interfaces.get(DEVICE) else {
            continue;
        };
        let adapter = properties
            .get("Adapter")
            .and_then(|value| <&zbus::zvariant::ObjectPath<'_>>::try_from(value).ok())
            .map(ToString::to_string);
        let Some(adapter) = adapter.filter(|path| state.adapter(path).is_some()) else {
            continue;
        };
        let mac = string(properties, "Address").unwrap_or_default();
        let device = BluetoothDevice {
            path: path.to_string(),
            adapter,
            name: label(properties, &mac),
            mac,
            icon: string(properties, "Icon").unwrap_or_default(),
            paired: boolean(properties, "Paired"),
            trusted: boolean(properties, "Trusted"),
            connected: boolean(properties, "Connected"),
        };
        if device.connected {
            state.connected.push(device);
        } else if device.paired || (!device.name.is_empty() && !looks_like_address(&device.name)) {
            state.available.push(device);
        }
    }
    state.adapters.sort_by(|a, b| a.path.cmp(&b.path));
    for devices in [&mut state.connected, &mut state.available] {
        devices.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.path.cmp(&b.path))
        });
    }
    state
}

pub(crate) fn add(objects: &mut Objects, path: OwnedObjectPath, interfaces: Interfaces) {
    objects.entry(path).or_default().extend(interfaces);
}

pub(crate) fn remove(objects: &mut Objects, path: &OwnedObjectPath, interfaces: &[String]) {
    if let Some(current) = objects.get_mut(path) {
        for interface in interfaces {
            current.remove(interface);
        }
        if current.is_empty() {
            objects.remove(path);
        }
    }
}

pub(crate) fn change(
    objects: &mut Objects,
    path: &OwnedObjectPath,
    interface: String,
    properties: Properties,
) {
    if let Some(current) = objects
        .get_mut(path)
        .and_then(|interfaces| interfaces.get_mut(&interface))
    {
        current.extend(properties);
    }
}

fn boolean(properties: &Properties, key: &str) -> bool {
    properties
        .get(key)
        .and_then(|value| bool::try_from(value).ok())
        .unwrap_or(false)
}

fn string(properties: &Properties, key: &str) -> Option<String> {
    properties
        .get(key)
        .and_then(|value| <&str>::try_from(value).ok())
        .map(str::to_owned)
}

fn label(properties: &Properties, fallback: &str) -> String {
    ["Alias", "Name"]
        .into_iter()
        .filter_map(|key| string(properties, key))
        .find(|name| !name.trim().is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

fn looks_like_address(value: &str) -> bool {
    let parts: Vec<_> = value.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.chars().all(|ch| ch.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn fixture() -> Objects {
        HashMap::from([
            (
                OwnedObjectPath::try_from("/org/bluez/hci0").unwrap(),
                HashMap::from([(
                    ADAPTER.to_owned(),
                    HashMap::from([
                        (
                            "Alias".to_owned(),
                            OwnedValue::from(zbus::zvariant::Str::from("Radio")),
                        ),
                        ("Powered".to_owned(), OwnedValue::from(true)),
                    ]),
                )]),
            ),
            (
                OwnedObjectPath::try_from("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").unwrap(),
                HashMap::from([(
                    DEVICE.to_owned(),
                    HashMap::from([
                        (
                            "Adapter".to_owned(),
                            OwnedValue::from(
                                zbus::zvariant::ObjectPath::try_from("/org/bluez/hci0").unwrap(),
                            ),
                        ),
                        (
                            "Address".to_owned(),
                            OwnedValue::from(zbus::zvariant::Str::from("AA:BB:CC:DD:EE:FF")),
                        ),
                        (
                            "Name".to_owned(),
                            OwnedValue::from(zbus::zvariant::Str::from("Keyboard")),
                        ),
                    ]),
                )]),
            ),
        ])
    }

    #[test]
    fn deltas_keep_partial_fields_and_handle_removal_and_missing_icons() {
        let mut objects = fixture();
        let path = OwnedObjectPath::try_from("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").unwrap();
        assert_eq!(snapshot(&objects).available[0].name, "Keyboard");
        change(
            &mut objects,
            &path,
            DEVICE.to_owned(),
            HashMap::from([("Connected".to_owned(), true.into())]),
        );
        let state = snapshot(&objects);
        assert_eq!(state.connected[0].name, "Keyboard");
        assert!(state.available.is_empty());
        remove(&mut objects, &path, &[DEVICE.to_owned()]);
        assert!(snapshot(&objects).connected.is_empty());
    }

    #[test]
    fn missing_adapter_and_wrong_property_types_are_tolerated() {
        let mut objects = fixture();
        let path = OwnedObjectPath::try_from("/org/bluez/hci0").unwrap();
        change(
            &mut objects,
            &path,
            ADAPTER.to_owned(),
            HashMap::from([("Powered".to_owned(), 17_u32.into())]),
        );
        assert!(!snapshot(&objects).adapters[0].powered);
        remove(&mut objects, &path, &[ADAPTER.to_owned()]);
        assert!(snapshot(&objects).available.is_empty());
    }
}
