use iced::{
    Element, Task,
    widget::{Column, Row, Space},
};
use oxibar_plugin_api::{
    ABI_VERSION, ApplyMode, PluginModel, PluginMsg, PluginStream, PluginViewContext,
    SettingDescriptor, SettingKind, drain_model_errors, plugin_model,
    toml::{Table, Value},
    with_model_read, with_model_write,
};
use oxiced::widgets::oxi_plugin::text;
use oxiced::{theme::theme_impl::OXITHEME, widgets::oxi_plugin};
use std::{
    io::Read,
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Duration,
};
static INTERVAL: OnceLock<Duration> = OnceLock::new();
#[derive(Debug, Clone, Default)]
struct Snapshot {
    cpu: u32,
    total: u64,
    idle: u64,
    memory_used: u64,
    memory_total: u64,
    uptime: u64,
}
#[derive(Debug)]
struct Model {
    snapshot: Snapshot,
    root: PathBuf,
    refreshing: bool,
    health: Option<String>,
    errors: Vec<String>,
}
#[derive(Debug, Clone)]
enum Message {
    Refresh,
    Snapshot(Result<Snapshot, String>),
}
fn msg(message: Message) -> PluginMsg {
    Arc::new(message)
}
fn read(root: &std::path::Path, name: &str) -> Result<String, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
        .open(root.join(name))
        .map_err(|error| error.to_string())?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("stats: expected regular proc file".into());
    }
    let mut value = String::new();
    file.take(65537)
        .read_to_string(&mut value)
        .map_err(|error| error.to_string())?;
    if value.len() > 65536 {
        return Err("stats: proc file too large".into());
    }
    Ok(value)
}
fn parse(cpu: &str, memory: &str, uptime: &str, previous: &Snapshot) -> Result<Snapshot, String> {
    let counters: Vec<u64> = cpu
        .lines()
        .find(|line| line.starts_with("cpu "))
        .ok_or("stats: missing cpu counters")?
        .split_whitespace()
        .skip(1)
        .take(8)
        .map(str::parse)
        .collect::<Result<_, _>>()
        .map_err(|_| "stats: invalid cpu counters")?;
    if counters.len() < 4 {
        return Err("stats: incomplete cpu counters".into());
    }
    let total = counters.iter().copied().fold(0u64, u64::saturating_add);
    let idle = counters[3].saturating_add(counters.get(4).copied().unwrap_or(0));
    let delta = total.saturating_sub(previous.total);
    let cpu = if previous.total == 0 || delta == 0 {
        0
    } else {
        ((100u128 * u128::from(delta.saturating_sub(idle.saturating_sub(previous.idle))))
            / u128::from(delta))
        .min(100) as u32
    };
    let property = |key: &str| {
        memory.lines().find_map(|line| {
            line.strip_prefix(key)
                .and_then(|value| value.split_whitespace().next())
                .and_then(|value| value.parse::<u64>().ok())
        })
    };
    let memory_total = property("MemTotal:").ok_or("stats: missing memory total")?;
    let available = property("MemAvailable:")
        .or_else(|| property("MemFree:"))
        .unwrap_or(0);
    let uptime = uptime
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or("stats: invalid uptime")? as u64;
    Ok(Snapshot {
        cpu,
        total,
        idle,
        memory_used: memory_total.saturating_sub(available),
        memory_total,
        uptime,
    })
}
oxibar_plugin_api::export_theme_changed!();

#[unsafe(no_mangle)]
pub extern "Rust" fn abi_version() -> u32 {
    ABI_VERSION
}
#[unsafe(no_mangle)]
pub extern "Rust" fn name() -> &'static str {
    "Stats"
}
#[unsafe(no_mangle)]
pub extern "Rust" fn model(config: Table) -> (PluginModel, Option<Task<PluginMsg>>) {
    let settings = config.get("stats").and_then(Value::as_table);
    INTERVAL.get_or_init(|| {
        Duration::from_secs(
            settings
                .and_then(|table| table.get("poll_seconds"))
                .and_then(Value::as_integer)
                .unwrap_or(2)
                .clamp(1, 3600) as u64,
        )
    });
    let root = settings
        .and_then(|table| table.get("proc_root"))
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/proc"));
    (
        plugin_model(Model {
            snapshot: Snapshot::default(),
            root,
            refreshing: false,
            health: None,
            errors: Vec::new(),
        }),
        Some(Task::done(msg(Message::Refresh))),
    )
}
#[unsafe(no_mangle)]
pub extern "Rust" fn update(model: PluginModel, message: PluginMsg) -> Option<Task<PluginMsg>> {
    let message = message.downcast_ref::<Message>()?.clone();
    with_model_write::<Model, _>(&model, |model| match message {
        Message::Refresh => {
            if model.refreshing {
                return None;
            }
            model.refreshing = true;
            let root = model.root.clone();
            let previous = model.snapshot.clone();
            Some(Task::perform(
                async move {
                    let (sender, receiver) = iced::futures::channel::oneshot::channel();
                    std::thread::spawn(move || {
                        let result = (|| {
                            parse(
                                &read(&root, "stat")?,
                                &read(&root, "meminfo")?,
                                &read(&root, "uptime")?,
                                &previous,
                            )
                        })();
                        let _ = sender.send(result);
                    });
                    receiver
                        .await
                        .unwrap_or_else(|_| Err("stats: worker stopped".into()))
                },
                |result| msg(Message::Snapshot(result)),
            ))
        }
        Message::Snapshot(result) => {
            model.refreshing = false;
            match result {
                Ok(snapshot) => {
                    model.snapshot = snapshot;
                    model.health = None;
                }
                Err(error) => {
                    if model.health.as_ref() != Some(&error) {
                        model.errors.push(error.clone());
                    }
                    model.health = Some(error);
                }
            }
            None
        }
    })
    .flatten()
}
#[unsafe(no_mangle)]
pub extern "Rust" fn launch(_: usize, _: PluginModel) -> Option<Task<PluginMsg>> {
    None
}
#[unsafe(no_mangle)]
pub extern "Rust" fn errors(model: PluginModel) -> Vec<String> {
    drain_model_errors::<Model>(&model, |model| &mut model.errors)
}
#[unsafe(no_mangle)]
pub extern "Rust" fn subscription() -> *mut PluginStream {
    let stream = iced::futures::stream::unfold(true, |first| async move {
        if !first {
            async_io::Timer::after(INTERVAL.get().copied().unwrap_or(Duration::from_secs(2))).await;
        }
        Some((msg(Message::Refresh), false))
    });
    Box::into_raw(Box::new(Box::pin(stream))) as *mut PluginStream
}

#[unsafe(no_mangle)]
pub extern "Rust" fn subscription_policy() -> oxibar_plugin_api::SubscriptionPolicy {
    oxibar_plugin_api::SubscriptionPolicy::WhenVisible
}

#[unsafe(no_mangle)]
pub extern "Rust" fn desktop_interactive(_model: PluginModel) -> bool {
    false
}
#[unsafe(no_mangle)]
pub extern "Rust" fn settings_schema() -> Vec<SettingDescriptor> {
    vec![
        SettingDescriptor::new(
            "stats.poll_seconds",
            "System stats refresh",
            SettingKind::Integer { min: 1, max: 3600 },
            Some(Value::Integer(2)),
            ApplyMode::Restart,
        ),
        SettingDescriptor::new(
            "stats.proc_root",
            "Stats proc root",
            SettingKind::Text,
            Some(Value::String("/proc".into())),
            ApplyMode::Restart,
        ),
    ]
}
#[unsafe(no_mangle)]
pub extern "Rust" fn view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        vec![
            text(format!("CPU {}%", model.snapshot.cpu))
                .size(OXITHEME.font_md)
                .style(oxi_plugin::text_primary)
                .into(),
        ]
    })
}
#[unsafe(no_mangle)]
pub extern "Rust" fn desktop_view(
    model: PluginModel,
    _: PluginViewContext,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    control_center_view(model)
}
#[unsafe(no_mangle)]
pub extern "Rust" fn control_center_view(
    model: PluginModel,
) -> Result<Vec<Element<'static, PluginMsg>>, std::io::Error> {
    with_model_read::<Model, _>(&model, |model| {
        let data = &model.snapshot;
        let memory = if data.memory_total == 0 {
            0
        } else {
            (100u128 * u128::from(data.memory_used) / u128::from(data.memory_total)).min(100) as u32
        };
        let mut body = Column::new()
            .spacing(OXITHEME.padding_lg)
            .push(
                text("System stats")
                    .size(OXITHEME.font_lg)
                    .style(oxi_plugin::text_primary),
            )
            .push(
                Row::new()
                    .push(text("CPU").style(oxi_plugin::text_muted))
                    .push(Space::new().width(iced::Length::Fill))
                    .push(text(format!("{}%", data.cpu)).style(oxi_plugin::text_primary)),
            )
            .push(oxi_plugin::level_meter(data.cpu as f32, 100.0))
            .push(
                text(format!(
                    "Memory {} / {} MiB",
                    data.memory_used / 1024,
                    data.memory_total / 1024
                ))
                .size(OXITHEME.font_md)
                .style(oxi_plugin::text_primary),
            )
            .push(oxi_plugin::level_meter(memory as f32, 100.0))
            .push(
                text(format!(
                    "Uptime {}h {}m",
                    data.uptime / 3600,
                    data.uptime / 60 % 60
                ))
                .size(OXITHEME.font_md)
                .style(oxi_plugin::text_muted),
            );
        if let Some(health) = &model.health {
            body = body.push(
                text(health.clone())
                    .size(OXITHEME.font_md)
                    .style(oxi_plugin::text_muted),
            );
        }
        vec![body.into()]
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counters_use_deltas_and_available_memory() {
        let previous = Snapshot {
            total: 100,
            idle: 50,
            ..Default::default()
        };
        let snapshot = parse(
            "cpu 40 0 10 70 0 0 0 0",
            "MemTotal: 2048 kB\nMemAvailable: 512 kB",
            "3660.5 0",
            &previous,
        )
        .unwrap();
        assert_eq!(snapshot.cpu, 0);
        assert_eq!(snapshot.memory_used, 1536);
        assert_eq!(snapshot.uptime, 3660);
        assert!(parse("cpu invalid", "", "0", &previous).is_err());
        let busy = parse("cpu 55 0 15 55", "MemTotal: 1024 kB", "0", &previous).unwrap();
        assert_eq!(busy.cpu, 80);
    }
}
