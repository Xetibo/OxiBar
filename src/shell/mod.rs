use std::collections::{BTreeMap, HashMap};

use iced_layershell::reexport::IcedId;

use crate::{
    config::{ConfigSnapshot, bars::BarConfig},
    layout::{self, BarDimensions},
    monitor::OutputSnapshot,
};

pub(crate) mod runtime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceKind {
    Bar(String),
    Launcher,
    Settings,
    ControlCenter,
    Osd,
    Modal(String),
    Panel(String),
    Toast(String, String),
}

#[derive(Debug, Clone)]
pub struct SurfaceRecord {
    pub kind: SurfaceKind,
    pub output: Option<String>,
    pub owner_bar: Option<IcedId>,
}

#[derive(Default)]
pub struct SurfaceRegistry {
    records: HashMap<IcedId, SurfaceRecord>,
}

impl SurfaceRegistry {
    pub fn register(
        &mut self,
        id: IcedId,
        kind: SurfaceKind,
        output: Option<String>,
        owner_bar: Option<IcedId>,
    ) {
        self.records.insert(
            id,
            SurfaceRecord {
                kind,
                output,
                owner_bar,
            },
        );
    }
    pub fn get(&self, id: IcedId) -> Option<&SurfaceRecord> {
        self.records.get(&id)
    }
    pub fn remove(&mut self, id: IcedId) -> Option<SurfaceRecord> {
        self.records.remove(&id)
    }

    pub fn detach_owner(&mut self, owner: IcedId) {
        for record in self.records.values_mut() {
            if record.owner_bar == Some(owner) {
                record.owner_bar = None;
            }
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (&IcedId, &SurfaceRecord)> {
        self.records.iter()
    }
}

#[derive(Debug, Clone)]
pub struct BarInstance {
    pub id: IcedId,
    pub output: Option<String>,
    pub config: BarConfig,
    pub size: BarDimensions,
    pub ready: bool,
}

pub(crate) struct Shell {
    pub command_output: Option<Option<String>>,
    pub registry: SurfaceRegistry,
    pub bars: BTreeMap<String, BarInstance>,
    pub inventory: Option<OutputSnapshot>,
    pub inventory_error: Option<String>,
    pub selected_output: Option<String>,
    pub active_bar: Option<IcedId>,
    pub request_source: Option<IcedId>,
    pub popup_bar: Option<IcedId>,
    pub plugin_origins: BTreeMap<String, IcedId>,
}

impl Shell {
    pub(crate) fn new(inventory: Result<OutputSnapshot, String>) -> Self {
        let selected_output = inventory
            .as_ref()
            .ok()
            .and_then(OutputSnapshot::focused)
            .map(|output| output.name.clone());
        Self {
            command_output: None,
            registry: SurfaceRegistry::default(),
            bars: BTreeMap::new(),
            inventory: inventory.as_ref().ok().cloned(),
            inventory_error: inventory.err(),
            selected_output,
            active_bar: None,
            request_source: None,
            popup_bar: None,
            plugin_origins: BTreeMap::new(),
        }
    }
    pub(crate) fn bar(&self, id: IcedId) -> Option<&BarInstance> {
        self.bars.values().find(|bar| bar.id == id)
    }
    pub(crate) fn bar_mut(&mut self, id: IcedId) -> Option<&mut BarInstance> {
        self.bars.values_mut().find(|bar| bar.id == id)
    }

    pub(crate) fn desired(
        &mut self,
        snapshot: &ConfigSnapshot,
        order: &[String],
    ) -> BTreeMap<String, (Option<String>, BarConfig, BarDimensions)> {
        let table = snapshot
            .effective
            .get("bar")
            .and_then(|value| value.as_table())
            .expect("validated bar");
        if table.get("enabled").and_then(|value| value.as_bool()) == Some(false) {
            return BTreeMap::new();
        }
        let selected: Vec<_> = table
            .get("outputs")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str())
            .collect();
        let enabled: Vec<_> = self
            .inventory
            .as_ref()
            .into_iter()
            .flat_map(|snapshot| &snapshot.outputs)
            .filter(|output| output.enabled)
            .collect();
        if self
            .selected_output
            .as_ref()
            .is_none_or(|name| !enabled.iter().any(|output| &output.name == name))
        {
            self.selected_output = self
                .inventory
                .as_ref()
                .and_then(OutputSnapshot::focused)
                .map(|output| output.name.clone());
        }
        let mut targets: Vec<_> = if selected.is_empty() {
            enabled
                .iter()
                .filter(|output| Some(&output.name) == self.selected_output.as_ref())
                .copied()
                .collect()
        } else {
            enabled
                .iter()
                .filter(|output| {
                    selected.contains(&"*") || selected.contains(&output.name.as_str())
                })
                .copied()
                .collect()
        };
        if targets.is_empty()
            && table
                .get("output_fallback")
                .and_then(|value| value.as_bool())
                != Some(false)
        {
            targets.extend(self.inventory.as_ref().and_then(OutputSnapshot::focused));
        }
        if targets.is_empty() {
            if self.inventory.is_some() {
                return BTreeMap::new();
            }
            let config = BarConfig::for_output(snapshot, None, order);
            return BTreeMap::from([(
                "default".to_owned(),
                (
                    None,
                    config.clone(),
                    BarDimensions {
                        width: config.width.unwrap_or(layout::DEFAULT_BAR_WIDTH),
                        height: config.height,
                    },
                ),
            )]);
        }
        targets
            .into_iter()
            .map(|output| {
                let config = BarConfig::for_output(snapshot, Some(&output.name), order);
                let size = BarDimensions {
                    width: config.width.unwrap_or(output.logical_size.0),
                    height: config.height,
                };
                (
                    output.name.clone(),
                    (Some(output.name.clone()), config, size),
                )
            })
            .collect()
    }
}
