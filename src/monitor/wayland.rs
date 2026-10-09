use std::collections::BTreeMap;

use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    protocol::{
        wl_output::{self, WlOutput},
        wl_registry::WlRegistry,
    },
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1,
    zxdg_output_v1::{self, ZxdgOutputV1},
};

use super::{Output, OutputMode, OutputSnapshot, output::logical_size};

#[derive(Default)]
struct State {
    outputs: BTreeMap<u32, (WlOutput, Output)>,
    manager: Option<ZxdgOutputManagerV1>,
    logical: BTreeMap<u32, (u32, u32)>,
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wayland_client::protocol::wl_registry::Event,
        _: &(),
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_output" => {
                    let output: WlOutput = registry.bind(name, version.min(4), queue, name);
                    if let Some(manager) = &state.manager {
                        manager.get_xdg_output(&output, queue, name);
                    }
                    state.outputs.insert(
                        name,
                        (
                            output,
                            Output {
                                name: String::new(),
                                description: String::new(),
                                make: String::new(),
                                model: String::new(),
                                serial: String::new(),
                                enabled: true,
                                focused: false,
                                physical_size: (0, 0),
                                logical_size: (0, 0),
                                position: (0, 0),
                                scale: 1.0,
                                transform: 0,
                                refresh_hz: 0.0,
                                vrr: None,
                                modes: Vec::new(),
                            },
                        ),
                    );
                }
                "zxdg_output_manager_v1" => {
                    let manager: ZxdgOutputManagerV1 =
                        registry.bind(name, version.min(3), queue, ());
                    for (id, (output, _)) in &state.outputs {
                        manager.get_xdg_output(output, queue, *id);
                    }
                    state.manager = Some(manager);
                }
                _ => (),
            }
        }
    }
}

impl Dispatch<WlOutput, u32> for State {
    fn event(
        state: &mut Self,
        _: &WlOutput,
        event: wl_output::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some((_, output)) = state.outputs.get_mut(id) else {
            return;
        };
        match event {
            wl_output::Event::Name { name } => output.name = name,
            wl_output::Event::Description { description } => output.description = description,
            wl_output::Event::Geometry {
                x,
                y,
                make,
                model,
                transform,
                ..
            } => {
                output.position = (x, y);
                output.make = make;
                output.model = model;
                if let WEnum::Value(transform) = transform {
                    output.transform = transform as u32;
                }
            }
            wl_output::Event::Scale { factor } if factor > 0 => output.scale = factor as f64,
            wl_output::Event::Mode {
                flags,
                width,
                height,
                refresh,
            } if width > 0 && height > 0 && refresh >= 0 => {
                let mode = OutputMode {
                    width: width as u32,
                    height: height as u32,
                    refresh_hz: refresh as f64 / 1000.0,
                };
                if let WEnum::Value(flags) = flags
                    && flags.contains(wl_output::Mode::Current)
                {
                    output.physical_size = (mode.width, mode.height);
                    output.refresh_hz = mode.refresh_hz;
                }
                output.modes.push(mode);
            }
            _ => (),
        }
    }
}

impl Dispatch<ZxdgOutputManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZxdgOutputManagerV1,
        _: <ZxdgOutputManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZxdgOutputV1, u32> for State {
    fn event(
        state: &mut Self,
        _: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some((_, output)) = state.outputs.get_mut(id) else {
            return;
        };
        match event {
            zxdg_output_v1::Event::Name { name } => output.name = name,
            zxdg_output_v1::Event::Description { description } => output.description = description,
            zxdg_output_v1::Event::LogicalPosition { x, y } => output.position = (x, y),
            zxdg_output_v1::Event::LogicalSize { width, height } if width > 0 && height > 0 => {
                state.logical.insert(*id, (width as u32, height as u32));
            }
            _ => (),
        }
    }
}

pub(super) fn inventory() -> Result<OutputSnapshot, String> {
    let connection =
        crate::app::connect_wayland().ok_or("Could not connect to Wayland for output inventory")?;
    let mut queue = connection.new_event_queue();
    let mut state = State::default();
    connection.display().get_registry(&queue.handle(), ());
    queue
        .roundtrip(&mut state)
        .map_err(|error| error.to_string())?;
    let advertised = state.outputs.len();
    queue
        .roundtrip(&mut state)
        .map_err(|error| error.to_string())?;
    queue
        .roundtrip(&mut state)
        .map_err(|error| error.to_string())?;
    let mut outputs: Vec<_> = state
        .outputs
        .into_iter()
        .filter_map(|(id, (_, mut output))| {
            if output.name.is_empty() {
                return None;
            }
            output.logical_size = state.logical.get(&id).copied().unwrap_or_else(|| {
                logical_size(output.physical_size, output.scale, output.transform)
            });
            let rotated = if matches!(output.transform, 1 | 3 | 5 | 7) {
                output.physical_size.1
            } else {
                output.physical_size.0
            };
            if output.logical_size.0 > 0 && rotated > 0 {
                output.scale = rotated as f64 / output.logical_size.0 as f64;
            }
            output.enabled = output.physical_size.0 > 0 && output.physical_size.1 > 0;
            Some(output)
        })
        .collect();
    outputs.sort_by(|a, b| a.name.cmp(&b.name));
    if outputs.is_empty() && advertised > 0 {
        return Err("Compositor advertises outputs without addressable names; using compositor-selected bar placement".to_owned());
    }
    Ok(OutputSnapshot {
        backend: "Wayland wl_output / xdg_output".to_owned(),
        outputs,
    })
}
