use super::{
    Output, OutputMode, OutputSnapshot,
    configuration::{self, DisplayConfig},
    controller::{Backend, Controller},
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use toml::{Table, Value};

#[test]
fn profiles_keep_fractional_refresh_and_reject_destructive_or_injected_layouts() {
    let value: Table = toml::from_str("profiles=[{name='DP-1',width=2560,height=1440,refresh_hz=143.981,scale=1.25,x=-2048,y=0,transform=1,vrr=true}]").unwrap();
    let config = configuration::parse(value.get("profiles")).unwrap();
    assert_eq!(config[0].mode.refresh_hz, 143.981);
    let mut default_refresh = config.clone();
    default_refresh[0].mode.refresh_hz = 0.0;
    assert!(configuration::validate(&default_refresh).is_ok());
    default_refresh[0].mode.refresh_hz = -1.0;
    assert!(configuration::validate(&default_refresh).is_err());
    assert_eq!(
        configuration::parse(Some(&Value::Array(
            config.iter().map(DisplayConfig::value).collect()
        )))
        .unwrap(),
        config
    );
    let mut bad = config.clone();
    bad[0].enabled = false;
    assert!(configuration::validate(&bad).is_err());
    bad = config.clone();
    bad[0].scale = f64::NAN;
    assert!(configuration::validate(&bad).is_err());
    bad = config.clone();
    bad[0].name = "DP-1; exec bad".into();
    assert!(configuration::validate(&bad).is_err());
    bad = config.clone();
    bad.push(bad[0].clone());
    assert!(configuration::validate(&bad).is_err());
}

struct Mock(Arc<Mutex<OutputSnapshot>>);
impl Backend for Mock {
    fn label(&self) -> &str {
        "mock displays"
    }
    fn inventory(&mut self) -> Result<OutputSnapshot, String> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn apply(&mut self, config: &[DisplayConfig]) -> Result<(), String> {
        let mut inventory = self.0.lock().unwrap();
        for desired in config {
            if let Some(output) = inventory
                .outputs
                .iter_mut()
                .find(|output| output.name == desired.name)
            {
                output.scale = desired.scale;
                output.position = desired.position;
                output.enabled = desired.enabled;
            }
        }
        Ok(())
    }
}
fn fixture(timeout: Duration) -> (Controller, Arc<Mutex<OutputSnapshot>>) {
    let inventory = Arc::new(Mutex::new(OutputSnapshot {
        backend: "mock".into(),
        outputs: vec![Output {
            name: "DP-1".into(),
            description: String::new(),
            make: String::new(),
            model: String::new(),
            serial: String::new(),
            enabled: true,
            focused: true,
            physical_size: (1920, 1080),
            logical_size: (1920, 1080),
            position: (0, 0),
            scale: 1.0,
            transform: 0,
            refresh_hz: 59.94,
            vrr: None,
            modes: vec![OutputMode {
                width: 1920,
                height: 1080,
                refresh_hz: 59.94,
            }],
        }],
    }));
    let state = inventory.clone();
    (
        Controller::spawn(Box::new(move || Ok(Box::new(Mock(state.clone())))), timeout).unwrap(),
        inventory,
    )
}
#[test]
fn timeout_owner_drop_and_stale_ids_revert_without_a_ui_or_host_reactor() {
    let (controller, state) = fixture(Duration::from_millis(60));
    let mut config = vec![DisplayConfig::from(&state.lock().unwrap().outputs[0])];
    config[0].scale = 1.25;
    let preview = iced::futures::executor::block_on(controller.preview(config.clone())).unwrap();
    assert_eq!(state.lock().unwrap().outputs[0].scale, 1.25);
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(state.lock().unwrap().outputs[0].scale, 1.0);
    assert!(
        iced::futures::executor::block_on(controller.keep(
            preview.id,
            Box::new(|| panic!("stale preview must not write settings"))
        ))
        .is_err()
    );
    let second = iced::futures::executor::block_on(controller.preview(config)).unwrap();
    assert_ne!(preview.id, second.id);
    drop(controller);
    assert_eq!(state.lock().unwrap().outputs[0].scale, 1.0);
}
#[test]
fn failed_revision_save_rolls_back_but_confirmed_sparse_save_survives_owner_drop() {
    let (controller, state) = fixture(Duration::from_secs(2));
    let mut config = vec![DisplayConfig::from(&state.lock().unwrap().outputs[0])];
    config[0].scale = 1.5;
    let first = iced::futures::executor::block_on(controller.preview(config.clone())).unwrap();
    assert!(
        iced::futures::executor::block_on(controller.keep(
            first.id,
            Box::new(|| Err("stale configuration revision".into()))
        ))
        .is_err()
    );
    assert_eq!(state.lock().unwrap().outputs[0].scale, 1.0);
    let second = iced::futures::executor::block_on(controller.preview(config.clone())).unwrap();
    let mut store = crate::config::store::ConfigStore::memory(Table::new(), Table::new());
    let revision = store.snapshot().revision;
    let value = Value::Array(config.iter().map(DisplayConfig::value).collect());
    let snapshot = iced::futures::executor::block_on(controller.keep(
        second.id,
        Box::new(move || {
            store.transact(
                crate::config::store::Mutation::Set("monitor.profiles".into(), value),
                Some(revision),
            )
        }),
    ))
    .unwrap();
    assert!(
        snapshot
            .effective
            .get("monitor")
            .unwrap()
            .get("profiles")
            .is_some()
    );
    drop(controller);
    assert_eq!(state.lock().unwrap().outputs[0].scale, 1.5);
}

#[test]
#[ignore = "requires OXIBAR_TEST_ISOLATED=1 and an owned private compositor; changes only its virtual output"]
fn private_compositor_native_preview_revert_and_shutdown_restore() {
    assert_eq!(std::env::var("OXIBAR_TEST_ISOLATED").as_deref(), Ok("1"));
    let controller = Controller::new().unwrap();
    let inventory = iced::futures::executor::block_on(controller.discover()).unwrap();
    let mut config: Vec<_> = inventory.outputs.iter().map(DisplayConfig::from).collect();
    let name = config[0].name.clone();
    let scale = config[0].scale;
    config[0].scale = 1.25;
    let preview = iced::futures::executor::block_on(controller.preview(config.clone())).unwrap();
    assert_eq!(
        preview
            .inventory
            .outputs
            .iter()
            .find(|output| output.name == name)
            .unwrap()
            .scale,
        1.25
    );
    let restored = iced::futures::executor::block_on(controller.revert(preview.id)).unwrap();
    assert_eq!(
        restored
            .outputs
            .iter()
            .find(|output| output.name == name)
            .unwrap()
            .scale,
        scale
    );
    iced::futures::executor::block_on(controller.preview(config)).unwrap();
    drop(controller);
    let again = Controller::new().unwrap();
    let restored = iced::futures::executor::block_on(again.discover()).unwrap();
    assert_eq!(
        restored
            .outputs
            .iter()
            .find(|output| output.name == name)
            .unwrap()
            .scale,
        scale
    );
}
